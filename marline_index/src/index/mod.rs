//! Generic sketch-based similarity indexes.
//!
//! This module provides a posting-list-backed inverted index for nearest-neighbor
//! search over arbitrary sketch implementations.

use std::hash::Hash;

use crate::sketch::Sketch;

pub use error::IndexError;
pub use inverted::InvertedSketchIndex;

mod error;
mod inverted;
pub mod store;

/// A sketch-based similarity search index.
///
/// # Type Parameters
///
/// * `K` — The key type (hash). Must be [`Clone`] + [`Eq`] + [`Hash`].
/// * `S` — The sketch type. Must implement [`Sketch`].
pub trait SketchIndexApi<K, S: Sketch>: Send + Sync
where
    K: Clone + Eq + Hash + Send + Sync,
{
    /// The error type returned by index operations.
    type Error;

    /// Search: finds the closest matching key for a query sketch.
    fn get(&self, query: &S) -> Result<Option<K>, Self::Error>;

    /// Inserts or updates an entry.
    fn put(&self, key: &K, sketch: S) -> Result<(), Self::Error>;

    /// Removes the entry associated with the given key.
    fn remove(&self, key: &K) -> Result<(), Self::Error>;

    /// Returns the top `k` entries most similar to the query sketch.
    fn top_k(&self, query: &S, k: usize) -> Result<Vec<(K, f64)>, Self::Error>;

    /// Removes all entries from the index.
    fn clear(&self) -> Result<(), Self::Error>;
}
