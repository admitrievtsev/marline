use crate::types::{BlockID, SuperFeature};
use marline_storage::ChunkHash;
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
