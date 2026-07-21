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
}
