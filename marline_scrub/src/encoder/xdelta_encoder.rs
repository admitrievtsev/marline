use crate::sbc_scrubber::ClusterPoint;
use crate::decoder::Decoder;
use crate::encoder::{
    count_delta_chunks_with_hash, encode_copy_instruction, encode_insert_instruction,
    get_parent_data, Encoder,
};
use crate::{ChunkType, SBCHash, SBCKey, SBCMap};
use chunkfs::Data;
use chunkfs::Database;
use std::cmp::min;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zstd::stream;

const BLOCK_SIZE: usize = 16;
const ADLER_MOD: u32 = 65521;

pub struct XdeltaEncoder {
    zstd_flag: bool,
}

impl Default for XdeltaEncoder {
    fn default() -> Self {
        Self::new(false)
    }
}

impl XdeltaEncoder {
    /// Creates a new XdeltaEncoder with specified compression settings.
    ///
    /// # Arguments
    /// * `zstd_flag` - Whether to apply zstd compression to delta-encoded data:
    ///   - `true`: Apply zstd compression (level 0).
    ///   - `false`: Store raw delta instructions.
    pub fn new(zstd_flag: bool) -> Self {
        XdeltaEncoder { zstd_flag }
    }

    /// Encodes a single chunk as delta against a parent chunk using xdelta algorithm.
    ///
    /// # Type Parameters
    /// - `D`: Decoder implementation for chunk retrieval
    /// - `Hash`: Hash type implementing SBCHash
    ///
    /// # Arguments
    /// * `target_map` - Thread-safe reference to chunk storage.
    /// * `chunk_data` - Raw data to encode.
    /// * `hash` - Content hash of the chunk data.
    /// * `parent_data` - Reference parent chunk data.
    /// * `word_hash_offsets` - Precomputed block positions from parent.
    /// * `parent_hash` - Hash of the parent chunk.
    ///
    /// # Returns
    /// Tuple containing:
    /// 1. `usize` - Always 0 (represents unused data)
    /// 2. `usize` - Size of stored delta data (after optional compression)
    /// 3. `SBCKey<Hash>` - Key where delta was stored
    fn encode_delta_chunk<D: Decoder, Hash: SBCHash>(
        &self,
        target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
        chunk_data: &[u8],
        hash: Hash,
        parent_data: &[u8],
        word_hash_offsets: &HashMap<u32, Vec<usize>>,
        parent_hash: Hash,
    ) -> (usize, usize, SBCKey<Hash>) {
        let mut delta_code = Vec::new();

        let mut i = 0;
        while i + BLOCK_SIZE <= chunk_data.len() {
            let adler_hash_word = adler32(&chunk_data[i..i + BLOCK_SIZE]);

            if !word_hash_offsets.contains_key(&adler_hash_word) {
                encode_insert_sequence(
                    chunk_data,
                    &mut i,
                    word_hash_offsets,
                    &mut delta_code,
                    adler_hash_word,
                );
            } else {
                encode_copy_sequence(
                    parent_data,
                    chunk_data,
                    &mut i,
                    &mut delta_code,
                    adler_hash_word,
                    word_hash_offsets,
                )
            }
        }
        if i < chunk_data.len() {
            let remaining_data = chunk_data[i..].to_vec();
            encode_insert_instruction(remaining_data, &mut delta_code);
        }

        let (processed_data, sbc_hash) = prepare_and_store_delta_chunk(
            target_map,
            hash,
            parent_hash,
            delta_code,
            self.zstd_flag,
        );
        (0, processed_data, sbc_hash)
    }
}

impl Encoder for XdeltaEncoder {
    /// Encodes a cluster of data chunks using Xdelta compression against a parent chunk.
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
        let word_hash_offsets = create_block_hashmap(parent_data.as_slice());

