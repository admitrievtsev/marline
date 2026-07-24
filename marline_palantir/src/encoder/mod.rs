//! Delta encoding abstractions for the Palantir pipeline.
//!
//! The [`PalantirEncoder`] trait defines the interface for encoding a new
//! chunk as a delta relative to a similar base chunk.  [`GdeltaEncoder`]
//! provides a concrete implementation using the gdelta algorithm.

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
}
