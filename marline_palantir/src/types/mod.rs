//! Core data types for the Palantir similarity detection pipeline.
//!
//! This module defines the fundamental abstractions used throughout the crate:
//!
//! * [`Chunk`] — a wrapper around raw byte data.
//! * [`ChunkDigest`] — a set of super-features that compactly represents
//!   a chunk's content for similarity search.
//! * [`SuperFeature`] — a single tiered similarity fingerprint.
//! * [`TierConfig`] — configuration for multi-tier feature grouping.
//! * [`SuperFeatureGenerator`] — trait for producing super-features from a chunk.

mod block_id;
mod chunk;
mod chunk_hash;
mod super_feature;
pub use block_id::BlockID;
pub use chunk::Chunk;
pub use chunk_hash::ChunkDigest;
pub use super_feature::{SuperFeature, TierConfig};
