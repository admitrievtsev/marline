use crate::clusterer::Clusterer;
use crate::decoder::Decoder;
use crate::encoder::Encoder;
use crate::{ChunkType, SBCHash, SBCKey, SBCMap};
use marline_sketcher::SBCHasher;

use chunkfs::{
    ChunkHash, Data, DataContainer, Database, IterableDatabase, Scrub, ScrubMeasurements,
};
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::collections::HashMap;
use std::io;
use std::io::{Error, ErrorKind};
use std::sync::Mutex;
use std::time::Instant;

const NUM_THREADS_FOR_HASHING: usize = 1;

pub type ClusterPoint<'a, Hash> = (Hash, &'a mut &'a mut DataContainer<SBCKey<Hash>>);
pub struct Clusters<'a, Hash: SBCHash>(pub HashMap<Hash, Vec<ClusterPoint<'a, Hash>>>);

pub trait EqCluster<'a, Hash: SBCHash> {
    fn partial_search(
        &mut self,
        key: &Hash,
        max_mismatches: usize,
    ) -> &mut Vec<ClusterPoint<'a, Hash>>;
}

impl<'a, Hash: SBCHash> EqCluster<'a, Hash> for Clusters<'a, Hash> {
    fn partial_search(&mut self, key: &Hash, matches: usize) -> &mut Vec<ClusterPoint<'a, Hash>> {
        let _entry = self.0.get_mut(key);
        match self.0.contains_key(key) {
            true => self.0.get_mut(key).unwrap(),
            false => {
                let key_parts = key.as_slice();
                //println!("Searching for key {:?}", key_parts);
                // Search fo
                let mut part_key = None;
                for (k, _v) in self.0.iter() {
                    let mut current_mismatches = 0;

                    #[allow(clippy::needless_range_loop)]
                    for part_idx in 0..k.as_slice().len() {
                        if k.as_slice()[part_idx] != key_parts[part_idx] {
                            current_mismatches += 1;
                            if current_mismatches >= matches {
                                break;
                            }
                        }
                    }

                    if current_mismatches >= matches {
                        continue;
                    } else {
                        part_key = Some(k.clone());
                        break;
                    }
                }
                if let Some(part_key) = part_key {
                    return self.0.get_mut(&part_key).unwrap();
                }
                self.0.insert(key.clone(), vec![]);
                self.0.get_mut(key).unwrap()
            }
        }
    }
}

/// Implements the `Database` trait for `SBCMap`, enabling it to act as a storage backend
/// for chunk-based filesystems (`chunkfs`).
///
/// This implementation provides methods to insert, retrieve, and check for chunks
/// identified by `SBCKey`. It handles both simple chunks stored as raw data and delta chunks
/// which are decoded on retrieval using the provided decoder.
///
/// # Type Parameters
///
/// * `D` - The decoder type implementing the `Decoder` trait, used to decode delta chunks.
/// * `Hash` - The hash type implementing the `SBCHash` trait, identifying chunks.
///
/// # Behavior
///
/// - `insert` stores the raw chunk bytes keyed by their `SBCKey`.
/// - `get` retrieves the chunk data:
///   - For `Simple` chunks, returns the stored bytes directly.
///   - For `Delta` chunks, recursively retrieves the parent chunk and applies the decoder to reconstruct the full chunk.
/// - `contains` checks if a chunk key exists in the storage.
impl<D: Decoder, Hash: SBCHash> Database<SBCKey<Hash>, Vec<u8>> for SBCMap<D, Hash> {
    /// Inserts a chunk into the storage.
    ///
    /// # Arguments
    ///
    /// * `sbc_hash` - The key identifying the chunk.
    /// * `chunk` - The raw byte content of the chunk.
    fn insert(&mut self, sbc_hash: SBCKey<Hash>, chunk: Vec<u8>) -> io::Result<()> {
        self.sbc_hashmap.insert(sbc_hash, chunk);
        Ok(())
    }

    /// Retrieves a chunk by its key.
    ///
    /// For `Simple` chunks, returns the stored bytes directly.
    /// For `Delta` chunks, recursively retrieves the parent chunk and decodes the delta
    /// to reconstruct the full chunk.
    ///
    /// # Arguments
    ///
    /// * `sbc_hash` - Reference to the chunk key to retrieve.
    ///
    /// # Returns
    ///
    /// The full chunk bytes as a `Vec<u8>`.
    fn get(&self, sbc_hash: &SBCKey<Hash>) -> io::Result<Vec<u8>> {
        let sbc_value = self
            .sbc_hashmap
            .get(sbc_hash)
            .ok_or(Error::new(ErrorKind::NotFound, "Chunk not found"))?;

        let chunk = match &sbc_hash.chunk_type {
            ChunkType::Simple => sbc_value.clone(),
            ChunkType::Delta {
                parent_hash,
                number: _,
            } => {
                // Recursively get the parent chunk as a simple chunk
                let parent_data = self.get(&SBCKey {
                    hash: parent_hash.clone(),
                    chunk_type: ChunkType::Simple,
                })?;

                // Decode the delta chunk using the decoder
                self.decoder.decode_chunk(parent_data, sbc_value.as_slice())
            }
        };
        Ok(chunk)
    }

