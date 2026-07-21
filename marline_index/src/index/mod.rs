//! Index traits and types for sketch-based similarity search.
//!
//! This module defines the core [`SketchKvindex`] trait and associated types
//! for building key-value indexes with nearest-neighbor search via sketches.

pub use error::IndexError;

use crate::sketch::Sketch;

mod error;

/// A unique identifier for index entries.
pub type EntryId = u64;

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
pub trait SketchKvindex<S: Send + Sync>: Send + Sync {
    /// The type of values stored in the index.
    type Value: Clone + Send + Sync;

    /// Returns the number of entries.
    fn len(&self) -> usize;

    /// Returns `true` if the index contains no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Looks up a value by key.
    ///
    /// Returns `None` if the key is not found.
    fn get(&self, key: &S) -> Option<Self::Value>;

    /// Inserts or updates an entry.
    ///
    /// Returns a [`PutOutcome`] describing the result.
    fn put(&self, key: &S, value: Self::Value) -> PutOutcome;

    /// Removes an entry by key. If the key does not exist, this is a no-op.
    fn remove(&self, key: &S);

    /// Finds the nearest neighbor for the given key.
    ///
    /// Returns `None` if the index is empty or no candidate meets the search
    /// criteria.
    fn nearest(
        &self,
        key: &S,
        search_options: SearchOptions,
    ) -> Option<SearchResult<S, Self::Value>>;

    /// Returns the top `k` closest entries.
    ///
    /// Results are sorted by increasing distance. If fewer than `k` entries
    /// exist, all matching entries are returned.
    fn top_k(
        &self,
        key: &S,
        k: usize,
        search_options: SearchOptions,
    ) -> Vec<SearchResult<S, Self::Value>>;

    /// Removes all entries.
    fn clear(&self);
}

/// The result of a [`SketchKvindex::put`] operation.
///
/// # Variants
///
/// * `Inserted` — A new entry was added.
/// * `Updated` — An existing entry was replaced.
pub enum PutOutcome {
    /// A new entry was inserted.
    Inserted { entry_id: EntryId },
    /// An existing entry was updated.
    Updated { entry_id: EntryId, previous_entry_id: EntryId },
}

/// Controls nearest-neighbor search behavior.
///
/// # Fields
///
/// * `min_intersection` — Minimum intersection for a candidate match.
/// * `return_zero_overlap` — Include entries with zero overlap.
/// * `exclude_exact` — Exclude the query key from results.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub min_intersection: EntryId,
    pub return_zero_overlap: bool,
    pub exclude_exact: bool,
}

/// A search result from a nearest-neighbor query.
///
/// # Type Parameters
///
/// * `S` — The sketch key type.
/// * `V` — The value type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult<S: Send + Sync, V: Send + Sync> {
    pub entry_id: EntryId,
    pub key: S,
    pub value: V,
    pub score: SimilarityScore,
}

/// The similarity between two sketches.
///
/// Stores intersection and union sizes. The Jaccard similarity can be
/// derived from these values.
///
/// # Fields
///
/// * `intersection` — The size of the sketch intersection.
/// * `union` — The size of the sketch union.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimilarityScore {
    intersection: usize,
    union: usize,
}

impl SimilarityScore {
    /// Computes the Jaccard similarity coefficient.
    ///
    /// Returns `intersection / union` as an `f64`. Returns `0.0` when `union`
    /// is zero.
    pub fn jaccard_similarity(&self) -> f64 {
        if self.union == 0 {
            0.0
        } else {
            self.intersection as f64 / self.union as f64
        }
    }

    /// Creates a score for two sketches of equal `size`.
    ///
    /// The union is computed as `size * 2 - intersection`.
    pub fn new(intersection: usize, size: usize) -> Self {
        SimilarityScore { intersection, union: size * 2 - intersection }
    }
}

/// An iterable index producing [`SearchResult`] values.
pub trait IterableSketchIndex: Iterator + Send {}

/// Provides statistics about the index.
pub trait SketchIndexStats {
    /// Returns a snapshot of index metrics.
    fn show_stats(&self) -> IndexStats;
}

/// A snapshot of index metrics.
///
/// # Fields
///
/// * `entries` — Total entries in the index.
/// * `distinct_elements` — Unique elements across all sketches.
/// * `posting_references` — Total posting-list references.
/// * `average_posting_len` — Average posting-list length.
/// * `maximum_posting_len` — Longest posting list.
pub struct IndexStats {
    pub entries: usize,
    pub distinct_elements: usize,
    pub posting_references: usize,
    pub average_posting_len: f64,
    pub maximum_posting_len: usize,
}

/// An entry in the index.
///
/// Stores an entry's unique identifier, sketch key, and associated value.
///
/// # Type Parameters
///
/// * `S` — The sketch key type.
/// * `V` — The value type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<S: Send + Sync, V: Send + Sync> {
    pub id: EntryId,
    pub key: S,
    pub value: V,
}

pub(crate) mod checkpoint;
pub(crate) mod codec;
pub(crate) mod in_memory;
pub(crate) mod posting_list;

#[cfg(test)]
mod tests {
    use crate::index::{SearchOptions, SimilarityScore};

    #[test]
    fn new_computes_union() {
        let score = SimilarityScore::new(78, 52);
        assert_eq!(score.union, 26);
    }

    #[test]
    fn new_full_overlap() {
        let score = SimilarityScore::new(100, 100);
        assert_eq!(score.intersection, 100);
        assert_eq!(score.union, 100);
    }

    #[test]
    fn new_zero_intersection() {
        let score = SimilarityScore::new(0, 50);
        assert_eq!(score.intersection, 0);
        assert_eq!(score.union, 100);
    }

    #[test]
    fn jaccard_similarity_half_overlap() {
        let score = SimilarityScore { intersection: 5, union: 10 };
        assert!((score.jaccard_similarity() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_identical() {
        let score = SimilarityScore { intersection: 10, union: 10 };
        assert!((score.jaccard_similarity() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_no_overlap() {
        let score = SimilarityScore { intersection: 0, union: 10 };
        assert!((score.jaccard_similarity() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_zero_union_returns_zero() {
        let score = SimilarityScore { intersection: 0, union: 0 };
        assert!((score.jaccard_similarity() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn search_options_default() {
        let opts = SearchOptions::default();
        assert_eq!(opts.min_intersection, 0);
        assert!(!opts.return_zero_overlap);
        assert!(!opts.exclude_exact);
    }
}
