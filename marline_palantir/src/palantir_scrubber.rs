use std::collections::HashMap;
use std::io;

use chunkfs::{
    ChunkHash, Data, DataContainer, IterableDatabase, Scrub, ScrubMeasurements,
};

use crate::encoder::PalantirEncoder;
use crate::types::{Chunk, SuperFeature, SuperFeatureGenerator};

pub type BlockId = u32;

pub trait SimilarityIndex {
    fn search(&self, super_features: &[SuperFeature]) -> Option<BlockId>;
    fn insert(&mut self, super_features: &[SuperFeature], block_id: BlockId);
}

#[allow(dead_code)]
pub struct StubIndex {
    tier1: HashMap<[u32; 3], BlockId>,
    tier2: HashMap<[u32; 4], BlockId>,
    tier3: HashMap<[u32; 6], BlockId>,
}

impl StubIndex {
    pub fn new() -> Self {
        Self {
            tier1: HashMap::new(),
            tier2: HashMap::new(),
            tier3: HashMap::new(),
        }
    }
}

impl SimilarityIndex for StubIndex {
    fn search(&self, _super_features: &[SuperFeature]) -> Option<BlockId> {
        None
    }

    fn insert(&mut self, _super_features: &[SuperFeature], _block_id: BlockId) {}
}

#[allow(dead_code)]
pub struct PalantirScrubber<S, I, E> {
    sf_gen: S,
    index: I,
    encoder: E,
    fp_threshold: f64,
    avg_comp_ratio: f64,
    chunks_processed: u64,
}

impl<S, I, E> PalantirScrubber<S, I, E> {
    pub fn new(sf_gen: S, index: I, encoder: E) -> Self {
        Self {
            sf_gen,
            index,
            encoder,
            fp_threshold: 0.9,
            avg_comp_ratio: 1.0,
            chunks_processed: 0,
        }
    }
}

impl<CDCHash, B, S, I, E> Scrub<CDCHash, B, CDCHash, HashMap<CDCHash, Vec<u8>>>
    for PalantirScrubber<S, I, E>
where
    CDCHash: ChunkHash,
    B: IterableDatabase<CDCHash, DataContainer<CDCHash>>,
    S: SuperFeatureGenerator,
    I: SimilarityIndex,
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
        let mut data_left = 0;

        for (hash, container) in database.iterator_mut() {
            match container.extract() {
                Data::Chunk(chunk_data) => {
                    let chunk = Chunk::new(chunk_data.clone());
                    let super_features = self.sf_gen.generate(&chunk);

                    match self.index.search(&super_features) {
                        Some(_) => {
                            // TODO: get base_data from index/storage, encode delta
                            //   let delta = self.encoder.encode(chunk_data, base_data);
                            //   let delta_zst = zstd::encode_all(&delta[..], 0).unwrap();
                            //   let simple_zst = zstd::encode_all(chunk_data, 0).unwrap();
                            //   let ratio = delta_zst.len() as f64 / simple_zst.len() as f64;
                            //   if ratio < self.fp_threshold {  // true positive
                            //       target_map.insert(hash.clone(), delta);
                            //       self.avg_comp_ratio = self.avg_comp_ratio * 0.95 + ratio * 0.05;
                            //   } else {  // false positive → store as simple
                            //       target_map.insert(hash.clone(), chunk_data.clone());
                            //   }
                            data_left += chunk_data.len();
                        }
                        None => {
                            target_map.insert(hash.clone(), chunk_data.clone());
                            processed_data += chunk_data.len();
                        }
                    }

                    self.index.insert(&super_features, 0);
                    container.make_target(vec![hash.clone()]);
                    self.chunks_processed += 1;
                }
                Data::TargetChunk(_) => {}
            }
        }

        Ok(ScrubMeasurements {
            processed_data,
            running_time: start.elapsed(),
            data_left,
            clusterization_report: None,
        })
    }
}