    /// Checks if the storage contains a chunk with the given key.
    ///
    /// # Arguments
    ///
    /// * `key` - Reference to the chunk key.
    ///
    /// # Returns
    ///
    /// `true` if the chunk exists, `false` otherwise.
    fn contains(&self, key: &SBCKey<Hash>) -> bool {
        self.sbc_hashmap.contains_key(key)
    }
}

impl<D: Decoder, Hash: SBCHash> IterableDatabase<SBCKey<Hash>, Vec<u8>> for SBCMap<D, Hash> {
    fn iterator(&self) -> Box<dyn Iterator<Item = (&SBCKey<Hash>, &Vec<u8>)> + '_> {
        Box::new(self.sbc_hashmap.iter())
    }
    fn iterator_mut(&mut self) -> Box<dyn Iterator<Item = (&SBCKey<Hash>, &mut Vec<u8>)> + '_> {
        Box::new(self.sbc_hashmap.iter_mut())
    }

    fn clear(&mut self) -> io::Result<()> {
        HashMap::clear(&mut self.sbc_hashmap);
        Ok(())
    }
}
/// Applies the Similarity-Based Chunking (SBC) algorithm to chunks obtained from
/// Content Defined Chunking (CDC).
///
/// `SBCScrubber` orchestrates the process of hashing, clustering, and encoding chunks
/// to optimize storage by exploiting similarity between chunks.
///
/// # Type Parameters
///
/// * `Hash` - The hash type implementing `SBCHash`, representing the hash of chunks.
/// * `H` - The hasher type implementing `Hasher` producing `Hash`.
/// * `C` - The clusterer type implementing `Clusterer` for grouping similar chunks.
/// * `E` - The encoder type implementing `Encoder` for encoding clusters into delta or simple chunks.
///
/// # Fields
///
/// * `hasher` - Responsible for computing similarity hashes of chunks.
/// * `clusterer` - Responsible for grouping chunks based on similarity hashes.
/// * `encoder` - Responsible for encoding clusters into delta-encoded or simple chunks.
///
/// # Overview
///
/// The scrubber performs the following steps:
/// 1. **Hashing**: Computes similarity hashes of all chunks in parallel.
/// 2. **Clustering**: Groups chunks by similarity using the clusterer.
/// 3. **Encoding**: Encodes the clusters into delta or simple chunks and stores them in the target map.
///
/// # Example
///
/// ```
/// extern crate chunkfs;
/// extern crate sbc_algorithm;
///
/// use chunkfs::chunkers::{SizeParams, SuperChunker};
/// use chunkfs::hashers::Sha256Hasher;
/// use chunkfs::FileSystem;
/// use sbc_algorithm::{SBCMap, SBCScrubber};
/// use sbc_algorithm::{decoder, encoder, hasher, clusterer};
/// use std::collections::HashMap;
/// use std::io;
///
/// fn main() -> io::Result<()> {
///     let data = vec![10; 1024 * 1024];
///     let chunk_size = SizeParams::new(2 * 1024, 8 * 1024, 16 * 1024);
///     let mut fs = FileSystem::new_with_scrubber(
///         HashMap::default(),
///         SBCMap::new(decoder::GdeltaDecoder::default()),
///         Box::new(SBCScrubber::new(
///             hasher::AronovichHasher,
///             clusterer::GraphClusterer::default(),
///             encoder::GdeltaEncoder::default(),
///         )),
///         Sha256Hasher::default(),
///     );
///     let mut handle = fs.create_file("file".to_string(), SuperChunker::new(chunk_size))?;
///     fs.write_to_file(&mut handle, &data)?;
///     fs.close_file(handle)?;
///
///     let read_handle = fs.open_file_readonly("file")?;
///     let read = fs.read_file_complete(&read_handle)?;
///
///     let cdc_dedup_ratio = fs.cdc_dedup_ratio();
///     let res = fs.scrub().unwrap();
///     let sbc_dedup_ratio = fs.total_dedup_ratio();
///     println!("CDC dedup ratio: {}", cdc_dedup_ratio);
///     println!("SBC dedup ratio: {}", cdc_dedup_ratio);
///     println!("ScrubMeasure: {:?}", res);
///     assert_eq!(read.len(), data.len());
///     Ok(())
/// }
/// ```
///
pub struct SBCScrubber<Hash, H, C, E>
where
    Hash: SBCHash,
    H: SBCHasher<Hash = Hash>,
    C: Clusterer<Hash>,
    E: Encoder,
{
    /// Hasher used to compute similarity hashes of chunks.
    hasher: H,

    /// Clusterer used to group chunks based on similarity.
    clusterer: C,

    /// Encoder used to encode clusters into delta or simple chunks.
    encoder: E,
}

