use crate::SBCHash;
use crate::decoder::Decoder;
use crate::encoder::gdelta_encoder::GEAR;
use crate::encoder::{
    Encoder, count_delta_chunks_with_hash, encode_copy_instruction, encode_insert_instruction,
    get_parent_data,
};
use crate::sbc_scrubber::ClusterPoint;
use crate::{ChunkType, SBCKey, SBCMap};
use chunkfs::{Data, Database};
use fasthash::spooky;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One kilobyte.
const KB: usize = 1024;
/// Expected arithmetic mean of all chunks present within a cluster (calculated empirically).
const AVERAGE_CHUNK_SIZE: usize = 8 * KB;
/// Threshold that determines when the Gear hash (fp) points to a chunk boundary.
const CHUNK_THRESHOLD: u64 = AVERAGE_CHUNK_SIZE as u64 / 2;

/// Use this enum when creating a DdeltaEncoder if you want to use the optimized version of Ddelta (Edelta).
pub enum EdeltaOptimizations {
    /// Use if speed is important.
    SpeedIsPriority,
    /// Use if high compression ratio is important.
    CompressionIsPriority,
}

/// Ddelta compression encoder.
pub struct DdeltaEncoder {
    edelta_optimizations: Option<EdeltaOptimizations>,
}

impl Default for DdeltaEncoder {
    /// Creates DdeltaEncoder without Edelta optimizations.
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder for DdeltaEncoder {
    /// Encodes a cluster of data chunks using Ddelta compression against a parent chunk.
    ///
    /// # Arguments
    /// * `target_map` - Thread-safe reference to the chunk storage map (Arc<Mutex>).
    /// * `cluster` - Mutable slice of ClusterPoints to process.
    /// * `parent_hash` - Hash of the suggested parent chunk for delta reference.
    ///
    /// # Returns
    /// A tuple containing:
    /// 1. `usize` - Total bytes of data that couldn't be delta-encoded (left as-is).
    /// 2. `usize` - Total bytes of processed delta-encoded data.
    fn encode_cluster<D: Decoder, Hash: SBCHash>(
        &self,
        target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
        cluster: &mut [ClusterPoint<Hash>],
        parent_hash: Hash,
    ) -> (usize, usize) {
        let mut processed_data = 0;
        let parent_chunk = get_parent_data(target_map.clone(), parent_hash.clone(), cluster);
        let mut data_left = parent_chunk.data_left;
        let parent_data = parent_chunk.parent_data;
        let source_chunks = gear_chunking(&parent_data);
        let mut source_chunks_indices = build_chunks_indices(&source_chunks);

        for (chunk_id, (hash, data_container)) in cluster.iter_mut().enumerate() {
            if parent_chunk.index > -1 && chunk_id == parent_chunk.index as usize {
                continue;
            }
            let mut target_hash = SBCKey::default();
            match data_container.extract() {
                Data::Chunk(data) => {
                    let (left_in_delta_chunk, processed_in_delta_chunk, sbc_hash) = self
                        .encode_delta_chunk(
                            target_map.clone(),
                            data,
                            hash.clone(),
                            parent_data.as_slice(),
                            &mut source_chunks_indices,
                            parent_hash.clone(),
                        );
                    data_left += left_in_delta_chunk;
                    processed_data += processed_in_delta_chunk;
                    target_hash = sbc_hash;
                }
                Data::TargetChunk(_) => {}
            }
            data_container.make_target(vec![target_hash]);
        }
        (data_left, processed_data)
    }
}

impl DdeltaEncoder {
    /// Use EdeltaOptimizations enum when creating a DdeltaEncoder if you want to use the optimized version of Ddelta (Edelta).
    /// Or pass None as a parameter.
    pub fn new() -> DdeltaEncoder {
        DdeltaEncoder {
            edelta_optimizations: None,
        }
    }

    pub fn new_with_edelta_optimizations(
        edelta_optimizations: EdeltaOptimizations,
    ) -> DdeltaEncoder {
        DdeltaEncoder {
            edelta_optimizations: Some(edelta_optimizations),
        }
    }

