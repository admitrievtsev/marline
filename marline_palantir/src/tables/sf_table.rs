use crate::types::{BlockID, SuperFeature};
use chunkfs::ChunkHash;
use marline_index::heuristic_index::{HeuristicInvertedSketchIndex, SearchConfig};
use marline_index::index::metrics::{Metric, MetricStorage, MetricsMap};
use marline_index::index::store::{IndexStorage, InvertedStorage, Store};
use marline_index::index::IndexError;
use marline_index::index::SketchIndexApi;
use marline_index::sketch::Sketch;
use marline_index::sketch::U32Sketch;
use std::hash::Hash;
use std::sync::Arc;

struct SharedStore<K, F>(Arc<IndexStorage<K, F>>);

impl<K, F> InvertedStorage<K, F> for SharedStore<K, F>
where
    K: Clone + Eq + Hash + Send + Sync,
    F: Copy + Eq + Hash + Send + Sync,
{
    fn posting_list(&self, feature: F) -> Result<Vec<K>, IndexError> {
        self.0.posting_list(feature)
    }
    fn insert_posting(&self, feature: F, key: K) -> Result<(), IndexError> {
        self.0.insert_posting(feature, key)
    }
    fn remove_posting(&self, feature: F, key: &K) -> Result<(), IndexError> {
        self.0.remove_posting(feature, key)
    }
    fn len_postings(&self) -> Result<usize, IndexError> {
        self.0.len_postings()
    }
    fn clear_postings(&self) -> Result<(), IndexError> {
        self.0.clear_postings()
    }
}

impl<K, F, S> Store<K, S> for SharedStore<K, F>
where
    K: Clone + Eq + Hash + Send + Sync,
    F: Copy + Eq + Hash + Send + Sync,
    S: Sketch<Feature = F>,
{
    fn insert_entry(&self, key: K, sketch: S) -> Result<(), IndexError> {
        self.0.insert_entry(key, sketch)
    }

    fn remove_entry(&self, key: &K) -> Result<(), IndexError> {
        <IndexStorage<K, F> as Store<K, S>>::remove_entry(&self.0, key)
    }

    fn clear(&self) -> Result<(), IndexError> {
        <IndexStorage<K, F> as Store<K, S>>::clear(&self.0)
    }
}

pub struct SFTable<H: ChunkHash + Send + Sync, const N: usize> {
    index: HeuristicInvertedSketchIndex<BlockID<H>, U32Sketch<N>, SharedStore<BlockID<H>, u32>>,
    store: SharedStore<BlockID<H>, u32>,
    metrics: MetricsMap<BlockID<H>>,
}

impl<H: ChunkHash + Send + Sync, const N: usize> SFTable<H, N> {
    pub fn new(config: SearchConfig) -> Self {
        let store = SharedStore(Arc::new(IndexStorage::new()));
        Self {
            index: HeuristicInvertedSketchIndex::with_config(
                SharedStore(Arc::clone(&store.0)),
                config,
            ),
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
        let old = self.metrics.get_metric(&result)?;
        self.metrics.set_metric(&result, f(old));
        Some(result)
    }

    /// Updates every stored metric and drops the blocks whose updated metric
    /// satisfies `cleanup_fn`.
    ///
    /// Returns the number of blocks evicted from the index.
    pub fn update_and_clean(
        &self,
        mut update_fn: impl FnMut(Metric) -> Metric,
        cleanup_fn: impl Fn(Metric) -> bool,
    ) -> Result<usize, IndexError> {
        let removed =
            self.metrics.update_and_collect(&mut |m| *m = update_fn(*m), &|m| cleanup_fn(m));

        let mut count = 0;
        let mut first_err: Option<IndexError> = None;
        for block_id in &removed {
            match self.index.remove(block_id) {
                Ok(()) => {
                    self.metrics.remove_metric(block_id);
                    count += 1;
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }

        first_err.map_or(Ok(count), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestTable = SFTable<u64, 3>;

    fn block(hash: u64) -> BlockID<u64> {
        BlockID::new(hash)
    }

    fn features(values: [u32; 3]) -> [SuperFeature; 3] {
        [
            SuperFeature::new(0, values[0]),
            SuperFeature::new(1, values[1]),
            SuperFeature::new(2, values[2]),
        ]
    }

    #[test]
    fn update_and_clean_drops_entry_from_index() {
        let table = TestTable::new(SearchConfig::default());
        let a = block(1);
        let feats = features([10, 20, 30]);

        table.insert(&a, &feats, 1);

        assert_eq!(table.nearest(&feats), Some(a.clone()));
        assert_eq!(table.len(), 3);

        table.update_and_clean(|m| m.saturating_sub(1), |m| m == 0).unwrap();

        assert_eq!(table.get_key_metric(&a), None);
        assert_eq!(table.nearest(&feats), None);
        assert_eq!(table.len(), 0);
        assert!(table.is_empty());
    }

    #[test]
    fn update_and_clean_keeps_surviving_entries() {
        let table = TestTable::new(SearchConfig::default());
        let a = block(1);
        let b = block(2);
        let feats_a = features([10, 20, 30]);
        let feats_b = features([40, 50, 60]);

        table.insert(&a, &feats_a, 1);
        table.insert(&b, &feats_b, 5);

        table.update_and_clean(|m| m.saturating_sub(1), |m| m == 0).unwrap();

        assert_eq!(table.get_key_metric(&a), None);
        assert_eq!(table.nearest(&feats_a), None);

        assert_eq!(table.get_key_metric(&b), Some(4));
        assert_eq!(table.nearest(&feats_b), Some(b));
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn expired_metric_is_not_resurrected_on_lookup() {
        let table = TestTable::new(SearchConfig::default());
        let a = block(1);
        let feats = features([10, 20, 30]);

        table.insert(&a, &feats, 1);
        table.update_and_clean(|m| m.saturating_sub(1), |m| m == 0).unwrap();

        assert_eq!(table.get_with_upd_metric(&feats, |m| m), None);
        assert_eq!(table.get_key_metric(&a), None);
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn repeated_cleanup_does_not_underflow() {
        let table = TestTable::new(SearchConfig::default());
        let a = block(1);
        let feats = features([10, 20, 30]);

        table.insert(&a, &feats, 2);

        for _ in 0..5 {
            table.update_and_clean(|m| m - 1, |m| m == 0).unwrap();
        }

        assert_eq!(table.get_key_metric(&a), None);
        assert_eq!(table.nearest(&feats), None);
    }
}
