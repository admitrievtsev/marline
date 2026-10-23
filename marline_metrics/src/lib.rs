//! Lightweight, opt-in latency metrics shared by Marlin pipeline implementations.
//!
//! A recorder is disabled by default. Enable all stages or selected stages before or
//! during a run. A measurement is included if its stage is enabled when the measured
//! operation starts; changing the setting does not interrupt an in-flight measurement.
//! Samples are aggregated in a per-run batch and merged into the recorder once when
//! that batch is dropped, avoiding shared locks on hot per-chunk paths.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Stable names for latency measurements emitted by Marlin pipelines.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Stage {
    Cdc = 0,
    SbcHashing,
    SbcClustering,
    SbcEncoding,
    SbcScrubTotal,
    PalantirFeatureGeneration,
    PalantirFingerprintLookup,
    PalantirSuperFeatureLookup,
    PalantirBaseLookup,
    PalantirEncoding,
    PalantirCompression,
    PalantirStorageWrite,
    PalantirMetadataUpdate,
    PalantirScrubTotal,
    PipelineScrub,
    PipelineTotal,
    OnlineChunkingAndFeatureCollection,
    OnlineFeatureGeneration,
    OnlineFingerprintLookup,
    OnlineSuperFeatureLookup,
    OnlineBaseLookup,
    OnlineEncoding,
    OnlineCompression,
    OnlineStorageWrite,
    OnlineMetadataUpdate,
    OnlineWriteTotal,
    OnlineFingerprintHash,
    OnlineDatasetTotal,
}

impl Stage {
    /// Every stage known to this version of the metrics API.
    pub const ALL: [Stage; 28] = [
        Stage::Cdc,
        Stage::SbcHashing,
        Stage::SbcClustering,
        Stage::SbcEncoding,
        Stage::SbcScrubTotal,
        Stage::PalantirFeatureGeneration,
        Stage::PalantirFingerprintLookup,
        Stage::PalantirSuperFeatureLookup,
        Stage::PalantirBaseLookup,
        Stage::PalantirEncoding,
        Stage::PalantirCompression,
        Stage::PalantirStorageWrite,
        Stage::PalantirMetadataUpdate,
        Stage::PalantirScrubTotal,
        Stage::PipelineScrub,
        Stage::PipelineTotal,
        Stage::OnlineChunkingAndFeatureCollection,
        Stage::OnlineFeatureGeneration,
        Stage::OnlineFingerprintLookup,
        Stage::OnlineSuperFeatureLookup,
        Stage::OnlineBaseLookup,
        Stage::OnlineEncoding,
        Stage::OnlineCompression,
        Stage::OnlineStorageWrite,
        Stage::OnlineMetadataUpdate,
        Stage::OnlineWriteTotal,
        Stage::OnlineFingerprintHash,
        Stage::OnlineDatasetTotal,
    ];

