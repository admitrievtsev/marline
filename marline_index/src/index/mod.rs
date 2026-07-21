//! Index traits and types for sketch-based similarity search.
//!
//! This module defines the core [`SketchKVindex`] trait and associated types
//! for building key-value indexes with nearest-neighbor search via sketches.
use std::error::Error;
use std::sync::Arc;

use crate::sketch::Sketch;

pub use error::IndexError;
mod error;
mod store;
/// A key-value index with similarity search via sketches.
///
/// # Type Parameters
///
/// * `S` — The sketch key type. Must be [`Send`] + [`Sync`].
///
/// # Associated Types
///
/// * [`Self::Value`] — The stored value type. Must implement [`Clone`],
///   [`Send`], and [`Sync`] so that [`get`](Self::get) can return an owned
///   value.
pub trait SketchKVindex<S: Send + Sync + Sketch>: Send + Sync {
    /// The type of values stored in the index.
    type Value;
    type Error: Error + Send + Sync + 'static;
    /// Returns the number of entries.
    fn len(&self) -> Result<usize, Self::Error>;
    /// Looks up a value by key.
    ///
    /// Returns `None` if the key is not found.
    fn get(&self, key: &S) -> Result<Option<Arc<Self::Value>>, Self::Error>;

    /// Inserts or updates an entry.
    fn put(&self, key: &S, value: Arc<Self::Value>) -> Result<(), Self::Error>;

    /// Removes an entry by key. If the key does not exist, this is a no-op.
    fn remove(&self, key: &S) -> Result<(), Self::Error>;
    /// Returns the top `k` closest entries.
    ///
    /// Results are sorted by increasing distance. If fewer than `k` entries
    /// exist, all matching entries are returned.
    fn top_k(&self, key: &S, k: usize) -> Result<Vec<(Arc<Self::Value>, usize)>, Self::Error>;

    /// Removes all entries.
    fn clear(&self) -> Result<(), Self::Error>;
}
