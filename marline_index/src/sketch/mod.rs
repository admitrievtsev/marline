//! Sketch data types for similarity search.
//!
//! This module defines the [`Sketch`] trait and its fixed-size
//! implementation [`FixedSketch<F, N>`], which represent compact fingerprints
//! of items. Sketches are sorted arrays of feature elements
//! that preserve similarity — similar chunks produce similar sketches.

use std::hash::Hash;

pub mod similarity;
pub use similarity::SimilarityScore;

/// Errors that can occur when creating a [`FixedSketch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchError {
    /// The sketch has zero elements (empty array provided).
    EmptySketch,
}

/// Trait for a fixed-size set of features used in similarity search.
///
/// Implementations must store elements sorted.
/// The trait provides O(N) set operations such as [`intersection_size`](Sketch::intersection_size)
/// and O(log N) membership checking via [`contains`](Sketch::contains).
pub trait Sketch: Eq + Hash + Clone + Send + Sync + 'static {
    /// Feature element stored in the sketch.
    type Feature: Copy + Ord + Eq + Hash + Send + Sync + 'static;

    /// Iterator over sketch elements. Produces elements in sorted order.
    type Iter<'a>: Iterator<Item = Self::Feature>
    where
        Self: 'a;

    /// Returns the number of elements in the sketch.
    fn len(&self) -> usize;

    /// Returns `true` if the sketch is empty.
    fn is_empty(&self) -> bool;

    /// Iterates over the sketch elements in sorted order.
    fn iter(&self) -> Self::Iter<'_>;

    /// Returns the sketch elements as a contiguous slice.
    fn as_slice(&self) -> &[Self::Feature];

    /// Returns the number of elements present in both sketches. O(N).
    fn intersection_size(&self, other: &Self) -> usize;

    /// Returns `true` if the sketch contains the given value. O(log N).
    fn contains(&self, value: Self::Feature) -> bool;
}

/// Fixed-size sketch backed by a sorted array of `N` feature values.
///
/// [`FixedSketch`] is the primary implementation of the [`Sketch`] trait.
/// It stores elements in a `[F; N]` array, sorted at construction time.
///
/// # Type Parameters
///
/// * `F` — The feature element type.
/// * `N` — The number of elements in the sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FixedSketch<F, const N: usize> {
    items: [F; N],
}

impl<F, const N: usize> FixedSketch<F, N>
where
    F: Copy + Ord + Eq + Hash,
{
    /// Creates a new `FixedSketch` from an array.
    ///
    /// The input is sorted automatically. Returns an error if the array
    /// if `N = 0`.
    ///
    /// # Errors
    ///
    /// - [`SketchError::EmptySketch`] if `N == 0`.
    pub fn new(mut items: [F; N]) -> Result<Self, SketchError> {
        if N == 0 {
            return Err(SketchError::EmptySketch);
        }
        items.sort_unstable();

        Ok(Self { items })
    }

    /// Returns the sketch elements as a fixed-size array reference.
    pub fn as_array(&self) -> &[F; N] {
        &self.items
    }
}

impl<F, const N: usize> Sketch for FixedSketch<F, N>
where
    F: Copy + Ord + Eq + Hash + Send + Sync + 'static,
{
    type Feature = F;
    type Iter<'a>
        = std::iter::Copied<std::slice::Iter<'a, F>>
    where
        F: 'a;

    fn len(&self) -> usize {
        N
    }

    fn is_empty(&self) -> bool {
        N == 0
    }

    fn iter(&self) -> Self::Iter<'_> {
        self.items.iter().copied()
    }

    fn as_slice(&self) -> &[F] {
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

    fn contains(&self, value: F) -> bool {
        self.items.binary_search(&value).is_ok()
    }
}

/// A fixed-size sketch of `u32` features.
pub type U32Sketch<const N: usize> = FixedSketch<u32, N>;

/// A fixed-size sketch of `u64` features.
pub type U64Sketch<const N: usize> = FixedSketch<u64, N>;

