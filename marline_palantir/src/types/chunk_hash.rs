use super::fingerprint::Fingerprint;
use super::super_feature::SuperFeature;

#[derive(Debug, Clone)]
pub struct ChunkHash {
    pub fingerprint: Fingerprint,
    pub super_features: Vec<SuperFeature>,
}

impl ChunkHash {
    pub fn new(fingerprint: Fingerprint, super_features: Vec<SuperFeature>) -> Self {
        Self { fingerprint, super_features }
    }
}
