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

pub struct Index<H: Clone + Eq + Hash + Send + Sync> {
    tier1: InvertedSketchIndex<H, U32Sketch<3>, IndexStorage<H, U32Sketch<3>>>,
    tier2: InvertedSketchIndex<H, U32Sketch<4>, IndexStorage<H, U32Sketch<4>>>,
    tier3: InvertedSketchIndex<H, U32Sketch<6>, IndexStorage<H, U32Sketch<6>>>,
}

impl<H: Clone + Eq + Hash + Send + Sync> Index<H> {
    pub fn new() -> Self {
        let tier1_storage = IndexStorage::new();
        let tier1 = InvertedSketchIndex::new(tier1_storage);
        let tier2_storage = IndexStorage::new();
        let tier2 = InvertedSketchIndex::new(tier2_storage);
        let tier3_storage = IndexStorage::new();
        let tier3 = InvertedSketchIndex::new(tier3_storage);

        Self { tier1, tier2, tier3 }
    }

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

    pub fn search(&self, sfs: &[SuperFeature]) -> Option<H> {
        let (s3, s4, s6) = Self::split_into_sketches(sfs)?;
        self.tier1
            .get(&s3)
            .ok()?
            .or_else(|| self.tier2.get(&s4).ok()?)
            .or_else(|| self.tier3.get(&s6).ok()?)
    }

    pub fn insert(&self, sfs: &[SuperFeature], hash: H) {
        if let Some((s3, s4, s6)) = Self::split_into_sketches(sfs) {
            let _ = self.tier1.put(&hash, s3);
            let _ = self.tier2.put(&hash, s4);
            let _ = self.tier3.put(&hash, s6);
        }
    }
}

#[allow(dead_code)]
pub struct PalantirScrubber<S, H: Clone + Eq + Hash + Send + Sync, E> {
    sf_gen: S,
    index: Index<H>,
    encoder: E,
    fp_threshold: f64,
    avg_comp_ratio: f64,
    chunks_processed: u64,
}

impl<S, H: Clone + Eq + Hash + Send + Sync, E> PalantirScrubber<S, H, E> {
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
