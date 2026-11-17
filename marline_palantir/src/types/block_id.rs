use chunkfs::ChunkHash;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockID<H: ChunkHash> {
    hash: H,
}

impl<H: ChunkHash> BlockID<H> {
    pub fn new(hash: H) -> Self {
        Self { hash }
    }

    pub fn hash(&self) -> &H {
        &self.hash
    }
}