        for (chunk_id, (hash, data_container)) in cluster.iter_mut().enumerate() {
            if parent_chunk.index > -1 && chunk_id == parent_chunk.index as usize {
                continue;
            }
            let mut target_hash = SBCKey::default();
            match data_container.extract() {
                Data::Chunk(data) => {
                    let (left, processed, sbc_hash) = self.encode_delta_chunk(
                        target_map.clone(),
                        data,
                        hash.clone(),
                        parent_data.as_slice(),
                        &word_hash_offsets,
                        parent_hash.clone(),
                    );
                    data_left += left;
                    processed_data += processed;
                    target_hash = sbc_hash;
                }
                Data::TargetChunk(_) => {}
            }
            data_container.make_target(vec![target_hash]);
        }
        (data_left, processed_data)
    }
}

/// Prepares and stores a delta-encoded chunk in the shared chunk map.
///
/// # Arguments
/// * `target_map` - Thread-safe reference to the chunk storage map (Arc<Mutex>).
/// * `hash` - Content hash of the original chunk data.
/// * `parent_hash` - Hash of the parent chunk this delta is based on.
/// * `delta_code` - Raw delta-encoded data to store.
/// * `zstd_flag` - Whether to apply zstd compression to the delta data.
///
/// # Returns
/// A tuple containing:
/// 1. `usize` - Final size of the stored data (after optional compression).
/// 2. `SBCKey<Hash>` - Key under which the chunk was stored.
fn prepare_and_store_delta_chunk<D: Decoder, Hash: SBCHash>(
    target_map: Arc<Mutex<&mut SBCMap<D, Hash>>>,
    hash: Hash,
    parent_hash: Hash,
    delta_code: Vec<u8>,
    zstd_flag: bool,
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

    let delta_code = if zstd_flag {
        stream::encode_all(delta_code.as_slice(), 0).unwrap()
    } else {
        delta_code
    };

    let processed_data = delta_code.len();
    let _ = target_map_lock.insert(sbc_hash.clone(), delta_code);

    (processed_data, sbc_hash)
}

/// Encodes a matching sequence as a COPY instruction.
///
/// # Arguments
/// * `parent_data` - Reference data containing the matching block.
/// * `chunk_data` - Current data being processed.
/// * `i` - Current position in `chunk_data` (updated after execution).
/// * `delta_code` - Output buffer for delta instructions.
/// * `initial_hash` - Adler-32 hash of the first block at position `i` in `chunk_data`.
/// * `word_hash_offsets` - A hash table mapping Adler-32 hashes of blocks in the parent data to their offsets. Used to detect when a match starts at the current position.
fn encode_copy_sequence(
    parent_data: &[u8],
    chunk_data: &[u8],
    i: &mut usize,
    delta_code: &mut Vec<u8>,
    initial_hash: u32,
    word_hash_offsets: &HashMap<u32, Vec<usize>>,
) {
    if *i >= chunk_data.len() || !word_hash_offsets.contains_key(&initial_hash) {
        return;
    }

    let offsets = match word_hash_offsets.get(&initial_hash) {
        Some(v) => v,
        None => return,
    };

    let mut best_len = 0;
    let mut best_offset = 0;

    for &offset in offsets {
        let max_len = min(parent_data.len() - offset, chunk_data.len() - *i);
        let mut equal_part_len = 0;

        while equal_part_len < max_len
            && parent_data[offset + equal_part_len] == chunk_data[*i + equal_part_len]
        {
            equal_part_len += 1;
        }

        if equal_part_len > best_len {
            best_len = equal_part_len;
            best_offset = offset;
        }
    }

    if best_len > 0 {
        encode_copy_instruction(best_len, best_offset, delta_code);
        *i += best_len;
    } else {
        let end = min(*i + BLOCK_SIZE, chunk_data.len());
        let insert_data = chunk_data[*i..end].to_vec();
        encode_insert_instruction(insert_data, delta_code);
        *i = end;
    }
}

