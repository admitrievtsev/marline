extern crate chunkfs;
extern crate marline_scrub;

use chunkfs::chunkers::{FastChunker, SizeParams};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::FileSystem;
use marline_metrics::{MetricsRecorder, Stage};
use marline_scrub::encoder::GdeltaEncoder;
use marline_scrub::{clusterer, decoder};
use marline_scrub::{SBCMap, SBCScrubber};
use marline_sketcher::SpeedSketchHasher;
use std::collections::HashMap;
use std::io;
use std::time::Instant;

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
    // let data = fs::read("/home/alexei/Work/chunkfs_eunner/src/files/kernels.tar")?;
    let data = vec![0u8; 10 * 1024 * 1024];
    let num_iterations = 1;
    let mut measurements = Vec::new();
    let metrics = MetricsRecorder::disabled();
    if std::env::var_os("MARLINE_METRICS").is_some() {
        metrics.set_all_enabled(true);
        println!("Runtime stage metrics enabled (MARLINE_METRICS is set)");
    }

    println!("Starting {} iterations of measurements...", num_iterations);

    for i in 0..num_iterations {
        let total_start = Instant::now();

        // According to Odess sizes
        let chunk_size = SizeParams::new(16 * 1024, 32 * 1024, 64 * 1024);

        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(decoder::GdeltaDecoder::new(false)),
            Box::new(
                SBCScrubber::new(
                    SpeedSketchHasher::new(),
                    clusterer::EqClusterer::new(0),
                    GdeltaEncoder::new(false),
                )
                .with_metrics(metrics.clone()),
            ),
            Sha256Hasher::default(),
        );

        let cdc_metrics_enabled = metrics.is_enabled(Stage::Cdc);
        let cdc_start = Instant::now();
        let mut handle = fs.create_file("file".to_string(), FastChunker::new(chunk_size))?;
        fs.write_to_file(&mut handle, &data)?;
        fs.close_file(handle)?;
        let cdc_time = cdc_start.elapsed();

        let read_handle = fs.open_file_readonly("file")?;
        let read = fs.read_file_complete(&read_handle)?;

        let cdc_dedup_ratio = fs.cdc_dedup_ratio();
        let mut metrics_batch = metrics.batch();
        if cdc_metrics_enabled {
            metrics_batch.record_unconditionally(Stage::Cdc, cdc_time);
        }

        let scrub_metrics_enabled = metrics.is_enabled(Stage::PipelineScrub);
        let scrub_start = Instant::now();
        let _res = fs.scrub()?;
        let scrub_time = scrub_start.elapsed();
        let sbc_dedup_ratio = fs.total_dedup_ratio();
        let total_metrics_enabled = metrics.is_enabled(Stage::PipelineTotal);
        let total_time = total_start.elapsed();
        if scrub_metrics_enabled {
            metrics_batch.record_unconditionally(Stage::PipelineScrub, scrub_time);
        }
        if total_metrics_enabled {
            metrics_batch.record_unconditionally(Stage::PipelineTotal, total_time);
        }
        drop(metrics_batch);

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

    if std::env::var_os("MARLINE_METRICS").is_some() {
        println!("\nStage latency metrics:");
        let snapshot = metrics.snapshot();
        for stage in &snapshot.stages {
            println!(
                "  {:<28} count={:<6} total={:>10.3} ms avg={:>10.3} ms min={:>10.3} ms max={:>10.3} ms p50={:>10.3} ms p90={:>10.3} ms p99={:>10.3} ms",
                stage.stage.as_str(),
                stage.count,
                stage.total.as_secs_f64() * 1_000.0,
                stage.average.as_secs_f64() * 1_000.0,
                stage.min.as_secs_f64() * 1_000.0,
                stage.max.as_secs_f64() * 1_000.0,
                stage.p50().unwrap_or_default().as_secs_f64() * 1_000.0,
                stage.p90().unwrap_or_default().as_secs_f64() * 1_000.0,
                stage.p99().unwrap_or_default().as_secs_f64() * 1_000.0,
            );
        }
        save_stage_metrics_csv(&snapshot)?;
        save_stage_metrics_json(&snapshot)?;
        println!("Stage metrics saved to stage_metrics.csv and stage_metrics.json");
    }

    println!("\nAll measurements completed and saved to measurements_2.csv");
    Ok(())
}

fn save_stage_metrics_csv(snapshot: &marline_metrics::MetricsSnapshot) -> io::Result<()> {
    let mut writer = csv::Writer::from_path("stage_metrics.csv")?;
    writer.write_record([
        "stage",
        "count",
        "total_ns",
        "average_ns",
        "min_ns",
        "max_ns",
        "p50_ns_approx",
        "p90_ns_approx",
        "p99_ns_approx",
    ])?;
    for stage in &snapshot.stages {
        writer.write_record([
            stage.stage.as_str().to_string(),
            stage.count.to_string(),
            stage.total.as_nanos().to_string(),
            stage.average.as_nanos().to_string(),
            stage.min.as_nanos().to_string(),
            stage.max.as_nanos().to_string(),
            stage.p50().unwrap_or_default().as_nanos().to_string(),
            stage.p90().unwrap_or_default().as_nanos().to_string(),
            stage.p99().unwrap_or_default().as_nanos().to_string(),
        ])?;
    }
    writer.flush()
}

fn save_stage_metrics_json(snapshot: &marline_metrics::MetricsSnapshot) -> io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create("stage_metrics.json")?;
    writeln!(file, "{{\n  \"stages\": [")?;
    for (index, stage) in snapshot.stages.iter().enumerate() {
        let comma = if index + 1 == snapshot.stages.len() { "" } else { "," };
        writeln!(
            file,
            "    {{\"stage\":\"{}\",\"count\":{},\"total_ns\":{},\"average_ns\":{},\"min_ns\":{},\"max_ns\":{},\"p50_ns_approx\":{},\"p90_ns_approx\":{},\"p99_ns_approx\":{}}}{}",
            stage.stage.as_str(),
            stage.count,
            stage.total.as_nanos(),
            stage.average.as_nanos(),
            stage.min.as_nanos(),
            stage.max.as_nanos(),
            stage.p50().unwrap_or_default().as_nanos(),
            stage.p90().unwrap_or_default().as_nanos(),
            stage.p99().unwrap_or_default().as_nanos(),
            comma,
        )?;
    }
    writeln!(file, "  ]\n}}")
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
