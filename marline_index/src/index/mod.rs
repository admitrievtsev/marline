//! Index traits and types for sketch-based similarity search.
//!
//! This module defines the core [`SketchKVindex`] trait and associated types
//! for building key-value indexes with nearest-neighbor search via sketches.
use std::error::Error;
use std::hash::Hash;

use crate::sketch::Sketch;

pub use error::IndexError;
mod error;
mod palantir;
mod store;
/// A key-value index with similarity search via sketches.
///
/// # Type Parameters
///
/// * `K` — The key type (hash). Must be [`Send`] + [`Sync`].
/// * `S` — The sketch type. Must implement [`Sketch`].
pub trait SketchKVindex<K, S: Send + Sync + Sketch>: Send + Sync
where
    K: Clone + Eq + Hash + Send + Sync,
{
    type Error: Error + Send + Sync + 'static;
    /// Returns the number of entries.
    fn len(&self) -> Result<usize, Self::Error>;
    /// Looks up a sketch by hash.
    ///
    /// Returns `None` if the key is not found.
    fn get(&self, key: &S) -> Result<Option<K>, Self::Error>;

    /// Inserts or updates a sketch for the given hash.
    fn put(&self, key: &K, sketch: S) -> Result<(), Self::Error>;

    /// Removes an entry by key. If the key does not exist, this is a no-op.
    fn remove(&self, key: &K) -> Result<(), Self::Error>;
    /// Returns the top `k` closest entries for the given query sketch.
    ///
    /// Results are sorted by decreasing Jaccard similarity.
    fn top_k(&self, query: &S, k: usize) -> Result<Vec<(K, f64)>, Self::Error>;

    /// Removes all entries.
    fn clear(&self) -> Result<(), Self::Error>;
}
