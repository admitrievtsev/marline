use crate::decoder::Decoder;
use crate::encoder::zdelta_encoder;
use crate::encoder::zdelta_match_pointers::{MatchPointers, ReferencePointerType};
use bit_vec::BitVec;
use huffman_compress::Tree;
use thiserror::Error;

/// Flag indicating a literal byte follows in the delta stream.
const LITERAL_FLAG: u8 = 0x00;
/// Bytes needed for a match instruction: flag, length_remainder, offset_high, offset_low.
const MATCH_INSTRUCTION_SIZE: usize = 4;
/// Minimum length of a match in the zdelta algorithm.
const MIN_MATCH_LENGTH: usize = 3;
/// Maximum length of a match in the zdelta algorithm.
const MAX_MATCH_LENGTH: usize = 1026;
/// Size of length block for match length encoding.
const LENGTH_BLOCK_SIZE: usize = 256;

/// Represents the decoder for zdelta-compressed data, capable of handling both raw and Huffman-encoded streams.
pub struct ZdeltaDecoder {
    huffman_tree: Option<Tree<u8>>,
}

impl ZdeltaDecoder {
    /// Creates a new `ZdeltaDecoder` instance.
    ///
    /// # Arguments
    /// * `use_huffman_encoding` - If true, enables Huffman decoding; otherwise, uses raw data.
    ///
    /// # Returns
    /// A new `ZdeltaDecoder` instance with the specified configuration.
    pub fn new(use_huffman_encoding: bool) -> Self {
        if use_huffman_encoding {
            let (_, huffman_tree) = zdelta_encoder::create_default_huffman_book_and_tree();
            Self {
                huffman_tree: Some(huffman_tree),
            }
        } else {
            Self { huffman_tree: None }
        }
    }

    /// Converts Huffman-encoded data into raw bytes using the Huffman tree.
    ///
    /// # Arguments
    /// * `data` - The Huffman-encoded byte slice.
    ///
    /// # Returns
    /// A vector of raw bytes decoded from the Huffman stream, or the input data if Huffman is disabled.
    ///
    /// # Notes
    /// Assumes the Huffman tree is initialized if Huffman encoding is enabled. Returns the input
    /// data as-is if no tree is present.
    pub fn huffman_to_raw(&self, data: &[u8]) -> Vec<u8> {
        let Some(tree) = &self.huffman_tree else {
            return data.to_vec();
        };

        let bit_buffer = BitVec::from_bytes(data);
        let mut decoder = tree.unbounded_decoder(bit_buffer);
        let mut output = Vec::new();
        let mut bits_processed = 0;

        while let Some(flag) = decoder.next() {
            bits_processed += 1;

            if flag == LITERAL_FLAG {
                if let Some(literal) = decoder.next() {
                    bits_processed += 1;
                    output.push(LITERAL_FLAG);
                    output.push(literal);
                } else {
                    log::warn!("Incomplete literal at bit {bits_processed}");
                    continue;
                }
            } else if (1..=20).contains(&flag) {
                if let (Some(length_remainder), Some(offset_high), Some(offset_low)) =
                    (decoder.next(), decoder.next(), decoder.next())
                {
                    output.push(flag);
                    output.push(length_remainder);
                    output.push(offset_high);
                    output.push(offset_low);
                } else {
                    log::warn!("Incomplete match at bit {bits_processed}");
                    continue;
                }
            } else {
                log::warn!("Unexpected flag {flag} at bit {bits_processed}");
                continue;
            }
        }

        output
    }
}

