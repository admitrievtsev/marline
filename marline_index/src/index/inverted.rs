use crate::index::store::Store;
use crate::index::{IndexError, SketchIndexApi};
use crate::sketch::{SimilarityScore, Sketch};
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

/// Generic inverted index for similarity search over sketches.
pub struct InvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    store: ST,
    _phantom: PhantomData<(K, S)>,
}

impl<K, S, ST> InvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    /// Creates a new index over the given storage backend.
    pub fn new(store: ST) -> Self {
        Self { store, _phantom: PhantomData }
    }

    /// Returns the underlying storage backend.
    pub fn into_store(self) -> ST {
        self.store
    }
}

impl<K, S, ST> SketchIndexApi<K, S> for InvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    type Error = IndexError;

    fn len(&self) -> Result<usize, Self::Error> {
        self.store.len_sketches()
    }

    fn is_empty(&self) -> Result<bool, Self::Error> {
        Ok(self.len()? == 0)
    }

    fn lookup(&self, key: &K) -> Result<Option<S>, Self::Error> {
        self.store.get_sketch(key)
    }

    fn get(&self, query: &S) -> Result<Option<K>, Self::Error> {
        Ok(self.top_k(query, 1)?.into_iter().next().map(|result| result.0))
    }

    fn put(&self, key: &K, sketch: S) -> Result<(), Self::Error> {
        self.store.insert_entry(key.clone(), sketch)?;
        Ok(())
    }

    fn remove(&self, key: &K) -> Result<(), Self::Error> {
        self.store.remove_entry(key)?;
        Ok(())
    }

    fn top_k(&self, query: &S, k: usize) -> Result<Vec<(K, f64)>, Self::Error> {
        if k == 0 {
            return Ok(Vec::new());
        }

        let mut candidates: HashMap<K, usize> = HashMap::new();
        for feature in query.iter() {
            for key in self.store.posting_list(feature)? {
                *candidates.entry(key).or_default() += 1;
            }
        }

        let mut scored = Vec::with_capacity(candidates.len());
        for (key, _posting_overlap) in candidates {
            let sketch = self.store.get_sketch(&key)?.ok_or_else(|| {
                IndexError::InconsistentStorage(String::from(
                    "posting list references a missing sketch",
                ))
            })?;
            let intersection = query.intersection_size(&sketch);
            let score = SimilarityScore::new_from_two(intersection, query.len(), sketch.len());
            scored.push((key, score.jaccard()));
        }

        scored.sort_by(|left, right| {
            right.1.partial_cmp(&left.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);

        Ok(scored)
    }

    fn clear(&self) -> Result<(), Self::Error> {
        self.store.clear()
    }
}

/// Deprecated compatibility alias for the previous Palantir-specific name.
#[deprecated(note = "use InvertedSketchIndex instead")]
pub type PalantirIndex<K, S, ST> = InvertedSketchIndex<K, S, ST>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::store::IndexStorage;
    use crate::sketch::{U32Sketch, U64Sketch};
    use std::sync::Arc;
    use std::thread;

    fn mk(vals: [u32; 6]) -> U32Sketch<6> {
        U32Sketch::new(vals).unwrap()
    }

    fn idx() -> InvertedSketchIndex<u64, U32Sketch<6>, IndexStorage<u64, U32Sketch<6>>> {
        InvertedSketchIndex::new(IndexStorage::new())
    }

    #[test]
    fn get_returns_closest_matching_key() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([1, 2, 3, 7, 8, 9])).unwrap();

        assert_eq!(index.get(&mk([1, 2, 3, 4, 11, 12])).unwrap(), Some(1));
    }

    #[test]
    fn get_returns_none_on_empty_index() {
        let index = idx();
        assert_eq!(index.get(&mk([1, 2, 3, 4, 5, 6])).unwrap(), None);
    }

    #[test]
    fn len_and_is_empty_track_puts() {
        let index = idx();
        assert!(index.is_empty().unwrap());
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([7, 8, 9, 10, 11, 12])).unwrap();

        assert_eq!(index.len().unwrap(), 2);
        assert!(!index.is_empty().unwrap());
    }

    #[test]
    fn lookup_returns_stored_sketch() {
        let index = idx();
        let sketch = mk([1, 2, 3, 4, 5, 6]);
        index.put(&1, sketch).unwrap();

        assert_eq!(index.lookup(&1).unwrap(), Some(sketch));
    }

    #[test]
    fn top_k_self_match_has_perfect_score() {
        let index = idx();
        let sketch = mk([1, 2, 3, 4, 5, 6]);
        index.put(&1, sketch).unwrap();

        let results = index.top_k(&sketch, 5).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn top_k_orders_by_similarity() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([1, 2, 3, 7, 8, 9])).unwrap();
        index.put(&3, mk([1, 2, 11, 12, 13, 14])).unwrap();

        let results = index.top_k(&mk([1, 2, 3, 4, 5, 6]), 3).unwrap();

        assert_eq!(results.len(), 3);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
        assert!((results[1].1 - 3.0 / 9.0).abs() < 1e-6);
        assert!((results[2].1 - 2.0 / 10.0).abs() < 1e-6);
    }

    #[test]
    fn top_k_returns_all_when_less_than_k() {
        let index = idx();
        let sketch = mk([1, 2, 3, 4, 5, 6]);
        index.put(&42, sketch).unwrap();

        assert_eq!(index.top_k(&sketch, 100).unwrap().len(), 1);
    }

    #[test]
    fn top_k_with_zero_k_returns_empty() {
        let index = idx();
        index.put(&42, mk([1, 2, 3, 4, 5, 6])).unwrap();

        assert!(index.top_k(&mk([1, 2, 3, 4, 5, 6]), 0).unwrap().is_empty());
    }

    #[test]
    fn top_k_empty_index_returns_empty() {
        let index = idx();
        assert!(index.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap().is_empty());
    }

    #[test]
    fn top_k_no_overlap_returns_only_candidates_with_overlap() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([7, 8, 9, 10, 11, 12])).unwrap();

        let results = index.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1);
    }

    #[test]
    fn put_overwrite_removes_old_postings() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&1, mk([10, 11, 12, 13, 14, 15])).unwrap();

        assert!(index.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap().is_empty());
        assert_eq!(index.get(&mk([10, 11, 12, 13, 14, 15])).unwrap(), Some(1));
        assert_eq!(index.len().unwrap(), 1);
    }

    #[test]
    fn repeated_put_same_key_does_not_duplicate_candidates() {
        let index = idx();
        let sketch = mk([1, 2, 3, 4, 5, 6]);

        index.put(&1, sketch).unwrap();
        index.put(&1, sketch).unwrap();

        let results = index.top_k(&sketch, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1);
    }

    #[test]
    fn remove_deletes_sketch_and_postings() {
        let index = idx();
        let sketch = mk([1, 2, 3, 4, 5, 6]);
        index.put(&1, sketch).unwrap();

        index.remove(&1).unwrap();

        assert_eq!(index.lookup(&1).unwrap(), None);
        assert!(index.top_k(&sketch, 5).unwrap().is_empty());
        assert_eq!(index.len().unwrap(), 0);
    }

    #[test]
    fn remove_missing_key_is_noop() {
        let index = idx();
        index.remove(&1).unwrap();
    }

    #[test]
    fn clear_removes_all_entries() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([7, 8, 9, 10, 11, 12])).unwrap();

        index.clear().unwrap();

        assert_eq!(index.len().unwrap(), 0);
        assert!(index.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap().is_empty());
    }

    #[test]
    fn supports_u64_features() {
        let index: InvertedSketchIndex<u64, U64Sketch<3>, IndexStorage<u64, U64Sketch<3>>> =
            InvertedSketchIndex::new(IndexStorage::new());
        let sketch = U64Sketch::<3>::new([10, 20, 30]).unwrap();

        index.put(&1, sketch).unwrap();

        assert_eq!(index.get(&sketch).unwrap(), Some(1));
    }

    #[test]
    fn concurrent_gets_do_not_deadlock() {
        let index = Arc::new(idx());
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();

        let mut handles = vec![];
        for _ in 0..4 {
            let index = index.clone();
            handles.push(thread::spawn(move || {
                let _ = index.get(&mk([1, 2, 3, 4, 5, 6]));
                let _ = index.len();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    }
}
