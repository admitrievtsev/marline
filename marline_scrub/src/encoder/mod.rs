mod ddelta_encoder;
mod gdelta_encoder;
mod levenshtein_encoder;
mod xdelta_encoder;
pub mod zdelta_comprassion_error;
pub mod zdelta_encoder;
pub mod zdelta_match_pointers;

use super::sbc_scrubber::{ClusterPoint, Clusters};
use crate::decoder::Decoder;
use crate::{ChunkType, SBCHash, SBCKey, SBCMap};
use chunkfs::{Data, Database, IterableDatabase};
pub use ddelta_encoder::DdeltaEncoder;
pub use ddelta_encoder::EdeltaOptimizations;
pub use gdelta_encoder::GdeltaEncoder;
pub use levenshtein_encoder::LevenshteinEncoder;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::sync::{Arc, Mutex, MutexGuard};
pub use xdelta_encoder::XdeltaEncoder;
pub(crate) use {gdelta_encoder::GEAR, levenshtein_encoder::Action};

/// A trait for encoding data clusters using Similarity Based Chunking (SBC).
///
/// Implementors of this trait provide methods to efficiently encode data chunks
/// by creating delta codes relative to parent chunks in a hierarchy.
pub trait Encoder {
    /// Encodes a single cluster of data chunks relative to a parent hash.
    ///
    /// # Parameters
    /// - `target_map`: Mutable reference to the SBC structure tracking chunk relationships
    /// - `cluster`: Mutable slice of (hash, data container) tuples to encode
    /// - `parent_hash`: Identifier for the parent chunk used as delta reference
    ///
    /// # Returns
    /// A tuple containing:
    /// - `usize`: Amount of unprocessed data remaining in cluster
    /// - `usize`: Amount of data successfully processed and encoded
    fn encode_cluster<D: Decoder, Hash: SBCHash>(
        &self,
        target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
        cluster: &mut [ClusterPoint<Hash>],
        parent_hash: Hash,
    ) -> (usize, usize);

    /// Batch processes multiple clusters through the encoding pipeline.
    ///
    /// # Parameters
    /// - `clusters`: Mutable HashMap of parent hashes to their associated data clusters
    /// - `target_map`: Mutable reference to the SBC structure tracking relationships
    ///
    /// # Returns
    /// A tuple containing:
    /// - `usize`: Total unprocessed data across all clusters
    /// - `usize`: Total processed data across all clusters
    ///
    /// # Note
    /// Provides default implementation that iterates through all clusters,
    /// but can be overridden for optimized batch processing strategies.
    fn encode_clusters<D: Decoder + Send, Hash: SBCHash>(
        &self,
        clusters: &mut Clusters<Hash>,
        target_map: &mut SBCMap<D, Hash>,
    ) -> (usize, usize)
    where
        Self: Sync,
    {
        let pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();

        let data_left = Mutex::new(0);
        let processed_data = Mutex::new(0);
        let target_map_ref = Arc::new(Mutex::new(target_map));
        pool.install(|| {
            clusters
                .0
                .par_iter_mut()
                .for_each(|(parent_hash, cluster)| {
                    let data_analyse = self.encode_cluster(
                        target_map_ref.clone(),
                        cluster.as_mut_slice(),
                        parent_hash.clone(),
                    );

                    let mut data_left_lock = data_left.lock().unwrap();
                    *data_left_lock += data_analyse.0;

                    let mut processed_data_lock = processed_data.lock().unwrap();
                    *processed_data_lock += data_analyse.1;
                });
        });
        (
            data_left.into_inner().unwrap(),
            processed_data.into_inner().unwrap(),
        )
    }
}

/// Encodes a sequence of raw bytes as an INSERT instruction in delta encoding format.
///
/// # Format Specification
/// The INSERT instruction is encoded as:
/// - 3 bytes: Length of the data (lower 23 bits) with MSB set to 1 (flag)
/// - N bytes: Raw data bytes to be inserted
///
/// # Arguments
/// * `insert_data` - The raw byte sequence to be inserted.
///   Maximum length supported is 2^23-1 bytes.
/// * `delta_code` - Output buffer where the encoded instruction will be appended.
///   Must have enough capacity for 3 + insert_data.len() bytes.
fn encode_insert_instruction(insert_data: Vec<u8>, delta_code: &mut Vec<u8>) {
    let len_bytes = &mut (insert_data.len() as u32).to_ne_bytes()[..3];
    len_bytes[2] |= 1 << 7;
    delta_code.extend_from_slice(len_bytes);
    delta_code.extend_from_slice(&insert_data);
}

