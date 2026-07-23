pub trait PalantirEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8>;
}

pub struct GdeltaEncoder;

impl PalantirEncoder for GdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        marline_scrub::encoder::gdelta_diff(new_chunk, base_chunk)
    }
}