/// Encodes a matching sequence as a INSERT instruction.
///
/// # Arguments
/// * `chunk_data` - Current data being processed.
/// * `i` - Current position in `chunk_data` (updated after execution).
/// * `word_hash_offsets` - A hash table mapping Adler-32 hashes of blocks in the parent data to their offsets. Used to detect when a match starts at the current position.
/// * `delta_code` - Output buffer for delta instructions.
/// * `initial_hash` - Adler-32 hash of the first block at position `i` in `chunk_data`.
fn encode_insert_sequence(
    chunk_data: &[u8],
    i: &mut usize,
    word_hash_offsets: &HashMap<u32, Vec<usize>>,
    delta_code: &mut Vec<u8>,
    initial_hash: u32,
) {
    if *i >= chunk_data.len() {
        return;
    }

    let mut current_hash = initial_hash;
    let mut insert_data = Vec::new();

    while !word_hash_offsets.contains_key(&current_hash) {
        insert_data.push(chunk_data[*i]);
        *i += 1;

        if *i + BLOCK_SIZE <= chunk_data.len() {
            current_hash = adler32(&chunk_data[*i..*i + BLOCK_SIZE]);
        } else {
            let right_border = min(*i + BLOCK_SIZE, chunk_data.len());
            insert_data.extend_from_slice(&chunk_data[*i..right_border]);
            *i = chunk_data.len();
            break;
        }
    }

    if !insert_data.is_empty() {
        encode_insert_instruction(insert_data, delta_code);
    }
}

/// Computes the Adler-32 checksum for a given byte slice.
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;

    for &byte in data {
        a = (a + byte as u32) % ADLER_MOD;
        b = (b + a) % ADLER_MOD;
    }

    (b << 16) | a
}