/// Encodes a COPY instruction.
///
/// A COPY instruction consists of:
/// - 3 bytes: Length of the data to copy.
/// - 3 bytes: Offset in the source data where to copy from.
///
/// # Parameters
/// * `equal_part_len` - Length of the data to copy (must be ≤ 2^24-1).
/// * `copy_instruction_offset` - Offset in the source data where the matching block begins (must be ≤ 2^24-1).
/// * `delta_code` - Output buffer where the encoded instruction will be appended.
fn encode_copy_instruction(
    equal_part_length: usize,
    copy_instruction_offset: usize,
    delta_code: &mut Vec<u8>,
) {
    let copy_instruction_len = &equal_part_length.to_ne_bytes()[..3];
    let copy_instruction_offset = &copy_instruction_offset.to_ne_bytes()[..3];
    delta_code.extend_from_slice(copy_instruction_len);
    delta_code.extend_from_slice(copy_instruction_offset);
}

fn count_delta_chunks_with_hash<D: Decoder, Hash: SBCHash>(
    target_map: &MutexGuard<&mut SBCMap<D, Hash>>,
    hash: &Hash,
) -> u16 {
    let count = target_map
        .iterator()
        .filter(|(sbc_key, _)| {
            sbc_key.hash == *hash
                && match sbc_key.chunk_type {
                    ChunkType::Delta {
                        parent_hash: _,
                        number: _,
                    } => true,
                    ChunkType::Simple => false,
                }
        })
        .count();
    count as u16
}

fn find_empty_cell<D: Decoder, Hash: SBCHash>(
    target_map: &MutexGuard<&mut SBCMap<D, Hash>>,
    hash: &Hash,
) -> Hash {
    let mut left = hash.clone();
    let mut right = hash.next_hash();
    loop {
        if target_map.contains(&SBCKey {
            hash: left.clone(),
            chunk_type: ChunkType::Simple,
        }) {
            left = left.last_hash();
        } else {
            return left;
        }

        if target_map.contains(&SBCKey {
            hash: right.clone(),
            chunk_type: ChunkType::Simple,
        }) {
            right = right.next_hash();
        } else {
            return right;
        }
    }
}

fn encode_simple_chunk<D: Decoder, Hash: SBCHash>(
    target_map: &mut MutexGuard<&mut SBCMap<D, Hash>>,
    data: &[u8],
    hash: Hash,
) -> (usize, SBCKey<Hash>) {
    let sbc_hash = SBCKey {
        hash: find_empty_cell(target_map, &hash),
        chunk_type: ChunkType::Simple,
    };

    let _ = target_map.insert(sbc_hash.clone(), data.to_vec());

    (data.len(), sbc_hash)
}

struct ParentChunkInCluster {
    index: i32,
    parent_data: Vec<u8>,
    data_left: usize,
}

fn get_parent_data<D: Decoder, Hash: SBCHash>(
    target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
    parent_hash: Hash,
    cluster: &mut [ClusterPoint<Hash>],
) -> ParentChunkInCluster {
    let mut target_map_lock = target_map.lock().unwrap();
    match target_map_lock.get(&SBCKey {
        hash: parent_hash.clone(),
        chunk_type: ChunkType::Simple,
    }) {
        Ok(parent_data) => ParentChunkInCluster {
            index: -1,
            parent_data,
            data_left: 0,
        },
        Err(_) => {
            let (_, parent_data_container) = &mut cluster[0];
            let parent_data = match parent_data_container.extract() {
                Data::Chunk(data) => data.clone(),
                Data::TargetChunk(_) => panic!(),
            };
            let (data_left, parent_sbc_hash) =
                encode_simple_chunk(&mut target_map_lock, parent_data.as_slice(), parent_hash);

            parent_data_container.make_target(vec![parent_sbc_hash]);
            ParentChunkInCluster {
                index: 0,
                parent_data,
                data_left,
            }
        }
    }
}
