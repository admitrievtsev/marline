use super::chunk::Chunk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SuperFeature {
    tier_id: u8,
    value: u32,
    version_id: u32,
}

impl SuperFeature {
    pub fn new(tier_id: u8, value: u32, version_id: u32) -> Self {
        Self { tier_id, value, version_id }
    }
    pub fn tier_id(&self) -> u8 {
        self.tier_id
    }
    pub fn value(&self) -> u32 {
        self.value
    }
    
    pub fn version_id(&self) -> u32 {
        self.version_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierConfig {
    tier_id: u8,
    k: u8,
    s: u8,
}

impl TierConfig {
    pub fn new(tier_id: u8, k: u8, s: u8) -> Self {
        Self { tier_id, k, s }
    }
    pub fn tier_id(&self) -> u8 {
        self.tier_id
    }
    pub fn k(&self) -> u8 {
        self.k
    }
    pub fn s(&self) -> u8 {
        self.s
    }
}

pub trait SuperFeatureGenerator {
    fn generate(&self, chunk: &Chunk) -> Vec<SuperFeature>;
}
