use std::collections::HashMap;
use std::hash::Hash;
use std::sync::RwLock;

/// A small metadata value associated with a key, used for false-positive filtering.
pub type Metric = u8;

/// Storage for per-key metrics.
///
/// Metrics are small values (e.g. false-positive counters) stored independently
/// from any sketch or posting-list storage.
pub trait MetricStorage<K>: Send + Sync
where
    K: Clone + Eq + Hash + Send + Sync,
{
    fn get_metric(&self, key: &K) -> Option<Metric>;
    fn set_metric(&self, key: &K, value: Metric) -> Option<Metric>;
    fn remove_metric(&self, key: &K) -> Option<Metric>;
    fn clear_all(&self);
}

/// In-memory implementation of [`MetricStorage`] backed by a [`RwLock`]-protected [`HashMap`].
pub struct MetricsMap<K>
where
    K: Clone + Eq + Hash + Send + Sync,
{
    data: RwLock<HashMap<K, Metric>>,
}

impl<K: Clone + Eq + Hash + Send + Sync> MetricsMap<K> {
    pub fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }

    /// Applies `update_fn` to every metric and returns the keys for which
    /// `cleanup_fn` returns `true`.
    ///
    /// Matching metrics are left in place so callers can remove derived state
    /// (e.g. inverted-index entries) before dropping the metrics themselves.
    pub fn update_and_collect(
        &self,
        update_fn: &mut dyn FnMut(&mut Metric),
        cleanup_fn: &dyn Fn(Metric) -> bool,
    ) -> Vec<K> {
        let mut data = self.data.write().unwrap();

        for metric in data.values_mut() {
            update_fn(metric);
        }

        data.iter().filter(|(_, &m)| cleanup_fn(m)).map(|(k, _)| k.clone()).collect()
    }
}

impl<K: Clone + Eq + Hash + Send + Sync> Default for MetricsMap<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K> MetricStorage<K> for MetricsMap<K>
where
    K: Clone + Eq + Hash + Send + Sync,
{
    fn get_metric(&self, key: &K) -> Option<Metric> {
        let data = self.data.read().unwrap();
        data.get(key).copied()
    }

    fn set_metric(&self, key: &K, value: Metric) -> Option<Metric> {
        let mut data = self.data.write().unwrap();
        data.insert(key.clone(), value)
    }

    fn remove_metric(&self, key: &K) -> Option<Metric> {
        let mut data = self.data.write().unwrap();
        data.remove(key)
    }

    fn clear_all(&self) {
        self.data.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_metric() {
        let m = MetricsMap::new();
        assert_eq!(m.set_metric(&42, 5), None);
        assert_eq!(m.get_metric(&42), Some(5));
    }

    #[test]
    fn get_nonexistent_metric() {
        let m = MetricsMap::new();
        assert_eq!(m.get_metric(&99), None);
    }

    #[test]
    fn set_overwrites_existing() {
        let m = MetricsMap::new();
        m.set_metric(&1, 3);
        assert_eq!(m.set_metric(&1, 7), Some(3));
        assert_eq!(m.get_metric(&1), Some(7));
    }

    #[test]
    fn remove_returns_previous_value() {
        let m = MetricsMap::new();
        m.set_metric(&1, 9);
        assert_eq!(m.remove_metric(&1), Some(9));
        assert_eq!(m.get_metric(&1), None);
    }

    #[test]
    fn clear_all_removes_everything() {
        let m = MetricsMap::new();
        m.set_metric(&1, 1);
        m.set_metric(&2, 2);
        m.clear_all();
        assert_eq!(m.get_metric(&1), None);
        assert_eq!(m.get_metric(&2), None);
    }

    #[test]
    fn update_and_collect_returns_entries_matching_cleanup() {
        let m = MetricsMap::new();
        m.set_metric(&1, 1);
        m.set_metric(&2, 5);
        m.set_metric(&3, 10);

        // Increment all metrics by 1, collect those > 7
        let removed = m.update_and_collect(&mut |m: &mut Metric| *m += 1, &|m: Metric| m > 7);

        assert_eq!(removed, vec![3]);
        assert_eq!(m.get_metric(&1), Some(2));
        assert_eq!(m.get_metric(&2), Some(6));
        assert_eq!(m.get_metric(&3), Some(11));

        for key in &removed {
            m.remove_metric(key);
        }
        assert_eq!(m.get_metric(&3), None);
    }
}