impl Default for ZdeltaDecoder {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Decoder for ZdeltaDecoder {
    /// Decodes a chunk of delta-encoded data into the original target data.
    ///
    /// # Arguments
    /// * `parent_data` - The reference data used for match instructions.
    /// * `delta_code` - The delta-encoded data containing literals and matches.
    ///
    /// # Returns
    /// A vector of bytes representing the decoded target data.
    ///
    /// # Description
    /// Iterates through the delta-encoded data, processing literals (marked by LITERAL_FLAG)
    /// and matches (marked by flags 1–20).
    /// Errors in match processing are logged and skipped.
    fn decode_chunk(&self, parent_data: Vec<u8>, delta_code: &[u8]) -> Vec<u8> {
        let mut output: Vec<u8> = Vec::new();
        let mut pointers = MatchPointers::new(0, 0, 0);
        let mut previous_offset: Option<i16> = None;

        let data_to_decode = self.huffman_to_raw(delta_code);

        let mut index_in_data_to_decode = 0;
        while index_in_data_to_decode < data_to_decode.len() {
            if data_to_decode[index_in_data_to_decode] == LITERAL_FLAG {
                if index_in_data_to_decode + 1 >= data_to_decode.len() {
                    break;
                }
                output.push(data_to_decode[index_in_data_to_decode + 1]);
                index_in_data_to_decode += 2;
                continue;
            }

            if index_in_data_to_decode + MATCH_INSTRUCTION_SIZE > data_to_decode.len() {
                log::warn!("Incomplete match data at index {index_in_data_to_decode}");
                index_in_data_to_decode += 1;
                continue;
            }

            let flag = data_to_decode[index_in_data_to_decode];
            let length_remainder = data_to_decode[index_in_data_to_decode + 1];
            let offset_high = data_to_decode[index_in_data_to_decode + 2];
            let offset_low = data_to_decode[index_in_data_to_decode + 3];
            index_in_data_to_decode += MATCH_INSTRUCTION_SIZE;

            let (length_coefficient, pointer_type, is_positive) = match decode_flag(flag) {
                Ok(res) => res,
                Err(e) => {
                    log::error!(
                        "Invalid flag {flag} at index {index_in_data_to_decode}, skipping: {e:?}"
                    );
                    index_in_data_to_decode += 1;
                    continue;
                }
            };

            let match_length = MIN_MATCH_LENGTH
                + length_remainder as usize
                + (length_coefficient as usize * LENGTH_BLOCK_SIZE);

            if match_length > MAX_MATCH_LENGTH {
                log::error!("Match length {match_length} exceeds MAX_MATCH_LENGTH at index {index_in_data_to_decode}");
                index_in_data_to_decode += 1;
                continue;
            }

            let offset = ((offset_high as i16) << 8) | offset_low as i16;
            let offset = if is_positive { offset } else { -offset };

            if let Err(e) = process_match(
                match_length,
                offset,
                pointer_type,
                &parent_data,
                &mut pointers,
                &mut output,
                &mut previous_offset,
            ) {
                log::error!("Failed to process match at index {index_in_data_to_decode}: {e:?}");
                index_in_data_to_decode += 1;
                continue;
            }
        }

        output
    }
}

/// Processes a match command in delta encoding.
///
/// # Arguments
/// * `length` - Number of bytes to copy (3..1026).
/// * `offset` - Relative offset from the pointer.
/// * `pointer_type` - Which reference to use (TargetLocal/Main/Auxiliary).
/// * `parent_data` - Reference data for Main/Auxiliary pointers.
/// * `pointers` - Current positions of pointers.
/// * `output` - Output buffer to write decoded data.
/// * `previous_offset` - Track previous offset for pointer strategy.
///
/// # Errors
/// Returns InvalidOffset or InvalidLength if parameters are out of bounds.
fn process_match(
    length: usize,
    offset: i16,
    pointer_type: ReferencePointerType,
    parent_data: &[u8],
    pointers: &mut MatchPointers,
    output: &mut Vec<u8>,
    previous_offset: &mut Option<i16>,
) -> Result<(), DecodeError> {
    let source_position = match pointer_type {
        ReferencePointerType::TargetLocal => {
            if offset > 0 || offset.unsigned_abs() as usize > output.len() {
                return Err(DecodeError::Offset);
            }
            output.len() - offset.unsigned_abs() as usize
        }
        _ => {
            let base_ptr = pointers.get(&pointer_type);
            let position = (base_ptr as isize + offset as isize) as usize;
            if position > parent_data.len() {
                return Err(DecodeError::Offset);
            }
            position
        }
    };

    let end_position = source_position
        .checked_add(length)
        .ok_or(DecodeError::Length)?;

    match pointer_type {
        ReferencePointerType::TargetLocal => {
            if end_position > output.len() {
                return Err(DecodeError::Length);
            }

            let data_to_copy = output[source_position..end_position].to_vec();
            output.extend_from_slice(&data_to_copy);
        }
        _ => {
            if end_position > parent_data.len() {
                return Err(DecodeError::Length);
            }

            output.extend_from_slice(&parent_data[source_position..end_position]);
        }
    }

    pointers.smart_update_after_match(
        source_position + length,
        offset,
        pointer_type,
        *previous_offset,
    );
    *previous_offset = Some(offset);
    Ok(())
}

fn decode_flag(flag: u8) -> Result<(u8, ReferencePointerType, bool), DecodeError> {
    match flag {
        1 => Ok((0, ReferencePointerType::TargetLocal, false)),
        2 => Ok((0, ReferencePointerType::Main, true)),
        3 => Ok((0, ReferencePointerType::Main, false)),
        4 => Ok((0, ReferencePointerType::Auxiliary, true)),
        5 => Ok((0, ReferencePointerType::Auxiliary, false)),
        6 => Ok((1, ReferencePointerType::TargetLocal, false)),
        7 => Ok((1, ReferencePointerType::Main, true)),
        8 => Ok((1, ReferencePointerType::Main, false)),
        9 => Ok((1, ReferencePointerType::Auxiliary, true)),
        10 => Ok((1, ReferencePointerType::Auxiliary, false)),
        11 => Ok((2, ReferencePointerType::TargetLocal, false)),
        12 => Ok((2, ReferencePointerType::Main, true)),
        13 => Ok((2, ReferencePointerType::Main, false)),
        14 => Ok((2, ReferencePointerType::Auxiliary, true)),
        15 => Ok((2, ReferencePointerType::Auxiliary, false)),
        16 => Ok((3, ReferencePointerType::TargetLocal, false)),
        17 => Ok((3, ReferencePointerType::Main, true)),
        18 => Ok((3, ReferencePointerType::Main, false)),
        19 => Ok((3, ReferencePointerType::Auxiliary, true)),
        20 => Ok((3, ReferencePointerType::Auxiliary, false)),
        _ => Err(DecodeError::Flag),
    }
}

/// Error types for zdelta decoding.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("Invalid flag value")]
    Flag,

