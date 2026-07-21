use super::super_feature::SuperFeature;

#[derive(Clone)]
pub struct ChunkDigest {
    pub super_features: Vec<SuperFeature>,
}

impl ChunkDigest {
    pub fn new(super_features: Vec<SuperFeature>) -> Self {
        Self { super_features }
    }
}
