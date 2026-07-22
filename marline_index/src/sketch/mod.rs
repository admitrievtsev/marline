//! Sketch data types for similarity search.
//!
//! This module defines the [`Sketch`] trait and its fixed-size
//! implementation [`FixedSketch<N>`], which represent compact fingerprints
//! of chunk contents. Sketches are sorted arrays of unique `u32` elements
//! that preserve similarity — similar chunks produce similar sketches.

use std::hash::Hash;

pub mod similarity;
pub use similarity::SimilarityScore;

/// Errors that can occur when creating a [`FixedSketch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchError {
    /// The sketch has zero elements (empty array provided).
    EmptySketch,
    /// The input contains duplicate values.
    DuplicateElement(u32),
}

/// Trait for a fixed-size set of `u32` elements used in similarity search.
///
/// Implementations must store elements sorted and free of duplicates.
/// The trait provides O(N) set operations such as [`intersection_size`](Sketch::intersection_size)
/// and O(log N) membership checking via [`contains`](Sketch::contains).
pub trait Sketch: Eq + Hash + Clone + Send + Sync + 'static {
    /// Iterator over sketch elements. Produces elements in sorted order.
    type Iter<'a>: Iterator<Item = u32>
    where
        Self: 'a;

    /// Returns the number of elements in the sketch.
    fn len(&self) -> usize;

    /// Returns `true` if the sketch is empty.
    fn is_empty(&self) -> bool;

    /// Iterates over the sketch elements in sorted order.
    fn iter(&self) -> Self::Iter<'_>;

    /// Returns the sketch elements as a contiguous slice.
    fn as_slice(&self) -> &[u32];

    /// Returns the number of elements present in both sketches. O(N).
    fn intersection_size(&self, other: &Self) -> usize;

    /// Returns `true` if the sketch contains the given value. O(log N).
    fn contains(&self, value: u32) -> bool;
}

/// Fixed-size sketch backed by a sorted array of `N` unique `u32` values.
///
/// [`FixedSketch`] is the primary implementation of the [`Sketch`] trait.
/// It stores elements in a `[u32; N]` array, sorted at construction time.
///
/// # Type Parameters
///
/// * `N` — The number of elements in the sketch. Supported values: 3, 4, 6.
///   (Aliases [`Sketch3`], [`Sketch4`], [`Sketch6`] are provided.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FixedSketch<const N: usize> {
    items: [u32; N],
}

impl<const N: usize> FixedSketch<N> {
    /// Creates a new `FixedSketch` from an array.
    ///
    /// The input is sorted automatically. Returns an error if the array
    /// contains duplicates or if `N = 0`.
    ///
    /// # Errors
    ///
    /// - [`SketchError::EmptySketch`] if `N == 0`.
    /// - [`SketchError::DuplicateElement`] if any duplicates are found.
    pub fn new(mut items: [u32; N]) -> Result<Self, SketchError> {
        if N == 0 {
            return Err(SketchError::EmptySketch);
        }
        items.sort_unstable();
        for pair in items.windows(2) {
            if pair[0] == pair[1] {
                return Err(SketchError::DuplicateElement(pair[0]));
            }
        }

        Ok(Self { items })
    }

    /// Returns the sketch elements as a fixed-size array reference.
    pub fn as_array(&self) -> &[u32; N] {
        &self.items
    }
}

impl<const N: usize> Sketch for FixedSketch<N> {
    type Iter<'a> = std::iter::Copied<std::slice::Iter<'a, u32>>;

    fn len(&self) -> usize {
        N
    }

    fn is_empty(&self) -> bool {
        N == 0
    }

    fn iter(&self) -> Self::Iter<'_> {
        self.items.iter().copied()
    }

    fn as_slice(&self) -> &[u32] {
        &self.items
    }

    fn intersection_size(&self, other: &Self) -> usize {
        let mut left = 0;
        let mut right = 0;
        let mut intersection = 0;

        while left < N && right < N {
            match self.items[left].cmp(&other.items[right]) {
                std::cmp::Ordering::Less => left += 1,
                std::cmp::Ordering::Greater => right += 1,
                std::cmp::Ordering::Equal => {
                    intersection += 1;
                    left += 1;
                    right += 1;
                }
            }
        }

        intersection
    }

    fn contains(&self, value: u32) -> bool {
        self.items.binary_search(&value).is_ok()
    }
}

