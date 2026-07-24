use std::collections::HashMap;
use std::hash::Hash;
use std::io;

use chunkfs::{ChunkHash, Data, DataContainer, IterableDatabase, Scrub, ScrubMeasurements};

use crate::encoder::PalantirEncoder;
use crate::types::{Chunk, SuperFeature, SuperFeatureGenerator};

use marline_index::index::store::IndexStorage;
use marline_index::index::InvertedSketchIndex;
use marline_index::index::SketchIndexApi;
use marline_index::sketch::{FixedSketch, U32Sketch};

/// A multi-tier similarity index for super-feature lookup.
///
/// `Index` maintains three tiers of [`InvertedSketchIndex`] with increasing
/// sketch sizes (`U32Sketch<3>`, `U32Sketch<4>`, `U32Sketch<6>`).  Lookups
/// probe the coarsest tier first and fall through to finer tiers on a miss,
/// providing a trade-off between search speed and accuracy.
pub struct Index<H: Clone + Eq + Hash + Send + Sync> {
    tier1: InvertedSketchIndex<H, U32Sketch<3>, IndexStorage<H, U32Sketch<3>>>,
    tier2: InvertedSketchIndex<H, U32Sketch<4>, IndexStorage<H, U32Sketch<4>>>,
    tier3: InvertedSketchIndex<H, U32Sketch<6>, IndexStorage<H, U32Sketch<6>>>,
}

impl<H: Clone + Eq + Hash + Send + Sync> Index<H> {
    /// Creates an empty three-tier similarity index.
    pub fn new() -> Self {
        let tier1_storage = IndexStorage::new();
        let tier1 = InvertedSketchIndex::new(tier1_storage);
        let tier2_storage = IndexStorage::new();
        let tier2 = InvertedSketchIndex::new(tier2_storage);
        let tier3_storage = IndexStorage::new();
        let tier3 = InvertedSketchIndex::new(tier3_storage);

        Self { tier1, tier2, tier3 }
    }

    /// Splits a slice of super-features into three fixed-size sketches by tier.
    ///
    /// Returns `None` if any tier does not contain exactly the right number
    /// of features to fill its sketch.
    fn split_into_sketches(
        sfs: &[SuperFeature],
    ) -> Option<(U32Sketch<3>, U32Sketch<4>, U32Sketch<6>)> {
        let t1: Vec<u32> = sfs.iter().filter(|sf| sf.tier_id() == 0).map(|sf| sf.value()).collect();
        let t2: Vec<u32> = sfs.iter().filter(|sf| sf.tier_id() == 1).map(|sf| sf.value()).collect();
        let t3: Vec<u32> = sfs.iter().filter(|sf| sf.tier_id() == 2).map(|sf| sf.value()).collect();

        Some((
            FixedSketch::new(t1.try_into().ok()?).ok()?,
            FixedSketch::new(t2.try_into().ok()?).ok()?,
            FixedSketch::new(t3.try_into().ok()?).ok()?,
        ))
    }

    /// Searches for a stored chunk hash similar to the given super-features.
    ///
    /// Probes tier 1 (coarsest) first, then tier 2, then tier 3 (finest).
    /// Returns the first match found, or `None` if no similar chunk exists.
    pub fn search(&self, sfs: &[SuperFeature]) -> Option<H> {
        let (s3, s4, s6) = Self::split_into_sketches(sfs)?;
        self.tier1
            .get(&s3)
            .ok()?
            .or_else(|| self.tier2.get(&s4).ok()?)
            .or_else(|| self.tier3.get(&s6).ok()?)
    }

    /// Inserts a chunk hash indexed by its super-features into all three tiers.
    pub fn insert(&self, sfs: &[SuperFeature], hash: H) {
        if let Some((s3, s4, s6)) = Self::split_into_sketches(sfs) {
            let _ = self.tier1.put(&hash, s3);
            let _ = self.tier2.put(&hash, s4);
            let _ = self.tier3.put(&hash, s6);
        }
    }
}

