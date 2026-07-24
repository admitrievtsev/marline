use super::chunk::Chunk;

/// A single tiered similarity fingerprint.
///
/// Each `SuperFeature` belongs to a specific tier and carries a hash value
/// derived from a group of raw features.  The tier structure enables
/// progressive similarity search: coarse tiers trade precision for speed,
/// while finer tiers provide higher accuracy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SuperFeature {
    /// Which tier this feature belongs to (0, 1, 2, …).
    tier_id: u8,
    /// The hashed super-feature value.
    value: u32,
    /// Version identifier for the algorithm that produced this feature.
    version_id: u32,
}

impl SuperFeature {
    /// Creates a new `SuperFeature`.
    pub fn new(tier_id: u8, value: u32, version_id: u32) -> Self {
        Self { tier_id, value, version_id }
    }

    /// Returns the tier index of this super-feature.
    pub fn tier_id(&self) -> u8 {
        self.tier_id
    }

    /// Returns the hashed value of this super-feature.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// Returns the algorithm version that produced this feature.
    pub fn version_id(&self) -> u32 {
        self.version_id
    }
}

/// Configuration for multi-tier super-feature grouping.
///
/// `tier_list` specifies the grouping size for each tier.  For example,
/// `vec![3, 4, 6]` creates three tiers where groups of 3, 4, and 6 raw
/// features are each hashed into a single super-feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierConfig {
    /// Group sizes for each tier, ordered from coarsest to finest.
    pub tier_list: Vec<u32>,
}

impl TierConfig {
    /// Creates a new `TierConfig` with the given tier group sizes.
    pub fn new(tier_list: Vec<u32>) -> Self {
        Self { tier_list }
    }
}

/// Generates a set of [`SuperFeature`] values from a [`Chunk`].
///
/// Implementations define how raw chunk bytes are converted into
/// similarity-preserving fingerprints that can be indexed and searched.
pub trait SuperFeatureGenerator {
    /// Computes super-features for the given chunk.
    fn generate(&self, chunk: &Chunk) -> Vec<SuperFeature>;
}