/// Deprecated compatibility alias for existing users.
#[deprecated(note = "use U32Sketch<3> or FixedSketch<u32, 3>")]
pub type Sketch3 = U32Sketch<3>;

/// Deprecated compatibility alias for existing users.
#[deprecated(note = "use U32Sketch<4> or FixedSketch<u32, 4>")]
pub type Sketch4 = U32Sketch<4>;

/// Deprecated compatibility alias for existing users.
#[deprecated(note = "use U32Sketch<6> or FixedSketch<u32, 6>")]
pub type Sketch6 = U32Sketch<6>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sorts_unsorted_input() {
        let s = U32Sketch::<3>::new([30, 10, 20]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
    }

    #[test]
    fn new_preserves_already_sorted() {
        let s = U32Sketch::<3>::new([10, 20, 30]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
    }

    #[test]
    fn new_different_permutations_equal() {
        let a = U32Sketch::<6>::new([60, 10, 30, 20, 50, 40]).unwrap();
        let b = U32Sketch::<6>::new([40, 50, 60, 10, 20, 30]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn new_rejects_empty() {
        let err = U32Sketch::<0>::new([]).unwrap_err();
        assert_eq!(err, SketchError::EmptySketch);
    }

    #[test]
    fn as_array_returns_sorted() {
        let s = U32Sketch::<3>::new([30, 10, 20]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
    }

    #[test]
    fn as_slice_matches_as_array() {
        let s = U32Sketch::<3>::new([30, 10, 20]).unwrap();
        assert_eq!(s.as_slice(), s.as_array() as &[u32]);
    }

    #[test]
    fn len_returns_n() {
        assert_eq!(U32Sketch::<6>::new([1, 2, 3, 4, 5, 6]).unwrap().len(), 6);
        assert_eq!(U32Sketch::<4>::new([1, 2, 3, 4]).unwrap().len(), 4);
        assert_eq!(U32Sketch::<3>::new([1, 2, 3]).unwrap().len(), 3);
    }

    #[test]
    fn is_empty_for_non_empty() {
        assert!(!U32Sketch::<6>::new([1, 2, 3, 4, 5, 6]).unwrap().is_empty());
    }

    #[test]
    fn iter_returns_all_elements() {
        let s = U32Sketch::<3>::new([30, 10, 20]).unwrap();
        let collected: Vec<u32> = s.iter().collect();
        assert_eq!(collected, vec![10, 20, 30]);
    }

    #[test]
    fn contains_present_value() {
        let s = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert!(s.contains(30));
    }

    #[test]
    fn contains_absent_value() {
        let s = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert!(!s.contains(99));
    }

    #[test]
    fn full_intersection() {
        let a = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        let b = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert_eq!(a.intersection_size(&b), 6);
    }

    #[test]
    fn partial_intersection() {
        let a = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        let b = U32Sketch::<6>::new([10, 20, 30, 70, 80, 90]).unwrap();
        assert_eq!(a.intersection_size(&b), 3);
    }

    #[test]
    fn zero_intersection() {
        let a = U32Sketch::<3>::new([10, 20, 30]).unwrap();
        let b = U32Sketch::<3>::new([40, 50, 60]).unwrap();
        assert_eq!(a.intersection_size(&b), 0);
    }

    #[test]
    fn self_intersection() {
        let a = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        assert_eq!(a.intersection_size(&a), 6);
    }

    #[test]
    fn intersection_is_commutative() {
        let a = U32Sketch::<6>::new([10, 20, 30, 40, 50, 60]).unwrap();
        let b = U32Sketch::<6>::new([10, 20, 30, 70, 80, 90]).unwrap();
        assert_eq!(a.intersection_size(&b), b.intersection_size(&a));
    }

    #[test]
    fn supports_u64_features() {
        let s = U64Sketch::<3>::new([30_u64, 10, 20]).unwrap();
        assert_eq!(s.as_array(), &[10, 20, 30]);
        assert!(s.contains(20));
    }
}