/// Creates a hash map that maps each block's hash to its first occurrence position in the source data.
///
/// # Arguments
/// * `source_data` - The reference data to be indexed.
///
/// # Returns
/// HashMap where:
/// - Key: Adler32 hash of a block.
/// - Value: First starting position of that block in source_data.
fn create_block_hashmap(source_data: &[u8]) -> HashMap<u32, Vec<usize>> {
    let mut i = 0;
    let mut block_position_map = HashMap::new();

    while i + BLOCK_SIZE <= source_data.len() {
        let block_hash = adler32(&source_data[i..i + BLOCK_SIZE]);
        block_position_map
            .entry(block_hash)
            .or_insert_with(Vec::new)
            .push(i);
        i += 1;
    }

    block_position_map
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::decoder;
    use crate::encoder::encode_simple_chunk;
    use crate::hasher::AronovichHash;
    use rand::prelude::StdRng;
    use rand::{Rng, RngExt, SeedableRng};

    const TEST_DATA_SIZE: usize = 8192;

    #[test]
    fn create_block_hashmap_should_return_empty_map_for_data_shorter_than_16_bytes() {
        let short_data = [0u8; 15];
        let result = create_block_hashmap(&short_data);
        assert!(result.is_empty());
    }

    #[test]
    fn create_block_hashmap_should_create_empty_map_for_empty_data() {
        let empty_data = [];
        let result = create_block_hashmap(&empty_data);
        assert!(result.is_empty());
    }

    #[test]
    fn create_block_hashmap_should_store_first_position_for_duplicate_blocks() {
        let data = b"abcdabcdabcdabcdabcdabcdabcdabcd";
        let result = create_block_hashmap(data);
        assert_eq!(
            result.get(&adler32(b"abcdabcdabcdabcd")),
            Some(&vec![0, 4, 8, 12, 16])
        );
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn encode_insert_sequence_should_insert_full_chunk_when_no_hash_matches() {
        let chunk_data = vec![10; 20];
        let word_hash_offsets = HashMap::new();
        let mut delta_code = Vec::new();
        let mut i = 0;

        let adler_hash = adler32(&chunk_data[i..i + BLOCK_SIZE]);
        encode_insert_sequence(
            &chunk_data,
            &mut i,
            &word_hash_offsets,
            &mut delta_code,
            adler_hash,
        );

        assert_eq!(i, chunk_data.len());

        let header = &delta_code[..3];
        assert_eq!(header, &[20, 0, 0x80]);

        let data = &delta_code[3..];
        assert_eq!(data, chunk_data.as_slice());
    }

    #[test]
    fn encode_insert_sequence_should_insert_partial_tail_when_less_than_block_size() {
        let chunk_data = vec![10; 5];
        let word_hash_offsets = HashMap::new();
        let mut delta_code = Vec::new();
        let mut i = 0;

        let adler_hash = 0;
        encode_insert_sequence(
            &chunk_data,
            &mut i,
            &word_hash_offsets,
            &mut delta_code,
            adler_hash,
        );
        assert_eq!(i, chunk_data.len());

        let header = &delta_code[..3];
        assert_eq!(header, &[5, 0, 0x80]);

        let data = &delta_code[3..];
        assert_eq!(data, chunk_data.as_slice());
    }

    #[test]
    fn encode_insert_sequence_should_not_insert_if_match_found_at_start() {
        let chunk_data = vec![10; 16];
        let mut word_hash_offsets = HashMap::new();
        let hash = adler32(&chunk_data);
        word_hash_offsets.insert(hash, vec![0]);

        let mut delta_code = Vec::new();
        let mut i = 0;

        encode_insert_sequence(
            &chunk_data,
            &mut i,
            &word_hash_offsets,
            &mut delta_code,
            hash,
        );

        assert!(delta_code.is_empty());
        assert_eq!(i, 0);
    }

    #[test]
    fn encode_insert_sequence_should_insert_only_part_of_data_if_match_found_later() {
        let mut chunk_data = vec![10; 32];
        chunk_data[..4].copy_from_slice(&[1, 2, 3, 4]);

        let hash_second_block = adler32(&chunk_data[16..32]);

        let mut word_hash_offsets = HashMap::new();
        word_hash_offsets.insert(hash_second_block, vec![16]);

        let mut delta_code = Vec::new();
        let mut i = 0;

        let initial_hash = adler32(&chunk_data[i..i + BLOCK_SIZE]);
        encode_insert_sequence(
            &chunk_data,
            &mut i,
            &word_hash_offsets,
            &mut delta_code,
            initial_hash,
        );

        let mut expected = vec![4, 0, 0x80];
        expected.extend_from_slice(&[1, 2, 3, 4]);

        assert_eq!(delta_code, expected);
        assert_eq!(i, 4);
    }

    #[test]
    fn encode_copy_sequence_should_encode_full_copy_when_blocks_match() {
        let parent_data = vec![10; 32];
        let chunk_data = vec![10; 32];

        let word_hash_offsets = create_block_hashmap(&parent_data);

        let mut i = 0;
        let mut delta_code = Vec::new();
        let initial_hash = adler32(&chunk_data[i..i + BLOCK_SIZE]);

        encode_copy_sequence(
            &parent_data,
            &chunk_data,
            &mut i,
            &mut delta_code,
            initial_hash,
            &word_hash_offsets,
        );

        let expected = vec![32, 0, 0, 0, 0, 0];
        assert_eq!(delta_code, expected);
        assert_eq!(i, 32);
    }

    #[test]
    fn encode_copy_sequence_should_handle_non_aligned_matches() {
        let parent = b"abcdefghijklmnopqrstuvwxyzABCDEF".to_vec();
        let chunk = b"ijklmnopqrstuvwxyzABCDEFGHIJKL".to_vec();
        let word_hash_offsets = create_block_hashmap(&parent);

        let mut i = 0;
        let mut delta = vec![];
        let hash = adler32(&chunk[i..i + BLOCK_SIZE]);

        encode_copy_sequence(
            &parent,
            &chunk,
            &mut i,
            &mut delta,
            hash,
            &word_hash_offsets,
        );

        assert_eq!(i, 24);
        assert_eq!(delta[..3], 24u32.to_ne_bytes()[..3]);
        assert_eq!(delta[3..6], 8u32.to_ne_bytes()[..3]);
    }

    #[test]
    fn encode_copy_sequence_should_limit_match_by_parent_data_size() {
        let parent = vec![0u8; 16];
        let chunk = vec![0u8; 32];
        let word_hash_offsets = create_block_hashmap(&parent);
        let hash = adler32(&parent[..BLOCK_SIZE]);

        let mut i = 0;
        let mut delta = vec![];

        encode_copy_sequence(
            &parent,
            &chunk,
            &mut i,
            &mut delta,
            hash,
            &word_hash_offsets,
        );

        assert_eq!(i, BLOCK_SIZE);
    }

    #[test]
    fn encode_copy_sequence_should_do_nothing_when_hash_not_found() {
        let parent = vec![0u8; 16];
        let chunk = vec![0u8; 16];
        let word_hash_offsets = create_block_hashmap(&parent);

        let mut i = 0;
        let mut delta = vec![];
        let invalid_hash = adler32(b"invalid_block____");

        encode_copy_sequence(
            &parent,
            &chunk,
            &mut i,
            &mut delta,
            invalid_hash,
            &word_hash_offsets,
        );

        assert!(delta.is_empty());
        assert_eq!(i, 0);
    }

    #[test]
    fn encode_copy_sequence_should_do_nothing_when_position_out_of_bounds() {
        let parent = vec![0u8; 16];
        let chunk = vec![0u8; 16];
        let word_hash_offsets = create_block_hashmap(&parent);

        let mut i = chunk.len();
        let mut delta = vec![];
        let hash = adler32(&[0; BLOCK_SIZE]);

        encode_copy_sequence(
            &parent,
            &chunk,
            &mut i,
            &mut delta,
            hash,
            &word_hash_offsets,
        );

        assert!(delta.is_empty());
        assert_eq!(i, chunk.len());
    }

    #[test]
    fn test_restore_similarity_chunk_1_byte_diff() {
        let mut data: Vec<u8> = generate_test_data_deterministic(13);
        let data2 = data.clone();
        if data[15] < 255 {
            data[15] = 255;
        } else {
            data[15] = 0;
        }

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_2_neighbor_byte_diff() {
        let mut data: Vec<u8> = generate_test_data_deterministic(56);
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

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_2_byte_diff() {
        let mut data: Vec<u8> = generate_test_data_deterministic(35);
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

        let (sbc_map, sbc_key) = create_map_and_key(&data, &data2);

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_offset_left() {
        let data: Vec<u8> = generate_test_data_deterministic(41);
        let data2 = data[15..].to_vec();

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_offset_right() {
        let data: Vec<u8> = generate_test_data_deterministic(65);
        let data2 = data[..8000].to_vec();

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_offset() {
        let data: Vec<u8> = generate_test_data_deterministic(45);
        let mut data2 = data[15..8000].to_vec();
        data2[0] /= 3;
        data2[7000] /= 3;

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

        assert_eq!(sbc_map.get(&sbc_key).unwrap(), data2);
    }

    #[test]
    fn test_restore_similarity_chunk_with_cyclic_shift_right() {
        let data: Vec<u8> = generate_test_data_deterministic(44);
        let mut data2 = data.clone();
        data2.extend(&data[8000..]);

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

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

        let (sbc_map, sbc_key) = create_map_and_key(data.as_slice(), data2.as_slice());

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

    fn generate_test_data_deterministic(seed: u64) -> Vec<u8> {
        let mut rng = StdRng::seed_from_u64(seed);
        (0..TEST_DATA_SIZE).map(|_| rng.random()).collect()
    }

    fn create_map_and_key<'a>(
        data: &'a [u8],
        data2: &'a [u8],
    ) -> (
        SBCMap<decoder::GdeltaDecoder, AronovichHash>,
        SBCKey<AronovichHash>,
    ) {
        let word_hash_offsets = create_block_hashmap(data);
        let mut binding = SBCMap::new(decoder::GdeltaDecoder::default());
        let sbc_map = Arc::new(Mutex::new(&mut binding));

        let (_, sbc_key) = encode_simple_chunk(
            &mut sbc_map.lock().unwrap(),
            data,
            AronovichHash::new_with_u32(0),
        );
        let (_, _, sbc_key_2) = XdeltaEncoder::default().encode_delta_chunk(
            sbc_map.clone(),
            data2,
            AronovichHash::new_with_u32(3),
            data,
            &word_hash_offsets,
            sbc_key.hash.clone(),
        );
        (binding, sbc_key_2)
    }
}
