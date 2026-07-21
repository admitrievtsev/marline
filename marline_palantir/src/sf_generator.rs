use crate::types::{Chunk, SuperFeatureGenerator};
use crate::GEAR;
use num::integer::gcd;
use std::hash::{DefaultHasher, Hasher};

pub struct PalantirHasher {
    sampling_rate: u64,
    linear_coefficients: Vec<u64>,
    tier_list: Vec<u32>,
    features_num: usize,
}

fn lcm_checked(a: u32, b: u32) -> Option<u32> {
    let gcd_val = gcd(a, b);
    (a / gcd_val).checked_mul(b)
}

fn lcm_vec(nums: &Vec<u32>) -> Option<u32> {
    let mut res: u32 = 1;
    for &i in nums {
        res = lcm_checked(res, i)?;
    }
    Some(res)
}

impl PalantirHasher {
    pub fn new(sampling_rate: u64, tier_list: Vec<u32>) -> Self {
        let features_num = lcm_vec(&tier_list).unwrap() as usize;
        let mut linear_coefficients = Vec::with_capacity(features_num);
        for _ in 0..features_num {
            linear_coefficients.push(rand::random());
        }
        Self { sampling_rate, linear_coefficients, tier_list, features_num }
    }
}

impl SuperFeatureGenerator for PalantirHasher {
    fn generate(&self, chunk: &Chunk) -> Vec<crate::types::SuperFeature> {
        let mut features = vec![u64::MAX; self.features_num];

        let mask = (1u64 << self.sampling_rate) - 1;
        let mut fp = 0u64;

        for &byte in chunk.as_bytes() {
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

        let mut super_features = Vec::new();
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
                let hash = hasher.finish() as u32;
                super_features.push(crate::types::SuperFeature::new(tier_id as u8, hash, 0));
            }
        }

        super_features
    }
}
