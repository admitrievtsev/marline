use super::super_feature::SuperFeature;

/// A compact digest of a chunk, composed of multiple [`SuperFeature`] values.
///
/// [`ChunkDigest`] is produced by a [`SuperFeatureGenerator`] and is used
/// to query the multi-tier similarity index.  It is essentially a typed
/// wrapper around `Vec<SuperFeature>`.
///
/// [`SuperFeatureGenerator`]: super::SuperFeatureGenerator
#[derive(Clone)]
pub struct ChunkDigest {
    /// The set of super-features representing this chunk.
    pub super_features: Vec<SuperFeature>,
}

impl ChunkDigest {
    /// Creates a new `ChunkDigest` from a vector of [`SuperFeature`]s.
    pub fn new(super_features: Vec<SuperFeature>) -> Self {
        Self { super_features }
    }
}
