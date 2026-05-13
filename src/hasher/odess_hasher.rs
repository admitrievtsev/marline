use crate::encoder::GEAR;
use crate::hasher::{SBCHash, SBCHasher};
use std::hash::{DefaultHasher, Hash, Hasher};

pub const SUPER_FEATURES_NUM: usize = 8;
pub const FEATURES_NUM: usize = 16;
#[derive(Default, Debug)]
pub struct OdessHash {
    hash: [u64; SUPER_FEATURES_NUM],
}

impl Hash for OdessHash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state)
    }
}

impl Clone for OdessHash {
    fn clone(&self) -> Self {
        OdessHash { hash: self.hash }
    }
}

impl Eq for OdessHash {}

impl PartialEq<Self> for OdessHash {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl SBCHash for OdessHash {
    fn new_with_u32(_: u32) -> Self {
        todo!()
    }

    fn next_hash(&self) -> Self {
        let mut odess_hash = self.clone();
        if odess_hash.hash[0] < u64::MAX {
            odess_hash.hash[0] += 1;
        } else if odess_hash.hash[1] < u64::MAX {
            odess_hash.hash[0] = 0;
            odess_hash.hash[1] += 1;
        } else if odess_hash.hash[2] < u64::MAX {
            odess_hash.hash[0] = 0;
            odess_hash.hash[1] = 0;
            odess_hash.hash[2] += 1;
        } else {
            odess_hash.hash = [u64::MAX; SUPER_FEATURES_NUM]
        }
        odess_hash
    }

    fn last_hash(&self) -> Self {
        let mut odess_hash = self.clone();
        if odess_hash.hash[0] > 0 {
            odess_hash.hash[0] -= 1;
        } else if odess_hash.hash[1] > 0 {
            odess_hash.hash[0] = u64::MAX;
            odess_hash.hash[1] -= 1;
        } else if odess_hash.hash[2] > 0 {
            odess_hash.hash[0] = u64::MAX;
            odess_hash.hash[1] = u64::MAX;
            odess_hash.hash[2] -= 1;
        } else {
            odess_hash.hash = [0u64; SUPER_FEATURES_NUM]
        }
        odess_hash
    }

    fn get_key_for_graph_clusterer(&self) -> u32 {
        todo!()
    }

    fn as_slice(&self) -> &[u64] {
        &self.hash
    }
}

/// Реализация метода Odess для вычисления признаков чанка
pub struct OdessHasher {
    sampling_rate: u64,
    linear_coeffs: [u64; FEATURES_NUM],
}

impl SBCHasher for OdessHasher {
    type Hash = OdessHash;
    fn calculate_hash(&self, chunk: &[u8]) -> OdessHash {
        let mut features = [u64::MAX; FEATURES_NUM];
        let mask = self.sampling_rate - 1;
        let mut fp = 0u64;

        for &byte in chunk {
            // Gear rolling hash: FP = (FP << 1) + Gear[byte]
            fp = (fp << 1).wrapping_add(GEAR[byte as usize]);

            // Content-defined sampling
            if fp & mask == 0 {
                for (i, feature) in features.iter_mut().enumerate() {
                    let transform = self.linear_coeffs[i]
                        .wrapping_mul(fp)
                        .wrapping_add(byte as u64)
                        % (1u64 << 32);
                    if *feature > transform {
                        *feature = transform;
                    }
                }
            }
        }
        if SUPER_FEATURES_NUM != FEATURES_NUM {
            let mut group_1 = features[0..SUPER_FEATURES_NUM].to_vec();
            let mut group_2 = features[SUPER_FEATURES_NUM..].to_vec();
            group_1.sort();
            group_2.sort();

            let mut sfs = [0; SUPER_FEATURES_NUM];
            for i in 0..SUPER_FEATURES_NUM {
                let mut hasher = DefaultHasher::new();
                hasher.write_u64(group_1[i]);
                hasher.write_u64(group_2[i]);
                sfs[i] = hasher.finish();
            }

            OdessHash { hash: sfs }
        } else {
            let mut slice = [0u64; SUPER_FEATURES_NUM];

            #[allow(clippy::manual_memcpy)]
            for i in 0..SUPER_FEATURES_NUM {
                slice[i] = features[i];
            }
            OdessHash { hash: slice }
        }
    }
}

impl Default for OdessHasher {
    fn default() -> Self {
        Self::new(7)
    }
}

impl OdessHasher {
    pub fn new(sampling_ratio: u32) -> Self {
        let sampling_rate = 1u64 << sampling_ratio;

        let linear_coeffs = [
            0x3f9c9a5d4e8a3b2a,
            0x7d4f1b2c3a6e5d8c,
            0x1a2b3c4d5e6f7a8b,
            0x2c3d4e5f6a7b8c9d,
            0x3e4f5a6b7c8d9e0f,
            0x4a5b6c7d8e9fa0b1,
            0x5c6d7e8f9a0b1c2d,
            0x6e7f8a9b0c1d2e3f,
            0x7a8b9c0d1e2f3a4b,
            0x8c9daebf0c1d2e3a,
            0x9ea0b1c2d3e4,
            0xa2b3c4d5e6f7,
            0x0c1d2e3f,
            0x1e2f3a4b,
            0x2c3d4e5f,
            0x3a4b5c6d,
        ];

        OdessHasher {
            sampling_rate,
            linear_coeffs,
        }
    }
}
