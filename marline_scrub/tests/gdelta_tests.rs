#[cfg(test)]
mod test {
    use marline_scrub::decoder::Decoder;
    use marline_scrub::decoder::GdeltaDecoder;
    use marline_scrub::encoder::GdeltaEncoder;

    #[test]
    fn test_identical_chunks() {
        let base = b"abcdefghijklmnop";
        let new = base.to_vec();
        let delta = GdeltaEncoder::default().encode_data(&new, base);
        let decoded = GdeltaDecoder::new(false).decode_chunk(base.to_vec(), &delta);
        assert_eq!(decoded, new, "identical chunk must survive round-trip");
    }

    #[test]
    fn test_diff_data_end() {
        let base = b"abcdefghijklmnop";
        let new = b"abcdefghijklmnopXYZ";
        let delta = GdeltaEncoder::default().encode_data(new, base);
        let decoded = GdeltaDecoder::new(false).decode_chunk(base.to_vec(), &delta);
        assert_eq!(decoded, new, "appended data must survive round-trip");
    }

    #[test]
    fn test_diff_data_start() {
        let base = b"abcdefghijklmnop";
        let new = b"XYZabcdefghijklmnop";
        let delta = GdeltaEncoder::default().encode_data(new, base);
        let decoded = GdeltaDecoder::new(false).decode_chunk(base.to_vec(), &delta);
        assert_eq!(decoded, new, "prepended data must survive round-trip");
    }

    #[test]
    fn test_diff_data_mid() {
        let base = b"abcdefghijklmnop";
        let new = b"abcdeXYZfghijklmnop";
        let delta = GdeltaEncoder::default().encode_data(new, base);
        let decoded = GdeltaDecoder::new(false).decode_chunk(base.to_vec(), &delta);
        assert_eq!(decoded, new, "insert-in-middle must survive round-trip");
    }

    #[test]
    fn test_full_diff_data() {
        let base = b"abcdefghijklmnop";
        let new = b"qwertyuioplkjhgf";
        let delta = GdeltaEncoder::default().encode_data(new, base);
        let decoded = GdeltaDecoder::new(false).decode_chunk(base.to_vec(), &delta);
        assert_eq!(decoded, new, "fully-different chunk must survive round-trip");
    }
}
