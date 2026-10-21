use crate::types::BlockID;
use marline_storage::ChunkHash;
use std::collections::HashMap;

pub struct FPTable<H: ChunkHash> {
    index: HashMap<H, BlockID<H>>,
}

#[allow(dead_code)]
impl<H: ChunkHash> FPTable<H> {
    pub fn new() -> Self {
        Self { index: HashMap::new() }
    }
    pub fn contains(&self, fingerprint: &H) -> bool {
        self.index.contains_key(fingerprint)
    }
    pub fn lookup(&self, fingerprint: &H) -> Option<&BlockID<H>> {
        self.index.get(fingerprint)
    }
    pub fn insert(&mut self, fingerprint: H, block_id: BlockID<H>) {
        self.index.insert(fingerprint, block_id);
    }
    pub fn remove(&mut self, fingerprint: &H) -> Option<BlockID<H>> {
        self.index.remove(fingerprint)
    }
    pub fn len(&self) -> usize {
        self.index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}
