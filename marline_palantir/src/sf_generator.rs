use crate::types::{Chunk, SuperFeature};
use crate::utils::lcm_vec;
use crate::GEAR;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};

/// Generates a set of [`SuperFeature`] values from a [`Chunk`].
///
/// Implementations define how raw chunk bytes are converted into
/// similarity-preserving fingerprints that can be indexed and searched.
pub trait SuperFeatureGenerator {
    /// Computes super-features for the given chunk.
    fn generate(&self, chunk: &Chunk) -> Vec<SuperFeature>;

    fn generate_with_table(&self, chunk: &Chunk) -> (Vec<SuperFeature>, HashMap<u32, u32>);

    fn generate_from_raw(&self, features: &[u32]) -> Vec<crate::types::SuperFeature>;
}

/// Generates super-features using a gear-hash rolling hash with random linear projections.
///
/// `PalantirHasher` implements the core feature-extraction algorithm of the
/// Palantir method.  It scans chunk bytes with a gear-hash rolling hash,
/// records minimum feature values per position, groups them by tier, and
/// produces a final set of [`SuperFeature`](crate::types::SuperFeature) values.
///
/// # How it works
///
/// 1. A rolling hash (`fp`) is updated for each byte using the `GEAR` table.
/// 2. When `fp` hits a zero value at the lowest `sampling_rate` bits, a
///    feature-minimisation step is triggered: each position's candidate value is
///    updated to `min(current, transform)` where `transform` is a linear
///    projection of the fingerprint.
/// 3. After the scan, raw features are grouped by `tier_list` sizes, sorted,
///    and hashed into the final [`SuperFeature`](crate::types::SuperFeature) values.
#[derive(Clone)]
pub struct PalantirHasher {
    /// Number of trailing zero bits required to trigger a feature update.
    sampling_rate: u64,
    /// Random linear coefficients for feature-position transformations.
    linear_coefficients: Vec<u64>,
    /// Group sizes for each super-feature tier.
    tier_list: Vec<u32>,
    /// Total number of raw features (LCM of tier_list).
    features_num: usize,
}

impl PalantirHasher {
    /// Creates a new `PalantirHasher`.
    ///
    /// The number of raw features is the least common multiple of all tier
    /// sizes.  Random linear coefficients are generated for each feature
    /// position.
    ///
    /// # Panics
    ///
    /// Panics if `lcm_vec(&tier_list)` overflows (i.e., the product of tier
    /// sizes exceeds `u32::MAX`).
    ///
    /// # Arguments
    /// * `sampling_rate` — Number of trailing-zero bits required on the
    ///   gear-hash fingerprint to trigger feature extraction.
    /// * `tier_list` — Group sizes for each tier (e.g., `vec![4, 3, 2]`).
    pub fn new(sampling_rate: u64, tier_list: Vec<u32>) -> Self {
        assert!(sampling_rate <= 64);
        let features_num = lcm_vec(&tier_list).unwrap() as usize;
        let mut linear_coefficients = Vec::with_capacity(features_num);
        for _ in 0..features_num {
            linear_coefficients.push(rand::random());
        }
        Self { sampling_rate, linear_coefficients, tier_list, features_num }
    }

    /// Dynamic override number of features per chunk
    pub fn with_features_num(sampling_rate: u64, tier_list: Vec<u32>, features_num: usize) -> Self {
        let mut linear_coefficients = Vec::with_capacity(features_num);
        for _ in 0..features_num {
            linear_coefficients.push(rand::random());
        }
        Self { sampling_rate, linear_coefficients, tier_list, features_num }
    }
}