/// Core scrubbing pipeline that applies the Palantir method to a storage backend.
///
/// For every chunk in the database, `PalantirScrubber`:
/// 1. Generates super-features via [`SuperFeatureGenerator`].
/// 2. Looks up similar chunks in the multi-tier [`Index`].
/// 3. If a match is found, delta-encodes the chunk; otherwise stores it raw.
///
/// The decision to store a delta uses an adaptive compression-ratio threshold
/// that tracks a running average.
///
/// [`SuperFeatureGenerator`]: crate::types::SuperFeatureGenerator
#[allow(dead_code)]
pub struct PalantirScrubber<S, H: Clone + Eq + Hash + Send + Sync, E> {
    /// The super-feature generator.
    sf_gen: S,
    /// Multi-tier similarity index.
    index: Index<H>,
    /// Delta encoder.
    encoder: E,
    /// False-positive threshold for delta encoding ratio.
    fp_threshold: f64,
    /// Running average compression ratio.
    avg_comp_ratio: f64,
    /// Total chunks processed.
    chunks_processed: u64,
}

impl<S, H: Clone + Eq + Hash + Send + Sync, E> PalantirScrubber<S, H, E> {
    /// Creates a new `PalantirScrubber`.
    ///
    /// # Arguments
    /// * `sf_gen` — The super-feature generator.
    /// * `index` — The multi-tier similarity index.
    /// * `encoder` — The delta encoder.
    ///
    /// # Defaults
    ///
    /// | Field | Value |
    /// |-------|-------|
    /// | `fp_threshold` | `0.9` — false-positive ratio cap |
    /// | `avg_comp_ratio` | `1.0` — running average starts at 1.0 (no compression benefit) |
    /// | `chunks_processed` | `0` |
    pub fn new(sf_gen: S, index: Index<H>, encoder: E) -> Self {
        Self { sf_gen, index, encoder, fp_threshold: 0.9, avg_comp_ratio: 1.0, chunks_processed: 0 }
    }
}

impl<CDCHash, B, S, E> Scrub<CDCHash, B, CDCHash, HashMap<CDCHash, Vec<u8>>>
    for PalantirScrubber<S, CDCHash, E>
where
    CDCHash: ChunkHash + Send + Sync,
    B: IterableDatabase<CDCHash, DataContainer<CDCHash>>,
    S: SuperFeatureGenerator,
    E: PalantirEncoder,
{
    /// Runs the Palantir scrub over all chunks in `database`.
    ///
    /// Every chunk is processed through the feature-generation → lookup → delta-or-store
    /// pipeline.  Delta decisions are based on an adaptive compression-ratio heuristic:
    /// a delta is stored only when `ratio < fp_threshold × avg_comp_ratio`, where
    /// `avg_comp_ratio` is an EMA that tracks recent compression efficiency.
    ///
    /// # Note
    ///
    /// `Data::TargetChunk` entries are silently skipped (decoder integration is pending).
    fn scrub<'a>(
        &mut self,
        database: &mut B,
        target_map: &mut HashMap<CDCHash, Vec<u8>>,
    ) -> io::Result<ScrubMeasurements>
    where
        CDCHash: 'a,
    {
        let start = std::time::Instant::now();
        let mut processed_data = 0;
        let data_left = 0;

        for (hash, container) in database.iterator_mut() {
            match container.extract() {
                Data::Chunk(chunk_data) => {
                    let chunk = Chunk::new(chunk_data.clone());
                    let super_features = self.sf_gen.generate(&chunk);

                    match self.index.search(&super_features) {
                        Some(base_hash) => {
                            if let Some(base_data) = target_map.get(&base_hash) {
                                let delta = self.encoder.encode(chunk_data, base_data);
                                let delta_compressed = zstd::encode_all(delta.as_slice(), 0)?;
                                let simple_compressed = zstd::encode_all(chunk_data.as_slice(), 0)?;
                                let ratio =
                                    delta_compressed.len() as f64 / simple_compressed.len() as f64;

                                if ratio < self.fp_threshold * self.avg_comp_ratio {
                                    target_map.insert(hash.clone(), delta);
                                    self.avg_comp_ratio = self.avg_comp_ratio * 0.95 + ratio * 0.05;
                                } else {
                                    target_map.insert(hash.clone(), chunk_data.clone());
                                }
                            } else {
                                target_map.insert(hash.clone(), chunk_data.clone());
                            }
                            processed_data += chunk_data.len();
                        }
                        None => {
                            target_map.insert(hash.clone(), chunk_data.clone());
                            processed_data += chunk_data.len();
                        }
                    }

                    self.index.insert(&super_features, hash.clone());
                    container.make_target(vec![hash.clone()]);
                    self.chunks_processed += 1;
                }
                Data::TargetChunk(_) => {} // todo: add decoder and get full cycle of chunk scrub
            }
        }

        Ok(ScrubMeasurements {
            processed_data,
            running_time: start.elapsed(),
            data_left,
            clusterization_report: None,
        })
    }
    // todo: add update() method for metadata manager
}
