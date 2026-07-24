use crate::index::error::IndexError;
use crate::index::store::{InvertedStorage, SketchStorage};
use crate::sketch::Sketch;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::RwLock;

/// In-memory storage backend backed by `RwLock`-protected maps.
pub struct IndexStorage<K, S: Sketch> {
    sketches: RwLock<HashMap<K, S>>,
    postings: RwLock<HashMap<S::Feature, HashSet<K>>>,
}

impl<K, S: Sketch> IndexStorage<K, S> {
    /// Creates an empty in-memory storage backend.
    pub fn new() -> Self {
        Self { sketches: RwLock::new(HashMap::new()), postings: RwLock::new(HashMap::new()) }
    }
}

impl<K, S> Default for IndexStorage<K, S>
where
    S: Sketch,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, S> SketchStorage<K, S> for IndexStorage<K, S>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
{
    fn get_sketch(&self, key: &K) -> Result<Option<S>, IndexError> {
        let sketches = self
            .sketches
            .read()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(sketches.get(key).cloned())
    }

    fn put_sketch(&self, key: K, sketch: S) -> Result<Option<S>, IndexError> {
        let mut sketches = self
            .sketches
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(sketches.insert(key, sketch))
    }

    fn remove_sketch(&self, key: &K) -> Result<Option<S>, IndexError> {
        let mut sketches = self
            .sketches
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(sketches.remove(key))
    }

    fn len_sketches(&self) -> Result<usize, IndexError> {
        let sketches = self
            .sketches
            .read()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(sketches.len())
    }

    fn clear_sketches(&self) -> Result<(), IndexError> {
        let mut sketches = self
            .sketches
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        sketches.clear();
        Ok(())
    }
}

impl<K, S> InvertedStorage<K, S::Feature> for IndexStorage<K, S>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
{
    fn posting_list(&self, feature: S::Feature) -> Result<Vec<K>, IndexError> {
        let postings = self
            .postings
            .read()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(postings.get(&feature).map(|keys| keys.iter().cloned().collect()).unwrap_or_default())
    }

    fn insert_posting(&self, feature: S::Feature, key: K) -> Result<(), IndexError> {
        let mut postings = self
            .postings
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        postings.entry(feature).or_default().insert(key);
        Ok(())
    }

    fn remove_posting(&self, feature: S::Feature, key: &K) -> Result<(), IndexError> {
        let mut postings = self
            .postings
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;

        if let Some(keys) = postings.get_mut(&feature) {
            keys.remove(key);
            if keys.is_empty() {
                postings.remove(&feature);
            }
        }
        Ok(())
    }

    fn len_postings(&self) -> Result<usize, IndexError> {
        let postings = self
            .postings
            .read()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(postings.len())
    }

    fn clear_postings(&self) -> Result<(), IndexError> {
        let mut postings = self
            .postings
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        postings.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::U32Sketch;
    use std::thread;

    type Mock = IndexStorage<u64, U32Sketch<6>>;

    fn make_sketch(vals: [u32; 6]) -> U32Sketch<6> {
        U32Sketch::new(vals).unwrap()
    }

    #[test]
    fn put_and_get_sketch_roundtrip() {
        let store = Mock::new();
        let sk = make_sketch([1, 2, 3, 4, 5, 6]);

        assert_eq!(store.put_sketch(42, sk).unwrap(), None);
        assert_eq!(store.get_sketch(&42).unwrap(), Some(sk));
    }

    #[test]
    fn put_sketch_returns_previous_on_overwrite() {
        let store = Mock::new();
        let sk1 = make_sketch([1, 2, 3, 4, 5, 6]);
        let sk2 = make_sketch([10, 20, 30, 40, 50, 60]);

        store.put_sketch(1, sk1).unwrap();
        assert_eq!(store.put_sketch(1, sk2).unwrap(), Some(sk1));
        assert_eq!(store.len_sketches().unwrap(), 1);
    }

    #[test]
    fn remove_sketch_returns_removed_value() {
        let store = Mock::new();
        let sk = make_sketch([1, 2, 3, 4, 5, 6]);

        store.put_sketch(1, sk).unwrap();
        assert_eq!(store.remove_sketch(&1).unwrap(), Some(sk));
        assert_eq!(store.get_sketch(&1).unwrap(), None);
    }

    #[test]
    fn insert_posting_is_idempotent() {
        let store = Mock::new();

        store.insert_posting(100, 42).unwrap();
        store.insert_posting(100, 42).unwrap();

        assert_eq!(store.posting_list(100).unwrap(), vec![42]);
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
    fn clear_removes_all_data() {
        let store = Mock::new();

        store.put_sketch(1, make_sketch([1, 2, 3, 4, 5, 6])).unwrap();
        store.insert_posting(1, 1).unwrap();
        store.clear_sketches().unwrap();
        store.clear_postings().unwrap();

        assert_eq!(store.len_sketches().unwrap(), 0);
        assert_eq!(store.len_postings().unwrap(), 0);
    }

    #[test]
    fn concurrent_reads_do_not_deadlock() {
        let store = Mock::new();
        let sk = make_sketch([1, 2, 3, 4, 5, 6]);

        store.put_sketch(1, sk).unwrap();
        store.insert_posting(100, 1).unwrap();

        let store = std::sync::Arc::new(store);
        let mut handles = vec![];

        for _ in 0..4 {
            let s = store.clone();
            handles.push(thread::spawn(move || {
                let _sk = s.get_sketch(&1);
                let _inv = s.posting_list(100);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }
}