    /// Stable snake-case identifier suitable for CSV and JSON output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Stage::Cdc => "cdc",
            Stage::SbcHashing => "sbc.hashing",
            Stage::SbcClustering => "sbc.clustering",
            Stage::SbcEncoding => "sbc.encoding",
            Stage::SbcScrubTotal => "sbc.scrub_total",
            Stage::PalantirFeatureGeneration => "palantir.feature_generation",
            Stage::PalantirFingerprintLookup => "palantir.fingerprint_lookup",
            Stage::PalantirSuperFeatureLookup => "palantir.super_feature_lookup",
            Stage::PalantirBaseLookup => "palantir.base_lookup",
            Stage::PalantirEncoding => "palantir.encoding",
            Stage::PalantirCompression => "palantir.compression",
            Stage::PalantirStorageWrite => "palantir.storage_write",
            Stage::PalantirMetadataUpdate => "palantir.metadata_update",
            Stage::PalantirScrubTotal => "palantir.scrub_total",
            Stage::PipelineScrub => "pipeline.scrub",
            Stage::PipelineTotal => "pipeline.total",
            Stage::OnlineChunkingAndFeatureCollection => {
                "palantir_online.fastcdc_chunking_and_raw_features"
            }
            Stage::OnlineFeatureGeneration => "palantir_online.feature_generation",
            Stage::OnlineFingerprintLookup => "palantir_online.fingerprint_lookup",
            Stage::OnlineSuperFeatureLookup => "palantir_online.super_feature_lookup",
            Stage::OnlineBaseLookup => "palantir_online.base_lookup",
            Stage::OnlineEncoding => "palantir_online.encoding",
            Stage::OnlineCompression => "palantir_online.compression",
            Stage::OnlineStorageWrite => "palantir_online.storage_write",
            Stage::OnlineMetadataUpdate => "palantir_online.metadata_update",
            Stage::OnlineWriteTotal => "palantir_online.write_total",
            Stage::OnlineFingerprintHash => "palantir_online.fingerprint_hash",
            Stage::OnlineDatasetTotal => "palantir_online.dataset_total",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }

    const fn bit(self) -> u64 {
        1u64 << self.index()
    }
}

const STAGE_COUNT: usize = Stage::ALL.len();
const HISTOGRAM_BUCKETS: usize = 64;

struct StageAggregate {
    count: u64,
    total_nanos: u128,
    min_nanos: u64,
    max_nanos: u64,
    histogram: [u64; HISTOGRAM_BUCKETS],
}

impl Default for StageAggregate {
    fn default() -> Self {
        Self {
            count: 0,
            total_nanos: 0,
            min_nanos: 0,
            max_nanos: 0,
            histogram: [0; HISTOGRAM_BUCKETS],
        }
    }
}

impl StageAggregate {
    fn record(&mut self, duration: Duration) {
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.count = self.count.saturating_add(1);
        self.total_nanos = self.total_nanos.saturating_add(nanos as u128);
        if self.count == 1 {
            self.min_nanos = nanos;
            self.max_nanos = nanos;
        } else {
            self.min_nanos = self.min_nanos.min(nanos);
            self.max_nanos = self.max_nanos.max(nanos);
        }
        let bucket = if nanos == 0 { 0 } else { (u64::BITS - nanos.leading_zeros() - 1) as usize };
        self.histogram[bucket] = self.histogram[bucket].saturating_add(1);
    }

    fn merge(&mut self, other: &StageAggregate) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            self.min_nanos = other.min_nanos;
            self.max_nanos = other.max_nanos;
        } else {
            self.min_nanos = self.min_nanos.min(other.min_nanos);
            self.max_nanos = self.max_nanos.max(other.max_nanos);
        }
        self.count = self.count.saturating_add(other.count);
        self.total_nanos = self.total_nanos.saturating_add(other.total_nanos);
        for (bucket, value) in self.histogram.iter_mut().zip(other.histogram) {
            *bucket = bucket.saturating_add(value);
        }
    }
}

struct RecorderState {
    stages: [StageAggregate; STAGE_COUNT],
}

impl Default for RecorderState {
    fn default() -> Self {
        Self { stages: std::array::from_fn(|_| StageAggregate::default()) }
    }
}

struct RecorderInner {
    enabled: std::sync::atomic::AtomicU64,
    state: Mutex<RecorderState>,
}

/// Cloneable handle used to enable/disable collection and read accumulated results.
#[derive(Clone)]
pub struct MetricsRecorder(Arc<RecorderInner>);

impl Default for MetricsRecorder {
    fn default() -> Self {
        Self::disabled()
    }
}

impl MetricsRecorder {
    /// Create a recorder with collection disabled for every stage.
    pub fn disabled() -> Self {
        Self(Arc::new(RecorderInner {
            enabled: std::sync::atomic::AtomicU64::new(0),
            state: Mutex::new(RecorderState::default()),
        }))
    }

