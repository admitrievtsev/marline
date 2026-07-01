use crate::decoder::Decoder;

/// Decoder based on Gdelta compression algorithm.
pub struct GdeltaDecoder {
    zstd_flag: bool,
}

impl GdeltaDecoder {
    pub fn new(zstd_flag: bool) -> Self {
        GdeltaDecoder { zstd_flag }
    }
}

impl Default for GdeltaDecoder {
    fn default() -> Self {
        Self::new(false)
    }
}

/// The method is based on copy and paste constructions.
/// The insert contains a handler of 3 bytes with the length of the insert and the data to insert.
/// To copy a handler of 6 bytes with a length and a shift in the parent chunk.
impl Decoder for GdeltaDecoder {
    fn decode_chunk(&self, parent_data: Vec<u8>, delta_code: &[u8]) -> Vec<u8> {
        let delta_code = if self.zstd_flag {
            zstd::decode_all(delta_code).unwrap()
        } else {
            delta_code.to_vec()
        };

        let mut chunk_data = Vec::new();
        let mut byte_id = 0;

        while byte_id < delta_code.len() {
            let mut buf = [0u8; 8];
            buf[..3].copy_from_slice(&delta_code[byte_id..byte_id + 3]);

            if buf[2] >= 128 {
                buf[2] -= 128;
                let insert_len = usize::from_ne_bytes(buf);
                chunk_data.extend_from_slice(&delta_code[byte_id + 3..byte_id + 3 + insert_len]);
                byte_id += 3 + insert_len
            } else {
                let copy_len = usize::from_ne_bytes(buf);
                buf[..3].copy_from_slice(&delta_code[byte_id + 3..byte_id + 6]);
                let copy_offset = usize::from_ne_bytes(buf);
                chunk_data.extend_from_slice(&parent_data[copy_offset..copy_offset + copy_len]);
                byte_id += 6
            }
        }
        chunk_data
    }
}
