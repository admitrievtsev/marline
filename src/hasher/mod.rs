pub use aronovich_hash::{AronovichHash, AronovichHasher};
pub use odess_hasher::{OdessHash, OdessHasher};
use std::hash;

mod aronovich_hash;
mod odess_hasher;

/// Defines core hash functionality for Similarity-Based Chunking (SBC).
pub trait SBCHash: hash::Hash + Clone + Eq + PartialEq + Default + Send + Sync {
    /// Creates a new hash instance from a 32-bit unsigned integer key.
    fn new_with_u32(key: u32) -> Self;

    /// Generates the successor hash in the similarity hash sequence.
    /// Used when exploring adjacent hashes in clustering operations.
    fn next_hash(&self) -> Self;

    /// Generates the predecessor hash in the similarity hash sequence.
    /// Used when exploring adjacent hashes in clustering operations.
    fn last_hash(&self) -> Self;

    /// Extracts a 32-bit key for graph clustering algorithms.
    fn get_key_for_graph_clusterer(&self) -> u32;

    /// Try to represent SBCHash as slice of u64. Used primary for Odess hasher.
    fn as_slice(&self) -> &[u64];
}

/// A hasher that produces `SBCHash`-compatible digests from raw data.
///
/// # Type Parameters
/// * `Hash` - The output hash type implementing `SBCHash`
pub trait SBCHasher {
    /// The concrete hash type produced by this hasher
    type Hash: SBCHash;

    /// Computes the similarity hash for a data chunk.
    fn calculate_hash(&self, chunk_data: &[u8]) -> Self::Hash;
}