    /// Create a recorder with every known stage enabled.
    pub fn enabled_all() -> Self {
        let recorder = Self::disabled();
        recorder.set_all_enabled(true);
        recorder
    }

    /// Enable or disable one stage. This is safe to call while a pipeline is running.
    pub fn set_enabled(&self, stage: Stage, enabled: bool) {
        use std::sync::atomic::Ordering;
        if enabled {
            self.0.enabled.fetch_or(stage.bit(), Ordering::Relaxed);
        } else {
            self.0.enabled.fetch_and(!stage.bit(), Ordering::Relaxed);
        }
    }

    /// Enable or disable all stages.
    pub fn set_all_enabled(&self, enabled: bool) {
        use std::sync::atomic::Ordering;
        let mask = if enabled { (1u64 << STAGE_COUNT) - 1 } else { 0 };
        self.0.enabled.store(mask, Ordering::Relaxed);
    }

    /// Check whether new measurements for `stage` will be collected.
    pub fn is_enabled(&self, stage: Stage) -> bool {
        use std::sync::atomic::Ordering;
        self.0.enabled.load(Ordering::Relaxed) & stage.bit() != 0
    }

    /// Read a consistent snapshot of all collected stage aggregates.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let state = self.0.state.lock().unwrap();
        MetricsSnapshot {
            stages: Stage::ALL
                .iter()
                .filter_map(|&stage| {
                    let aggregate = &state.stages[stage.index()];
                    (aggregate.count > 0).then(|| StageMetrics {
                        stage,
                        count: aggregate.count,
                        total: duration_from_nanos(aggregate.total_nanos),
                        average: duration_from_nanos(
                            aggregate.total_nanos / aggregate.count as u128,
                        ),
                        min: duration_from_nanos(aggregate.min_nanos as u128),
                        max: duration_from_nanos(aggregate.max_nanos as u128),
                        histogram: aggregate.histogram,
                    })
                })
                .collect(),
        }
    }

    /// Clear collected measurements without changing which stages are enabled.
    pub fn clear(&self) {
        *self.0.state.lock().unwrap() = RecorderState::default();
    }

    /// Start a local aggregation batch. The batch merges its data on drop.
    pub fn batch(&self) -> MetricsBatch {
        MetricsBatch { recorder: self.clone(), stages: Vec::new() }
    }
}

/// Per-run accumulator. Samples are kept locally and merged under one lock on drop.
pub struct MetricsBatch {
    recorder: MetricsRecorder,
    // Allocate a stage histogram only on the first recorded sample for that stage.
    // This keeps an attached-but-disabled recorder cheap as well.
    stages: Vec<(Stage, StageAggregate)>,
}

impl MetricsBatch {
    /// Run an operation and record its elapsed time if the stage is enabled at entry.
    pub fn measure<T>(&mut self, stage: Stage, operation: impl FnOnce() -> T) -> T {
        if !self.recorder.is_enabled(stage) {
            return operation();
        }
        let start = Instant::now();
        let result = operation();
        self.record_unconditionally(stage, start.elapsed());
        result
    }

    /// Add an already measured duration if the stage is currently enabled.
    pub fn record(&mut self, stage: Stage, duration: Duration) {
        if self.recorder.is_enabled(stage) {
            self.record_unconditionally(stage, duration);
        }
    }

    /// Add a duration that was started while the stage was enabled, even if it has
    /// since been disabled.
    pub fn record_unconditionally(&mut self, stage: Stage, duration: Duration) {
        if let Some(index) = self.stages.iter().position(|(current, _)| *current == stage) {
            self.stages[index].1.record(duration);
        } else {
            let mut aggregate = StageAggregate::default();
            aggregate.record(duration);
            self.stages.push((stage, aggregate));
        }
    }
}

impl Drop for MetricsBatch {
    fn drop(&mut self) {
        if self.stages.is_empty() {
            return;
        }
        let mut state = self.recorder.0.state.lock().unwrap();
        for (stage, source) in &self.stages {
            state.stages[stage.index()].merge(source);
        }
    }
}

