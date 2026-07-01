#[cfg(test)]
mod test {
    extern crate chunkfs;
    extern crate sbc_algorithm;
    use chunkfs::chunkers::SuperChunker;
    use chunkfs::hashers::Sha256Hasher;
    use chunkfs::FileSystem;
    use sbc_algorithm::decoder::{GdeltaDecoder, LevenshteinDecoder};
    use sbc_algorithm::encoder::{GdeltaEncoder, LevenshteinEncoder};
    use sbc_algorithm::{clusterer, hasher, SBCMap, SBCScrubber};
    use std::collections::HashMap;

    #[test]
    fn test_data_recovery_levenshtein() {
        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(LevenshteinDecoder::default()),
            Box::new(SBCScrubber::new(
                hasher::AronovichHasher,
                clusterer::GraphClusterer::default(),
                LevenshteinEncoder::default(),
            )),
            Sha256Hasher::default(),
        );
        let mut handle = fs
            .create_file("file".to_string(), SuperChunker::default())
            .unwrap();
        let data = generate_data(3);
        fs.write_to_file(&mut handle, &data).unwrap();
        fs.close_file(handle).unwrap();

        let _res = fs.scrub().unwrap();

        let handle = fs.open_file("file", SuperChunker::default()).unwrap();
        let read = fs.read_file_complete(&handle).unwrap();
        assert_eq!(read, data);
    }

    #[test]
    fn test_data_recovery_gdelta() {
        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(GdeltaDecoder::default()),
            Box::new(SBCScrubber::new(
                hasher::AronovichHasher,
                clusterer::GraphClusterer::default(),
                GdeltaEncoder::default(),
            )),
            Sha256Hasher::default(),
        );
        let mut handle = fs
            .create_file("file".to_string(), SuperChunker::default())
            .unwrap();
        let data = generate_data(8);
        fs.write_to_file(&mut handle, &data).unwrap();
        fs.close_file(handle).unwrap();

        let _res = fs.scrub().unwrap();

        let handle = fs.open_file("file", SuperChunker::default()).unwrap();
        let read = fs.read_file_complete(&handle).unwrap();
        assert_eq!(read, data);
    }
    const MB: usize = 1024 * 1024;

    fn generate_data(mb_size: usize) -> Vec<u8> {
        let bytes = mb_size * MB;
        (0..bytes).map(|_| rand::random::<u8>()).collect()
    }
}
