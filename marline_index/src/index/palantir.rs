use crate::index::store::{Store, Tier};
use crate::index::{IndexError, SketchKVindex};
use crate::sketch::SimilarityScore;
use crate::sketch::Sketch;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

#[allow(dead_code)]
pub trait SketchKVPalantir<K, S: Send + Sync + Sketch>: Send + Sync + SketchKVindex<K, S>
where
    K: Clone + Eq + Hash + Send + Sync,
{
    fn get_db(&self) -> Result<Option<S>, Self::Error>;

    fn put_db(&self, key: &K) -> Result<(), Self::Error>;

    fn top_k_db(&self, key: &K) -> Result<Vec<(K, f64)>, Self::Error>;
}

#[allow(dead_code)]
fn tier_for_sketch_size(len: usize) -> Result<Tier, IndexError> {
    match len {
        3 => Ok(Tier::One),
        4 => Ok(Tier::Two),
        6 => Ok(Tier::Three),
        _ => Err(IndexError::InvalidSearchOptions),
    }
}

#[allow(dead_code)]
pub struct PalantirIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    store: ST,

    _phantom: PhantomData<(K, S)>,
}

impl<K, S, ST> PalantirIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    #[allow(dead_code)]
    pub fn new(store: ST) -> Self {
        Self { store, _phantom: PhantomData }
    }
}

impl<K, S, ST> SketchKVindex<K, S> for PalantirIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    type Error = IndexError;

    fn len(&self) -> Result<usize, Self::Error> {
        self.store.len_sketches()
    }

    fn get(&self, key: &S) -> Result<Option<K>, Self::Error> {
        let most_simmilar = self.top_k(key, 1)?;
        if most_simmilar.len() == 0 {
            return Ok(None);
        } else {
            return Ok(Some(most_simmilar[0].0.clone()));
        }
    }

    fn put(&self, key: &K, sketch: S) -> Result<(), Self::Error> {
        self.store.put_sketch(key, &sketch)?;

        let tier = tier_for_sketch_size(sketch.len())?;
        for sf in sketch.iter() {
            self.store.put_inverted(tier, sf, key)?;
        }
        Ok(())
    }

    fn remove(&self, _key: &K) -> Result<(), Self::Error> {
        todo!("remove requires store to implement remove_sketch")
    }

    fn top_k(&self, query: &S, k: usize) -> Result<Vec<(K, f64)>, Self::Error> {
        let tier = tier_for_sketch_size(query.len())?;
        let mut counts: HashMap<K, usize> = HashMap::new();

        for sf in query.iter() {
            let hashes = self.store.get_inverted(tier, sf)?;
            for h in hashes {
                *counts.entry(h).or_default() += 1;
            }
        }

        let query_len = query.len();
        let mut scored: Vec<(K, f64)> = counts
            .into_iter()
            .map(|(h, overlap)| {
                let cand = self.store.get_sketch(&h).unwrap().unwrap();
                let score = SimilarityScore::new_from_two(overlap, query_len, cand.len());
                (h, score.jaccard_similarity())
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);

        Ok(scored)
    }

    fn clear(&self) -> Result<(), Self::Error> {
        todo!("clear requires store to implement clear")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::store::index_storage::IndexStorage;
    use crate::sketch::FixedSketch;
    use std::sync::Arc;
    use std::thread;

    fn mk(vals: [u32; 6]) -> FixedSketch<6> {
        FixedSketch::new(vals).unwrap()
    }

    fn idx() -> PalantirIndex<u64, FixedSketch<6>, IndexStorage<u64, FixedSketch<6>>> {
        PalantirIndex::new(IndexStorage::new())
    }

    // --- get ---

    #[test]
    fn get_returns_closest_matching_hash() {
        let pl = idx();
        pl.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        pl.put(&2, mk([1, 2, 3, 7, 8, 9])).unwrap();
        // query shares 4 sfs with hash=1, 3 sfs with hash=2
        assert_eq!(pl.get(&mk([1, 2, 3, 4, 11, 12])).unwrap(), Some(1));
    }

    #[test]
    fn get_returns_none_on_empty_index() {
        let pl = idx();
        assert_eq!(pl.get(&mk([1, 2, 3, 4, 5, 6])).unwrap(), None);
    }

    // --- len ---

    #[test]
    fn len_increases_on_put() {
        let pl = idx();
        pl.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        pl.put(&2, mk([7, 8, 9, 10, 11, 12])).unwrap();
        assert_eq!(pl.len().unwrap(), 2);
    }

    // --- top_k ---

    #[test]
    fn top_k_self_match_has_perfect_score() {
        let pl = idx();
        let sk = mk([1, 2, 3, 4, 5, 6]);
        pl.put(&1, sk).unwrap();
        let r = pl.top_k(&sk, 5).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, 1);
        assert!((r[0].1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn top_k_orders_by_similarity() {
        let pl = idx();
        pl.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        pl.put(&2, mk([1, 2, 3, 7, 8, 9])).unwrap();
        pl.put(&3, mk([1, 2, 11, 12, 13, 14])).unwrap();

        let r = pl.top_k(&mk([1, 2, 3, 4, 5, 6]), 3).unwrap();
        assert_eq!(r.len(), 3);
        assert!((r[0].1 - 1.0).abs() < 1e-6);
        assert!((r[1].1 - 3.0 / 9.0).abs() < 1e-6);
        assert!((r[2].1 - 2.0 / 10.0).abs() < 1e-6);
    }

    #[test]
    fn top_k_returns_all_when_less_than_k() {
        let pl = idx();
        let sk = mk([1, 2, 3, 4, 5, 6]);
        pl.put(&42, sk).unwrap();
        assert_eq!(pl.top_k(&sk, 100).unwrap().len(), 1);
    }

    #[test]
    fn top_k_empty_index_returns_empty() {
        let pl = idx();
        assert!(pl.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap().is_empty());
    }

    #[test]
    fn top_k_no_overlap_returns_only_self() {
        let pl = idx();
        pl.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        pl.put(&2, mk([7, 8, 9, 10, 11, 12])).unwrap();
        let r = pl.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, 1);
    }

    // --- concurrency ---

    #[test]
    fn concurrent_gets_do_not_deadlock() {
        let pl = Arc::new(idx());
        pl.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();

        let mut handles = vec![];
        for _ in 0..4 {
            let p = pl.clone();
            handles.push(thread::spawn(move || {
                let _ = p.get(&mk([1, 2, 3, 4, 5, 6]));
                let _ = p.len();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
