use crate::encoder::PalantirEncoder;
use crate::error::PalantirError;
use crate::lifecycle_manager::LifecycleTierConfig;
use crate::metadata_manager::MetadataManager;
use crate::mock_rocksdb::{Entry, MockRocksDBMap};
use crate::palantir_scrubber::get_entry;
use crate::sf_generator::SuperFeatureGenerator;
use crate::types::{BlockID, TierConfig};
use chunkfs::{ChunkHash, Database};
use marline_cdc::{collect_raw_features, ChunkerBuilder};
use marline_index::heuristic_index::SearchConfig;
use std::collections::HashMap;
use std::hash::Hash;

pub trait OnlineSBC<
    S,
    H: Clone + Eq + Hash + Send + Sync + 'static + std::default::Default,
    E,
    const N: usize,
>
{
    fn write(&mut self, data: &[u8]) -> Result<(), PalantirError>;

    fn bytes_stored(&self) -> usize;
}

pub struct PalantirOnline<
    S,
    H: Clone + Eq + Hash + Send + Sync + 'static + std::default::Default,
    E: PalantirEncoder,
    const N: usize,
> {
    sf_gen: S,
    /// Multi-tier similarity index.
    metadata_manager: MetadataManager<H, N>,
    /// Delta encoder.
    encoder: E,
    /// False-positive threshold for delta encoding ratio.
    fp_threshold: f64,
    /// Compute super-features with a second scan over each chunk (baseline
    /// two-pass mode) instead of reusing raw features collected during chunking.
    naive: bool,
    /// Total chunks processed.
    chunks_processed: u64,
    /// Count of chunks stored as deltas.
    delta_stored: u64,
    /// Tracks chunk hash → base hash for stored deltas (chain rebuild).
    delta_bases: HashMap<H, H>,
    manifest_base: MockRocksDBMap,
    target_map: HashMap<H, Vec<u8>>,
}

#[allow(unused_variables)]
impl<S: SuperFeatureGenerator, E: PalantirEncoder, const N: usize> OnlineSBC<S, [u8; 32], E, N>
    for PalantirOnline<S, [u8; 32], E, N>
{
    fn write(&mut self, data: &[u8]) -> Result<(), PalantirError> {
        let builder = ChunkerBuilder::default();
        let chunker = builder.chunker(data);

        for (chunk, sf_raw) in chunker {
            let chunk_data = &data[chunk.offset..chunk.offset + chunk.length].to_vec();
            let mut hasher = blake3::Hasher::new();
            //println!("size: {}, SF{:?}", chunk_data.len(), sf_raw);
            hasher.update(chunk_data);
            let hash = hasher.finalize();
            let hash = hash.as_bytes();
            // let super_features = self.sf_gen.generate(&Chunk::new(chunk_data.clone()));

            match self.metadata_manager.lookup_fingerprint(hash) {
                Some(_) => {}
                None => {
                    let super_features = if self.naive {
                        // Baseline: recompute features with a second scan
                        // over the chunk data instead of reusing the
                        // in-pass features.
                        let sf = collect_raw_features(chunk_data, builder.min_size());
                        debug_assert!(sf == sf_raw, "two-pass features must equal in-pass");
                        self.sf_gen.generate_from_raw(&sf)
                    } else {
                        self.sf_gen.generate_from_raw(&sf_raw)
                    };

                    match self.metadata_manager.lookup_super_features(&super_features) {
                        Some((base_hash, _)) => {
                            let entry = get_entry(
                                &mut self.manifest_base,
                                &self.delta_bases,
                                &base_hash.hash,
                            );

                            let data = entry.get_data().to_vec();

                            match entry.get_manifest() {
                                Some(manifest) => {
                                    let delta = self.encoder.encode_with_manifest(
                                        chunk_data,
                                        entry.get_data(),
                                        manifest,
                                    );
                                    let delta_compressed =
                                        zstd::encode_all(delta.as_slice(), 0).unwrap();
                                    let simple_compressed =
                                        zstd::encode_all(chunk_data.as_slice(), 0).unwrap();
                                    let ratio = delta_compressed.len() as f64
                                        / simple_compressed.len() as f64;
                                    if ratio < self.fp_threshold {
                                        let _ = self.target_map.insert(*hash, delta_compressed);
                                        self.delta_bases.insert(*hash, base_hash.hash);
                                        self.delta_stored += 1;
                                    } else {
                                        self.target_map.insert(*hash, chunk_data.clone());
                                        let _ = &mut self
                                            .manifest_base
                                            .insert(*hash, Entry::from(chunk_data.clone()));
                                    }
                                }
                                None => {
                                    /*
                                    let fp_table = E::generate_fp_table(&data);
                                    let _ = self.manifest_base.insert(*hash, Entry::new(fp_table.clone(), chunk_data.clone()));

                                    let delta = self
                                        .encoder
                                        .encode_with_manifest(chunk_data, &data, &fp_table.unwrap());
                                        */

                                    //let delta = self.encoder.encode(chunk_data, &data);

                                    //let delta_compressed =
                                    //   zstd::encode_all(delta.as_slice(), 0).unwrap();

                                    // let delta_compressed = zstd::encode_all(chunk_data.as_slice(), 0).unwrap();
                                    let delta_compressed = data;

                                    let _ = self.target_map.insert(*hash, delta_compressed);
                                    self.delta_bases.insert(*hash, base_hash.hash);
                                    self.delta_stored += 1;
                                }
                            }
                        }
                        None => {
                            // let fp_table = E::generate_fp_table(&chunk.as_bytes());
                            let fp_table = None;
                            // let chunk_data = zstd::encode_all(chunk_data.as_slice(), 0).unwrap();
                            self.target_map.insert(*hash, chunk_data.clone());
                            let _ = &self
                                .manifest_base
                                .insert(*hash, Entry::new(fp_table, chunk_data.clone()));
                        }
                    }

                    self.metadata_manager.add_block(*hash, &super_features, BlockID::new(*hash));
                }
            }
            self.chunks_processed += 1;
        }
        Ok(())
    }
    fn bytes_stored(&self) -> usize {
        let mut size = 0usize;
        for val in self.target_map.values() {
            size += val.len()
        }
        size
    }
}

