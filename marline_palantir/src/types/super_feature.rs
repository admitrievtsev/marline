#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SuperFeature {
    tier_id: u8,
    value: u64,
    version_id: u32,
}

impl SuperFeature {
    pub fn new(tier_id: u8, value: u64, version_id: u32) -> Self {
        Self { tier_id, value, version_id }
    }
    pub fn tier_id(&self) -> u8 {
        self.tier_id
    }
    pub fn value(&self) -> u64 {
        self.value
    }
    
    pub fn version_id(&self) -> u32 {
        self.version_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierConfig {
    pub tier_list: Vec<u32>,
}

impl TierConfig {
    pub fn new(tier_list: Vec<u32>) -> Self {
        Self { tier_list }
    }
}

pub trait SuperFeatureGenerator {
    fn generate(&self, chunk: &[u8]) -> Vec<SuperFeature>;
}
