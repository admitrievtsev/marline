//! Content-defined chunking (FastCDC v2020) with in-pass feature collection.
//!
//! [`CdcConfig`] is the entry point: it validates chunk size parameters and
//! builds slice ([`FastCDC`]) and streaming ([`StreamCDC`]) chunkers.

// The module stays `pub` only because the doc-tests of the vendored `fastcdc.rs`
// refer to the `marline_cdc::fastcdc` path; the supported API is the re-exports below.
#[doc(hidden)]
pub mod fastcdc;

pub use fastcdc::{
    collect_raw_features, Chunk, ChunkData, Error as StreamError, FastCDC, StreamCDC,
};

use marline_utils::{KB, MB};

use fastcdc::{AVERAGE_MAX, AVERAGE_MIN, MAXIMUM_MAX, MAXIMUM_MIN, MINIMUM_MAX, MINIMUM_MIN};

/// Error returned when chunk size parameters are rejected.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Sizes are outside the supported ranges or are not ordered as `min <= avg <= max`.
    #[error("invalid chunk sizes: min={min}, avg={avg}, max={max}")]
    InvalidSizes {
        /// Requested minimum chunk size.
        min: usize,
        /// Requested average chunk size.
        avg: usize,
        /// Requested maximum chunk size.
        max: usize,
    },
}

/// Validated chunk size parameters for content-defined chunking.
#[derive(Debug, Clone, Copy)]
pub struct CdcConfig {
    min_size: usize,
    avg_size: usize,
    max_size: usize,
}

/// Default sizes: 512 KB minimum, 1 MB average and 2 MB maximum chunk size.
impl Default for CdcConfig {
    fn default() -> Self {
        Self { min_size: 512 * KB, avg_size: MB, max_size: 2 * MB }
    }
}

impl CdcConfig {
    /// Creates a configuration from minimum, average and maximum chunk sizes in bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InvalidSizes`] if any size is outside its supported range
    /// or the sizes are not ordered as `min_size <= avg_size <= max_size`.
    pub fn new(min_size: usize, avg_size: usize, max_size: usize) -> Result<Self, ConfigError> {
        let valid = (MINIMUM_MIN..=MINIMUM_MAX).contains(&min_size)
            && (AVERAGE_MIN..=AVERAGE_MAX).contains(&avg_size)
            && (MAXIMUM_MIN..=MAXIMUM_MAX).contains(&max_size)
            && min_size <= avg_size
            && avg_size <= max_size;
        if !valid {
            return Err(ConfigError::InvalidSizes { min: min_size, avg: avg_size, max: max_size });
        }
        Ok(Self { min_size, avg_size, max_size })
    }

    /// Minimum chunk size; also the start of the feature-collection window inside each chunk.
    pub fn min_size(&self) -> usize {
        self.min_size
    }

