//! Benchmark: writing many Linux-kernel versions through the online Palantir
//! pipeline with the `N3_G4-3-2` tier configuration and the built-in
//! [`LifecycleManager<3>`] policy.
//!
//! The benchmark streams every kernel version, in chronological order, into a
//! single [`PalantirOnline`] instance.  [`PalantirOnline::write`] closes a
//! backup version (calling [`PalantirOnline::finish_version`]) after every
//! `chunks_per_version` chunks written, so versions are cut on chunk
//! boundaries at roughly the requested amount of data.  The final, partial
//! version is closed explicitly after the last kernel.
//!
//! Run `palantir_lifecycle_bench --help` for available command-line options.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "support/args.rs"]
mod args;

use args::{add_benchmark_args, data_dir, output};
use clap::{error::ErrorKind, value_parser, Arg, Command};
use marline_index::heuristic_index::SearchConfig;
use marline_palantir::encoder::GdeltaEncoder;
use marline_palantir::lifecycle_manager::LifecycleManager;
use marline_palantir::palantir_online::{OnlineSBC, PalantirOnline, DEFAULT_CHUNKS_PER_VERSION};
use marline_palantir::sf_generator::PalantirHasher;
use marline_palantir::types::TierConfig;

/// Tier group sizes for the single benchmarked configuration `N3_G4-3-2`.
const TIERS: [u32; 3] = [4, 3, 2];
const CONFIG_NAME: &str = "N3_G4-3-2";

/// Chronological order of the Linux-kernel backup versions (~18 GiB total).
const KERNEL_VERSIONS: [&str; 38] = [
    "linux-3.0.50",
    "linux-3.0.80",
    "linux-3.0.101",
    "linux-3.1.5",
    "linux-3.1.10",
    "linux-3.2.50",
    "linux-3.2.70",
    "linux-3.2.93",
    "linux-3.3.4",
    "linux-3.3.8",
    "linux-3.4.50",
    "linux-3.4.80",
    "linux-3.4.113",
    "linux-3.5.3",
    "linux-3.5.7",
    "linux-3.6.5",
    "linux-3.6.11",
    "linux-3.7.5",
    "linux-3.7.10",
    "linux-3.8.5",
    "linux-3.8.13",
    "linux-3.9.5",
    "linux-3.9.11",
    "linux-3.10.50",
    "linux-3.10.80",
    "linux-3.10.107",
    "linux-3.11.10",
    "linux-3.12.30",
    "linux-3.12.74",
    "linux-3.13.11",
    "linux-3.14.40",
    "linux-3.14.79",
    "linux-3.15.10",
    "linux-3.16.40",
    "linux-3.16.85",
    "linux-3.17.8",
    "linux-3.18.140",
    "linux-3.19.8",
];

fn cli() -> clap::ArgMatches {
    add_benchmark_args(
        Command::new("palantir_lifecycle_bench")
            .about("Benchmark lifecycle-aware Palantir backups of Linux kernel versions")
            .arg(
                Arg::new("max-versions")
                    .long("max-versions")
                    .value_name("COUNT")
                    .help("Limit the number of chronological kernel versions processed (default: all)")
                    .value_parser(value_parser!(usize)),
            )
            .arg(
                Arg::new("runs")
                    .long("runs")
                    .value_name("COUNT")
                    .help("Number of benchmark repetitions (default: 1)")
                    .value_parser(value_parser!(usize)),
            )
            .arg(
                Arg::new("chunks-per-version")
                    .long("chunks-per-version")
                    .value_name("COUNT")
                    .help("Number of chunks written before the next backup version starts (default: 1500)")
                    .value_parser(value_parser!(usize)),
            ),
    )
    .get_matches()
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                collect_files(&p, files);
            } else if p.is_file() {
                files.push(p);
            }
        }
    }
}

