use super::Encoder;
use marline_scrub::decoder::{Decoder, ZdeltaDecoder};
use marline_scrub::encoder::zdelta_encoder::ZdeltaEncoder as ScrubZdeltaEncoder;

#[derive(Clone, Copy, Debug, Default)]
pub struct ZdeltaEncoder;

impl Encoder for ZdeltaEncoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        ScrubZdeltaEncoder::new(false).encode_data(new_chunk, base_chunk)
    }

    fn name(&self) -> &'static str {
        "zdelta"
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        ZdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }
}
