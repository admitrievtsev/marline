//! Content-defined chunking (FastCDC v2020) with in-pass feature collection.

#[doc(hidden)]
pub mod fastcdc;

pub use fastcdc::{
    collect_raw_features, Chunk, ChunkData, Error as StreamError, FastCDC, StreamCDC,
};

#[cfg(test)]
mod tests {
    use super::*;
    use fastcdc::{AVERAGE_MIN, MAXIMUM_MIN, MINIMUM_MIN};
    use marline_utils::{KB, MB};
    use std::io::{self, Cursor, Read};

    // Production-scale chunk sizes: 512 KB minimum, 1 MB average, 2 MB maximum.
    const DEFAULT_MIN: usize = 512 * KB;
    const DEFAULT_AVG: usize = MB;
    const DEFAULT_MAX: usize = 2 * MB;

    /// Advances a xorshift64 state (Marsaglia, 2003) and returns the new state.
    fn xorshift64(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// A deterministic pseudo-random data generator based on [`xorshift64`].
    ///
    /// Used for reproducible CDC (Content-Defined Chunking) testing, without external dependencies such as the `rand` crate.
    /// It generates a byte stream with a period of 2^64 - 1 and a uniform distribution;
    /// this prevents the GEAR rolling hash from degenerating on periodic patterns
    /// and ensures realistic chunk boundary detection.
    fn generate_test_data(size: usize) -> Vec<u8> {
        let mut state: u64 = 0xDEAD_BEEF_CAFE_BABE;
        (0..size).map(|_| (xorshift64(&mut state) & 0xFF) as u8).collect()
    }

    /// Number of raw features collected per chunk.
    const FEATURE_COUNT: usize = 12;

    /// Checks that `chunks` tile `[0, len)` contiguously and respect the size bounds
    /// (every chunk except the last is in `[min, max]`, the last is `<= max`).
    fn assert_chunk_layout(
        chunks: &[(Chunk, [u32; FEATURE_COUNT])],
        len: usize,
        min: usize,
        max: usize,
    ) {
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
        slice_chunks: &[(Chunk, [u32; FEATURE_COUNT])],
        stream_chunks: &[(ChunkData, [u32; FEATURE_COUNT])],
    ) {
        assert_eq!(
            slice_chunks.len(),
            stream_chunks.len(),
            "Chunk count must match between slice and stream"
        );

        for ((slice_chunk, slice_feat), (stream_chunk, stream_feat)) in
            slice_chunks.iter().zip(stream_chunks)
        {
            let offset = slice_chunk.offset;
            let length = slice_chunk.length;

            assert_eq!(
                offset, stream_chunk.offset as usize,
                "Offset mismatch for slice chunk at offset {offset}"
            );
            assert_eq!(length, stream_chunk.length, "Length mismatch at offset {offset}");
            assert_eq!(slice_chunk.hash, stream_chunk.hash, "Hash mismatch at offset {offset}");
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
            let portion = 1 + (xorshift64(&mut self.state) % 7) as usize;
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
    fn test_chunker_at_lower_size_bounds() {
        let data = generate_test_data(64 * KB);
        let chunks: Vec<_> = FastCDC::new(&data, MINIMUM_MIN, AVERAGE_MIN, MAXIMUM_MIN).collect();
        assert_chunk_layout(&chunks, data.len(), MINIMUM_MIN, MAXIMUM_MIN);
    }

    #[test]
    fn test_chunker_invariants_at_default_sizes() {
        let data = generate_test_data(6 * MB);
        let chunks: Vec<_> = FastCDC::new(&data, DEFAULT_MIN, DEFAULT_AVG, DEFAULT_MAX).collect();

        assert!(chunks.len() >= 2, "6 MB input must produce several chunks, got {}", chunks.len());
        assert_chunk_layout(&chunks, data.len(), DEFAULT_MIN, DEFAULT_MAX);
    }

    #[test]
    fn test_chunker_empty() {
        let data: &[u8] = &[];
        let chunks: Vec<_> = FastCDC::new(data, 256, 1024, 4096).collect();

        assert!(chunks.is_empty(), "Empty input should not yield any chunks");
    }

    #[test]
    fn test_stream_chunker_empty() {
        let reader = Cursor::new(Vec::<u8>::new());
        let chunks: Vec<_> = StreamCDC::new(reader, 256, 1024, 4096)
            .collect::<Result<Vec<_>, _>>()
            .expect("Reading an empty stream should succeed");

        assert!(chunks.is_empty(), "Empty stream should produce no chunks");
    }

    #[test]
    fn test_chunker_data_smaller_than_min_size() {
        let data = generate_test_data(128); // Smaller than min_size (256)
        let chunks: Vec<_> = FastCDC::new(&data, 256, 1024, 4096).collect();

        assert_eq!(chunks.len(), 1, "Exactly one trailing chunk should be produced");

        let (chunk, _features) = &chunks[0];
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.length, 128);
    }

    #[test]
    fn test_chunker_invariants() {
        let (min_size, avg_size, max_size) = (256, 1024, 4096);
        let data = generate_test_data(64 * KB);
        let chunks: Vec<_> = FastCDC::new(&data, min_size, avg_size, max_size).collect();

        assert!(!chunks.is_empty());
        assert_chunk_layout(&chunks, data.len(), min_size, max_size);
    }

    #[test]
    fn test_uniform_data_hits_max_size() {
        let max_size = 4096;

        // All-zero input is a deliberate pathological case: the rolling hash never
        // finds a content-defined cut point, so every cut is forced at max_size.
        let data = vec![0u8; 20 * KB];
        let chunks: Vec<_> = FastCDC::new(&data, 256, 1024, max_size).collect();

        assert_eq!(chunks.len(), 5);
        for (i, (chunk, _)) in chunks.iter().enumerate() {
            assert_eq!(chunk.offset, i * max_size, "Unexpected offset of chunk {i}");
            assert_eq!(chunk.length, max_size, "Chunk {i} must be cut exactly at max_size");
        }
    }

    #[test]
    fn test_stream_chunker_matches_chunker() {
        let data = generate_test_data(32 * KB);

        let slice_chunks: Vec<_> = FastCDC::new(&data, 256, 1024, 4096).collect();
        let stream_chunks: Vec<_> = StreamCDC::new(Cursor::new(&data), 256, 1024, 4096)
            .collect::<Result<Vec<_>, _>>()
            .expect("Reading from Cursor should not fail");

        assert_stream_matches_slice(&data, &slice_chunks, &stream_chunks);
    }

    #[test]
    fn test_stream_chunker_short_reads_match_chunker() {
        let data = generate_test_data(32 * KB);

        let slice_chunks: Vec<_> = FastCDC::new(&data, 256, 1024, 4096).collect();
        let reader = ShortReader { data: &data, pos: 0, state: 0x0123_4567_89AB_CDEF };
        let stream_chunks: Vec<_> = StreamCDC::new(reader, 256, 1024, 4096)
            .collect::<Result<Vec<_>, _>>()
            .expect("Short reads should not fail");

        assert_stream_matches_slice(&data, &slice_chunks, &stream_chunks);
    }

    #[test]
    fn test_stream_chunker_propagates_read_error() {
        let reader = FailingReader { data: generate_test_data(1000), pos: 0 };

        let first = StreamCDC::new(reader, 256, 1024, 4096).next();
        assert!(
            matches!(first, Some(Err(StreamError::IoError(_)))),
            "Read failure must surface as StreamError::IoError, got {first:?}"
        );
    }
}
