use marline_storage::ChunkHash;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockID<H: ChunkHash> {
    pub hash: H,
}

impl<H: ChunkHash> BlockID<H> {
    pub fn new(hash: H) -> Self {
        Self { hash }
    }
}
