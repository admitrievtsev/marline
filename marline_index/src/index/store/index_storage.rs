use crate::index::error::IndexError;
use crate::index::store::InvertedStorage;
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::hash::Hash;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// In-memory posting-list storage backed by `RwLock`-protected maps.
pub struct IndexStorage<K, F> {
    postings: RwLock<HashMap<F, HashSet<K>>>,
    entries: RwLock<HashMap<K, Vec<F>>>,
}

impl<K, F> IndexStorage<K, F> {
    pub fn new() -> Self {
        Self { postings: RwLock::new(HashMap::new()), entries: RwLock::new(HashMap::new()) }
    }

    fn postings_read(&self) -> RwLockReadGuard<'_, HashMap<F, HashSet<K>>> {
        self.postings.read().expect("postings lock poisoned")
    }

    fn postings_write(&self) -> RwLockWriteGuard<'_, HashMap<F, HashSet<K>>> {
        self.postings.write().expect("postings lock poisoned")
    }

    fn store_entry_features(&self, key: K, features: Vec<F>) -> bool
    where
        K: Eq + Hash,
    {
        match self.entries.write().expect("entries lock poisoned").entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(features);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    fn take_entry_features(&self, key: &K) -> Option<Vec<F>>
    where
        K: Eq + Hash,
    {
        self.entries.write().expect("entries lock poisoned").remove(key)
    }
}

impl<K, F> Default for IndexStorage<K, F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, F> InvertedStorage<K, F> for IndexStorage<K, F>
where
    K: Clone + Eq + Hash + Send + Sync,
    F: Copy + Eq + Hash + Send + Sync,
{
    fn posting_list(&self, feature: F) -> Result<Vec<K>, IndexError> {
        let postings = self.postings_read();
        Ok(postings.get(&feature).map(|keys| keys.iter().cloned().collect()).unwrap_or_default())
    }

    fn posting_len(&self, feature: F) -> Result<usize, IndexError> {
        let postings = self.postings_read();
        Ok(postings.get(&feature).map_or(0, HashSet::len))
    }

    fn accumulate_posting(
        &self,
        feature: F,
        candidates: &mut HashMap<K, usize>,
    ) -> Result<usize, IndexError> {
        let postings = self.postings_read();
        let Some(keys) = postings.get(&feature) else {
            return Ok(0);
        };
        for key in keys {
            *candidates.entry(key.clone()).or_default() += 1;
        }
        Ok(keys.len())
    }

    fn verify_candidates(
        &self,
        feature: F,
        candidates: &mut HashMap<K, usize>,
    ) -> Result<super::CandidateVerification, IndexError> {
        let postings = self.postings_read();
        let Some(keys) = postings.get(&feature) else {
            return Ok(Default::default());
        };
        let mut work = super::CandidateVerification::default();
        // Hash collections iterate over capacity, so probe whichever collection is smaller.
        if candidates.capacity() < keys.capacity() {
            work.candidate_probes = candidates.len();
            for (key, overlap) in candidates.iter_mut() {
                if keys.contains(key) {
                    *overlap += 1;
                    work.matches += 1;
                }
            }
        } else {
            work.posting_visits = keys.len();
            for key in keys {
                if let Some(overlap) = candidates.get_mut(key) {
                    *overlap += 1;
                    work.matches += 1;
                }
            }
        }
        Ok(work)
    }

    fn insert_posting(&self, feature: F, key: K) -> Result<(), IndexError> {
        self.postings_write().entry(feature).or_default().insert(key);
        Ok(())
    }

    fn remove_posting(&self, feature: F, key: &K) -> Result<(), IndexError> {
        let mut postings = self.postings_write();
        if let Some(keys) = postings.get_mut(&feature) {
            keys.remove(key);
            if keys.is_empty() {
                postings.remove(&feature);
            }
        }
        Ok(())
    }

    fn len_postings(&self) -> Result<usize, IndexError> {
        Ok(self.postings_read().len())
    }

    fn clear_postings(&self) -> Result<(), IndexError> {
        self.postings_write().clear();
        self.entries.write().expect("entries lock poisoned").clear();
        Ok(())
    }
}

