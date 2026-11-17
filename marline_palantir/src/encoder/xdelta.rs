use marline_scrub::decoder::{Decoder, GdeltaDecoder};
use marline_scrub::encoder::XdeltaEncoder as ScrubXdeltaEncoder;

use super::Encoder;

#[derive(Clone, Copy, Debug, Default)]
pub struct XdeltaEncoder;

impl Encoder for XdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        ScrubXdeltaEncoder::default().encode_data(new_chunk, base_chunk)
    }

    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        _manifest: &std::collections::HashMap<u64, u32>,
    ) -> Vec<u8> {
        self.encode(new_chunk, base_chunk)
    }

    fn name(&self) -> &'static str {
        "xdelta"
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        GdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }
}