    /// Encodes a single data chunk using delta compression against a reference.
    ///
    /// # Arguments
    /// * `target_map` - Shared map for storing compressed chunks.
    /// * `target_data` - The data to be compressed.
    /// * `target_hash` - Hash identifier for the target data.
    /// * `source_data` - Reference data to compare against.
    /// * `source_chunks_indices` - Key is the chunk hash, value is its first position in the source data.
    /// * `source_hash` - Hash identifier for the parent/reference data.
    ///
    /// # Returns
    /// 1. Number of uncompressed bytes.
    /// 2. Total bytes processed.
    /// 3. Storage key for the compressed delta.
    fn encode_delta_chunk<D: Decoder, Hash: SBCHash>(
        &self,
        target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
        target_data: &[u8],
        target_hash: Hash,
        source_data: &[u8],
        source_chunks_indices: &mut HashMap<u64, usize>,
        source_hash: Hash,
    ) -> (usize, usize, SBCKey<Hash>) {
        let mut delta_code: Vec<u8> = Vec::new();
        let target_chunks = gear_chunking(target_data);

        for mut target_chunk_position in 0..target_chunks.len() {
            let target_chunk = target_chunks[target_chunk_position];
            match self.edelta_optimizations {
                Some(EdeltaOptimizations::SpeedIsPriority) => process_target_chunk_with_edelta(
                    source_data,
                    target_data,
                    source_chunks_indices,
                    &target_chunks,
                    &mut target_chunk_position,
                    &mut delta_code,
                    EdeltaOptimizations::SpeedIsPriority,
                ),
                Some(EdeltaOptimizations::CompressionIsPriority) => {
                    process_target_chunk_with_edelta(
                        source_data,
                        target_data,
                        source_chunks_indices,
                        &target_chunks,
                        &mut target_chunk_position,
                        &mut delta_code,
                        EdeltaOptimizations::CompressionIsPriority,
                    );
                }
                None => process_target_chunk_with_ddelta(
                    source_data,
                    source_chunks_indices,
                    target_chunk,
                    &mut delta_code,
                ),
            }

            if target_chunk_position >= target_chunks.len() {
                break;
            }
        }

        let (processed_data, sbc_hash) =
            store_delta_chunk(target_map, target_hash, source_hash, delta_code);
        (0, processed_data, sbc_hash)
    }
}

fn process_target_chunk_with_edelta(
    source_data: &[u8],
    target_data: &[u8],
    source_chunks_indices: &mut HashMap<u64, usize>,
    target_chunks: &[&[u8]],
    target_chunk_position: &mut usize,
    delta_code: &mut Vec<u8>,
    edelta_optimizations: EdeltaOptimizations,
) {
    if *target_chunk_position >= target_chunks.len() {
        return;
    }

    let mut target_chunk = target_chunks[*target_chunk_position];
    match edelta_optimizations {
        EdeltaOptimizations::SpeedIsPriority => {
            if let Some((
                start_match_position_in_source_data,
                number_of_processed_chunks,
                match_length,
                length_of_unprocessed_residue,
            )) = find_match_compression_is_priority(
                source_data,
                source_chunks_indices,
                *target_chunk_position,
                target_chunks,
            ) {
                encode_copy_instruction(
                    match_length,
                    start_match_position_in_source_data,
                    delta_code,
                );
                *target_chunk_position += number_of_processed_chunks;
                if length_of_unprocessed_residue == 0 {
                    return;
                }

                target_chunk = target_chunks[*target_chunk_position - 1];
                process_target_chunk_with_ddelta(
                    source_data,
                    source_chunks_indices,
                    &target_chunk[target_chunk.len() - length_of_unprocessed_residue..],
                    delta_code,
                );
            } else {
                encode_insert_instruction(target_chunk.to_vec(), delta_code);
                *target_chunk_position += 1;
            };
        }
        EdeltaOptimizations::CompressionIsPriority => {
            if let Some((
                start_match_position_in_source_data,
                number_of_processed_chunks,
                match_length,
                length_of_unprocessed_residue,
            )) = find_match_compression_is_priority(
                source_data,
                source_chunks_indices,
                *target_chunk_position,
                target_chunks,
            ) {
                encode_copy_instruction(
                    match_length,
                    start_match_position_in_source_data,
                    delta_code,
                );
                let mut start_match_in_target_data: usize = 0;
                for current_target_chunk in target_chunks.iter().take(*target_chunk_position) {
                    start_match_in_target_data += current_target_chunk.len();
                }
                let chunk_hash = spooky::hash64(
                    &target_data
                        [start_match_in_target_data..start_match_in_target_data + match_length],
                );

                source_chunks_indices
                    .entry(chunk_hash)
                    .or_insert(start_match_position_in_source_data);
                *target_chunk_position += number_of_processed_chunks;
                if length_of_unprocessed_residue == 0 {
                    return;
                }

                target_chunk = target_chunks[*target_chunk_position - 1];
                process_target_chunk_with_ddelta(
                    source_data,
                    source_chunks_indices,
                    &target_chunk[target_chunk.len() - length_of_unprocessed_residue..],
                    delta_code,
                );
            } else {
                encode_insert_instruction(target_chunk.to_vec(), delta_code);
                *target_chunk_position += 1;
            };
        }
    }
}

/// Encodes a part in the target data without Edelta optimizations.
fn process_target_chunk_with_ddelta(
    source_data: &[u8],
    source_chunks_indices: &HashMap<u64, usize>,
    target_chunk: &[u8],
    delta_code: &mut Vec<u8>,
) {
    match find_match_ddelta(source_data, source_chunks_indices, target_chunk) {
        Some(start_of_match_in_source_data) => {
            encode_copy_instruction(
                target_chunk.len(),
                start_of_match_in_source_data,
                delta_code,
            );
        }
        None => {
            encode_insert_instruction(target_chunk.to_vec(), delta_code);
        }
    }
}

/// Stores a delta-encoded chunk in the shared chunk map.
///
/// # Arguments
/// * `target_map` - Thread-safe reference to the chunk storage map (Arc<Mutex>).
/// * `target_hash` - Content hash of the original chunk data.
/// * `source_hash` - Hash of the parent chunk this delta is based on.
/// * `delta_code` - Raw delta-encoded data to store.
/// * `zstd_flag` - Whether to apply zstd compression to the delta data.
///
/// # Returns
/// A tuple containing:
/// 1. `usize` - Final size of the stored data (after optional compression).
/// 2. `SBCKey<Hash>` - Key under which the chunk was stored.
fn store_delta_chunk<D: Decoder, Hash: SBCHash>(
    target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
    hash: Hash,
    parent_hash: Hash,
    delta_code: Vec<u8>,
) -> (usize, SBCKey<Hash>) {
    let mut target_map_lock = target_map.lock().unwrap();
    let number_delta_chunk = count_delta_chunks_with_hash(&target_map_lock, &hash);
    let sbc_hash = SBCKey {
        hash,
        chunk_type: ChunkType::Delta {
            parent_hash,
            number: number_delta_chunk,
        },
    };

    let processed_data = delta_code.len();
    let _ = target_map_lock.insert(sbc_hash.clone(), delta_code);

    (processed_data, sbc_hash)
}

/// Finds the longest matching byte sequence between source data and target chunks using delta compression.
///
/// This function implements Scheme 1 of the Edelta algorithm, which extends matches across chunk boundaries
/// while maintaining the original chunk indexing for the base data.
///
/// # Arguments
/// * `source_data` - The complete base data as a contiguous byte slice
/// * `source_chunks_indices` - Precomputed hash map of chunk hashes to their positions in `source_data`
/// * `target_chunks` - Target data split into chunks (slice of byte slices)
/// * `target_chunk_position` - Starting chunk index in `target_chunks` to begin matching
///
/// # Returns
/// `Option<(usize, usize, usize, usize)>` where:
///
/// * `Some((
/// start_match_position_in_source_data,
/// number_of_processed_chunks,
/// match_length,
/// length_of_unprocessed_residue
/// ))` - Start position in `source_data`, number of the processed chunks, length of the longest match and
///   the number of bytes in the last chunk that remained unprocessed.
/// * `None` - If no match found or invalid input position
fn find_match_compression_is_priority(
    source_data: &[u8],
    source_chunks_indices: &HashMap<u64, usize>,
    target_chunk_position: usize,
    target_chunks: &[&[u8]],
) -> Option<(usize, usize, usize, usize)> {
    if target_chunk_position > target_chunks.len() {
        return None;
    }

    let start_of_match_in_source_data = find_match_ddelta(
        source_data,
        source_chunks_indices,
        target_chunks[target_chunk_position],
    )?;
    let mut number_of_processed_chunks = 1;
    let mut source_byte_index =
        start_of_match_in_source_data + target_chunks[target_chunk_position].len();

    let mut match_length = target_chunks[target_chunk_position].len();
    let mut target_chunk_position = target_chunk_position + 1;
    while target_chunk_position < target_chunks.len() {
        let mut target_chunk = target_chunks[target_chunk_position];

        let mut target_byte_index = 0usize;
        while source_data[source_byte_index] == target_chunk[target_byte_index] {
            match_length += 1;

            source_byte_index += 1;
            target_byte_index += 1;
            target_byte_index %= target_chunk.len();

            if source_byte_index >= source_data.len() {
                number_of_processed_chunks += 1;
                let length_of_unprocessed_residue =
                    (target_chunk.len() - target_byte_index) % target_chunk.len();
                return Some((
                    start_of_match_in_source_data,
                    number_of_processed_chunks,
                    match_length,
                    length_of_unprocessed_residue,
                ));
            }

            if target_byte_index == 0 {
                target_chunk_position += 1;
                if target_chunk_position >= target_chunks.len() {
                    number_of_processed_chunks += 1;
                    return Some((
                        start_of_match_in_source_data,
                        number_of_processed_chunks,
                        match_length,
                        0,
                    ));
                }

                target_chunk = target_chunks[target_chunk_position];
                break;
            }
        }

        number_of_processed_chunks += 1;
        if source_data[source_byte_index] != target_chunk[target_byte_index] {
            let length_of_unprocessed_residue =
                (target_chunk.len() - target_byte_index) % target_chunk.len();
            return Some((
                start_of_match_in_source_data,
                number_of_processed_chunks,
                match_length,
                length_of_unprocessed_residue,
            ));
        }

        if target_byte_index != 0 {
            target_chunk_position += 1;
        }
    }

    Some((
        start_of_match_in_source_data,
        number_of_processed_chunks,
        match_length,
        0,
    ))
}

/// Finds a matching chunk in source data for the given target chunk.
///
/// # Arguments
/// * `source_data` - The original/reference data slice to search in
/// * `source_chunks_indices` - Precomputed hash map of chunk hashes to their positions in source_data
/// * `target_data` - The chunk of data to find in the source
///
/// # Returns
/// * `Some(usize)` - The starting position of the matching chunk in source_data if found
/// * `None` - If no matching chunk was found
fn find_match_ddelta(
    source_data: &[u8],
    source_chunks_indices: &HashMap<u64, usize>,
    target_chunk: &[u8],
) -> Option<usize> {
    let target_hash = spooky::hash64(target_chunk);
    let &source_position = source_chunks_indices.get(&target_hash)?;

    if source_position + target_chunk.len() > source_data.len() {
        return None;
    }

    let source_slice = &source_data[source_position..source_position + target_chunk.len()];
    if source_slice != target_chunk {
        return None;
    }

    Some(source_position)
}

/// Creates an index of chunks for quick matching.
///
/// # Arguments
/// * `source_chunks` - vector of chunks from the base data block.
///
/// # Returns
/// Hash table, where key is the chunk hash, value is its first position in the source data.
fn build_chunks_indices(source_chunks: &Vec<&[u8]>) -> HashMap<u64, usize> {
    let mut chunks_indices: HashMap<u64, usize> = HashMap::new();
    let mut current_index: usize = 0;
    for chunk in source_chunks {
        let chunk_hash = spooky::hash64(chunk);
        chunks_indices.entry(chunk_hash).or_insert(current_index);
        current_index += chunk.len();
    }

    chunks_indices
}

/// Splits input data into chunks using Gear-based Content-Defined Chunking (CDC) algorithm.
///
/// # Parameters
/// * `data` - Input byte slice to be chunked.
///
/// # Returns
/// Vector of byte slices (chunks) referencing the original data.
fn gear_chunking(data: &[u8]) -> Vec<&[u8]> {
    let mut source_chunks: Vec<&[u8]> = Vec::new();
    let mut current_window_hash: u64 = 0;
    let mut start_current_chunk = 0;

    let mask = (1 << AVERAGE_CHUNK_SIZE.next_power_of_two().trailing_zeros()) - 1;
    let mut data_index = 0;
    while data_index < data.len() {
        current_window_hash =
            (current_window_hash << 1).wrapping_add(GEAR[data[data_index] as usize]);

        if (current_window_hash & mask) == CHUNK_THRESHOLD {
            source_chunks.push(&data[start_current_chunk..data_index]);
            start_current_chunk = data_index;
        }

        data_index += 1;
    }

    if start_current_chunk < data.len() {
        source_chunks.push(&data[start_current_chunk..data.len()]);
    }

    source_chunks
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::decoder;
    use crate::encoder::ddelta_encoder::EdeltaOptimizations::{
        CompressionIsPriority, SpeedIsPriority,
    };
    use crate::encoder::encode_simple_chunk;
    use crate::hasher::AronovichHash;
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};

