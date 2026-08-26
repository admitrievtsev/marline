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
}

impl SuperFeature {
    pub fn new(tier_id: u8, value: u32) -> Self {
        Self { tier_id, value }
    }

    /// Returns the tier index of this super-feature.
    pub fn tier_id(&self) -> u8 {
        self.tier_id
    }

    /// Returns the hashed value of this super-feature.
    pub fn value(&self) -> u32 {
        self.value
    }
}

/// Configuration for multi-tier super-feature grouping.
///
/// `tier_list` specifies the group size (number of raw features packed into one
/// super-feature) per tier.  The MetadataManager internally computes the number
/// of super-features per tier from the LCM of all group sizes.
///
/// For example, group sizes `[4, 3, 2]` give LCM = 12, producing
/// 12/4 = 3, 12/3 = 4, and 12/2 = 6 super-features per tier
/// (coarse → fine, i.e. many SFs in fine tier, few in coarse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierConfig<const N: usize> {
    /// Group size per tier (raw features packed into each super-feature).
    pub tier_list: [u32; N],
    /// Override for total raw features (defaults to LCM of tier_list).
    pub features_num: Option<usize>,
}

impl<const N: usize> TierConfig<N> {
    /// Creates a new `TierConfig` with the given group sizes per tier.
    pub fn new(tier_list: [u32; N]) -> Self {
        Self { tier_list, features_num: None }
    }

    pub fn with_features_num(tier_list: [u32; N], features_num: usize) -> Self {
        Self { tier_list, features_num: Some(features_num) }
    }
}
