use std::cmp::min;
use std::collections::HashMap;

use super::PalantirEncoder;
use crate::GEAR;

#[derive(Debug, Default)]
pub struct GdeltaEncoder;

impl GdeltaEncoder {
    pub fn new() -> Self {
        GdeltaEncoder
    }
}

fn build_hash_table(data: &[u8]) -> HashMap<u64, usize> {
    let word_size: usize = 16;
    let move_bts: usize = 64 / word_size;
    let mask_bts: usize = (data.len() as f64).log2() as usize;
    let mut word_hash_offsets: HashMap<u64, usize> = HashMap::new();
    let mut fp: u64 = 0;

    for i in 0..(word_size - 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[data[i] as usize]);
    }

    for i in 0..(data.len() - word_size + 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[data[i + word_size - 1] as usize]);
        let word_hash: u64 = fp >> (64 - mask_bts);
        word_hash_offsets.insert(word_hash, i);
    }

    word_hash_offsets
}

fn encode_gdelta(new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
    let mut delta_code = Vec::new();
    let word_size: usize = 16;
    let move_bts: usize = 64 / word_size;
    let mask_bts: usize = (base_chunk.len() as f64).log2() as usize;
    let hash_table = build_hash_table(base_chunk);

    let mut anchor: usize = 0;
    let mut fp = 0u64;

    for j in 0..(word_size - 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[j] as usize]);
    }

    let mut j = 0;
    while j < new_chunk.len() - word_size + 1 {
        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[j + word_size - 1] as usize]);
        let word_hash: u64 = fp >> (64 - mask_bts);

        if let Some(&offset) = hash_table.get(&word_hash) {
            let mut equal_part_len: usize = 0;
            for k in 0..min(base_chunk.len() - offset, new_chunk.len() - j) {
                if base_chunk[offset + k] != new_chunk[j + k] {
                    break;
                }
                equal_part_len += 1;
            }

            if equal_part_len >= word_size {
                //Insert instruction
                let insert_data_len = j - anchor;
                if insert_data_len > 0 {
                    let insert_data = &new_chunk[anchor..(anchor + insert_data_len)];
                    let mut len_bytes = (insert_data_len as u32).to_ne_bytes()[..3].to_vec();
                    len_bytes[2] |= 1 << 7;
                    delta_code.extend_from_slice(&len_bytes);
                    delta_code.extend_from_slice(insert_data);
                }

                //Copy instruction
                let copy_len = &equal_part_len.to_ne_bytes()[..3];
                let copy_offset = &offset.to_ne_bytes()[..3];
                delta_code.extend_from_slice(copy_len);
                delta_code.extend_from_slice(copy_offset);

                anchor = j + equal_part_len;
                j = anchor - 1;
                if j < new_chunk.len() - word_size {
                    for k in anchor..(anchor + word_size - 1) {
                        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[k] as usize]);
                    }
                }
            }
        }

        if j >= new_chunk.len() - word_size {
            let insert_data_len = new_chunk.len() - anchor;
            let insert_data = &new_chunk[anchor..(anchor + insert_data_len)];
            let mut len_bytes = (insert_data_len as u32).to_ne_bytes()[..3].to_vec();
            len_bytes[2] |= 1 << 7;
            delta_code.extend_from_slice(&len_bytes);
            delta_code.extend_from_slice(insert_data);
        }

        j += 1;
    }

    delta_code
}

impl PalantirEncoder for GdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        encode_gdelta(new_chunk, base_chunk)
    }
}
