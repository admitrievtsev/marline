// End-to-end SBC pipeline tests over the synthetic corpus from `common`.
//
// Mirrors the production config used by `marline/src/main.rs`: FastCDC chunking,
// SpeedSketch similarity hashing, exact clustering and Gdelta delta encoding.
mod common;

use chunkfs::chunkers::{FastChunker, SizeParams};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::{DataContainer, FileSystem};
use common::{build, Options};
use marline_scrub::clusterer::EqClusterer;
use marline_scrub::decoder::GdeltaDecoder;
use marline_scrub::encoder::GdeltaEncoder;
use marline_scrub::{SBCKey, SBCMap, SBCScrubber};
use marline_sketcher::{SpeedSketchHash, SpeedSketchHasher};
use std::collections::HashMap;

/// Concrete key type: CDC hash -> similarity key.
type SketcherKey = SBCKey<SpeedSketchHash>;
/// Deduplication DB: chunk hash -> container holding an SBC key.
type ChunkDb = HashMap<[u8; 32], DataContainer<SketcherKey>>;

/// Min/avg/max chunk sizes (bytes), mirroring `marline/src/main.rs`.
const CHUNK_MIN: usize = 16 * 1024;
const CHUNK_AVG: usize = 32 * 1024;
const CHUNK_MAX: usize = 64 * 1024;

/// Creates a filesystem with the same SBC pipeline as `marline/src/main.rs`.
fn make_fs() -> FileSystem<ChunkDb, [u8; 32], SketcherKey, SBCMap<GdeltaDecoder, SpeedSketchHash>> {
    FileSystem::new_with_scrubber(
        ChunkDb::new(),
        SBCMap::new(GdeltaDecoder::new(false)),
        Box::new(SBCScrubber::new(
            SpeedSketchHasher::new(),
            EqClusterer::new(0),
            GdeltaEncoder::new(false),
        )),
        Sha256Hasher::default(),
    )
}

/// Writes every corpus document as its own file, returning the originals so
/// read-back can be verified per file.
fn write_all(
    fs: &mut FileSystem<ChunkDb, [u8; 32], SketcherKey, SBCMap<GdeltaDecoder, SpeedSketchHash>>,
    documents: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    let origins: Vec<Vec<u8>> = documents.to_vec();
    for (i, doc) in documents.iter().enumerate() {
        let mut handle = fs
            .create_file(
                format!("file_{i}"),
                FastChunker::new(SizeParams::new(CHUNK_MIN, CHUNK_AVG, CHUNK_MAX)),
            )
            .unwrap();
        fs.write_to_file(&mut handle, doc).unwrap();
        fs.close_file(handle).unwrap();
    }
    origins
}

/// Reads every file back fully, in increasing index order.
fn read_all(
    fs: &FileSystem<ChunkDb, [u8; 32], SketcherKey, SBCMap<GdeltaDecoder, SpeedSketchHash>>,
    count: usize,
) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let handle = fs.open_file_readonly(format!("file_{i}")).unwrap();
            fs.read_file_complete(&handle).unwrap()
        })
        .collect()
}

/// Prints a small aligned key/value block under a title.
fn report(title: &str, rows: &[(&str, String)]) {
    eprintln!("==== {title} ====");
    for (label, value) in rows {
        eprintln!("  {label:<22} {value}");
    }
    eprintln!("--------------------");
}

/// Formats a byte count with space-separated digit groups for readability.
fn fmt_size(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[test]
fn full_corpus_survives_scrub_roundtrip() {
    let corpus = build(42, &Options::default());
    let docs = corpus.all();

    let mut fs = make_fs();
    let origins = write_all(&mut fs, &docs);

    let pre = read_all(&fs, docs.len());
    assert!(pre == origins, "read-back before scrub must match the source");

    let cdc_before = fs.cdc_dedup_ratio();

    let measurements = fs.scrub().unwrap();

    let post = read_all(&fs, docs.len());
    assert!(post == origins, "read-back after scrub must match the source");

    // CDC ratio only makes sense for the raw chunk database, so read it before
    // the scrub turns every container into a TargetChunk.
    let cdc = cdc_before;
    let total = fs.total_dedup_ratio();
    assert!(cdc > 1.0, "similar documents should already de-duplicate at the CDC level: {cdc}");
    assert!(
        total >= cdc,
        "SBC total ratio must not regress relative to CDC: total={total}, cdc={cdc}"
    );

    let total_bytes: usize = docs.iter().map(Vec::len).sum();
    let cluster_report = measurements
        .clusterization_report
        .as_ref()
        .map(|c| c.number_of_clusters)
        .unwrap_or_default();
    report(
        "SBC — full corpus round-trip",
        &[
            ("docs", docs.len().to_string()),
            ("total bytes", fmt_size(total_bytes)),
            ("cdc ratio", format!("{cdc:.3}")),
            ("sbc ratio", format!("{total:.3}")),
            ("scrub processed", fmt_size(measurements.processed_data)),
            ("scrub left", fmt_size(measurements.data_left)),
            ("clusters", cluster_report.to_string()),
        ],
    );
}

/// Reads one named file back fully.
fn read_file(
    fs: &FileSystem<ChunkDb, [u8; 32], SketcherKey, SBCMap<GdeltaDecoder, SpeedSketchHash>>,
    name: &str,
) -> Vec<u8> {
    let handle = fs.open_file_readonly(name).unwrap();
    fs.read_file_complete(&handle).unwrap()
}

#[test]
fn edge_case_documents_roundtrip() {
    let corpus = build(7, &Options::default());
    let edge: Vec<u8> = corpus.edge_cases.concat();

    let mut fs = make_fs();
    let mut handle = fs
        .create_file("edge", FastChunker::new(SizeParams::new(CHUNK_MIN, CHUNK_AVG, CHUNK_MAX)))
        .unwrap();
    fs.write_to_file(&mut handle, &edge).unwrap();
    fs.close_file(handle).unwrap();

    let origins = read_file(&fs, "edge");
    let measurements = fs.scrub().unwrap();
    let post = read_file(&fs, "edge");
    assert!(origins == post);
    assert!(post == edge, "edge-case documents must survive the scrub");

    report(
        "SBC — edge-case round-trip",
        &[
            ("bytes", fmt_size(edge.len())),
            ("scrub processed", fmt_size(measurements.processed_data)),
            ("scrub left", fmt_size(measurements.data_left)),
        ],
    );
}
