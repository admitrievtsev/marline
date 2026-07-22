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

    /// Creates a score for two sketches of potentially different sizes.
    ///
    /// The union is computed as `len_a + len_b - intersection`.
    pub fn new_from_two(intersection: usize, len_a: usize, len_b: usize) -> Self {
        SimilarityScore { intersection, union: len_a + len_b - intersection }
    }
}

#[cfg(test)]
mod tests {
    use crate::sketch::SimilarityScore;

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
    fn new_from_two_different_sizes() {
        let score = SimilarityScore::new_from_two(3, 4, 5);
        // union = 4 + 5 - 3 = 6
        assert_eq!(score.union, 6);
        assert_eq!(score.intersection, 3);
    }

    #[test]
    fn new_from_two_same_sizes() {
        let from_two = SimilarityScore::new_from_two(5, 10, 10);
        let from_one = SimilarityScore::new(5, 10);
        assert_eq!(from_two.union, from_one.union);
        assert_eq!(from_two.intersection, from_one.intersection);
    }
}
