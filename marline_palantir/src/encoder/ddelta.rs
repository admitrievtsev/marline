use marline_scrub::decoder::{Decoder, GdeltaDecoder};
use marline_scrub::encoder::DdeltaEncoder as ScrubDdeltaEncoder;

use super::Encoder;

#[derive(Clone, Copy, Debug, Default)]
pub struct DdeltaEncoder;

impl Encoder for DdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        ScrubDdeltaEncoder::default().encode_data(new_chunk, base_chunk)
    }

    fn name(&self) -> &'static str {
        "ddelta"
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        GdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }
}
