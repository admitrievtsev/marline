use crate::types::{BlockID, SuperFeature};
use chunkfs::ChunkHash;
use marline_index::heuristic_index::{HeuristicInvertedSketchIndex, SearchConfig};
use marline_index::index::metrics::{Metric, MetricStorage, MetricsMap};
use marline_index::index::store::{CandidateVerification, IndexStorage, InvertedStorage, Store};
use marline_index::index::IndexError;
use marline_index::index::SketchIndexApi;
use marline_index::sketch::Sketch;
use marline_index::sketch::U32Sketch;
use std::hash::Hash;
use std::sync::Arc;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

struct SharedStore<K, F> {
    inner: Arc<IndexStorage<K, F>>,
    #[cfg(test)]
    calls: Arc<SharedStoreCalls>,
}

#[cfg(test)]
#[derive(Default)]
struct SharedStoreCalls {
    posting_list_calls: AtomicUsize,
    posting_len_calls: AtomicUsize,
    accumulation_calls: AtomicUsize,
    verification_calls: AtomicUsize,
    visited_records: AtomicUsize,
}

impl<K, F> SharedStore<K, F> {
    fn new() -> Self {
        Self {
            inner: Arc::new(IndexStorage::new()),
            #[cfg(test)]
            calls: Arc::new(SharedStoreCalls::default()),
        }
    }

    fn shared(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            #[cfg(test)]
            calls: Arc::clone(&self.calls),
        }
    }
}

impl<K, F> InvertedStorage<K, F> for SharedStore<K, F>
where
    K: Clone + Eq + Hash + Send + Sync,
    F: Copy + Eq + Hash + Send + Sync,
{
    fn posting_list(&self, feature: F) -> Result<Vec<K>, IndexError> {
        #[cfg(test)]
        self.calls.posting_list_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.posting_list(feature)
    }
    fn posting_len(&self, feature: F) -> Result<usize, IndexError> {
        #[cfg(test)]
        self.calls.posting_len_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.posting_len(feature)
    }
    fn accumulate_posting(
        &self,
        feature: F,
        candidates: &mut std::collections::HashMap<K, usize>,
    ) -> Result<usize, IndexError> {
        let visited = self.inner.accumulate_posting(feature, candidates)?;
        #[cfg(test)]
        {
            self.calls.accumulation_calls.fetch_add(1, Ordering::Relaxed);
            self.calls.visited_records.fetch_add(visited, Ordering::Relaxed);
        }
        Ok(visited)
    }
    fn verify_candidates(
        &self,
        feature: F,
        candidates: &mut std::collections::HashMap<K, usize>,
    ) -> Result<CandidateVerification, IndexError> {
        let work = self.inner.verify_candidates(feature, candidates)?;
        #[cfg(test)]
        {
            self.calls.verification_calls.fetch_add(1, Ordering::Relaxed);
            self.calls
                .visited_records
                .fetch_add(work.posting_visits + work.candidate_probes, Ordering::Relaxed);
        }
        Ok(work)
    }
    fn insert_posting(&self, feature: F, key: K) -> Result<(), IndexError> {
        self.inner.insert_posting(feature, key)
    }
    fn remove_posting(&self, feature: F, key: &K) -> Result<(), IndexError> {
        self.inner.remove_posting(feature, key)
    }
    fn len_postings(&self) -> Result<usize, IndexError> {
        self.inner.len_postings()
    }
    fn clear_postings(&self) -> Result<(), IndexError> {
        self.inner.clear_postings()
    }
}

impl<K, F, S> Store<K, S> for SharedStore<K, F>
where
    K: Clone + Eq + Hash + Send + Sync,
    F: Copy + Eq + Hash + Send + Sync,
    S: Sketch<Feature = F>,
{
    fn insert_entry(&self, key: K, sketch: S) -> Result<(), IndexError> {
        self.inner.insert_entry(key, sketch)
    }

    fn remove_entry(&self, key: &K) -> Result<(), IndexError> {
        <IndexStorage<K, F> as Store<K, S>>::remove_entry(&self.inner, key)
    }

    fn clear(&self) -> Result<(), IndexError> {
        <IndexStorage<K, F> as Store<K, S>>::clear(&self.inner)
    }
}

pub struct SFTable<H: ChunkHash + Send + Sync, const N: usize> {
    index: HeuristicInvertedSketchIndex<BlockID<H>, U32Sketch<N>, SharedStore<BlockID<H>, u32>>,
    store: SharedStore<BlockID<H>, u32>,
    metrics: MetricsMap<BlockID<H>>,
}

impl<H: ChunkHash + Send + Sync, const N: usize> SFTable<H, N> {
    pub fn new(config: SearchConfig) -> Self {
        let store = SharedStore::new();
        Self {
            index: HeuristicInvertedSketchIndex::with_config(store.shared(), config),
            store,
            metrics: MetricsMap::new(),
        }
    }

