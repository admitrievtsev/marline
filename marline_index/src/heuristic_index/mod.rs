pub use heuristics::{min_overlap_for_jaccard, SearchConfig};

use crate::index::metrics::{Metric, MetricStorage, MetricsMap};
use crate::index::store::Store;
use crate::index::{IndexError, SketchIndexApi};
use crate::sketch::{SimilarityScore, Sketch};
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::RwLock;

mod heuristics;

/// API for heuristic-guided index operations.
///
/// Provides configuration and false-positive feedback on top of [`SketchIndexApi`].
pub trait HeuristicIndexApi<K, S>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    Self: SketchIndexApi<K, S>,
{
    /// Replaces the active search configuration.
    fn set_config(&self, config: SearchConfig);

    /// Returns a clone of the active search configuration.
    fn config(&self) -> SearchConfig;

    /// Increments the false-positive counter for `key` (saturating at `u8::MAX`).
    fn record_false_positive(&self, key: &K);

    /// Clears the false-positive counter for `key` after a successful verify.
    fn record_verified(&self, key: &K);

    /// Returns the current false-positive metric for `key`, if any.
    fn get_metric(&self, key: &K) -> Option<Metric>;
}

/// Inverted index with heuristic-guided search.
pub struct HeuristicInvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    store: ST,
    config: RwLock<SearchConfig>,
    metrics: MetricsMap<K>,
    _phantom: PhantomData<(K, S)>,
}

impl<K, S, ST> HeuristicInvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    pub fn new(store: ST) -> Self {
        Self::with_config(store, SearchConfig::default())
    }

    pub fn with_config(store: ST, config: SearchConfig) -> Self {
        Self {
            store,
            config: RwLock::new(config),
            metrics: MetricsMap::new(),
            _phantom: PhantomData,
        }
    }

    pub fn into_store(self) -> ST {
        self.store
    }

    /// Exact top-1 search that uses a rare prefix and stops once the winner is proven.
    fn rare_first_best(
        &self,
        query: &S,
        config: &SearchConfig,
        min_overlap: usize,
    ) -> Result<Option<K>, IndexError> {
        let n = query.len();
        let mut features = Vec::with_capacity(n);
        for feature in query.iter() {
            let df = self.store.posting_len(feature)?;
            if config.max_df.is_some_and(|limit| df > limit) {
                continue;
            }
            features.push((feature, df));
        }
        if features.len() < min_overlap {
            return Ok(None);
        }

        let min_df = features.iter().map(|(_, df)| *df).min().unwrap_or(0);
        let max_df = features.iter().map(|(_, df)| *df).max().unwrap_or(0);
        let mut prefix_len = features.len();
        if max_df >= min_df.saturating_mul(2) {
            features.sort_by_key(|&(_, df)| df);
            let proposed_prefix_len = features.len() - min_overlap + 1;
            let full_posting_work =
                features.iter().fold(0usize, |sum, (_, df)| sum.saturating_add(*df));
            let prefix_posting_work = features[..proposed_prefix_len]
                .iter()
                .fold(0usize, |sum, (_, df)| sum.saturating_add(*df));
            let estimated_suffix_work = features[proposed_prefix_len..]
                .iter()
                .fold(0usize, |sum, (_, df)| sum.saturating_add((*df).min(prefix_posting_work)));
            let estimated_prefix_work = prefix_posting_work.saturating_add(estimated_suffix_work);
            if estimated_prefix_work.saturating_mul(2) <= full_posting_work {
                prefix_len = proposed_prefix_len;
            }
        }

        let mut candidates = HashMap::new();
        for &(feature, _) in &features[..prefix_len] {
            self.store.accumulate_posting(feature, &mut candidates)?;
        }
        if let Some(threshold) = config.fp_metric_threshold {
            candidates.retain(|key, _| {
                self.metrics.get_metric(key).map(|metric| metric < threshold).unwrap_or(true)
            });
        }
        if candidates.is_empty() {
            return Ok(None);
        }

        let suffix_len = features.len() - prefix_len;
        for (suffix_index, &(feature, _)) in features[prefix_len..].iter().enumerate() {
            self.store.verify_candidates(feature, &mut candidates)?;
            let remaining = suffix_len - suffix_index - 1;
            candidates.retain(|_, overlap| overlap.saturating_add(remaining) >= min_overlap);

            if remaining > 0 {
                let best = candidates
                    .iter()
                    .filter(|(_, overlap)| **overlap >= min_overlap)
                    .max_by_key(|(_, overlap)| **overlap);
                if let Some((best_key, best_overlap)) = best {
                    let best_possible_competitor = candidates
                        .iter()
                        .filter(|(key, _)| *key != best_key)
                        .map(|(_, overlap)| overlap.saturating_add(remaining).min(n))
                        .max()
                        .unwrap_or(0);
                    if *best_overlap > best_possible_competitor {
                        return Ok(Some((*best_key).clone()));
                    }
                }
            }
            if candidates.is_empty() {
                return Ok(None);
            }
        }

        Ok(candidates
            .into_iter()
            .filter(|(_, overlap)| *overlap >= min_overlap)
            .max_by_key(|(_, overlap)| *overlap)
            .map(|(key, _)| key))
    }

    fn gather_candidates(
        &self,
        query: &S,
        config: &SearchConfig,
    ) -> Result<HashMap<K, usize>, IndexError> {
        let n = query.len();
        let mut features: Vec<(S::Feature, usize)> = Vec::with_capacity(n);

        for feature in query.iter() {
            let df = self.store.posting_list(feature)?.len();
            if let Some(max_df) = config.max_df {
                if df > max_df {
                    continue;
                }
            }
            features.push((feature, df));
        }

        if config.rare_first {
            features.sort_by_key(|&(_, df)| df);
        }

        let mut candidates: HashMap<K, usize> = HashMap::new();
        for (feature, _) in features {
            for key in self.store.posting_list(feature)? {
                *candidates.entry(key).or_default() += 1;
            }
        }

        Ok(candidates)
    }
}

