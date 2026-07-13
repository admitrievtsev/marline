extern crate chunkfs;
extern crate marline_scrub;

use chunkfs::chunkers::{FastChunker, SizeParams};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::FileSystem;
use marline_scrub::encoder::GdeltaEncoder;
use marline_scrub::{clusterer, decoder};
use marline_scrub::{SBCMap, SBCScrubber};
use marline_sketcher::OdessHasher;
use std::collections::HashMap;
use std::time::Instant;
use std::{fs, io};

#[derive(Debug)]
struct Measurement {
    iteration: usize,
    cdc_dedup_ratio: f64,
    sbc_dedup_ratio: f64,
    cdc_time_ms: u128,
    scrub_time_ms: u128,
    total_time_ms: u128,
}

fn main() -> io::Result<()> {
    let data = fs::read("/home/alexei/Work/chunkfs_eunner/src/files/kernels.tar")?;
    // let data = vec![0u8; 10 * 1024 * 1024];
    let num_iterations = 1;
    let mut measurements = Vec::new();

    println!("Starting {} iterations of measurements...", num_iterations);

    for i in 0..num_iterations {
        let total_start = Instant::now();

        // According to Odess sizes
        let chunk_size = SizeParams::new(16 * 1024, 32 * 1024, 64 * 1024);

        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(decoder::GdeltaDecoder::new(false)),
            Box::new(SBCScrubber::new(
                OdessHasher::default(),
                clusterer::EqClusterer::new(6),
                GdeltaEncoder::new(false),
            )),
            Sha256Hasher::default(),
        );

        let mut handle = fs.create_file("file".to_string(), FastChunker::new(chunk_size))?;
        fs.write_to_file(&mut handle, &data)?;
        fs.close_file(handle)?;

        let read_handle = fs.open_file_readonly("file")?;
        let read = fs.read_file_complete(&read_handle)?;

        let cdc_dedup_ratio = fs.cdc_dedup_ratio();
        let cdc_time = total_start.elapsed();

        let scrub_start = Instant::now();
        let _res = fs.scrub()?;
        let scrub_time = scrub_start.elapsed();

        let sbc_dedup_ratio = fs.total_dedup_ratio();
        let total_time = total_start.elapsed();

        println!("Iteration {} completed", i + 1);
        println!("  CDC dedup ratio: {}", cdc_dedup_ratio);
        println!("  SBC dedup ratio: {}", sbc_dedup_ratio);
        println!("  CDC time: {:.2} ms", cdc_time.as_millis());
        println!("  Scrub time: {:.2} ms", scrub_time.as_millis());
        println!("  Total time: {:.2} ms", total_time.as_millis());

        measurements.push(Measurement {
            iteration: i + 1,
            cdc_dedup_ratio,
            sbc_dedup_ratio,
            cdc_time_ms: cdc_time.as_millis(),
            scrub_time_ms: scrub_time.as_millis(),
            total_time_ms: total_time.as_millis(),
        });

        assert_eq!(read.len(), data.len());
    }

    // Save measurements to CSV
    save_to_csv(&measurements)?;

    println!("\nAll measurements completed and saved to measurements.csv");
    Ok(())
}

fn save_to_csv(measurements: &[Measurement]) -> io::Result<()> {
    let mut wtr = csv::Writer::from_path("measurements_2.csv")?;

    // Write header
    wtr.write_record([
        "iteration",
        "cdc_dedup_ratio",
        "sbc_dedup_ratio",
        "cdc_time_ms",
        "scrub_time_ms",
        "total_time_ms",
    ])?;

    // Write data
    for m in measurements {
        wtr.write_record(&[
            m.iteration.to_string(),
            m.cdc_dedup_ratio.to_string(),
            m.sbc_dedup_ratio.to_string(),
            m.cdc_time_ms.to_string(),
            m.scrub_time_ms.to_string(),
            m.total_time_ms.to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}