impl<Hash, H, C, E> SBCScrubber<Hash, H, C, E>
where
    Hash: SBCHash,
    H: SBCHasher<Hash = Hash>,
    C: Clusterer<Hash>,
    E: Encoder,
{
    /// Creates a new `SBCScrubber` with the given hasher, clusterer, and encoder.
    ///
    /// # Arguments
    ///
    /// * `hasher` - The hasher instance.
    /// * `clusterer` - The clusterer instance.
    /// * `encoder` - The encoder instance.
    ///
    /// # Returns
    ///
    /// A new `SBCScrubber` ready to process chunks.
    pub fn new(hasher: H, clusterer: C, encoder: E) -> Self {
        SBCScrubber {
            hasher,
            clusterer,
            encoder,
        }
    }
}

impl<CDCHash, B, D, H, C, E, Hash> Scrub<CDCHash, B, SBCKey<Hash>, SBCMap<D, Hash>>
    for SBCScrubber<Hash, H, C, E>
where
    CDCHash: ChunkHash,
    for<'data> B:
        IterableDatabase<CDCHash, DataContainer<SBCKey<Hash>>> + IntoParallelRefMutIterator<'data>,
    H: SBCHasher<Hash = Hash> + Sync,
    C: Clusterer<Hash>,
    D: Decoder + Send,
    E: Encoder + Sync,
    Hash: SBCHash,
{
    /// Applies the SBC algorithm to the chunks in the given database, storing results in the target map.
    ///
    /// This method performs hashing, clustering, and encoding in sequence, measuring the time taken by each step.
    ///
    /// # Arguments
    ///
    /// * `database` - The source database containing CDC chunks wrapped in `DataContainer`.
    /// * `target_map` - The target storage map to store processed chunks.
    ///
    /// # Returns
    ///
    /// A `ScrubMeasurements` struct containing metrics about the operation.
    fn scrub<'a>(
        &mut self,
        database: &mut B,
        target_map: &mut SBCMap<D, Hash>,
    ) -> io::Result<ScrubMeasurements>
    where
        CDCHash: 'a,
    {
        // Create a thread pool with a fixed number of threads for hashing
        let pool = ThreadPoolBuilder::new()
            .num_threads(NUM_THREADS_FOR_HASHING)
            .build()
            .unwrap();

        // Collect mutable references to all data containers from the database
        let mut mut_refs_database: Vec<_> = database.values_mut().collect();

        // Mutex-protected vector to accumulate (hash, data_container) pairs after hashing
        let sbc_hash_chunk: Mutex<Vec<_>> = Mutex::default();

        // 1. Hashing: compute similarity hashes in parallel
        let time_start = Instant::now();
        pool.install(|| {
            mut_refs_database.par_iter_mut().for_each(|data_container| {
                match data_container.extract() {
                    Data::Chunk(data) => {
                        let sbc_hash = self.hasher.calculate_hash(data.as_slice());
                        let mut chunk_sbc_hash_lock = sbc_hash_chunk.lock().unwrap();
                        chunk_sbc_hash_lock.push((sbc_hash, data_container));
                    }
                    Data::TargetChunk(_) => {
                        // Handling for target chunks not implemented yet
                        todo!()
                    }
                }
            });
        });
        let time_hashing = time_start.elapsed().as_secs_f64();
        print!("HashTime: {time_hashing:.4};");

        // 2. Clustering: group chunks by similarity
        let time_clusterize_start = time_start.elapsed();
        let (mut clusters, clusterization_report) = self
            .clusterer
            .clusterize(sbc_hash_chunk.into_inner().unwrap());
        let time_clusterize =
            time_start.elapsed().as_secs_f64() - time_clusterize_start.as_secs_f64();
        print!("ClusterTime: {time_clusterize:.4};");

        // 3. Encoding: encode clusters and store in target map
        let time_encode_start = time_start.elapsed();
        let (data_left, processed_data) = self.encoder.encode_clusters(&mut clusters, target_map);
        let time_encode = time_start.elapsed().as_secs_f64() - time_encode_start.as_secs_f64();
        print!("EncodeTime: {time_encode:.4};");

        let running_time = time_start.elapsed();

        Ok(ScrubMeasurements {
            processed_data,
            running_time,
            data_left,
            clusterization_report,
        })
    }
}
