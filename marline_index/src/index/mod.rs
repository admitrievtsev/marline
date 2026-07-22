//! Core index traits for sketch-based similarity search.
//!
//! This module defines [`SketchKVindex`], the primary trait for key-value
//! indexes that support nearest-neighbor search via sketches. The trait
//! separates the key type (a chunk hash) from the sketch type (a compact
//! fingerprint), enabling flexible index implementations.

use std::error::Error;
use std::hash::Hash;

use crate::sketch::Sketch;

pub use error::IndexError;
mod error;
mod palantir;
mod store;

/// A key-value index with similarity search via sketches.
///
/// [`SketchKVindex`] provides two levels of access:
///
/// - **Search API**: [`get`](SketchKVindex::get) finds the closest match
///   given a query sketch; [`top_k`](SketchKVindex::top_k) returns the `k`
///   most similar entries.
/// - **Direct API**: [`lookup`](SketchKVindex::lookup) performs a
///   hash → sketch lookup; [`put`](SketchKVindex::put) inserts a sketch
///   indexed by its chunk hash.
///
/// Implementations must be [`Send`] + [`Sync`] for safe concurrent access.
///
/// # Type Parameters
///
/// * `K` — The key type (typically a chunk hash). Must be [`Clone`], [`Eq`],
///   [`Hash`], [`Send`], and [`Sync`].
/// * `S` — The sketch type. Must implement [`Sketch`] plus [`Send`] + [`Sync`].
pub trait SketchKVindex<K, S: Send + Sync + Sketch>: Send + Sync
where
    K: Clone + Eq + Hash + Send + Sync,
{
    /// The error type returned by index operations.
    type Error: Error + Send + Sync + 'static;

    /// Returns the number of entries in the index.
    fn len(&self) -> Result<usize, Self::Error>;

    /// Direct lookup: returns the sketch stored for the given hash key.
    ///
    /// Returns `Ok(None)` if the key is not present in the index.
    fn lookup(&self, key: &K) -> Result<Option<S>, Self::Error>;

    /// Search: finds the closest matching hash for a query sketch.
    ///
    /// This is equivalent to `top_k(query, 1)` and returns the single most
    /// similar entry's hash. Returns `Ok(None)` if the index is empty.
    fn get(&self, key: &S) -> Result<Option<K>, Self::Error>;

    /// Inserts or updates an entry.
    ///
    /// Stores the sketch under the given hash and updates the internal
    /// inverted index for efficient similarity search.
    fn put(&self, key: &K, sketch: S) -> Result<(), Self::Error>;

    /// Removes the entry associated with the given hash.
    ///
    /// If the key does not exist, this is a no-op.
    fn remove(&self, key: &K) -> Result<(), Self::Error>;

    /// Returns the top `k` entries most similar to the query sketch.
    ///
    /// Results are sorted by **decreasing** Jaccard similarity.
    /// Each element is a `(hash, jaccard_score)` pair.
    fn top_k(&self, query: &S, k: usize) -> Result<Vec<(K, f64)>, Self::Error>;

    /// Removes all entries from the index.
    fn clear(&self) -> Result<(), Self::Error>;
}