impl<K, F, S> crate::index::store::Store<K, S> for IndexStorage<K, F>
where
    K: Clone + Eq + Hash + Send + Sync,
    F: Copy + Eq + Hash + Send + Sync,
    S: crate::sketch::Sketch<Feature = F>,
{
    fn insert_entry(&self, key: K, sketch: S) -> Result<(), IndexError> {
        let features: Vec<_> = sketch.iter().collect();
        if !self.store_entry_features(key.clone(), features.clone()) {
            return Ok(());
        }
        for feature in features {
            self.insert_posting(feature, key.clone())?;
        }
        Ok(())
    }

    fn remove_entry(&self, key: &K) -> Result<(), IndexError> {
        if let Some(features) = self.take_entry_features(key) {
            for feature in features {
                self.remove_posting(feature, key)?;
            }
        }
        Ok(())
    }

    fn clear(&self) -> Result<(), IndexError> {
        self.clear_postings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Mock = IndexStorage<u64, u32>;

    #[test]
    fn insert_posting_is_idempotent() {
        let store = Mock::new();
        store.insert_posting(100, 42).unwrap();
        store.insert_posting(100, 42).unwrap();
        assert_eq!(store.posting_list(100).unwrap(), vec![42]);
    }

    #[test]
    fn posting_accumulation_and_candidate_verification_preserve_overlap_counts() {
        let store = Mock::new();
        for key in 0..1000 {
            store.insert_posting(1, key).unwrap();
        }
        let mut candidates = HashMap::from([(2, 5), (999, 1), (5000, 4)]);
        assert_eq!(store.posting_len(1).unwrap(), 1000);
        let work = store.verify_candidates(1, &mut candidates).unwrap();
        assert_eq!(work.matches, 2);
        assert_eq!(work.candidate_probes, 3);
        assert_eq!(candidates[&999], 2);

        let mut accumulated = HashMap::new();
        assert_eq!(store.accumulate_posting(1, &mut accumulated).unwrap(), 1000);
        assert_eq!(accumulated.len(), 1000);

        store.insert_posting(2, 2).unwrap();
        candidates.insert(2, 4);
        let work = store.verify_candidates(2, &mut candidates).unwrap();
        assert_eq!(work.matches, 1);
        assert_eq!(candidates[&2], 5);
    }

    #[test]
    fn posting_lists_are_independent_by_feature() {
        let store = Mock::new();
        store.insert_posting(100, 1).unwrap();
        store.insert_posting(200, 2).unwrap();
        assert_eq!(store.posting_list(100).unwrap(), vec![1]);
        assert_eq!(store.posting_list(200).unwrap(), vec![2]);
    }

    #[test]
    fn remove_posting_removes_empty_feature_bucket() {
        let store = Mock::new();
        store.insert_posting(100, 1).unwrap();
        store.remove_posting(100, &1).unwrap();
        assert!(store.posting_list(100).unwrap().is_empty());
        assert_eq!(store.len_postings().unwrap(), 0);
    }

    #[test]
    fn clear_removes_all_postings() {
        let store = Mock::new();
        store.insert_posting(1, 1).unwrap();
        store.clear_postings().unwrap();
        assert_eq!(store.len_postings().unwrap(), 0);
    }

    #[test]
    fn entry_features_are_used_for_key_only_removal() {
        let store = Mock::new();
        assert!(store.store_entry_features(42, vec![100, 200]));
        store.insert_posting(100, 42).unwrap();
        store.insert_posting(200, 42).unwrap();

        assert_eq!(store.take_entry_features(&42), Some(vec![100, 200]));
        assert_eq!(store.take_entry_features(&42), None);
    }

    #[test]
    fn duplicate_entry_key_is_ignored() {
        let store = Mock::new();
        assert!(store.store_entry_features(42, vec![100]));
        assert!(!store.store_entry_features(42, vec![200]));
    }

    #[test]
    fn concurrent_reads_do_not_deadlock() {
        use std::thread;

        let store = Mock::new();
        store.insert_posting(100, 1).unwrap();

        let store = std::sync::Arc::new(store);
        let mut handles = vec![];

        for _ in 0..4 {
            let s = store.clone();
            handles.push(thread::spawn(move || {
                let _inv = s.posting_list(100);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }
}