/// A 3-element sketch (Palantir Tier-1, coarse-grained).
pub type Sketch3 = FixedSketch<3>;

/// A 4-element sketch (Palantir Tier-2, medium-grained).
pub type Sketch4 = FixedSketch<4>;

/// A 6-element sketch (Palantir Tier-3, fine-grained).
pub type Sketch6 = FixedSketch<6>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sorts_unsorted_input() {
        let s = FixedSketch::<3>::new([30, 10, 20]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
    }

    #[test]
    fn new_preserves_already_sorted() {
        let s = FixedSketch::<3>::new([10, 20, 30]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
    }

    #[test]
    fn new_rejects_duplicates() {
        let err = FixedSketch::<3>::new([1, 2, 2]).unwrap_err();
        assert_eq!(err, SketchError::DuplicateElement(2));
    }

    #[test]
    fn new_different_permutations_equal() {
        let a = FixedSketch::<6>::new([60, 10, 30, 20, 50, 40]).unwrap();
        let b = FixedSketch::<6>::new([40, 50, 60, 10, 20, 30]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn new_rejects_empty() {
        let err = FixedSketch::<0>::new([]).unwrap_err();
        assert_eq!(err, SketchError::EmptySketch);
    }

    #[test]
    fn as_array_returns_sorted() {
        let s = FixedSketch::<3>::new([30, 10, 20]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
    }

    #[test]
    fn as_slice_matches_as_array() {
        let s = FixedSketch::<3>::new([30, 10, 20]).unwrap();
        assert_eq!(s.as_slice(), s.as_array() as &[u32]);
    }

    #[test]
    fn len_returns_n() {
        assert_eq!(Sketch6::new([1, 2, 3, 4, 5, 6]).unwrap().len(), 6);
        assert_eq!(Sketch4::new([1, 2, 3, 4]).unwrap().len(), 4);
        assert_eq!(Sketch3::new([1, 2, 3]).unwrap().len(), 3);
    }

    #[test]
    fn is_empty_for_non_empty() {
        assert!(!Sketch6::new([1, 2, 3, 4, 5, 6]).unwrap().is_empty());
    }

    #[test]
    fn iter_returns_all_elements() {
        let s = FixedSketch::<3>::new([30, 10, 20]).unwrap();
        let collected: Vec<u32> = s.iter().collect();
        assert_eq!(collected, vec![10, 20, 30]);
    }

    #[test]
    fn contains_present_value() {
        let s = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert!(s.contains(30));
    }

    #[test]
    fn contains_absent_value() {
        let s = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert!(!s.contains(99));
    }

    #[test]
    fn full_intersection() {
        let a = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        let b = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert_eq!(a.intersection_size(&b), 6);
    }

    #[test]
    fn partial_intersection() {
        let a = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        let b = FixedSketch::<6>::new([10, 20, 30, 70, 80, 90]).unwrap();
        assert_eq!(a.intersection_size(&b), 3);
    }

    #[test]
    fn zero_intersection() {
        let a = FixedSketch::<3>::new([10, 20, 30]).unwrap();
        let b = FixedSketch::<3>::new([40, 50, 60]).unwrap();
        assert_eq!(a.intersection_size(&b), 0);
    }

    #[test]
    fn self_intersection() {
        let a = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert_eq!(a.intersection_size(&a), 6);
    }

    #[test]
    fn intersection_is_commutative() {
        let a = FixedSketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        let b = FixedSketch::<6>::new([10, 20, 30, 70, 80, 90]).unwrap();
        assert_eq!(a.intersection_size(&b), b.intersection_size(&a));
    }
}