    /// Splits an in-memory buffer into content-defined chunks.
    ///
    /// The iterator yields each chunk together with its raw features. Every chunk
    /// except the last one is at least `min_size` bytes; the last one may be shorter.
    /// An empty input yields no chunks.
    pub fn chunker<'a>(&self, data: &'a [u8]) -> FastCDC<'a> {
        FastCDC::new(data, self.min_size, self.avg_size, self.max_size)
    }

    /// Splits a byte stream into content-defined chunks.
    ///
    /// Produces the same chunks and features as [`Self::chunker`] on the same bytes.
    /// Every chunk except the last one is at least `min_size` bytes; the last one may
    /// be shorter. An empty stream yields no chunks. Read failures are reported as
    /// [`StreamError::IoError`].
    pub fn stream_chunker<R: std::io::Read>(&self, reader: R) -> StreamCDC<R> {
        StreamCDC::new(reader, self.min_size, self.avg_size, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Read};

    /// A deterministic pseudo-random data generator based on the Xorshift64 algorithm (Marsaglia, 2003).
    ///
    /// Used for reproducible CDC (Content-Defined Chunking) testing, without external dependencies such as the `rand` crate.
    /// It generates a byte stream with a period of 2^64 - 1 and a uniform distribution;
    /// this prevents the GEAR rolling hash from degenerating on periodic patterns
    /// and ensures realistic chunk boundary detection.
    fn generate_test_data(size: usize) -> Vec<u8> {
        let mut state: u64 = 0xDEAD_BEEF_CAFE_BABE;
        (0..size)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state & 0xFF) as u8
            })
            .collect()
    }

    /// Checks that `chunks` tile `[0, len)` contiguously and respect the size bounds
    /// (every chunk except the last is in `[min, max]`, the last is `<= max`).
    fn assert_chunk_layout(chunks: &[(Chunk, [u32; 12])], len: usize, min: usize, max: usize) {
        let mut total_bytes = 0;
        let last = chunks.len().saturating_sub(1);
        for (i, (chunk, _features)) in chunks.iter().enumerate() {
            assert_eq!(chunk.offset, total_bytes, "Chunk offset sequence is broken at chunk {i}");
            total_bytes += chunk.length;
            if i < last {
                assert!(
                    chunk.length >= min,
                    "Chunk {i} length {} is smaller than min_size ({min})",
                    chunk.length
                );
            }
            assert!(
                chunk.length <= max,
                "Chunk {i} length {} exceeds max_size ({max})",
                chunk.length
            );
        }
        assert_eq!(total_bytes, len, "Chunks must cover the whole input");
    }

    /// Checks that streaming chunks are element-wise identical to slice chunks of `data`.
    fn assert_stream_matches_slice(
        data: &[u8],
        slice_chunks: &[(Chunk, [u32; 12])],
        stream_chunks: &[(ChunkData, [u32; 12])],
    ) {
        assert_eq!(
            slice_chunks.len(),
            stream_chunks.len(),
            "Chunk count must match between slice and stream"
        );

        for (i, ((slice_chunk, slice_feat), (stream_chunk, stream_feat))) in
            slice_chunks.iter().zip(stream_chunks).enumerate()
        {
            let offset = slice_chunk.offset;
            let length = slice_chunk.length;

            assert_eq!(offset, stream_chunk.offset as usize, "Offset mismatch at chunk {i}");
            assert_eq!(length, stream_chunk.length, "Length mismatch at chunk {i}");
            assert_eq!(slice_chunk.hash, stream_chunk.hash, "Hash mismatch at chunk {i}");
            assert!(slice_feat == stream_feat, "Extracted features mismatch at offset {offset}");
            assert!(
                data[offset..offset + length] == stream_chunk.data[..],
                "Stream chunk payload does not match original slice bytes at offset {offset}"
            );
        }
    }

    /// Reader that returns between 1 and 7 bytes per `read`, with portion sizes
    /// drawn from a xorshift64 sequence.
    struct ShortReader<'a> {
        data: &'a [u8],
        pos: usize,
        state: u64,
    }

    impl Read for ShortReader<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let remaining = &self.data[self.pos..];
            if remaining.is_empty() || buf.is_empty() {
                return Ok(0);
            }
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            let portion = 1 + (self.state % 7) as usize;
            let n = portion.min(buf.len()).min(remaining.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.pos += n;
            Ok(n)
        }
    }

    /// Reader that returns its data once and fails on every subsequent `read`.
    struct FailingReader {
        data: Vec<u8>,
        pos: usize,
    }

    impl Read for FailingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let remaining = &self.data[self.pos..];
            if remaining.is_empty() {
                return Err(io::Error::other("simulated read failure"));
            }
            let n = remaining.len().min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn test_new_rejects_invalid_sizes() {
        let cases = [
            (0, 256, 1024),
            (1, 256, 1024),
            (MINIMUM_MIN - 1, 256, 1024),
            (MINIMUM_MAX + 1, AVERAGE_MAX, MAXIMUM_MAX),
            (256, 1024, 0),
            (64, AVERAGE_MAX + 1, MAXIMUM_MAX),
            (64, AVERAGE_MIN - 1, 1024),
            (64, 256, MAXIMUM_MIN - 1),
            (64, 256, MAXIMUM_MAX + 1),
            (4096, 1024, 8192),
            (256, 4096, 2048),
        ];
        for (min, avg, max) in cases {
            assert!(
                matches!(CdcConfig::new(min, avg, max), Err(ConfigError::InvalidSizes { .. })),
                "Sizes (min={min}, avg={avg}, max={max}) must be rejected"
            );
        }
    }

    #[test]
    fn test_new_accepts_range_bounds() {
        assert!(CdcConfig::new(MINIMUM_MAX, AVERAGE_MAX, MAXIMUM_MAX).is_ok());

        let config =
            CdcConfig::new(MINIMUM_MIN, AVERAGE_MIN, MAXIMUM_MIN).expect("lower bounds are valid");
        let data = generate_test_data(64 * KB);
        let chunks: Vec<_> = config.chunker(&data).collect();
        assert_chunk_layout(&chunks, data.len(), MINIMUM_MIN, MAXIMUM_MIN);
    }

    #[test]
    fn test_default_is_valid() {
        let d = CdcConfig::default();
        assert!(CdcConfig::new(d.min_size, d.avg_size, d.max_size).is_ok());
    }

    #[test]
    fn test_default_chunker_invariants() {
        let config = CdcConfig::default();
        let data = generate_test_data(6 * MB);
        let chunks: Vec<_> = config.chunker(&data).collect();

        assert!(chunks.len() >= 2, "6 MB input must produce several chunks, got {}", chunks.len());
        assert_chunk_layout(&chunks, data.len(), config.min_size, config.max_size);
    }

    #[test]
    fn test_chunker_empty() {
        let config = CdcConfig::new(256, 1024, 4096).expect("valid config");
        let data: &[u8] = &[];
        let chunks: Vec<_> = config.chunker(data).collect();

        assert!(chunks.is_empty(), "Empty input should not yield any chunks");
    }

    #[test]
    fn test_stream_chunker_empty() {
        let config = CdcConfig::new(256, 1024, 4096).expect("valid config");
        let reader = Cursor::new(Vec::<u8>::new());
        let chunks: Vec<_> = config
            .stream_chunker(reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("Reading an empty stream should succeed");

        assert!(chunks.is_empty(), "Empty stream should produce no chunks");
    }

    #[test]
    fn test_chunker_data_smaller_than_min_size() {
        let config = CdcConfig::new(256, 1024, 4096).expect("valid config");
        let data = generate_test_data(128); // Smaller than min_size (256)
        let chunks: Vec<_> = config.chunker(&data).collect();

        assert_eq!(chunks.len(), 1, "Exactly one trailing chunk should be produced");

        let (chunk, _features) = &chunks[0];
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.length, 128);
    }

    #[test]
    fn test_chunker_invariants() {
        let (min_size, avg_size, max_size) = (256, 1024, 4096);
        let config = CdcConfig::new(min_size, avg_size, max_size).expect("valid config");

        let data = generate_test_data(64 * KB);
        let chunks: Vec<_> = config.chunker(&data).collect();

        assert!(!chunks.is_empty());
        assert_chunk_layout(&chunks, data.len(), min_size, max_size);
    }

    #[test]
    fn test_uniform_data_hits_max_size() {
        let max_size = 4096;
        let config = CdcConfig::new(256, 1024, max_size).expect("valid config");

        // All-zero input is a deliberate pathological case: the rolling hash never
        // finds a content-defined cut point, so every cut is forced at max_size.
        let data = vec![0u8; 20 * KB];
        let chunks: Vec<_> = config.chunker(&data).collect();

        assert_eq!(chunks.len(), 5);
        for (i, (chunk, _)) in chunks.iter().enumerate() {
            assert_eq!(chunk.offset, i * max_size, "Unexpected offset of chunk {i}");
            assert_eq!(chunk.length, max_size, "Chunk {i} must be cut exactly at max_size");
        }
    }

    #[test]
    fn test_stream_chunker_matches_chunker() {
        let config = CdcConfig::new(256, 1024, 4096).expect("valid config");
        let data = generate_test_data(32 * KB);

        let slice_chunks: Vec<_> = config.chunker(&data).collect();
        let stream_chunks: Vec<_> = config
            .stream_chunker(Cursor::new(&data))
            .collect::<Result<Vec<_>, _>>()
            .expect("Reading from Cursor should not fail");

        assert_stream_matches_slice(&data, &slice_chunks, &stream_chunks);
    }

    #[test]
    fn test_stream_chunker_short_reads_match_chunker() {
        let config = CdcConfig::new(256, 1024, 4096).expect("valid config");
        let data = generate_test_data(32 * KB);

        let slice_chunks: Vec<_> = config.chunker(&data).collect();
        let reader = ShortReader { data: &data, pos: 0, state: 0x0123_4567_89AB_CDEF };
        let stream_chunks: Vec<_> = config
            .stream_chunker(reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("Short reads should not fail");

        assert_stream_matches_slice(&data, &slice_chunks, &stream_chunks);
    }

    #[test]
    fn test_stream_chunker_propagates_read_error() {
        let config = CdcConfig::new(256, 1024, 4096).expect("valid config");
        let reader = FailingReader { data: generate_test_data(1000), pos: 0 };

        let first = config.stream_chunker(reader).next();
        assert!(
            matches!(first, Some(Err(StreamError::IoError(_)))),
            "Read failure must surface as StreamError::IoError, got {first:?}"
        );
    }
}