    #[test]
    fn process_target_chunk_with_edelta_should_process_full_match_with_compression_priority() {
        let source_data = b"prefix_match_suffix";
        let source_chunks: Vec<&[u8]> = vec![b"prefix_", b"match_", b"suffix"];
        let target_chunks: Vec<&[u8]> = vec![b"match_"];

        let mut source_indices = build_chunks_indices(&source_chunks);
        let mut position = 0;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            source_data,
            b"match_",
            &mut source_indices,
            &target_chunks,
            &mut position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert_eq!(position, 1);
        assert!(!delta_code.is_empty());
    }

    #[test]
    fn process_target_chunk_with_edelta_should_insert_when_no_match_found() {
        let source_data = b"source_data";
        let source_chunks: Vec<&[u8]> = vec![b"source", b"_data"];
        let target_chunks: Vec<&[u8]> = vec![b"no_match"];

        let mut source_indices = build_chunks_indices(&source_chunks);
        let mut position = 0;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            source_data,
            b"no_match",
            &mut source_indices,
            &target_chunks,
            &mut position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert_eq!(position, 1);
        assert!(!delta_code.is_empty());
    }

    #[test]
    fn process_target_chunk_with_edelta_should_handle_partial_match_with_residue() {
        let source_data = b"data_part1_part2";
        let source_chunks: Vec<&[u8]> = vec![b"data_", b"part1_", b"part2"];
        let target_chunks: Vec<&[u8]> = vec![b"part1_", b"par"];

        let mut source_indices = build_chunks_indices(&source_chunks);
        let mut position = 0;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            source_data,
            b"part1_par",
            &mut source_indices,
            &target_chunks,
            &mut position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert_eq!(position, 2);
        assert!(delta_code.len() > 1);
    }