    #[error("Invalid length value")]
    Length,

    #[error("Invalid offset value")]
    Offset,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::zdelta_encoder::ZdeltaEncoder;
    use crate::encoder::zdelta_match_pointers::ReferencePointerType;
    use crate::encoder::zdelta_match_pointers::ReferencePointerType::{Auxiliary, TargetLocal};
    use bit_vec::BitVec;
    use huffman_compress::CodeBuilder;
    use std::collections::HashMap;

    #[test]
    fn decode_chunk_should_handle_basic_literals() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![], &[0x00, b'X', 0x00, b'Y']);
        assert_eq!(result, vec![b'X', b'Y']);
    }

    #[test]
    fn decode_chunk_should_handle_basic_match() {
        let decoder = ZdeltaDecoder::new(false);
        let parent_data = vec![b'a', b'b', b'c'];
        let delta_code = vec![2, 0, 0, 0];
        let result = decoder.decode_chunk(parent_data, &delta_code);
        assert_eq!(result, vec![b'a', b'b', b'c']);
    }

    #[test]
    fn decode_chunk_should_handle_mixed_literals_and_matches() {
        let decoder = ZdeltaDecoder::new(false);
        let parent_data = vec![b'a', b'b', b'c', b'd'];
        let delta_code = vec![0x00, b'X', 2, 1, 0, 0, 0x00, b'Y'];
        let result = decoder.decode_chunk(parent_data, &delta_code);
        assert_eq!(result, vec![b'X', b'a', b'b', b'c', b'd', b'Y']);
    }

    #[test]
    fn decode_chunk_should_handle_incomplete_literal() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![], &[0x00]);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn decode_chunk_should_handle_incomplete_match() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![b'a'], &[1, 0, 0]);
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn decode_chunk_should_handle_invalid_flag() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![b'a'], &[21, 0, 0, 0]);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn decode_chunk_should_handle_excessive_match_length() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![b'a'], &[16, 255, 0, 0]);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn decode_chunk_should_handle_empty_input() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![], &[]);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn decode_chunk_should_handle_max_length_match() {
        let decoder = ZdeltaDecoder::new(false);
        let parent_data = vec![0; MAX_MATCH_LENGTH];
        let delta_code = vec![17, 255, 0, 0];
        let result = decoder.decode_chunk(parent_data, &delta_code);
        assert_eq!(result.len(), MAX_MATCH_LENGTH);
    }

    #[test]
    fn decode_chunk_should_handle_trailing_literals_after_incomplete_match() {
        let decoder = ZdeltaDecoder::new(false);
        let result = decoder.decode_chunk(vec![b'a'], &[1, 0, 0, 0x00, b'X', 0x00, b'Y']);
        assert_eq!(result, vec![b'Y']);
    }

    #[test]
    fn process_match_should_track_previous_offset_for_pointer_strategy() {
        let mut pointers = MatchPointers::new(0, 0, 0);
        let mut output = Vec::new();
        let parent_data = vec![b'x'; 5000];
        let mut previous_offset = None;

        process_match(
            100,
            100,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut previous_offset,
        )
        .unwrap();
        assert_eq!(pointers.get(&ReferencePointerType::Main), 200);
        assert_eq!(pointers.get(&Auxiliary), 0);

        process_match(
            100,
            50,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut previous_offset,
        )
        .unwrap();
        assert_eq!(pointers.get(&ReferencePointerType::Main), 350);
        assert_eq!(pointers.get(&Auxiliary), 0);

        process_match(
            100,
            2000,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut previous_offset,
        )
        .unwrap();
        assert_eq!(pointers.get(&ReferencePointerType::Main), 350);
        assert_eq!(pointers.get(&Auxiliary), 2450);
    }

    #[test]
    fn process_match_should_copy_from_target_local() {
        let mut pointers = MatchPointers::new(0, 0, 0);
        let mut output = vec![b'a', b'b', b'c'];
        let parent_data = vec![];

        process_match(
            3,
            -3,
            TargetLocal,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();
        assert_eq!(output, vec![b'a', b'b', b'c', b'a', b'b', b'c']);
        assert_eq!(pointers.get(&TargetLocal), 3);

        process_match(
            3,
            -3,
            TargetLocal,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();
        assert_eq!(
            output,
            vec![b'a', b'b', b'c', b'a', b'b', b'c', b'a', b'b', b'c']
        );
        assert_eq!(pointers.get(&TargetLocal), 6);
    }

    #[test]
    fn process_match_should_copy_from_main_reference() {
        let mut pointers = MatchPointers::new(0, 2, 0);
        let mut output = Vec::new();
        let parent_data = vec![b'a', b'b', b'c', b'd', b'e'];

        process_match(
            2,
            1,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();

        assert_eq!(output, vec![b'd', b'e']);
        assert_eq!(pointers.get(&ReferencePointerType::Main), 5);
    }

    #[test]
    fn process_match_should_copy_from_auxiliary_reference() {
        let mut pointers = MatchPointers::new(0, 0, 1);
        let mut output = Vec::new();
        let parent_data = vec![b'a', b'b', b'c', b'd', b'e'];

        process_match(
            2,
            -1,
            Auxiliary,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();

        assert_eq!(output, vec![b'a', b'b']);
        assert_eq!(pointers.get(&Auxiliary), 2);
    }

    #[test]
    fn process_match_should_return_error_for_invalid_target_local_offset() {
        let mut pointers = MatchPointers::default();
        let mut output = vec![b'a', b'b'];
        let parent_data = Vec::new();

        let result = process_match(
            1,
            -3,
            TargetLocal,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        );

        assert!(matches!(result, Err(DecodeError::Offset)));
    }

    #[test]
    fn process_match_should_return_error_for_positive_target_local_offset() {
        let mut pointers = MatchPointers::default();
        let mut output = vec![b'a', b'b'];
        let parent_data = Vec::new();

        let result = process_match(
            1,
            1,
            TargetLocal,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        );

        assert!(matches!(result, Err(DecodeError::Offset)));
    }

    #[test]
    fn process_match_should_return_error_for_invalid_reference_offset() {
        let mut pointers = MatchPointers::new(0, 2, 0);
        let mut output = Vec::new();
        let parent_data = vec![b'a', b'b', b'c'];

        let result = process_match(
            2,
            2,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        );

        assert!(matches!(result, Err(DecodeError::Offset)));
    }

    #[test]
    fn process_match_should_handle_max_length_match() {
        let mut pointers = MatchPointers::new(0, 0, 0);
        let mut output = Vec::new();
        let parent_data = vec![b'x'; MAX_MATCH_LENGTH + 10];

        process_match(
            MAX_MATCH_LENGTH,
            0,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();

        assert_eq!(output.len(), MAX_MATCH_LENGTH);
        assert_eq!(pointers.get(&ReferencePointerType::Main), MAX_MATCH_LENGTH);
    }

    #[test]
    fn process_match_should_handle_zero_offset() {
        let mut pointers = MatchPointers::new(0, 2, 0);
        let mut output = Vec::new();
        let parent_data = vec![b'a', b'b', b'c', b'd', b'e'];

        process_match(
            2,
            0,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();

        assert_eq!(output, vec![b'c', b'd']);
        assert_eq!(pointers.get(&ReferencePointerType::Main), 4);
    }

    #[test]
    fn process_match_should_handle_consecutive_matches() {
        let mut pointers = MatchPointers::new(0, 0, 0);
        let mut output = Vec::new();
        let parent_data = vec![b'a', b'b', b'c', b'd', b'e', b'f'];

        process_match(
            2,
            0,
            ReferencePointerType::Main,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();

        process_match(
            2,
            -2,
            TargetLocal,
            &parent_data,
            &mut pointers,
            &mut output,
            &mut None,
        )
        .unwrap();

        assert_eq!(output, vec![b'a', b'b', b'a', b'b']);
        assert_eq!(pointers.get(&TargetLocal), 2);
    }

    #[test]
    fn huffman_to_raw_should_decode_single_match() {
        let decoder = create_test_decoder();

        let mut buffer = BitVec::new();
        buffer.extend(BitVec::from_bytes(&[2, 7, 0, 100]));

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(decoded, vec![2, 7, 0, 100]);
    }

    #[test]
    fn huffman_to_raw_should_decode_multiple_matches() {
        let decoder = create_test_decoder();

        let input = vec![2, 7, 0, 100, 10, 41, 4, 0];

        let mut buffer = BitVec::new();
        buffer.extend(BitVec::from_bytes(&input));

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(decoded, input);
    }

    #[test]
    fn huffman_to_raw_should_handle_empty_input() {
        let decoder = create_test_decoder();
        let decoded = decoder.huffman_to_raw(&[]);
        assert_eq!(decoded, vec![]);
    }

    #[test]
    fn huffman_to_raw_should_return_raw_data_when_huffman_disabled() {
        let decoder = ZdeltaDecoder::new(false);
        let data = vec![1, 2, 3, 4];
        let decoded = decoder.huffman_to_raw(&data);
        assert_eq!(decoded, data);
    }

    #[test]
    fn huffman_to_raw_should_handle_incomplete_last_match() {
        let decoder = create_test_decoder();

        let input = vec![2, 7, 0, 100, 10, 41, 4];

        let mut buffer = BitVec::new();
        buffer.extend(BitVec::from_bytes(&input));

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(decoded, vec![2, 7, 0, 100]);
    }

    #[test]
    fn huffman_to_raw_should_decode_max_values() {
        let decoder = create_test_decoder();

        let input = vec![16, 255, 127, 255, 20, 255, 127, 254];

        let mut buffer = BitVec::new();
        buffer.extend(BitVec::from_bytes(&input));

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(decoded, input);
    }

    #[test]
    fn huffman_to_raw_should_preserve_byte_order() {
        let decoder = create_test_decoder();

        let input = vec![2, 10, 0x12, 0x34];

        let mut buffer = BitVec::new();
        buffer.extend(BitVec::from_bytes(&input));

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(decoded[2], 0x12);
        assert_eq!(decoded[3], 0x34);
    }

    #[test]
    fn huffman_to_raw_should_handle_all_pointer_types() {
        let decoder = create_test_decoder();

        let input = vec![
            1, 10, 0, 100, // TargetLocal
            2, 20, 1, 200, // Main, positive
            3, 30, 2, 100, // Main, negative
            4, 40, 3, 200, // Auxiliary, positive
            5, 50, 4, 100, // Auxiliary, negative
        ];

        let mut buffer = BitVec::new();
        buffer.extend(BitVec::from_bytes(&input));

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(decoded, input);
    }

    #[test]
    fn huffman_to_raw_should_decode_huffman_encoded_data() {
        let decoder = ZdeltaDecoder::new(true);

        let test_cases = vec![
            vec![2, 7, 0, 100],      // length=10, offset=100
            vec![10, 41, 4, 0],      // length=300, offset=-1024
            vec![16, 255, 127, 255], // length=1026, offset=32767
        ];

        let mut full_bitvec = BitVec::new();
        for case in &test_cases {
            let mut buffer = BitVec::new();
            let (huffman_book, _) = zdelta_encoder::create_default_huffman_book_and_tree();
            huffman_book.encode(&mut buffer, &case[0]).unwrap();
            huffman_book.encode(&mut buffer, &case[1]).unwrap();
            huffman_book.encode(&mut buffer, &case[2]).unwrap();
            huffman_book.encode(&mut buffer, &case[3]).unwrap();
            full_bitvec.extend(buffer);
        }

        let encoded_data = full_bitvec.to_bytes();

        let decoded = decoder.huffman_to_raw(&encoded_data);

        let expected_raw: Vec<u8> = test_cases.iter().flatten().cloned().collect();
        assert_eq!(decoded, expected_raw);
    }

    #[test]
    fn huffman_to_raw_should_handle_invalid_huffman_data_gracefully() {
        let decoder = ZdeltaDecoder::new(true);

        let invalid_data = vec![0xFF, 0xFF, 0xFF];
        let result = decoder.huffman_to_raw(&invalid_data);

        assert_ne!(result, invalid_data);
        assert!(result.is_empty());
    }

    #[test]
    fn huffman_to_raw_should_decode_single_literal_correctly() {
        let decoder = ZdeltaDecoder::new(true);
        let encoder = ZdeltaEncoder::new(true);
        let mut buffer = BitVec::new();

        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &LITERAL_FLAG)
            .expect("Literal flag must be in codebook");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &b'A')
            .expect("All literals (0-255) must be in codebook");

        let encoded = buffer.to_bytes();

        let decoded = decoder.huffman_to_raw(&encoded);
        assert_eq!(decoded, vec![LITERAL_FLAG, b'A']);
    }

    #[test]
    fn huffman_to_raw_should_handle_mixed_literals_and_matches() {
        let decoder = ZdeltaDecoder::new(true);
        let encoder = ZdeltaEncoder::new(true);
        let mut buffer = BitVec::new();

        // Literal 'A'
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &LITERAL_FLAG)
            .expect("Literal flag must be in codebook");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &b'A')
            .expect("Literal must be in codebook");

        // Match
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &2)
            .expect("Flag must be in codebook");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &10)
            .expect("Length remainder must be in codebook");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &0)
            .expect("Offset high must be in codebook");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &100)
            .expect("Offset low must be in codebook");

        // Literal 'B'
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &LITERAL_FLAG)
            .expect("Literal flag must be in codebook");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &b'B')
            .expect("Literal must be in codebook");

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert_eq!(
            decoded,
            vec![LITERAL_FLAG, b'A', 2, 10, 0, 100, LITERAL_FLAG, b'B']
        );
    }

    #[test]
    fn huffman_to_raw_should_ignore_unknown_markers() {
        let decoder = ZdeltaDecoder::new(true);
        let encoder = ZdeltaEncoder::new(true);
        let mut buffer = BitVec::new();

        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &21)
            .expect("Should encode invalid flag");
        encoder
            .huffman_book()
            .as_ref()
            .unwrap()
            .encode(&mut buffer, &65)
            .expect("Should encode byte");

        let encoded = buffer.to_bytes();
        let decoded = decoder.huffman_to_raw(&encoded);

        assert!(decoded.is_empty());
    }

    fn create_test_decoder() -> ZdeltaDecoder {
        let mut frequencies = HashMap::new();
        for i in 0..=255 {
            frequencies.insert(i, 1);
        }
        let (_, tree) = CodeBuilder::from_iter(frequencies).finish();

        ZdeltaDecoder {
            huffman_tree: Some(tree),
        }
    }
}
