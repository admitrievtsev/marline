//! Storage traits and in-memory backend for sketch indexes.
//!
//! Storage is intentionally independent from any product-specific tiering or
//! retention policy. It stores sketches by key and posting lists by feature.

use crate::index::error::IndexError;
use crate::sketch::Sketch;

pub mod index_storage;
pub use index_storage::IndexStorage;

/// Storage for direct key -> sketch records.
pub trait SketchStorage<K, S: Sketch>: Send + Sync
where
    K: Clone + Send + Sync,
{
    /// Returns the sketch stored for the given key, or `None`.
    fn get_sketch(&self, key: &K) -> Result<Option<S>, IndexError>;

    /// Stores the sketch and returns the previous sketch for the key, if any.
    fn put_sketch(&self, key: K, sketch: S) -> Result<Option<S>, IndexError>;

    /// Removes the sketch stored for the key, if any.
    fn remove_sketch(&self, key: &K) -> Result<Option<S>, IndexError>;

    /// Returns the number of sketches stored.
    fn len_sketches(&self) -> Result<usize, IndexError>;

    /// Removes all sketches.
    fn clear_sketches(&self) -> Result<(), IndexError>;
}

/// Storage for feature -> posting-list records.
pub trait InvertedStorage<K, F>: Send + Sync
where
    K: Clone + Send + Sync,
    F: Copy + Send + Sync,
{
    /// Returns all keys that contain the feature.
    fn posting_list(&self, feature: F) -> Result<Vec<K>, IndexError>;

    /// Adds the key to the feature's posting list.
    fn insert_posting(&self, feature: F, key: K) -> Result<(), IndexError>;

    /// Removes the key from the feature's posting list.
    fn remove_posting(&self, feature: F, key: &K) -> Result<(), IndexError>;

    /// Returns the number of distinct indexed features.
    fn len_postings(&self) -> Result<usize, IndexError>;

    /// Removes all posting lists.
    fn clear_postings(&self) -> Result<(), IndexError>;
}

/// Complete storage backend required by [`crate::index::InvertedSketchIndex`].
pub trait Store<K, S>: SketchStorage<K, S> + InvertedStorage<K, S::Feature>
where
    K: Clone + Send + Sync,
    S: Sketch,
{
}

impl<K, S, T> Store<K, S> for T
where
    K: Clone + Send + Sync,
    S: Sketch,
    T: SketchStorage<K, S> + InvertedStorage<K, S::Feature>,
{
}