impl<K, S, ST> HeuristicIndexApi<K, S> for HeuristicInvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    fn set_config(&self, config: SearchConfig) {
        *self.config.write().expect("config lock poisoned") = config;
    }

    fn config(&self) -> SearchConfig {
        self.config.read().expect("config lock poisoned").clone()
    }

    fn record_false_positive(&self, key: &K) {
        let current = self.metrics.get_metric(key).unwrap_or(0);
        self.metrics.set_metric(key, current.saturating_add(1));
    }

    fn record_verified(&self, key: &K) {
        self.metrics.remove_metric(key);
    }

    fn get_metric(&self, key: &K) -> Option<Metric> {
        self.metrics.get_metric(key)
    }
}

impl<K, S, ST> SketchIndexApi<K, S> for HeuristicInvertedSketchIndex<K, S, ST>
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<K, S>,
{
    type Error = IndexError;

    fn get(&self, query: &S) -> Result<Option<K>, Self::Error> {
        let config = self.config();
        let min_overlap = config
            .min_jaccard
            .map(|threshold| min_overlap_for_jaccard(threshold, query.len()))
            .unwrap_or(1);
        if config.rare_first && min_overlap > 1 {
            return self.rare_first_best(query, &config, min_overlap);
        }
        Ok(self.top_k(query, 1)?.into_iter().next().map(|result| result.0))
    }

    fn put(&self, key: &K, sketch: S) -> Result<(), Self::Error> {
        self.store.insert_entry(key.clone(), sketch)?;
        Ok(())
    }

    fn remove(&self, key: &K) -> Result<(), Self::Error> {
        self.store.remove_entry(key)?;
        self.metrics.remove_metric(key);
        Ok(())
    }

    fn top_k(&self, query: &S, k: usize) -> Result<Vec<(K, f64)>, Self::Error> {
        if k == 0 || query.is_empty() {
            return Ok(Vec::new());
        }

        let config = self.config();
        let n = query.len();
        let mut candidates = self.gather_candidates(query, &config)?;

        if let Some(threshold) = config.fp_metric_threshold {
            candidates.retain(|key, _| {
                self.metrics.get_metric(key).map(|m| m < threshold).unwrap_or(true)
            });
        }

        if let Some(t) = config.min_jaccard {
            let min_overlap = min_overlap_for_jaccard(t, n);
            candidates.retain(|_, &mut overlap| overlap >= min_overlap);
        }

        let mut scored: Vec<(K, f64)> = candidates
            .into_iter()
            .map(|(key, overlap)| {
                let score = SimilarityScore::new_from_two(overlap, n, n);
                (key, score.jaccard())
            })
            .collect();

        scored.sort_by(|left, right| {
            right.1.partial_cmp(&left.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);

        Ok(scored)
    }

    fn clear(&self) -> Result<(), Self::Error> {
        self.metrics.clear_all();
        self.store.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::store::IndexStorage;
    use crate::sketch::U32Sketch;

    fn idx() -> HeuristicInvertedSketchIndex<u64, U32Sketch<6>, IndexStorage<u64, u32>> {
        HeuristicInvertedSketchIndex::new(IndexStorage::new())
    }

    fn idx_with(
        config: SearchConfig,
    ) -> HeuristicInvertedSketchIndex<u64, U32Sketch<6>, IndexStorage<u64, u32>> {
        HeuristicInvertedSketchIndex::with_config(IndexStorage::new(), config)
    }

    fn mk(vals: [u32; 6]) -> U32Sketch<6> {
        U32Sketch::new(vals)
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
    fn remove_clears_postings_and_metrics() {
        let index = idx();
        let sketch = mk([1, 2, 3, 4, 5, 6]);
        index.put(&1, sketch).unwrap();
        index.record_false_positive(&1);
        assert_eq!(index.get_metric(&1), Some(1));

        index.remove(&1).unwrap();
        assert_eq!(index.get_metric(&1), None);
        assert!(index.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap().is_empty());
    }

    #[test]
    fn clear_removes_all_entries() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([7, 8, 9, 10, 11, 12])).unwrap();

        index.clear().unwrap();

        assert!(index.top_k(&mk([1, 2, 3, 4, 5, 6]), 5).unwrap().is_empty());
    }

    #[test]
    fn supports_u64_features() {
        use crate::sketch::U64Sketch;

        let index: HeuristicInvertedSketchIndex<u64, U64Sketch<3>, IndexStorage<u64, u64>> =
            HeuristicInvertedSketchIndex::new(IndexStorage::new());
        let sketch = U64Sketch::<3>::new([10, 20, 30]);

        index.put(&1, sketch).unwrap();

        assert_eq!(index.get(&sketch).unwrap(), Some(1));
    }

    #[test]
    fn concurrent_gets_do_not_deadlock() {
        use std::sync::Arc;
        use std::thread;

        let index = Arc::new(idx());
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();

        let mut handles = vec![];
        for _ in 0..4 {
            let index = index.clone();
            handles.push(thread::spawn(move || {
                let _ = index.get(&mk([1, 2, 3, 4, 5, 6]));
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    }

    // --- Heuristic tests ---

    #[test]
    fn max_df_skips_frequent_features_but_keeps_exact_match() {
        let index = idx_with(SearchConfig { max_df: Some(2), ..SearchConfig::default() });

        for key in 1..=5u64 {
            index
                .put(
                    &key,
                    mk([
                        1,
                        key as u32 * 10,
                        key as u32 * 10 + 1,
                        key as u32 * 10 + 2,
                        key as u32 * 10 + 3,
                        key as u32 * 10 + 4,
                    ]),
                )
                .unwrap();
        }
        index.put(&100, mk([1, 90, 91, 92, 93, 94])).unwrap();

        let results = index.top_k(&mk([1, 90, 91, 92, 93, 94]), 5).unwrap();
        assert!(results.iter().any(|(k, _)| *k == 100));
        assert!(!results.iter().any(|(k, _)| (1..=5).contains(k)));
    }

    #[test]
    fn min_jaccard_prunes_weak_candidates() {
        let index = idx_with(SearchConfig { min_jaccard: Some(0.5), ..SearchConfig::default() });

        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();
        index.put(&2, mk([1, 2, 11, 12, 13, 14])).unwrap();
        index.put(&3, mk([1, 2, 3, 15, 16, 17])).unwrap();
        index.put(&4, mk([1, 2, 3, 4, 18, 19])).unwrap();

        let results = index.top_k(&mk([1, 2, 3, 4, 5, 6]), 10).unwrap();
        let keys: Vec<u64> = results.iter().map(|(k, _)| *k).collect();

        assert!(keys.contains(&1));
        assert!(keys.contains(&4));
        assert!(!keys.contains(&2));
        assert!(!keys.contains(&3));
    }

    #[test]
    fn rare_first_preserves_exact_scores() {
        let index = idx_with(SearchConfig {
            rare_first: true,
            min_jaccard: Some(0.5),
            ..SearchConfig::default()
        });

        for key in 1..=8u64 {
            index
                .put(
                    &key,
                    mk([
                        1,
                        key as u32 * 10,
                        key as u32 * 10 + 1,
                        key as u32 * 10 + 2,
                        key as u32 * 10 + 3,
                        key as u32 * 10 + 4,
                    ]),
                )
                .unwrap();
        }

        let target = mk([1, 200, 201, 202, 203, 204]);
        index.put(&99, target).unwrap();

        let results = index.top_k(&target, 5).unwrap();
        assert_eq!(results[0].0, 99);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rare_first_top1_returns_the_unique_best_candidate() {
        let index = idx_with(SearchConfig {
            rare_first: true,
            min_jaccard: Some(0.5),
            ..SearchConfig::default()
        });
        let query = mk([1, 2, 3, 4, 5, 6]);
        index.put(&999, query).unwrap();
        for (key, feature) in (10..16_u64).zip(1..=6_u32) {
            index
                .put(
                    &key,
                    mk([
                        feature,
                        100 + feature,
                        200 + feature,
                        300 + feature,
                        400 + feature,
                        500 + feature,
                    ]),
                )
                .unwrap();
        }

        assert_eq!(index.get(&query).unwrap(), Some(999));
        assert_eq!(
            index.get(&query).unwrap(),
            index.top_k(&query, 1).unwrap().first().map(|r| r.0)
        );
    }

    #[test]
    fn fp_metric_filters_keys_at_threshold() {
        let index =
            idx_with(SearchConfig { fp_metric_threshold: Some(3), ..SearchConfig::default() });

        let sketch = mk([1, 2, 3, 4, 5, 6]);
        index.put(&1, sketch).unwrap();
        index.put(&2, mk([1, 2, 3, 7, 8, 9])).unwrap();

        index.record_false_positive(&1);
        index.record_false_positive(&1);
        index.record_false_positive(&1);
        assert_eq!(index.get_metric(&1), Some(3));

        let results = index.top_k(&sketch, 5).unwrap();
        assert!(!results.iter().any(|(k, _)| *k == 1));
        assert!(results.iter().any(|(k, _)| *k == 2));
    }

    #[test]
    fn record_verified_clears_fp_metric() {
        let index = idx();
        index.put(&1, mk([1, 2, 3, 4, 5, 6])).unwrap();

        index.record_false_positive(&1);
        index.record_false_positive(&1);
        assert_eq!(index.get_metric(&1), Some(2));

        index.record_verified(&1);
        assert_eq!(index.get_metric(&1), None);

        assert_eq!(index.get(&mk([1, 2, 3, 4, 5, 6])).unwrap(), Some(1));
    }
}
