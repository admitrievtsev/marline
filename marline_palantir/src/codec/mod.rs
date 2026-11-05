use marline_scrub::decoder::{Decoder, GdeltaDecoder, ZdeltaDecoder};
use marline_scrub::encoder::{ddelta_diff, gdelta_diff, xdelta_diff, zdelta_diff};
use marline_sketcher::AronovichHash;

pub trait PalantirCodec {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8>;
    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8>;
    fn name(&self) -> &'static str;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GdeltaCodec;

#[derive(Clone, Copy, Debug, Default)]
pub struct XdeltaCodec;

#[derive(Clone, Copy, Debug, Default)]
pub struct DdeltaCodec;

#[derive(Clone, Copy, Debug, Default)]
pub struct ZdeltaCodec;

#[derive(Clone, Copy, Debug)]
pub enum AnyCodec {
    Gdelta(GdeltaCodec),
    Xdelta(XdeltaCodec),
    Ddelta(DdeltaCodec),
    Zdelta(ZdeltaCodec),
}

fn insert_instruction(data: &[u8]) -> Vec<u8> {
    let mut len = (data.len() as u32).to_ne_bytes();
    len[2] |= 1 << 7;
    let mut delta = Vec::with_capacity(3 + data.len());
    delta.extend_from_slice(&len[..3]);
    delta.extend_from_slice(data);
    delta
}

impl PalantirCodec for GdeltaCodec {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        if new_chunk.len() < 16 || base_chunk.len() < 16 {
            insert_instruction(new_chunk)
        } else {
            gdelta_diff(new_chunk, base_chunk)
        }
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        GdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }

    fn name(&self) -> &'static str {
        "gdelta"
    }
}

impl PalantirCodec for XdeltaCodec {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        xdelta_diff(new_chunk, base_chunk)
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        GdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }

    fn name(&self) -> &'static str {
        "xdelta"
    }
}

impl PalantirCodec for DdeltaCodec {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        ddelta_diff(new_chunk, base_chunk)
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        GdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }

    fn name(&self) -> &'static str {
        "ddelta"
    }
}

impl PalantirCodec for ZdeltaCodec {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        zdelta_diff::<AronovichHash>(new_chunk, base_chunk)
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        ZdeltaDecoder::new(false).decode_chunk(base_chunk.to_vec(), delta)
    }

    fn name(&self) -> &'static str {
        "zdelta"
    }
}

impl PalantirCodec for AnyCodec {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        match self {
            Self::Gdelta(codec) => codec.encode(new_chunk, base_chunk),
            Self::Xdelta(codec) => codec.encode(new_chunk, base_chunk),
            Self::Ddelta(codec) => codec.encode(new_chunk, base_chunk),
            Self::Zdelta(codec) => codec.encode(new_chunk, base_chunk),
        }
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        match self {
            Self::Gdelta(codec) => codec.decode(base_chunk, delta),
            Self::Xdelta(codec) => codec.decode(base_chunk, delta),
            Self::Ddelta(codec) => codec.decode(base_chunk, delta),
            Self::Zdelta(codec) => codec.decode(base_chunk, delta),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Gdelta(codec) => codec.name(),
            Self::Xdelta(codec) => codec.name(),
            Self::Ddelta(codec) => codec.name(),
            Self::Zdelta(codec) => codec.name(),
        }
    }
}
