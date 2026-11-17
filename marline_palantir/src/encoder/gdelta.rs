use marline_scrub::decoder::{Decoder, GdeltaDecoder};
use marline_scrub::encoder::GdeltaEncoder as ScrubGdeltaEncoder;
use std::collections::HashMap;

use super::Encoder;

#[derive(Clone, Copy, Debug, Default)]
pub struct GdeltaEncoder;

impl Encoder for GdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        ScrubGdeltaEncoder::default().encode_data(new_chunk, base_chunk)
    }

    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        manifest: &HashMap<u64, u32>,
    ) -> Vec<u8> {
        ScrubGdeltaEncoder::default().encode_data_with_manifest(new_chunk, base_chunk, manifest)
    }

    fn name(&self) -> &'static str {
        "gdelta"
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        GdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }

    fn generate_fp_table(&self, base_chunk: &[u8]) -> Option<HashMap<u64, u32>> {
        let word_size = 16;
        let move_bts = 64 / word_size;
        let mask_bts = (base_chunk.len() as f64).log2() as usize;
        let mut word_hash_offsets = HashMap::with_capacity(base_chunk.len());
        let mut fp = 0u64;

        for &byte in base_chunk.iter().take(word_size - 1) {
            fp = (fp << move_bts).wrapping_add(marline_sketcher::gear(byte));
        }
        for i in word_size..(base_chunk.len() - word_size + 1) {
            fp = (fp << move_bts)
                .wrapping_add(marline_sketcher::gear(base_chunk[i + word_size - 1]));
            word_hash_offsets.insert(fp >> (64 - mask_bts), i as u32);
        }
        Some(word_hash_offsets)
    }
}
