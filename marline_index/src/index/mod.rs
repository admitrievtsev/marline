//! Generic sketch-based similarity indexes.
//!
//! This module provides a storage-backed inverted index for nearest-neighbor
//! search over arbitrary sketch implementations.

use std::error::Error;
use std::hash::Hash;

use crate::sketch::Sketch;

pub use error::IndexError;
#[allow(deprecated)]
pub use inverted::{InvertedSketchIndex, PalantirIndex};

mod error;
mod inverted;
pub mod store;

/// A key-value index with similarity search via sketches.
///
/// This trait preserves the behavior of the previous Palantir-specific index
/// API while removing product-specific tiering from the abstraction.
pub trait SketchIndexApi<K, S: Sketch>: Send + Sync
where
    K: Clone + Eq + Hash + Send + Sync,
{
    /// The error type returned by index operations.
    type Error: Error + Send + Sync + 'static;

    /// Returns the number of entries in the index.
    fn len(&self) -> Result<usize, Self::Error>;

    /// Returns `true` when the index has no entries.
    fn is_empty(&self) -> Result<bool, Self::Error>;

    /// Direct lookup: returns the sketch stored for the given key.
    fn lookup(&self, key: &K) -> Result<Option<S>, Self::Error>;

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

/// Deprecated compatibility alias for the previous trait name.
#[deprecated(note = "use SketchIndexApi instead")]
pub trait SketchKVindex<K, S: Sketch>: SketchIndexApi<K, S>
where
    K: Clone + Eq + Hash + Send + Sync,
{
}

#[allow(deprecated)]
impl<K, S, T> SketchKVindex<K, S> for T
where
    K: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    T: SketchIndexApi<K, S>,
{
}