impl SuperFeatureGenerator for PalantirHasher {
    /// Generates super-features from a chunk's content.
    ///
    /// The algorithm:
    /// 1. Initialises `features_num` raw feature slots to `u64::MAX`.
    /// 2. Iterates each byte, updating the gear-hash fingerprint.
    /// 3. On a sampling hit, conditionally updates each feature slot with a
    ///    linear transformation of the fingerprint.
    /// 4. Groups the final features by tier, sorts each group, and hashes them
    ///    into a [`SuperFeature`](crate::types::SuperFeature).
    fn generate(&self, chunk: &Chunk) -> Vec<crate::types::SuperFeature> {
        let data = chunk.as_bytes();
        let mut features = vec![u64::MAX; self.features_num];

        let mask = (1u64 << self.sampling_rate) - 1;
        let mut fp = 0u64;

        for &byte in data {
            fp = (fp << 1).wrapping_add(GEAR[byte as usize]);

            if fp & mask == 0 {
                for (i, feature) in features.iter_mut().enumerate().take(self.features_num) {
                    let transform =
                        self.linear_coefficients[i].wrapping_mul(fp).wrapping_add(byte as u64)
                            % (1u64 << 32);
                    if *feature > transform {
                        *feature = transform;
                    }
                }
            }
        }
        let capacity =
            self.tier_list.iter().map(|tier| self.features_num as u32 / tier).sum::<u32>() as usize;
        let mut super_features: Vec<SuperFeature> = Vec::with_capacity(capacity);
        for (tier_id, &group_size) in self.tier_list.iter().enumerate() {
            let gs = group_size as usize;
            let num_sf = self.features_num / gs;
            for sf_idx in 0..num_sf {
                let start = sf_idx * gs;
                let mut group: Vec<u64> = features[start..start + gs].to_vec();
                group.sort();
                let mut hasher = DefaultHasher::new();
                for &val in &group {
                    hasher.write_u64(val);
                }
                let hash = hasher.finish();
                super_features.push(crate::types::SuperFeature::new(tier_id as u8, hash as u32));
            }
        }

        super_features
    }

    fn generate_from_raw(&self, features: &[u32]) -> Vec<crate::types::SuperFeature> {
        let capacity =
            self.tier_list.iter().map(|tier| self.features_num as u32 / tier).sum::<u32>() as usize;
        let mut super_features: Vec<SuperFeature> = Vec::with_capacity(capacity);
        for (tier_id, &group_size) in self.tier_list.iter().enumerate() {
            let gs = group_size as usize;
            let num_sf = self.features_num / gs;
            for sf_idx in 0..num_sf {
                let start = sf_idx * gs;
                let mut group: Vec<u32> = features[start..start + gs].to_vec();
                group.sort();
                let mut hasher = DefaultHasher::new();
                for &val in &group {
                    hasher.write_u32(val);
                }
                let hash = hasher.finish();
                super_features.push(crate::types::SuperFeature::new(tier_id as u8, hash as u32));
            }
        }

        super_features
    }

    fn generate_with_table(&self, chunk: &Chunk) -> (Vec<SuperFeature>, HashMap<u32, u32>) {
        let data = chunk.as_bytes();
        let mut features = vec![u64::MAX; self.features_num];

        let mask = (1u64 << self.sampling_rate) - 1;
        let mut fp = 0u64;

        for &byte in data {
            fp = (fp << 1).wrapping_add(GEAR[byte as usize]);

            if fp & mask == 0 {
                for (i, feature) in features.iter_mut().enumerate().take(self.features_num) {
                    let transform =
                        self.linear_coefficients[i].wrapping_mul(fp).wrapping_add(byte as u64)
                            % (1u64 << 32);
                    if *feature > transform {
                        *feature = transform;
                    }
                }
            }
        }
        let capacity =
            self.tier_list.iter().map(|tier| self.features_num as u32 / tier).sum::<u32>() as usize;
        let mut super_features: Vec<SuperFeature> = Vec::with_capacity(capacity);
        for (tier_id, &group_size) in self.tier_list.iter().enumerate() {
            let gs = group_size as usize;
            let num_sf = self.features_num / gs;
            for sf_idx in 0..num_sf {
                let start = sf_idx * gs;
                let mut group: Vec<u64> = features[start..start + gs].to_vec();
                group.sort();
                let mut hasher = DefaultHasher::new();
                for &val in &group {
                    hasher.write_u64(val);
                }
                let hash = hasher.finish();
                super_features.push(crate::types::SuperFeature::new(tier_id as u8, hash as u32));
            }
        }

        (super_features, HashMap::new())
    }
}
