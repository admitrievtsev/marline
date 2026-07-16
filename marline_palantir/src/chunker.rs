use crate::types::Chunk;

pub trait Chunker {
    fn chunk(&self, data: &[u8]) -> Vec<Chunk>;
}