    pub fn insert(&self, block_id: &BlockID<H>, features: &[SuperFeature], metric: Metric) {
        let vals: [u32; N] = features
            .iter()
            .map(SuperFeature::value)
            .collect::<Vec<_>>()
            .try_into()
            .expect("features length must match table tier width N");
        self.index.put(block_id, U32Sketch::new(vals)).expect("index put failed");
        self.metrics.set_metric(block_id, metric);
    }

    pub fn remove_block(&self, block_id: &BlockID<H>) {
        let _ = self.index.remove(block_id);
        self.metrics.remove_metric(block_id);
    }

    pub fn remove_sf(&self, sf: &SuperFeature) {
        let value = sf.value();
        let keys: Vec<BlockID<H>> = self.store.posting_list(value).unwrap_or_default();
        for key in keys {
            let _ = self.index.remove(&key);
            self.metrics.remove_metric(&key);
        }
    }

    pub fn len(&self) -> usize {
        self.store.len_postings().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.store.len_postings().map_or(true, |n| n == 0)
    }

    pub fn nearest(&self, features: &[SuperFeature]) -> Option<BlockID<H>> {
        let vals: [u32; N] =
            features.iter().map(SuperFeature::value).collect::<Vec<_>>().try_into().ok()?;
        let query = U32Sketch::new(vals);
        self.index.get(&query).unwrap_or(None)
    }

    pub fn get_key_metric(&self, block_id: &BlockID<H>) -> Option<Metric> {
        self.metrics.get_metric(block_id)
    }

    pub fn set_key_metric(&self, block_id: &BlockID<H>, value: Metric) {
        self.metrics.set_metric(block_id, value);
    }

    pub fn get_with_upd_metric(
        &self,
        features: &[SuperFeature],
        f: impl FnOnce(Metric) -> Metric,
    ) -> Option<BlockID<H>> {
        let vals: [u32; N] =
            features.iter().map(SuperFeature::value).collect::<Vec<_>>().try_into().ok()?;
        let query = U32Sketch::new(vals);
        let result = self.index.get(&query).unwrap_or(None)?;
        let old = self.metrics.get_metric(&result).unwrap_or(0);
        self.metrics.set_metric(&result, f(old));
        Some(result)
    }

    pub fn update_and_clean(
        &self,
        mut update_fn: impl FnMut(Metric) -> Metric,
        cleanup_fn: impl Fn(Metric) -> bool,
    ) -> Result<(), IndexError> {
        self.metrics.update_and_clean(&mut |m| *m = update_fn(*m), &|m| cleanup_fn(m));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marline_index::index::SketchIndexApi;
    use std::sync::atomic::Ordering;

    fn features(values: [u32; 6]) -> Vec<SuperFeature> {
        values.into_iter().map(|value| SuperFeature::new(0, value)).collect()
    }

    #[test]
    fn rare_first_search_uses_fast_store_ops_and_stops_before_full_suffix() {
        let table = SFTable::<[u8; 32], 6>::new(SearchConfig {
            min_jaccard: Some(0.5),
            rare_first: true,
            ..SearchConfig::default()
        });
        let query = features([1, 2, 3, 4, 5, 6]);
        let best = BlockID::new([1; 32]);
        let runner_up = BlockID::new([2; 32]);
        table.insert(&best, &features([1, 2, 3, 4, 200, 201]), 0);
        table.insert(&runner_up, &features([1, 300, 301, 302, 303, 304]), 0);

        for id in 0..2u8 {
            let key = BlockID::new([10 + id; 32]);
            table.insert(&key, &features([4, 400 + id as u32, 500, 600, 700, 800]), 0);
        }
        for id in 0..100u8 {
            let key = BlockID::new([20u8.wrapping_add(id); 32]);
            table.insert(&key, &features([5, 6, 900 + id as u32, 1000, 1100, 1200]), 0);
        }

        // The exact top-k path provides the reference answer. Reset counters so
        // the following call measures only the early-exit top-1 path.
        let vals: [u32; 6] =
            query.iter().map(SuperFeature::value).collect::<Vec<_>>().try_into().unwrap();
        let query_sketch = U32Sketch::new(vals);
        let expected = table.index.top_k(&query_sketch, 1).unwrap()[0].0.clone();
        table.store.calls.posting_list_calls.store(0, Ordering::Relaxed);
        table.store.calls.posting_len_calls.store(0, Ordering::Relaxed);
        table.store.calls.accumulation_calls.store(0, Ordering::Relaxed);
        table.store.calls.verification_calls.store(0, Ordering::Relaxed);
        table.store.calls.visited_records.store(0, Ordering::Relaxed);

        assert_eq!(table.nearest(&query), Some(expected.clone()));
        assert_eq!(expected, best);
        assert_eq!(table.store.calls.posting_list_calls.load(Ordering::Relaxed), 0);
        assert_eq!(table.store.calls.posting_len_calls.load(Ordering::Relaxed), 6);
        assert_eq!(table.store.calls.accumulation_calls.load(Ordering::Relaxed), 3);
        assert_eq!(table.store.calls.verification_calls.load(Ordering::Relaxed), 1);
        assert!(table.store.calls.visited_records.load(Ordering::Relaxed) < 100);
    }
}
