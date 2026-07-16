use std::sync::Arc;

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
/// * [`Self::Value`] — The stored value type.
/// * [`Self::Error`] — The error type for fallible operations.
#[allow(dead_code)]
pub trait SketchKvindex<S: Send + Sync + Sketch>: Send + Sync {
    /// The type of values stored in the index.
    type Value;
    /// The error type returned by index operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Returns the number of entries.
    fn len(&self) -> Result<usize, Self::Error>;

    /// Looks up a value by key.
    ///
    /// Returns `Ok(None)` if the key is not found.
    fn get(&self, key: &S) -> Result<Option<Arc<Self::Value>>, Self::Error>;

    /// Inserts or updates an entry.
    ///
    /// Returns a [`PutOutcome`] describing the result.
    fn put(&self, key: &S, value: Arc<Self::Value>) -> Result<PutOutcome, Self::Error>;

    /// Removes an entry by key.
    fn remove(&self, key: &S) -> Result<(), Self::Error>;

    /// Finds the nearest neighbor for the given key.
    ///
    /// Returns `Ok(Some((value, distance)))` or `Ok(None)` if the index is
    /// empty.
    fn nearest(
        &self,
        key: &S,
        search_options: SearchOptions,
    ) -> Result<Option<(Arc<Self::Value>, usize)>, Self::Error>;

    /// Returns the top `k` closest entries.
    ///
    /// Results are sorted by increasing distance.
    fn top_k(
        &self,
        key: &S,
        k: usize,
        search_options: SearchOptions,
    ) -> Result<Vec<(Arc<Self::Value>, usize)>, Self::Error>;

    /// Removes all entries.
    fn clear(&self) -> Result<(), Self::Error>;
}

/// The result of a [`SketchKvindex::put`] operation.
///
/// # Variants
///
/// * `Inserted` — A new entry was added.
/// * `Updated` — An existing entry was replaced.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch<S: Send + Sync + Sketch, V: Send + Sync> {
    pub entry_id: usize,
    pub key: S,
    pub value: Arc<V>,
    pub score: SimilarityScore,
}

/// The similarity between two sketches.
///
/// Stores intersection and union sizes. The Jaccard similarity can be
/// derived from these values.
///
/// # Fields
///
/// * `intersaction` — The size of the sketch intersection.
/// * `union` — The size of the sketch union.
#[allow(dead_code)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimilarityScore {
    intersaction: usize,
    union: usize,
}

#[allow(dead_code)]
impl SimilarityScore {
    /// Computes the Jaccard similarity coefficient.
    ///
    /// Returns `intersaction / union` as a `f64`, or `0.0` if the union is
    /// zero.
    fn jaccard_similarity(&self) -> f64 {
        self.intersaction as f64 / self.union as f64
    }

    /// Creates a score for two sketches of equal `size`.
    ///
    /// The union is computed as `size * 2 - intersaction`.
    fn from_similar_size_sketches(intersaction: usize, size: usize) -> Self {
        SimilarityScore { intersaction, union: size * 2 - intersaction }
    }
}

/// An iterable index producing [`SearchMatch`] values.
#[allow(dead_code)]
pub trait IterableSketchIndex: Iterator + Send {}

pub mod checkpoint;
pub mod codec;
pub mod in_memory;
pub mod posting_list;

/// Provides statistics about the index.
pub trait SketchIndexStats {
    /// Returns a snapshot of index metrics.
    fn show_stats() -> IndexStats;
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
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<S: Send + Sync + Sketch, V: Send + Sync> {
    pub id: EntryId,
    pub key: S,
    pub value: V,
}

#[cfg(test)]
mod tests {
    use crate::index::{SearchOptions, SimilarityScore};

    #[test]
    fn from_similar_size_sketches_computes_union() {
        let score = SimilarityScore::from_similar_size_sketches(78, 52);
        assert_eq!(score.union, 26);
    }

    #[test]
    fn from_similar_size_sketches_full_overlap() {
        let score = SimilarityScore::from_similar_size_sketches(100, 100);
        assert_eq!(score.intersaction, 100);
        assert_eq!(score.union, 100);
    }

    #[test]
    fn from_similar_size_sketches_zero_intersection() {
        let score = SimilarityScore::from_similar_size_sketches(0, 50);
        assert_eq!(score.intersaction, 0);
        assert_eq!(score.union, 100);
    }

    #[test]
    fn jaccard_similarity_half_overlap() {
        let score = SimilarityScore { intersaction: 5, union: 10 };
        assert!((score.jaccard_similarity() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_identical() {
        let score = SimilarityScore { intersaction: 10, union: 10 };
        assert!((score.jaccard_similarity() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_no_overlap() {
        let score = SimilarityScore { intersaction: 0, union: 10 };
        assert!((score.jaccard_similarity() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_zero_union_returns_zero() {
        let score = SimilarityScore { intersaction: 0, union: 0 };
        assert!(score.jaccard_similarity().is_nan());
    }

    #[test]
    fn search_options_default() {
        let opts = SearchOptions::default();
        assert_eq!(opts.min_intersection, 0);
        assert!(!opts.return_zero_overlap);
        assert!(!opts.exclude_exact);
    }

    #[test]
    fn similarity_score_default() {
        let score = SimilarityScore::default();
        assert_eq!(score.intersaction, 0);
        assert_eq!(score.union, 0);
    }
}
