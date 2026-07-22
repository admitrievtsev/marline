//! Storage layer for the Palantir index.
//!
//! This module defines the [`Store`] trait — the abstract storage interface
//! that separates index logic from the backing database. In-memory
//! [`IndexStorage`] is provided for testing; a production RocksDB backend
//! can be added by implementing [`Store`] on the RocksDB wrapper.

use crate::index::error::IndexError;
use crate::sketch::Sketch;

pub mod index_storage;

/// Palantir superfeature tiers.
///
/// A chunk's sketch is placed into one of three inverted-index tiers
/// based on its sketch size:
///
/// | Tier  | Sketch size | Granularity | Retention |
/// |-------|-------------|-------------|-----------|
/// | `One`    | 3           | Coarse      | All versions |
/// | `Two`    | 4           | Medium      | Last N versions |
/// | `Three`  | 6           | Fine        | Last M versions |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    One,
    Two,
    Three,
}

/// Abstract storage interface for sketch and inverted-index data.
///
/// `Store` is the low-level persistence layer used by [`PalantirIndex`] to
/// read and write sketches and superfeature posting lists.
///
/// Implementations handle thread safety internally (e.g., via `RwLock`).
///
/// # Type Parameters
///
/// * `H` — The hash type used as the primary key for entries.
/// * `S` — The sketch type stored as the value.
///
/// # Operations
///
/// - **Sketches**: `get_sketch` / `put_sketch` for hash → sketch storage.
/// - **Inverted index**: `get_inverted` / `put_inverted` / `remove_inverted`
///   for superfeature → list-of-hashes mappings, partitioned by [`Tier`].
/// - **Counts**: `len_sketches` / `len_inverted` for statistics.
pub trait Store<H, S: Sketch>: Send + Sync
where
    H: Clone + Send + Sync,
{
    /// Returns the sketch stored for the given hash, or `None`.
    fn get_sketch(&self, hash: &H) -> Result<Option<S>, IndexError>;

    /// Stores the sketch under the given hash.
    fn put_sketch(&self, hash: &H, sketch: &S) -> Result<(), IndexError>;

    /// Returns all hashes that share the given superfeature in the tier.
    fn get_inverted(&self, tier: Tier, sf: u32) -> Result<Vec<H>, IndexError>;

    /// Adds a hash to the posting list for `(tier, sf)`.
    fn put_inverted(&self, tier: Tier, sf: u32, hash: &H) -> Result<(), IndexError>;

    /// Removes a hash from the posting list for `(tier, sf)`.
    #[allow(dead_code)]
    fn remove_inverted(&self, tier: Tier, sf: u32, hash: &H) -> Result<(), IndexError>;

    /// Returns the total number of sketches stored.
    fn len_sketches(&self) -> Result<usize, IndexError>;

    /// Returns the number of distinct superfeatures in the given tier's inverted index.
    #[allow(dead_code)]
    fn len_inverted(&self, tier: Tier) -> Result<usize, IndexError>;

    //fn clear()
}