/// Immutable metrics snapshot.
#[derive(Clone, Debug, Default)]
pub struct MetricsSnapshot {
    pub stages: Vec<StageMetrics>,
}

/// Aggregate latency data for one stage.
#[derive(Clone, Debug)]
pub struct StageMetrics {
    pub stage: Stage,
    pub count: u64,
    pub total: Duration,
    pub average: Duration,
    pub min: Duration,
    pub max: Duration,
    histogram: [u64; HISTOGRAM_BUCKETS],
}

impl StageMetrics {
    /// Approximate percentile from a fixed-memory, base-2 latency histogram.
    /// The result is an upper bound for the selected histogram bucket.
    pub fn percentile(&self, percentile: f64) -> Option<Duration> {
        if self.count == 0 || !percentile.is_finite() || !(0.0..=1.0).contains(&percentile) {
            return None;
        }
        let rank = ((percentile * self.count as f64).ceil() as u64).max(1);
        let mut cumulative = 0u64;
        for (index, count) in self.histogram.iter().enumerate() {
            cumulative = cumulative.saturating_add(*count);
            if cumulative >= rank {
                let upper_bound = if index == 63 { u64::MAX } else { (1u64 << (index + 1)) - 1 };
                return Some(duration_from_nanos(upper_bound as u128));
            }
        }
        Some(self.max)
    }

    pub fn p50(&self) -> Option<Duration> {
        self.percentile(0.50)
    }

    pub fn p90(&self) -> Option<Duration> {
        self.percentile(0.90)
    }

    pub fn p99(&self) -> Option<Duration> {
        self.percentile(0.99)
    }
}

fn duration_from_nanos(nanos: u128) -> Duration {
    let max_nanos = u64::MAX as u128 * 1_000_000_000 + 999_999_999;
    let nanos = nanos.min(max_nanos);
    Duration::new((nanos / 1_000_000_000) as u64, (nanos % 1_000_000_000) as u32)
}

#[cfg(test)]
mod tests {
    use super::{MetricsRecorder, Stage};
    use std::time::Duration;

    #[test]
    fn disabled_recorder_does_not_collect() {
        let recorder = MetricsRecorder::disabled();
        {
            let mut batch = recorder.batch();
            batch.measure(Stage::SbcHashing, || ());
        }
        assert!(recorder.snapshot().stages.is_empty());
    }

    #[test]
    fn batches_merge_samples_and_expose_percentiles() {
        let recorder = MetricsRecorder::enabled_all();
        {
            let mut batch = recorder.batch();
            batch.record(Stage::Cdc, Duration::from_nanos(10));
            batch.record(Stage::Cdc, Duration::from_nanos(20));
        }
        let snapshot = recorder.snapshot();
        let cdc = &snapshot.stages[0];
        assert_eq!(cdc.count, 2);
        assert_eq!(cdc.min, Duration::from_nanos(10));
        assert_eq!(cdc.max, Duration::from_nanos(20));
        assert_eq!(cdc.p50(), Some(Duration::from_nanos(15)));
    }

    #[test]
    fn stage_can_be_toggled_independently() {
        let recorder = MetricsRecorder::disabled();
        recorder.set_enabled(Stage::SbcClustering, true);
        assert!(recorder.is_enabled(Stage::SbcClustering));
        assert!(!recorder.is_enabled(Stage::SbcHashing));
    }

    #[test]
    fn measurement_started_while_enabled_is_kept_if_disabled_during_operation() {
        let recorder = MetricsRecorder::enabled_all();
        {
            let mut batch = recorder.batch();
            let toggle = recorder.clone();
            batch.measure(Stage::Cdc, || {
                toggle.set_enabled(Stage::Cdc, false);
            });
        }
        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.stages[0].stage, Stage::Cdc);
        assert_eq!(snapshot.stages[0].count, 1);
    }
}
