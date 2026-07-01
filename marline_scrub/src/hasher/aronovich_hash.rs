use crate::SBCHash;
use crate::hasher::SBCHasher;
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Range;

const BLOCKS_IN_C_SPECTRUM_COUNT: usize = 8;
const MIN_SPACE_VALUE: u32 = 1;
const BITS_IN_F_SPECTRUM_BLOCKS_COUNT: u32 = 3;
const BLOCKS_IN_F_SPECTRUM_COUNT: usize = 16;
const SHIFT_FOR_PAIR: u8 = 3;
const BLOCKS_FOR_P_SPECTRUM_INDEXES: Range<usize> = 5..9;
const MIN_FREQUENCY_FOR_BYTE: u32 = 50;

#[derive(Debug)]
pub struct AronovichHash {
    hash: u32,
}

impl Hash for AronovichHash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state)
    }
}

impl Clone for AronovichHash {
    fn clone(&self) -> Self {
        AronovichHash::new_with_u32(self.hash)
    }
}

impl Eq for AronovichHash {}

impl PartialEq<Self> for AronovichHash {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Default for AronovichHash {
    fn default() -> Self {
        Self::new_with_u32(u32::default())
    }
}

impl SBCHash for AronovichHash {
    fn new_with_u32(hash: u32) -> Self {
        AronovichHash { hash }
    }
    fn next_hash(&self) -> Self {
        AronovichHash {
            hash: self.hash.saturating_add(1),
        }
    }

    fn last_hash(&self) -> Self {
        AronovichHash {
            hash: self.hash.saturating_sub(1),
        }
    }

    fn get_key_for_graph_clusterer(&self) -> u32 {
        self.hash
    }

    fn as_slice(&self) -> &[u64] {
        unimplemented!()
    }
}
pub struct AronovichHasher;

impl SBCHasher for AronovichHasher {
    type Hash = AronovichHash;

