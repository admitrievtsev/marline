#[cfg(test)]
mod test {
    extern crate chunkfs;
    extern crate marline_scrub;
    extern crate marline_sketcher;

    use chunkfs::chunkers::SuperChunker;
    use chunkfs::hashers::Sha256Hasher;
    use chunkfs::FileSystem;
    #[cfg(feature = "runtime-metrics")]
    use marline_metrics::{MetricsRecorder, Stage};
    use marline_scrub::decoder::{GdeltaDecoder, LevenshteinDecoder};
    use marline_scrub::encoder::{GdeltaEncoder, LevenshteinEncoder};
    use marline_scrub::{clusterer, SBCMap, SBCScrubber};
    use std::collections::HashMap;

    #[test]
    fn test_data_recovery_levenshtein() {
        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(LevenshteinDecoder::default()),
            Box::new(SBCScrubber::new(
                marline_sketcher::AronovichHasher,
                clusterer::GraphClusterer::default(),
                LevenshteinEncoder::default(),
            )),
            Sha256Hasher::default(),
        );
        let mut handle = fs.create_file("file".to_string(), SuperChunker::default()).unwrap();
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
                marline_sketcher::AronovichHasher,
                clusterer::GraphClusterer::default(),
                GdeltaEncoder::default(),
            )),
            Sha256Hasher::default(),
        );
        let mut handle = fs.create_file("file".to_string(), SuperChunker::default()).unwrap();
        let data = generate_data(8);
        fs.write_to_file(&mut handle, &data).unwrap();
        fs.close_file(handle).unwrap();

        let _res = fs.scrub().unwrap();

        let handle = fs.open_file("file", SuperChunker::default()).unwrap();
        let read = fs.read_file_complete(&handle).unwrap();
        assert_eq!(read, data);
    }

    #[cfg(feature = "runtime-metrics")]
    #[test]
    fn scrubber_records_enabled_stage_metrics() {
        let metrics = MetricsRecorder::enabled_all();
        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(GdeltaDecoder::default()),
            Box::new(
                SBCScrubber::new(
                    marline_sketcher::AronovichHasher,
                    clusterer::GraphClusterer::default(),
                    GdeltaEncoder::default(),
                )
                .with_metrics(metrics.clone()),
            ),
            Sha256Hasher::default(),
        );
        let data = vec![7; 32 * 1024];
        let mut handle =
            fs.create_file("metrics-test".to_string(), SuperChunker::default()).unwrap();
        fs.write_to_file(&mut handle, &data).unwrap();
        fs.close_file(handle).unwrap();

        fs.scrub().unwrap();

        let snapshot = metrics.snapshot();
        for expected in
            [Stage::SbcHashing, Stage::SbcClustering, Stage::SbcEncoding, Stage::SbcScrubTotal]
        {
            let stage = snapshot.stages.iter().find(|metric| metric.stage == expected).unwrap();
            assert_eq!(stage.count, 1, "unexpected count for {}", expected.as_str());
        }
    }

    const MB: usize = 1024 * 1024;

    fn generate_data(mb_size: usize) -> Vec<u8> {
        let bytes = mb_size * MB;
        (0..bytes).map(|_| rand::random::<u8>()).collect()
    }
}
