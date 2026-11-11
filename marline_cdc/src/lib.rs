pub mod fastcdc;

pub use fastcdc::{
    collect_raw_features, cut_gear, Chunk, ChunkData, FastCDC, Normalization,
    StreamCDC, GEAR, GEAR_LS, MASKS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CdcConfig {
    pub min_size: usize,
    pub avg_size: usize,
    pub max_size: usize,
}

impl Default for CdcConfig {
    fn default() -> Self {
        Self {
            min_size: 1024 * 512,
            avg_size: 1024 * 1024,
            max_size: 1024 * 2048,
        }
    }
}

impl CdcConfig {
    pub fn new(min_size: usize, avg_size: usize, max_size:usize) -> Self{
        Self {
            min_size,
            avg_size,
            max_size,
        }
    }

    pub fn chunk_slice<'a>(&self, data: &'a [u8]) -> FastCDC<'a> {
        FastCDC::new(data, self.min_size, self.avg_size, self.max_size)
    }

    pub fn chunk_stream<R: std::io::Read>(&self, reader: R) -> StreamCDC<R> {
        StreamCDC::new(reader, self.min_size, self.avg_size, self.max_size)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_cdc_config_default() {
        let config = CdcConfig::default();
        assert_eq!(config.min_size, 1024 * 512);
        assert_eq!(config.avg_size, 1024 * 1024);
        assert_eq!(config.max_size, 1024 * 2048);
    }

    #[test]
    fn test_cdc_config_new() {
        let config = CdcConfig::new(256, 1024, 4096);
        assert_eq!(config.min_size, 256);
        assert_eq!(config.avg_size, 1024);
        assert_eq!(config.max_size, 4096);
    }

    #[test]
    fn test_cdc_config_traits() {
        let config = CdcConfig::new(100, 200, 400);

        // Verify Copy and Clone
        let copied = config;
        let cloned = config.clone();

        // Verify PartialEq and Eq
        assert_eq!(config, copied);
        assert_eq!(config, cloned);
        assert_ne!(config, CdcConfig::default());

        // Verify Debug representation
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("CdcConfig"));
        assert!(debug_str.contains("min_size: 100"));
    }

    #[test]
    fn test_chunk_slice_empty() {
        let config = CdcConfig::new(256, 1024, 4096);
        let data: &[u8] = &[];
        let chunks: Vec<_> = config.chunk_slice(data).collect();

        assert!(chunks.is_empty(), "Empty input should not yield any chunks");
    }

    #[test]
    fn test_chunk_slice_data_smaller_than_min_size() {
        let config = CdcConfig::new(256, 1024, 4096);
        let data = vec![0xAB; 128]; // Smaller than min_size (256)
        let chunks: Vec<_> = config.chunk_slice(&data).collect();

        assert_eq!(chunks.len(), 1, "Exactly one trailing chunk should be produced");

        let (chunk, _features) = &chunks[0];
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.length, 128);
    }

    #[test]
    fn test_chunk_slice_invariants() {
        let min_size = 256;
        let avg_size = 1024;
        let max_size = 4096;
        let config = CdcConfig::new(min_size, avg_size, max_size);

        // Deterministic pseudo-random data (64 KiB)
        let data: Vec<u8> = (0..64 * 1024).map(|i| ((i * 31 + 17) % 256) as u8).collect();
        let chunks: Vec<_> = config.chunk_slice(&data).collect();

        assert!(!chunks.is_empty());

        let mut total_bytes = 0;
        let num_chunks = chunks.len();

        for (i, (chunk, _features)) in chunks.iter().enumerate() {
            // Offset must strictly match the cumulative length of preceding chunks
            assert_eq!(chunk.offset, total_bytes, "Chunk offset sequence is broken");
            total_bytes += chunk.length;

            // All chunks except possibly the last one must be >= min_size
            if i < num_chunks - 1 {
                assert!(
                    chunk.length >= min_size,
                    "Chunk length {} is smaller than min_size ({})",
                    chunk.length,
                    min_size
                );
            }

            // No chunk must exceed max_size
            assert!(
                chunk.length <= max_size,
                "Chunk length {} exceeds max_size ({})",
                chunk.length,
                max_size
            );
        }

        // Total chunked bytes must match the original slice length
        assert_eq!(total_bytes, data.len());
    }

    #[test]
    fn test_chunk_stream_matches_chunk_slice() {
        let config = CdcConfig::new(256, 1024, 4096);
        let data: Vec<u8> = (0..32 * 1024).map(|i| (i % 251) as u8).collect();

        // Chunks from slice: Vec<(Chunk, [u32; 12])>
        let slice_chunks: Vec<_> = config.chunk_slice(&data).collect();

        // Chunks from stream: Vec<(ChunkData, [u32; 12])>
        let reader = Cursor::new(&data);
        let stream_chunks: Vec<_> = config
            .chunk_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("Reading from Cursor should not fail");

        assert_eq!(
            slice_chunks.len(),
            stream_chunks.len(),
            "Chunk count must match between slice and stream"
        );

        for ((slice_chunk, _), (stream_chunk, _)) in slice_chunks.iter().zip(stream_chunks.iter()) {
            assert_eq!(slice_chunk.offset, stream_chunk.offset as usize);
            assert_eq!(slice_chunk.length, stream_chunk.length as usize);
        }
    }

    #[test]
    fn test_chunk_stream_empty() {
        let config = CdcConfig::new(256, 1024, 4096);
        let reader = Cursor::new(Vec::<u8>::new());
        let chunks: Vec<_> = config
            .chunk_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("Reading an empty stream should succeed");

        assert!(chunks.is_empty(), "Empty stream should produce no chunks");
    }
}