/// Concatenates every file of a kernel tree into a single buffer, in
/// lexicographic path order so the stream is deterministic.
fn load_kernel(dir: &Path) -> Vec<u8> {
    let mut paths = Vec::new();
    collect_files(dir, &mut paths);
    paths.sort();
    let mut all = Vec::new();
    for p in &paths {
        if let Ok(content) = std::fs::read(p) {
            all.extend_from_slice(&content);
        }
    }
    all
}

/// Ensures every kernel version in `versions` is extracted under `base`,
/// downloading and unpacking the missing ones from kernel.org.
fn ensure_datasets(base: &Path, versions: &[&str]) -> Vec<PathBuf> {
    std::fs::create_dir_all(base).expect("failed to create data directory");

    let mut ready = Vec::new();
    for &version in versions {
        let dir = base.join(version);
        let needs_dl = if dir.is_dir() {
            dir.read_dir().map_or(true, |mut it| it.next().is_none())
        } else {
            true
        };

        if needs_dl {
            eprintln!("Kernel dataset {} not found – downloading …", version);
            download_kernel(version, base);
        }
        ready.push(dir);
    }
    ready
}

fn download_kernel(version: &str, base: &Path) {
    let url = format!("https://cdn.kernel.org/pub/linux/kernel/v3.x/{}.tar.xz", version);
    let archive = base.join(format!("{}.tar.xz", version));

    // try curl, then wget
    let ok = std::process::Command::new("curl")
        .args(["-fSL", "-o"])
        .arg(&archive)
        .arg(&url)
        .status()
        .ok()
        .and_then(|s| s.success().then_some(true))
        .unwrap_or(false)
        || std::process::Command::new("wget")
            .args(["-O"])
            .arg(&archive)
            .arg(&url)
            .status()
            .ok()
            .and_then(|s| s.success().then_some(true))
            .unwrap_or(false);

    if !ok {
        panic!(
            "Failed to download {}.\n\
             Install curl or wget, or place the dataset manually at {}",
            url,
            base.display()
        );
    }

    let status = std::process::Command::new("tar")
        .args(["-xJf"])
        .arg(&archive)
        .arg("-C")
        .arg(base)
        .status()
        .expect("tar command not found – is it installed?");

    if !status.success() {
        panic!("tar extraction failed for {}", archive.display());
    }

    std::fs::remove_file(&archive).ok();
    eprintln!("  {} extracted OK", version);
}

