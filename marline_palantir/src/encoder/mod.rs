//! Delta encoding abstractions for the Palantir pipeline.
//!
//! The [`PalantirEncoder`] trait defines the interface for encoding a new
//! chunk as a delta relative to a similar base chunk.  [`GdeltaEncoder`]
//! provides a concrete implementation using the gdelta algorithm.

use crate::GEARU32;
use std::collections::HashMap;

/// Produces a delta encoding of `new_chunk` relative to `base_chunk`.
///
/// If two chunks are similar enough, the delta will be significantly smaller
/// than the raw chunk, saving storage space in a deduplication system.
pub trait PalantirEncoder {
    /// Encodes `new_chunk` as a delta with respect to `base_chunk`.
    ///
    /// # Arguments
    /// * `new_chunk` — The chunk to be delta-encoded.
    /// * `base_chunk` — The similar reference chunk.
    ///
    /// # Returns
    /// A byte vector containing the delta representation.
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8>;

    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        manifest: &HashMap<u32, u32>,
    ) -> Vec<u8>;
    fn generate_fp_table(chunk: &[u8]) -> Option<HashMap<u32, u32>>;
}

/// A [`PalantirEncoder`] that delegates to the gdelta diff algorithm.
///
/// Gdelta computes byte-level differences between two chunks and produces
/// a compact edit script.
pub struct GdeltaEncoder;

impl PalantirEncoder for GdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        marline_scrub::encoder::gdelta_diff(new_chunk, base_chunk)
    }
    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        manifest: &HashMap<u32, u32>,
    ) -> Vec<u8> {
        marline_scrub::encoder::gdelta_diff_new(new_chunk, base_chunk, manifest)
    }

    fn generate_fp_table(chunk: &[u8]) -> Option<HashMap<u32, u32>> {
        if chunk.len() < 10000 {
            return None;
        }
        let mut word_hash_offsets: HashMap<u32, u32> = HashMap::with_capacity(chunk.len());
        let mut fp: u32 = 0;
        let word_size: u32 = 16;
        let move_bts: u32 = 32 / word_size;

        let mask_bts: u32 = (chunk.len() as f64).log2() as u32;

        for i in 0..(word_size - 1) {
            fp = (fp << move_bts).wrapping_add(GEARU32[chunk[i as usize] as usize]);
        }

        for i in word_size..(chunk.len() as u32 - word_size + 1) {
            fp = (fp << move_bts)
                .wrapping_add(GEARU32[chunk[(i + word_size - 1) as usize] as usize]);
            let word_hash: u32 = fp >> (32 - mask_bts);
            word_hash_offsets.insert(word_hash, i);
        }
        Some(word_hash_offsets)
    }
}