    #[test]
    fn process_target_chunk_with_edelta_should_skip_processing_when_position_out_of_bounds() {
        let source_data = b"data";
        let source_chunks: Vec<&[u8]> = vec![b"data"];
        let target_chunks: Vec<&[u8]> = vec![b"data"];

        let mut source_indices = build_chunks_indices(&source_chunks);
        let mut position = 1;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            source_data,
            b"data",
            &mut source_indices,
            &target_chunks,
            &mut position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert_eq!(position, 1);
        assert!(delta_code.is_empty());
    }

    #[test]
    fn process_target_chunk_with_edelta_should_process_multiple_chunks_in_extended_match() {
        let source_data = b"chunk1_chunk2_chunk3";
        let source_chunks: Vec<&[u8]> = vec![b"chunk1_", b"chunk2_", b"chunk3"];
        let target_chunks: Vec<&[u8]> = vec![b"chunk1_", b"chunk2_"];

        let mut source_indices = build_chunks_indices(&source_chunks);
        let mut position = 0;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            source_data,
            b"chunk1_chunk2_",
            &mut source_indices,
            &target_chunks,
            &mut position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert_eq!(position, 2);
        assert!(!delta_code.is_empty());
    }

    #[test]
    fn process_target_chunk_with_edelta_should_handle_empty_target_chunk() {
        let source_data = b"data";
        let source_chunks: Vec<&[u8]> = vec![b"data"];
        let target_chunks: Vec<&[u8]> = vec![b""];

        let mut source_indices = build_chunks_indices(&source_chunks);
        let mut position = 0;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            source_data,
            b"",
            &mut source_indices,
            &target_chunks,
            &mut position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert_eq!(position, 1);
    }