    fn calculate_hash(&self, chunk_data: &[u8]) -> AronovichHash {
        let mut byte_value_byte_frequency = HashMap::new();
        let mut pair_value_pair_frequency = HashMap::new();
        let mut last_byte = chunk_data[0];
        byte_value_byte_frequency.insert(last_byte, 1u32);
        for byte in &chunk_data[1..] {
            let byte_count = byte_value_byte_frequency.entry(*byte).or_insert(0);
            *byte_count += 1;

            let pair_count = pair_value_pair_frequency
                .entry((last_byte, *byte))
                .or_insert(0u32);
            *pair_count += 1;
            last_byte = *byte;
        }

        let c_f_hash = processing_of_c_f_spectrum(byte_value_byte_frequency);
        let p_hash = processing_of_p_spectrum(pair_value_pair_frequency);
        AronovichHash::new_with_u32(c_f_hash ^ p_hash)
    }
}

fn processing_of_c_spectrum(c_f_spectrum: &[(&u8, &u32)]) -> u32 {
    let mut spaces_in_c_spectrum = Vec::new();
    for byte_index in 0..c_f_spectrum.len() - 1 {
        let frequency_delta =
            (c_f_spectrum[byte_index].1 - c_f_spectrum[byte_index + 1].1) * (byte_index + 1) as u32;
        if frequency_delta >= MIN_SPACE_VALUE
            && *c_f_spectrum[byte_index + 1].1 >= MIN_FREQUENCY_FOR_BYTE
        {
            spaces_in_c_spectrum.push((byte_index, frequency_delta));
        }
    }
    spaces_in_c_spectrum.sort_by(|a, b| {
        if b.1 != a.1 {
            b.1.cmp(&a.1)
        } else {
            a.0.cmp(&b.0)
        }
    });

    let mut spaces_in_c_spectrum_indexes = Vec::new();
    for space in spaces_in_c_spectrum.iter().take(std::cmp::min(
        spaces_in_c_spectrum.len(),
        BLOCKS_IN_C_SPECTRUM_COUNT,
    )) {
        spaces_in_c_spectrum_indexes.push(space.0);
    }
    spaces_in_c_spectrum_indexes.sort();

    let mut hash: u32 = 0;

    let mut start_block = 0;
    for (block_number, id_end_block) in spaces_in_c_spectrum_indexes.iter().enumerate() {
        let end_block = *id_end_block;
        let block = &c_f_spectrum[start_block..=end_block];
        let mut block_hash = 0;
        for byte_frequency in block {
            block_hash ^= *byte_frequency.0 as u32;
        }

        block_hash <<= (BLOCKS_IN_C_SPECTRUM_COUNT - block_number) * 3;
        hash ^= block_hash;
        start_block = end_block + 1;
    }
    hash
}

fn find_first_significant_bit(block: u32) -> u32 {
    let mut number = block;
    let mut bit_index = 0;
    while number > 1 {
        number >>= 1;
        bit_index += 1;
    }
    bit_index
}

fn processing_of_f_spectrum(c_f_spectrum: &[(&u8, &u32)]) -> u32 {
    let mut hash: u32 = 0;
    let shifts = [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 6];

    for block_index in 0..std::cmp::min(c_f_spectrum.len(), BLOCKS_IN_F_SPECTRUM_COUNT) {
        let mut block_hash = *c_f_spectrum[block_index].1;
        block_hash <<= BITS_IN_F_SPECTRUM_BLOCKS_COUNT;
        let significant_bit = find_first_significant_bit(block_hash);
        block_hash >>= significant_bit - BITS_IN_F_SPECTRUM_BLOCKS_COUNT;
        block_hash %= 1 << BITS_IN_F_SPECTRUM_BLOCKS_COUNT;

        block_hash <<= shifts[block_index];
        hash ^= block_hash;
    }

    hash
}

fn processing_of_pair(pair: &(u8, u8)) -> u32 {
    let byte1 = ((pair.0 % (1 << (8 - SHIFT_FOR_PAIR))) << SHIFT_FOR_PAIR)
        + pair.0 / (1 << (8 - SHIFT_FOR_PAIR));
    let byte2 =
        ((pair.1 % (1 << SHIFT_FOR_PAIR)) << (8 - SHIFT_FOR_PAIR)) + pair.1 / (1 << SHIFT_FOR_PAIR);
    ((byte1 as u32) << 4) ^ (byte2 as u32)
}

fn processing_of_p_spectrum(pair_value_pair_frequency: HashMap<(u8, u8), u32>) -> u32 {
    let mut p_spectrum: Vec<(&(u8, u8), &u32)> = pair_value_pair_frequency.iter().collect();
    p_spectrum.sort_by(|a, b| {
        if b.1 != a.1 {
            b.1.cmp(a.1)
        } else if a.0.0 != b.0.0 {
            a.0.0.cmp(&b.0.0)
        } else {
            a.0.1.cmp(&b.0.1)
        }
    });
    let mut hash: u32 = 0;
    for block_index in BLOCKS_FOR_P_SPECTRUM_INDEXES {
        if block_index >= p_spectrum.len() {
            break;
        }
        hash ^= processing_of_pair(p_spectrum[block_index].0) << 20;
    }

    hash
}

fn processing_of_c_f_spectrum(byte_value_byte_frequency: HashMap<u8, u32>) -> u32 {
    let mut c_f_spectrum: Vec<(&u8, &u32)> = byte_value_byte_frequency.iter().collect();
    c_f_spectrum.sort_by(|a, b| {
        if b.1 != a.1 {
            b.1.cmp(a.1)
        } else {
            a.0.cmp(b.0)
        }
    });
    let c_hash = processing_of_c_spectrum(c_f_spectrum.as_slice());
    let f_hash = processing_of_f_spectrum(c_f_spectrum.as_slice());
    c_hash ^ f_hash
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_processing_of_pair() {
        let a = 175u8;
        let b = 113u8;
        let processed_pair = processing_of_pair(&(a, b));
        let name = &format!("{processed_pair:b}");
        assert_eq!(name, "11111111110")
    }

    #[test]
    fn test_processing_of_p_spectrum_with_one_pair() {
        let mut p_spectrum = HashMap::new();
        for i in 0..6 {
            p_spectrum.insert((175u8 + i as u8, 113u8), i);
        }
        let processed_p_spectrum = processing_of_p_spectrum(p_spectrum);
        let name = &format!("{processed_p_spectrum:b}");
        assert_eq!(name, "1111111111000000000000000000000")
    }

    #[test]
    fn test_processing_of_p_spectrum_with_two_eq_pairs() {
        let mut p_spectrum = HashMap::new();
        for _ in 0..7 {
            p_spectrum.insert((175u8, 113u8), 0u32);
        }
        let processed_p_spectrum = processing_of_p_spectrum(p_spectrum);
        assert_eq!(processed_p_spectrum, 0)
    }

    #[test]
    fn test_processing_of_p_spectrum() {
        let mut p_spectrum = HashMap::new();
        for i in 0..6 {
            p_spectrum.insert((175u8 + i as u8, 113u8), i);
        }
        p_spectrum.insert((7u8, 7u8), 0u32);
        let processed_p_spectrum = processing_of_p_spectrum(p_spectrum);
        let name = &format!("{processed_p_spectrum:b}");
        assert_eq!(name, "1001001111000000000000000000000")
    }

    pub fn return_p_spectrum_hash(data: &[u8]) -> u32 {
        let mut pair_value_pair_frequency = HashMap::new();
        let mut last_byte = data[0];
        for byte in &data[1..] {
            let pair_count = pair_value_pair_frequency
                .entry((last_byte, *byte))
                .or_insert(0u32);
            *pair_count += 1;
            last_byte = *byte;
        }
        processing_of_p_spectrum(pair_value_pair_frequency)
    }

    #[test]
    fn test_pairs_vec_for_eq_chunks() {
        let chunk: Vec<u8> = (0..300).map(|_| rand::random::<u8>()).collect();
        let pairs_vec_1 = return_p_spectrum_hash(chunk.as_slice());
        let pairs_vec_2 = return_p_spectrum_hash(chunk.as_slice());
        assert_eq!(pairs_vec_1, pairs_vec_2)
    }

    fn return_c_f_spectrum_hash(data: &[u8]) -> u32 {
        let mut byte_value_byte_frequency = HashMap::new();
        for byte in data {
            let byte_count = byte_value_byte_frequency.entry(*byte).or_insert(0);
            *byte_count += 1;
        }
        processing_of_c_f_spectrum(byte_value_byte_frequency)
    }

    #[test]
    fn test_c_f_spectrum_for_eq_chunks() {
        let chunk: Vec<u8> = (0..8192).map(|_| rand::random::<u8>()).collect();
        let c_f_hash_1 = return_c_f_spectrum_hash(chunk.as_slice());
        let c_f_hash_2 = return_c_f_spectrum_hash(chunk.as_slice());
        assert_eq!(c_f_hash_1, c_f_hash_2)
    }

    #[test]
    fn test_c_f_hash_for_different_1_byte() {
        let chunk: Vec<u8> = (0..8192).map(|_| rand::random::<u8>()).collect();
        let mut similarity_chunk = chunk.clone();
        if similarity_chunk[15] == 255 {
            similarity_chunk[15] = 0;
        } else {
            similarity_chunk[15] = 255;
        }
        let c_f_hash_1 = return_c_f_spectrum_hash(chunk.as_slice());
        let c_f_hash_2 = return_c_f_spectrum_hash(similarity_chunk.as_slice());
        assert_eq!(c_f_hash_1, c_f_hash_2);
        assert!(u32::abs_diff(c_f_hash_1, c_f_hash_2) <= 32)
    }

    #[test]
    fn test_hash_for_eq_chunks() {
        let chunk: Vec<u8> = (0..8192).map(|_| rand::random::<u8>()).collect();
        let hash = AronovichHasher.calculate_hash(chunk.as_slice());
        let eq_hash = AronovichHasher.calculate_hash(chunk.as_slice());
        assert_eq!(hash, eq_hash)
    }
}
