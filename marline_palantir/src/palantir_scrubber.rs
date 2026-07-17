use std::collections::HashMap;
use std::io;

use chunkfs::{
    ChunkHash, Data, DataContainer, IterableDatabase, Scrub, ScrubMeasurements,
};

use crate::types::{Chunk, Fingerprint, FingerprintGenerator, SuperFeature, SuperFeatureGenerator};

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
pub struct PalantirScrubber<F, S, I> {
    fingerprint_gen: F,
    sf_gen: S,
    index: I,
    // todo: encoder
}

impl<CDCHash, B, F, S, I> Scrub<CDCHash, B, Fingerprint, HashMap<Fingerprint, Vec<u8>>>
    for PalantirScrubber<F, S, I>
where
    CDCHash: ChunkHash,
    B: IterableDatabase<CDCHash, DataContainer<Fingerprint>>,
    F: FingerprintGenerator,
    S: SuperFeatureGenerator,
    I: SimilarityIndex,
{
    fn scrub<'a>(
        &mut self,
        database: &mut B,
        target_map: &mut HashMap<Fingerprint, Vec<u8>>,
    ) -> io::Result<ScrubMeasurements>
    where
        CDCHash: 'a,
    {
        let start = std::time::Instant::now();
        let mut processed_data = 0;
        let mut data_left = 0;

        for (_hash, container) in database.iterator_mut() {
            match container.extract() {
                Data::Chunk(chunk_data) => {
                    let chunk = Chunk::new(chunk_data.clone());
                    let fingerprint = self.fingerprint_gen.generate(&chunk);
                    let super_features = self.sf_gen.generate(&chunk);

                    if let Some(_parent_id) = self.index.search(&super_features) {
                        // TODO: delta encode
                        data_left += chunk_data.len();
                    } else {
                        target_map.insert(fingerprint, chunk_data.clone());
                        processed_data += chunk_data.len();
                    }

                    self.index.insert(&super_features, 0);
                    container.make_target(vec![fingerprint]);
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
