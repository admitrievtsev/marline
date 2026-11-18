use crate::GEAR;
use std::cmp::min;
use std::collections::HashMap;

pub fn bounded_gdelta_diff(new_chunk: &[u8], base_chunk: &[u8], limit: usize) -> Option<Vec<u8>> {
    let word_size: usize = 16;
    if new_chunk.len() < word_size || base_chunk.len() < word_size {
        return if new_chunk.len() >= limit { None } else { Some(new_chunk.to_vec()) };
    }
    let move_bts: usize = 64 / word_size;
    let mask_bts: usize = (base_chunk.len() as f64).log2() as usize;
    let mut word_hash_offsets: HashMap<u64, usize> = HashMap::with_capacity(base_chunk.len());
    let mut fp: u64 = 0;

    for i in 0..(word_size - 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[base_chunk[i] as usize]);
    }

    for i in word_size..(base_chunk.len() - word_size + 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[base_chunk[i + word_size - 1] as usize]);
        let word_hash: u64 = fp >> (64 - mask_bts);
        word_hash_offsets.insert(word_hash, i);
    }

    let mut delta_code = Vec::new();
    let mut anchor: usize = 0;
    let mut fp = 0u64;

    for j in 0..(word_size - 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[j] as usize]);
    }

    let mut j = 0;
    while j + word_size <= new_chunk.len() {
        // Pending unmatched bytes must be emitted as INSERT before any later match.
        let pending = if j > anchor { 3 + j - anchor } else { 0 };
        if delta_code.len().saturating_add(pending) >= limit {
            return None;
        }

        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[j + word_size - 1] as usize]);
        let word_hash: u64 = fp >> (64 - mask_bts);

        if let Some(&offset) = word_hash_offsets.get(&word_hash) {
            let mut equal_part_len: usize = 0;
            for k in 0..min(base_chunk.len() - offset, new_chunk.len() - j) {
                if base_chunk[offset + k] != new_chunk[j + k] {
                    break;
                }
                equal_part_len += 1;
            }

            if equal_part_len >= word_size {
                let insert_data_len = j - anchor;
                if insert_data_len > 0 {
                    let insert_data = &new_chunk[anchor..(anchor + insert_data_len)];
                    let mut insert_instruction = insert_data_len.to_ne_bytes();
                    insert_instruction[2] |= 1 << 7;
                    delta_code.extend_from_slice(&insert_instruction[..3]);
                    delta_code.extend_from_slice(insert_data);
                }

                let copy_instruction_len = &equal_part_len.to_ne_bytes()[..3];
                let copy_instruction_offset = &offset.to_ne_bytes()[..3];
                delta_code.extend_from_slice(copy_instruction_len);
                delta_code.extend_from_slice(copy_instruction_offset);

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
            let mut insert_instruction = insert_data_len.to_ne_bytes();
            insert_instruction[2] |= 1 << 7;
            delta_code.extend_from_slice(&insert_instruction[..3]);
            delta_code.extend_from_slice(insert_data);
        }

        j += 1;
    }

    if delta_code.len() >= limit {
        None
    } else {
        Some(delta_code)
    }
}
pub fn bounded_gdelta_diff_new(
    new_chunk: &[u8],
    base_chunk: &[u8],
    manifest: &HashMap<u64, u32>,
    limit: usize,
) -> Option<Vec<u8>> {
    let base_len = base_chunk.len();

    let new_len = new_chunk.len();
    let word_size: usize = 16;
    if new_len < word_size || base_len < word_size {
        return if new_chunk.len() >= limit { None } else { Some(new_chunk.to_vec()) };
    }
    let move_bts: usize = 64 / word_size;
    let mask_bts: usize = (base_chunk.len() as f64).log2() as usize;

    let mut delta_code = Vec::new();
    let mut anchor: usize = 0;
    let mut fp = 0u64;

    for j in 0..(word_size - 1) {
        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[j] as usize]);
    }

    let mut j = 0;
    while j + word_size <= new_len {
        // Pending unmatched bytes must be emitted as INSERT before any later match.
        let pending = if j > anchor { 3 + j - anchor } else { 0 };
        if delta_code.len().saturating_add(pending) >= limit {
            return None;
        }

        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[j + word_size - 1] as usize]);
        let word_hash: u64 = fp >> (64 - mask_bts);

        if let Some(&offset) = manifest.get(&(word_hash)) {
            let mut equal_part_len: usize = 0;
            for k in 0..min(base_len - offset as usize, new_len - j) {
                if base_chunk[offset as usize + k] != new_chunk[j + k] {
                    break;
                }
                equal_part_len += 1;
            }

            if equal_part_len >= word_size {
                let insert_data_len = j - anchor;
                if insert_data_len > 0 {
                    let insert_data = &new_chunk[anchor..(anchor + insert_data_len)];
                    let mut insert_instruction = insert_data_len.to_ne_bytes();
                    insert_instruction[2] |= 1 << 7;
                    delta_code.extend_from_slice(&insert_instruction[..3]);
                    delta_code.extend_from_slice(insert_data);
                }

                let copy_instruction_len = &equal_part_len.to_ne_bytes()[..3];
                let copy_instruction_offset = &offset.to_ne_bytes()[..3];
                delta_code.extend_from_slice(copy_instruction_len);
                delta_code.extend_from_slice(copy_instruction_offset);

                anchor = j + equal_part_len;
                j = anchor - 1;
                if j < new_len - word_size {
                    for k in anchor..(anchor + word_size - 1) {
                        fp = (fp << move_bts).wrapping_add(GEAR[new_chunk[k] as usize]);
                    }
                }
            }
        }

        if j >= new_len - word_size {
            let insert_data_len = new_len - anchor;
            let insert_data = &new_chunk[anchor..(anchor + insert_data_len)];
            let mut insert_instruction = insert_data_len.to_ne_bytes();
            insert_instruction[2] |= 1 << 7;
            delta_code.extend_from_slice(&insert_instruction[..3]);
            delta_code.extend_from_slice(insert_data);
        }

        j += 1;
    }

    if delta_code.len() >= limit {
        None
    } else {
        Some(delta_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::{GdeltaEncoder, PalantirEncoder};

    fn fixture() -> (Vec<u8>, Vec<u8>) {
        let base: Vec<u8> = (0..4096).map(|i| ((i * 31 + i / 7) % 251) as u8).collect();
        let mut target = base.clone();
        target[733..1041].fill(0xA7);
        target[2800..3019].fill(0x39);
        (target, base)
    }

    #[test]
    fn bounded_delta_matches_full_encoding_below_cutoff() {
        let (target, base) = fixture();
        let full = marline_scrub::encoder::gdelta_diff(&target, &base);
        assert_eq!(bounded_gdelta_diff(&target, &base, full.len() + 1), Some(full.clone()));
        assert_eq!(bounded_gdelta_diff(&target, &base, full.len()), None);
    }

    #[test]
    fn bounded_manifest_delta_matches_full_encoding_below_cutoff() {
        let (target, base) = fixture();
        let manifest = GdeltaEncoder::generate_fp_table(&base).unwrap();
        let full = marline_scrub::encoder::gdelta_diff_new(&target, &base, &manifest);
        assert_eq!(
            bounded_gdelta_diff_new(&target, &base, &manifest, full.len() + 1),
            Some(full.clone())
        );
        assert_eq!(bounded_gdelta_diff_new(&target, &base, &manifest, full.len()), None);
    }
}