    #[test]
    fn process_target_chunk_with_edelta_should_process_two_chunks() {
        let source_data = b"abcdefghijklmnopqrstuvwxyz".to_vec();
        let target_chunks: Vec<&[u8]> = vec![b"cdefgh", b"ijklmn"];

        let source_chunks: Vec<&[u8]> = vec![b"ab", b"cdefgh", b"ijklmnop", b"qrstuvwxyz"];
        let mut source_chunks_indices = build_chunks_indices(&source_chunks);

        let mut target_chunk_position = 0;
        let mut delta_code = Vec::new();

        process_target_chunk_with_edelta(
            &source_data,
            b"cdefghijklmn",
            &mut source_chunks_indices,
            &target_chunks,
            &mut target_chunk_position,
            &mut delta_code,
            CompressionIsPriority,
        );

        assert!(!delta_code.is_empty());
        assert_eq!(target_chunk_position, 2);
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_partial_match_at_chunk_boundary() {
        let source_data = b"prefix_data_match_suffix";
        let source_chunks: Vec<&[u8]> = vec![b"prefix_", b"data_", b"match_", b"suffix"];
        let target_chunks: Vec<&[u8]> = vec![b"data_", b"matc"];

        let source_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &source_indices, 0, &target_chunks),
            Some((7, 2, 9, 0))
        );
    }

    #[test]
    fn find_match_compression_is_priority_should_return_none_when_no_initial_chunk_match() {
        let source_data = b"source_data";
        let source_chunks: Vec<&[u8]> = vec![b"sour", b"ce_d", b"ata"];
        let target_chunks: Vec<&[u8]> = vec![b"no_match", b"data"];

        let source_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &source_indices, 0, &target_chunks),
            None
        );
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_source_exhaustion_during_extended_match() {
        let source_data = b"short_source";
        let source_chunks: Vec<&[u8]> = vec![b"short_", b"source"];
        let target_chunks: Vec<&[u8]> = vec![b"short_", b"source", b"extra"];

        let source_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &source_indices, 0, &target_chunks),
            Some((0, 2, 12, 0))
        );
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_mismatch_in_middle_of_extended_match() {
        let source_data = b"match_part1_part2_part3";
        let source_chunks: Vec<&[u8]> = vec![b"match_", b"part1_", b"part2_", b"part3"];
        let target_chunks: Vec<&[u8]> = vec![b"match_", b"part1_", b"XXXXX_", b"part3"];

        let source_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &source_indices, 0, &target_chunks),
            Some((0, 2, 12, 0))
        );
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_single_byte_chunks() {
        let source_data = b"abcdef";
        let source_chunks: Vec<&[u8]> = vec![b"a", b"b", b"c", b"d", b"e", b"f"];
        let target_chunks: Vec<&[u8]> = vec![b"c", b"d", b"e"];

        let source_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &source_indices, 0, &target_chunks),
            Some((2, 3, 3, 0))
        );
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_variable_size_chunks() {
        let source_data = b"abc_defgh_ijklmn";
        let source_chunks: Vec<&[u8]> = vec![b"abc_", b"defgh_", b"ijklmn"];
        let target_chunks: Vec<&[u8]> = vec![b"abc_", b"defgh_", b"ijk"];

        let source_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &source_indices, 0, &target_chunks),
            Some((0, 3, 13, 0))
        );
    }

    #[test]
    fn find_match_compression_is_priority_should_return_correct_value_for_exact_match() {
        let source_data = b"__PATTERN1__PATTERN2__";
        let target_chunks: Vec<&[u8]> = vec![b"_PATTERN2__"];
        let source_chunks = vec![
            &source_data[0..source_data.len() / 2],
            &source_data[source_data.len() / 2..],
        ];
        let chunk_indices = build_chunks_indices(&source_chunks);

        assert_eq!(
            find_match_compression_is_priority(source_data, &chunk_indices, 0, &target_chunks),
            Some((11, 1, 11, 0))
        )
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_one_chunk() {
        let source_data = b"test1test2test";
        let source_chunks: Vec<&[u8]> = vec![b"test", b"1test", b"2test"];
        let target_chunks: Vec<&[u8]> = vec![b"test", b"1test", b"#test"];
        let source_chunks_indices = build_chunks_indices(&source_chunks);
        assert_eq!(
            find_match_compression_is_priority(
                source_data,
                &source_chunks_indices,
                0,
                &target_chunks,
            ),
            Some((0, 2, 9, 0))
        )
    }

    #[test]
    fn find_match_compression_is_priority_should_handle_two_chunks() {
        let source_data = b"test1test2test";
        let source_chunks: Vec<&[u8]> = vec![b"test", b"1test", b"2test"];
        let target_chunks: Vec<&[u8]> = vec![b"test", b"1test", b"2te#t"];
        let source_chunks_indices = build_chunks_indices(&source_chunks);
        assert_eq!(
            find_match_compression_is_priority(
                source_data,
                &source_chunks_indices,
                1,
                &target_chunks,
            ),
            Some((4, 2, 8, 2))
        )
    }

    #[test]
    fn find_match_should_return_none_for_empty_source_or_target() {
        let empty_data = &[];
        let empty_indices = HashMap::new();
        assert_eq!(
            find_match_ddelta(empty_data, &empty_indices, b"test"),
            None,
            "Empty source should return None"
        );

        let non_empty_data = b"valid_data";
        let chunks = gear_chunking(non_empty_data);
        let indices = build_chunks_indices(&chunks);
        assert_eq!(
            find_match_ddelta(non_empty_data, &indices, empty_data),
            None,
            "Empty target should return None"
        );
    }

    #[test]
    fn find_match_should_return_none_for_non_matching_data() {
        let source_data = vec![0u8; AVERAGE_CHUNK_SIZE * 2];
        let target_data = vec![1u8; AVERAGE_CHUNK_SIZE];

        let source_chunks = gear_chunking(&source_data);
        let source_indices = build_chunks_indices(&source_chunks);
        assert_eq!(
            find_match_ddelta(&source_data, &source_indices, &target_data),
            None,
            "Non-matching data should return None"
        );
    }

    #[test]
    fn find_match_should_return_position_for_exact_match() {
        let data = b"__PATTERN1__PATTERN2__";
        let pattern = b"__PATTERN1_";
        let chunks = vec![&data[0..data.len() / 2], &data[data.len() / 2..]];

        let chunk_indices = build_chunks_indices(&chunks);

        assert_eq!(
            find_match_ddelta(data, &chunk_indices, pattern),
            Some(0),
            "Should find pattern at known position"
        );
    }

    #[test]
    fn build_chunks_indices_should_map_chunks_to_correct_positions() {
        let chunks: Vec<&[u8]> = vec![&[1u8; AVERAGE_CHUNK_SIZE], &[2u8; AVERAGE_CHUNK_SIZE]];

        let indices = build_chunks_indices(&chunks);
        assert_eq!(
            indices.get(&spooky::hash64(chunks[0])),
            Some(&0),
            "First chunk should be at position 0"
        );
        assert_eq!(
            indices.get(&spooky::hash64(chunks[1])),
            Some(&AVERAGE_CHUNK_SIZE),
            "Second chunk should be at position AVERAGE_CHUNK_SIZE"
        );
    }

    #[test]
    fn build_chunks_indices_should_handle_duplicate_hashes_correctly() {
        let chunks: Vec<&[u8]> = vec![&[1u8; AVERAGE_CHUNK_SIZE], &[1u8; AVERAGE_CHUNK_SIZE]];

        let indices = build_chunks_indices(&chunks);
        let hash = spooky::hash64(chunks[0]);
        assert_eq!(
            Some(&0),
            indices.get(&hash),
            "Only first position should be stored for duplicates"
        );
        assert_eq!(
            indices.len(),
            1,
            "HashMap should contain only one entry for duplicate chunks"
        );
    }

    #[test]
    fn gear_chunking_should_handle_empty_data() {
        let data = &[];
        assert_eq!(gear_chunking(data).len(), 0);
    }

    #[test]
    fn gear_chunking_should_handle_data_smaller_than_chunk() {
        let data = b"abc";
        let chunks = gear_chunking(data);
        assert_eq!(chunks, vec![b"abc".to_vec()]);
    }

    #[test]
    fn gear_chunking_should_return_chunk_for_exact_chunk_boundary() {
        let data = b"abcdefgh";
        let chunks = gear_chunking(data);
        assert_eq!(chunks, vec![b"abcdefgh".to_vec()]);
    }

    #[test]
    fn gear_chunking_should_split_data_into_multiple_chunks() {
        let mut rng = rand::thread_rng();
        let mut data = vec![0u8; AVERAGE_CHUNK_SIZE * 1000];
        rng.fill(&mut data[..]);

        let chunks = gear_chunking(&data);
        assert!(
            chunks.len() > 1,
            "Data should be split into multiple chunks"
        );
    }

    #[test]
    fn test_restore_similarity_chunk_1_byte_diff() {
        let mut data: Vec<u8> = generate_test_data();
        let data2 = data.clone();
        if data[15] < 255 {
            data[15] = 255;
        } else {
            data[15] = 0;
        }

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), SpeedIsPriority);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_2_neighbor_byte_diff() {
        let mut data: Vec<u8> = generate_test_data();
        let data2 = data.clone();
        if data[15] < 255 {
            data[15] = 255;
        } else {
            data[15] = 0;
        }
        if data[16] < 255 {
            data[16] = 255;
        } else {
            data[16] = 0;
        }

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), SpeedIsPriority);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_2_byte_diff() {
        let mut data: Vec<u8> = generate_test_data();
        let data2 = data.clone();
        if data[15] < 255 {
            data[15] = 255;
        } else {
            data[15] = 0;
        }
        if data[106] < 255 {
            data[106] = 255;
        } else {
            data[106] = 0;
        }

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), CompressionIsPriority);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_offset_left() {
        let data: Vec<u8> = generate_test_data();
        let data2 = data[15..].to_vec();

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), SpeedIsPriority);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_offset_right() {
        let data: Vec<u8> = generate_test_data();
        let data2 = data[..8000].to_vec();

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), CompressionIsPriority);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_offset() {
        let data: Vec<u8> = generate_test_data();
        let mut data2 = data[15..8000].to_vec();
        data2[0] /= 3;
        data2[7000] /= 3;

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), CompressionIsPriority);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_cyclic_shift_right() {
        let data: Vec<u8> = generate_test_data();
        let mut data2 = data.clone();
        data2.extend(&data[8000..]);

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), SpeedIsPriority);

        assert_ne!(data, []);
        assert_eq!(
            sbc_key.chunk_type,
            ChunkType::Delta {
                parent_hash: AronovichHash::new_with_u32(0),
                number: 0
            }
        );
        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_cyclic_shift_left() {
        let data: Vec<u8> = generate_test_data_deterministic(42);
        let mut data2 = data[..192].to_vec();
        data2.extend(&data);

        let (sbc_map, sbc_key) =
            create_map_and_key(data.as_slice(), data2.as_slice(), SpeedIsPriority);

        assert_ne!(data, []);
        assert_eq!(
            sbc_key.chunk_type,
            ChunkType::Delta {
                parent_hash: AronovichHash::new_with_u32(0),
                number: 0
            }
        );
        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    fn generate_test_data() -> Vec<u8> {
        const TEST_DATA_SIZE: usize = 8192;
        (0..TEST_DATA_SIZE).map(|_| rng.random()).collect()
    }

    fn generate_test_data_deterministic(seed: u64) -> Vec<u8> {
        const TEST_DATA_SIZE: usize = 8192;
        let mut rng = StdRng::seed_from_u64(seed);
        (0..TEST_DATA_SIZE).map(|_| rng.random()).collect()
    }

    fn create_map_and_key<'a>(
        data: &'a [u8],
        data2: &'a [u8],
        edelta_optimizations: EdeltaOptimizations,
    ) -> (
        SBCMap<decoder::GdeltaDecoder, AronovichHash>,
        SBCKey<AronovichHash>,
    ) {
        let source_chunks = gear_chunking(data);
        let mut word_hash_offsets = build_chunks_indices(&source_chunks);
        let mut binding = SBCMap::new(decoder::GdeltaDecoder::default());
        let sbc_map = Arc::new(Mutex::new(&mut binding));

        let (_, sbc_key) = encode_simple_chunk(
            &mut sbc_map.lock().unwrap(),
            data,
            AronovichHash::new_with_u32(0),
        );
        let ddelta_encoder = DdeltaEncoder::new_with_edelta_optimizations(edelta_optimizations);
        let (_, _, sbc_key_2) = ddelta_encoder.encode_delta_chunk(
            sbc_map.clone(),
            data2,
            AronovichHash::new_with_u32(3),
            data,
            &mut word_hash_offsets,
            sbc_key.hash.clone(),
        );
        (binding, sbc_key_2)
    }
}