fn main() {
    let matches = cli();
    let base = data_dir(&matches);
    let max_versions =
        matches.get_one::<usize>("max-versions").copied().unwrap_or(KERNEL_VERSIONS.len());
    let runs = matches.get_one::<usize>("runs").copied().unwrap_or(1);
    let chunks_per_version = matches
        .get_one::<usize>("chunks-per-version")
        .copied()
        .unwrap_or(DEFAULT_CHUNKS_PER_VERSION);
    let out = output(&matches);

    if !(1..=KERNEL_VERSIONS.len()).contains(&max_versions) {
        clap::Error::raw(
            ErrorKind::ValueValidation,
            format!("--max-versions must be between 1 and {}", KERNEL_VERSIONS.len()),
        )
        .exit();
    }
    if runs == 0 {
        clap::Error::raw(ErrorKind::ValueValidation, "--runs must be greater than zero").exit();
    }
    if chunks_per_version == 0 {
        clap::Error::raw(
            ErrorKind::ValueValidation,
            "--chunks-per-version must be greater than zero",
        )
        .exit();
    }
    let versions = &KERNEL_VERSIONS[..max_versions];
    let dirs = ensure_datasets(&base, versions);

    println!(
        "config={CONFIG_NAME} versions={max_versions} runs={runs} \
         chunks_per_version={chunks_per_version}"
    );

    let mut csv = String::from(
        "config,run,version_index,chunks,bytes_mib,stored_mib,dedup_ratio,duration_s,finish_s,evicted_t0,evicted_t1,evicted_t2\n",
    );

    for run in 0..runs {
        let sf_gen = PalantirHasher::new(7, TIERS.to_vec());
        let mut online = PalantirOnline::new(
            sf_gen,
            GdeltaEncoder {},
            TierConfig::new(TIERS),
            LifecycleManager::<3>::default_configs(),
            SearchConfig::default(),
        );
        online.set_chunks_per_version(chunks_per_version);

        let mut total_bytes: u64 = 0;
        let mut total_write = Duration::ZERO;

        for (version_index, dir) in dirs.iter().enumerate() {
            let version = versions[version_index];
            let data = load_kernel(dir);
            let t0 = Instant::now();
            online.write(&data).unwrap();
            let elapsed = t0.elapsed();

            let mib = data.len() as f64 / (1024.0 * 1024.0);
            let mbps = if elapsed.as_secs_f64() > 0.0 { mib / elapsed.as_secs_f64() } else { 0.0 };
            total_bytes += data.len() as u64;
            total_write += elapsed;

            println!(
                "run={run} kernel={version:<14} {mib:>9.2} MiB  \
                 write={:>7.2}s  {mbps:>8.2} MiB/s  chunks={}  sf_posts={}",
                elapsed.as_secs_f64(),
                online.chunks_processed(),
                online.sf_table_size(),
            );
        }

        // Close the final, partial version.
        let _ = online.finish_version().unwrap();

        println!(
            "{:<8} {:<6} {:<9} {:<10} {:<10} {:<10} {:<12} {:<10} {:<4} {:<4} {:<4} {:<10}",
            "run",
            "ver",
            "chunks",
            "MiB",
            "stored_MiB",
            "dedup",
            "duration_s",
            "finish_s",
            "ev0",
            "ev1",
            "ev2",
            "cum_sf_posts"
        );
        for report in online.version_reports() {
            let bytes_mib = report.bytes() as f64 / (1024.0 * 1024.0);
            let stored_mib = report.stored_bytes() as f64 / (1024.0 * 1024.0);
            let dedup = if report.stored_bytes() > 0 {
                report.bytes() as f64 / report.stored_bytes() as f64
            } else {
                0.0
            };
            let finish_s = report.finish_duration().as_secs_f64();
            println!(
                "{:<8} {:<6} {:<9} {:<10.2} {:<10.2} {:<10.4} {:<12.3} {:<10.3} {:<4} {:<4} {:<4} {:<9}",
                run,
                report.index(),
                report.chunks(),
                bytes_mib,
                stored_mib,
                dedup,
                report.duration().as_secs_f64(),
                finish_s,
                report.evicted()[0],
                report.evicted()[1],
                report.evicted()[2],
                online.sf_table_size(),
            );
            csv.push_str(&format!(
                "{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{}\n",
                CONFIG_NAME,
                run,
                report.index(),
                report.chunks(),
                bytes_mib,
                stored_mib,
                dedup,
                report.duration().as_secs_f64(),
                finish_s,
                report.evicted()[0],
                report.evicted()[1],
                report.evicted()[2],
            ));
        }

        let total_mib = total_bytes as f64 / (1024.0 * 1024.0);
        let mbps = if total_write.as_secs_f64() > 0.0 {
            total_mib / total_write.as_secs_f64()
        } else {
            0.0
        };
        let stored = online.bytes_stored();
        let stored_mib = stored as f64 / (1024.0 * 1024.0);
        let dedup = if stored > 0 { total_bytes as f64 / stored as f64 } else { 0.0 };
        println!(
            "run={run} TOTAL {total_mib:.2} MiB in {:.2}s ({mbps:.2} MiB/s), \
             versions={}, chunks={}, fp_entries={}, sf_posts={}, deltas={}, \
             stored={stored_mib:.2} MiB, dedup={dedup:.4}",
            total_write.as_secs_f64(),
            online.version_reports().len(),
            online.chunks_processed(),
            online.fp_table_size(),
            online.sf_table_size(),
            online.delta_stored(),
        );
    }

    if let Some(path) = out {
        std::fs::write(&path, &csv).unwrap();
        println!("CSV written to {}", path.display());
    } else {
        print!("{csv}");
    }
}
