use crate::index::error::IndexError;
use crate::index::store::InvertedStorage;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// In-memory posting-list storage backed by `RwLock`-protected maps.
pub struct IndexStorage<K, F> {
    postings: RwLock<HashMap<F, HashSet<K>>>,
}

impl<K, F> IndexStorage<K, F> {
    pub fn new() -> Self {
        Self { postings: RwLock::new(HashMap::new()) }
    }

    fn postings_read(&self) -> RwLockReadGuard<'_, HashMap<F, HashSet<K>>> {
        self.postings.read().expect("postings lock poisoned")
    }

    fn postings_write(&self) -> RwLockWriteGuard<'_, HashMap<F, HashSet<K>>> {
        self.postings.write().expect("postings lock poisoned")
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
        Ok(())
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
