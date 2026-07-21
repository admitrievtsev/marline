pub mod gdelta;

pub trait PalantirEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8>;
}
