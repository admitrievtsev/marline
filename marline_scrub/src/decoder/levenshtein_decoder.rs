use crate::decoder::Decoder;
use crate::encoder::Action;

/// Decoder based on Levenshtein compression algorithm.
pub struct LevenshteinDecoder {
    zstd_flag: bool,
}

impl Default for LevenshteinDecoder {
    fn default() -> Self {
        Self::new(false)
    }
}
impl LevenshteinDecoder {
    pub fn new(zstd_flag: bool) -> Self {
        LevenshteinDecoder { zstd_flag }
    }
}

impl Decoder for LevenshteinDecoder {
    /// Decodes a chunk by applying delta actions to the given parent data.
    ///
    /// # Arguments
    ///
    /// * `parent_data` - The original chunk data to be modified.
    /// * `delta_code` - A byte slice encoding the delta actions to apply.
    ///
    /// # Returns
    ///
    /// A new `Vec<u8>` containing the fully decoded chunk.
    fn decode_chunk(&self, mut parent_data: Vec<u8>, delta_code: &[u8]) -> Vec<u8> {
        let delta_code = if self.zstd_flag {
            zstd::decode_all(delta_code).unwrap()
        } else {
            delta_code.to_vec()
        };

        let mut buf = [0u8; 4];
        let mut byte_index = 0;

        while byte_index < delta_code.len() {
            // Read next 4 bytes as a big-endian u32 delta action code
            buf.copy_from_slice(&delta_code[byte_index..byte_index + 4]);
            let delta_action = u32::from_be_bytes(buf);

            // Decode the delta action into operation, index, and byte value
            let (action, index, byte_value) = get_delta_action(delta_action);

            // Apply the delta action to the parent data
            match action {
                Action::Del => {
                    parent_data.remove(index);
                }
                Action::Add => {
                    parent_data.insert(index, byte_value);
                }
                Action::Rep => {
                    parent_data[index] = byte_value;
                }
            }
            byte_index += 4;
        }
        parent_data
    }
}

/// Decodes a delta action packed into a 32-bit integer.
///
/// This function extracts three components from a packed `u32` value:
/// 1. The delta operation type ([`Action`])
/// 2. The byte index in the chunk where the operation applies
/// 3. The byte value (for `Rep` and `Add` operations)
///
/// # Bit Layout
/// The 32-bit value is divided as follows:
/// - Bits 30-31 (2 bits): Action type
/// - Bits 22-29 (8 bits): Byte value
/// - Bits 0-21 (22 bits): Index in chunk
///
/// ```text
///  3                   2                   1                   0
///  1 0 9 8 7 6 5 4 3 2 1 0 9 8 7 6 5 4 3 2 1 0 9 8 7 6 5 4 3 2 1 0
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |   Action    |      Byte Value     |          Index            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
///
/// # Arguments
/// * `code` - Packed 32-bit value containing action metadata
///
/// # Returns
/// A tuple containing:
/// - [`Action`] variant (operation type)
/// - `usize` index in the chunk
/// - `u8` byte value (for replacement/insertion)
pub(crate) fn get_delta_action(code: u32) -> (Action, usize, u8) {
    let action = match code / (1 << 30) {
        0 => Action::Rep,
        1 => Action::Add,
        2 => Action::Del,
        _ => panic!("Invalid action code in delta encoding"),
    };
    let byte_value = ((code % (1 << 30)) >> 22) as u8;
    let index = (code % (1 << 22)) as usize;
    (action, index, byte_value)
}