impl<
        S,
        H: ChunkHash + Clone + Eq + Hash + Send + Sync + 'static,
        E: PalantirEncoder,
        const N: usize,
    > PalantirOnline<S, H, E, N>
{
    /// Creates a new `PalantirScrubber`.
    ///
    /// # Arguments
    /// * `sf_gen` — The super-feature generator.
    /// * `encoder` — The delta encoder.
    /// * `tier_config` — Tier configuration (super-feature sizes per tier).
    /// * `lifecycle_configs` — Per-tier lifecycle policies.
    ///
    /// # Defaults
    ///
    /// | Field | Value |
    /// |-------|-------|
    /// | `fp_threshold` | `0.9` — false-positive ratio cap |
    /// | `chunks_processed` | `0` |
    /// | `delta_stored` | `0` | number of stored chunks by delta
    /// | `delta_bases` | empty hashmap | remember of chunk is delta of other chunk
    pub fn new(
        sf_gen: S,
        encoder: E,
        tier_config: TierConfig<N>,
        lifecycle_configs: [LifecycleTierConfig; N],
        search_config: SearchConfig,
    ) -> Self {
        Self {
            sf_gen,
            metadata_manager: MetadataManager::new(tier_config, lifecycle_configs, &search_config),
            encoder,
            fp_threshold: 0.9,
            naive: false,
            chunks_processed: 0,
            delta_stored: 0,
            delta_bases: HashMap::new(),
            manifest_base: MockRocksDBMap::new(),
            target_map: HashMap::new(),
        }
    }

    /// Switches between the optimized single-pass and baseline two-pass
    /// super-feature computation.
    ///
    /// When `true`, super-features for each chunk are computed with a second
    /// full scan of the chunk data (`generate`); when `false` (default), raw
    /// features collected during chunking are reused (`generate_from_raw`).
    pub fn set_naive(&mut self, naive: bool) {
        self.naive = naive;
    }
}
