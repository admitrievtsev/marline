use crate::types::{Chunk, ChunkHash};

pub trait Hasher {
    fn hash(&self, chunk: &Chunk) -> ChunkHash;
}
