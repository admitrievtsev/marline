// use crate::chunker::Chunker;
// use crate::error::PalantirError;
// use crate::hasher::Hasher;
// use crate::types::ChunkHash;
//
// #[derive(Debug, Clone)]
// pub struct Pipeline<C, H> {
//     chunker: C,
//     hasher: H,
// }
//
// impl<C: Chunker, H: Hasher> Pipeline<C, H> {
//     pub fn new(chunker: C, hasher: H) -> Self {
//         Self { chunker, hasher }
//     }
//     pub fn process_stream(&mut self, _data: &[u8]) -> Result<Vec<ChunkHash>, PalantirError> { todo!() }
// }