use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use chunkfs::chunkers::{FastChunker, SizeParams};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::{DataContainer, FileSystem};
use marline_index::heuristic_index::SearchConfig;
use marline_palantir::encoder::GdeltaEncoder;
use marline_palantir::lifecycle_manager::LifecycleManager;
use marline_palantir::palantir_online::{OnlineSBC, PalantirOnline};
use marline_palantir::sf_generator::PalantirHasher;
use marline_palantir::types::TierConfig;

fn fmt_sc(sc: &SearchConfig) -> String {
    format!(
        "mj={:.2} df={} rf={} fp={}",
        sc.min_jaccard.unwrap_or(-1.0),
        sc.max_df.map_or(-1, |v| v as i32),
        if sc.rare_first { 1 } else { 0 },
        sc.fp_metric_threshold.map_or(-1, |v| v as i32),
    )
}

struct Config {
    name: &'static str,
    tier_list: Vec<u32>,
    features_num_override: Option<usize>,
}

fn configs() -> Vec<Config> {
    vec![
        Config { name: "ODESS", tier_list: vec![4], features_num_override: Some(12) },
        Config { name: "N2_G3-2", tier_list: vec![3, 2], features_num_override: None },
        Config { name: "N3_G4-3-2", tier_list: vec![4, 3, 2], features_num_override: None },
        Config { name: "N3_G8-4-2", tier_list: vec![12, 4, 2], features_num_override: None },
        Config { name: "N4_G6-4-3-2", tier_list: vec![6, 4, 3, 2], features_num_override: None },
        Config {
            name: "N5_G12-6-4-3-2",
            tier_list: vec![12, 6, 4, 3, 2],
            features_num_override: None,
        },
    ]
}

fn search_config_grid() -> Vec<SearchConfig> {
    // your heuristics
    let min_jaccard = [];
    let max_df = [];
    let rare_first = [];
    let fp_threshold = [];

    let mut configs = vec![SearchConfig::default()];
    for &mj in &min_jaccard {
        for &md in &max_df {
            for &rf in &rare_first {
                for &fp in &fp_threshold {
                    configs.push(SearchConfig {
                        min_jaccard: Some(mj),
                        max_df: md,
                        rare_first: rf,
                        fp_metric_threshold: fp,
                    });
                }
            }
        }
    }
    configs
}

fn collect_files(dir: &Path, files: &mut Vec<Vec<u8>>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Ok(content) = std::fs::read(&p) {
                    files.push(content);
                }
            } else if p.is_dir() {
                collect_files(&p, files);
            }
        }
    }
}

#[allow(dead_code)]
fn run_metrics(
    name: &str,
    scrubber: impl chunkfs::Scrub<
            [u8; 32],
            HashMap<[u8; 32], DataContainer<[u8; 32]>>,
            [u8; 32],
            HashMap<[u8; 32], Vec<u8>>,
        > + 'static,
    kernel_files: &[Vec<Vec<u8>>],
) {
    let database: HashMap<[u8; 32], DataContainer<[u8; 32]>> = HashMap::default();
    let target_map = HashMap::new();
    let hasher = Sha256Hasher::default();

    let mut fs = FileSystem::new_with_scrubber(database, target_map, Box::new(scrubber), hasher);

    let chunk_size = SizeParams::new(8192, 32768, 65536);
    let original_total: usize = kernel_files.iter().flat_map(|f| f.iter()).map(|d| d.len()).sum();
    let start = Instant::now();

    let mut file_id = 0u64;
    for files in kernel_files {
        for data in files {
            let chunker = FastChunker::new(chunk_size);
            let mut handle = fs.create_file(format!("f{}", file_id), chunker).unwrap();
            fs.write_to_file(&mut handle, data).unwrap();
            fs.close_file(handle).unwrap();
            file_id += 1;
        }
    }

    let cdc_ratio = fs.cdc_dedup_ratio();
    fs.scrub().unwrap();
    let total_ratio = fs.total_dedup_ratio();
    let elapsed = start.elapsed();

    let stored_mb = (original_total as f64 / total_ratio) / (1024.0 * 1024.0);
    let orig_mb = original_total as f64 / (1024.0 * 1024.0);
    let mbps = if elapsed.as_secs_f64() > 0.0 {
        original_total as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    } else {
        0.0
    };

    println!(
        "{:<50} {:>10.4} {:>12.4} {:>12.3} {:>12.3} {:>10.2} {:>10.2}",
        name,
        cdc_ratio,
        total_ratio,
        stored_mb,
        orig_mb,
        elapsed.as_secs_f64(),
        mbps,
    );
}

fn run_sbc(name: &str, kernel_dirs: &[std::path::PathBuf]) {
    let encoder = GdeltaEncoder {};
    let sf_gen = PalantirHasher::new(7, vec![2, 3, 4]);
    let mut online_sbc = PalantirOnline::new(
        sf_gen,
        encoder,
        TierConfig { tier_list: [2, 3, 4], features_num: None },
        LifecycleManager::<3>::default_configs(),
        Default::default(),
    );

    // Stream kernels one at a time: read a kernel dir, concatenate it, write it,
    // then drop the buffers. Keeping all kernels resident at once (~5 GiB for the
    // current 9-version chain) is what used to blow past available RAM.
    let mut original_total: usize = 0;
    let start = Instant::now();
    for dir in kernel_dirs {
        let mut files = Vec::new();
        collect_files(dir, &mut files);
        original_total += files.iter().map(Vec::len).sum::<usize>();

        let mut aged = Vec::new();
        for data in &files {
            aged.extend_from_slice(data);
        }
        drop(files); // free per-file buffers before write holds the concat

        online_sbc.write(&aged).unwrap();
        drop(aged); // free the concat buffer once the chunks have been copied out
    }

    let elapsed = start.elapsed();

    let total_ratio = original_total as f64 / online_sbc.bytes_stored() as f64;

    let stored_mb = (original_total as f64 / total_ratio) / (1024.0 * 1024.0);
    let orig_mb = original_total as f64 / (1024.0 * 1024.0);
    let mbps = if elapsed.as_secs_f64() > 0.0 {
        original_total as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    } else {
        0.0
    };

    println!(
        "{:<50} {:>10.4} {:>12.4} {:>12.3} {:>12.3} {:>10.2} {:>10.2}",
        name,
        1.0f64,
        total_ratio,
        stored_mb,
        orig_mb,
        elapsed.as_secs_f64(),
        mbps,
    );
}

fn ensure_datasets() -> Vec<std::path::PathBuf> {
    const KERNEL_VERSIONS: [&str; 9] = [
        // Group A: the 3.x incremental chain, including the 3.4.5 -> 3.4.6
        // micro-increment and the final releases of the 3.x line.
        "linux-3.4.5",
        "linux-3.4.6",
        "linux-3.6.6",
        "linux-3.8.7",
        "linux-3.10.8",
        "linux-3.12.9",
        "linux-3.14.79",
        "linux-3.16.85",
        // Group B: a single 4.x release for the epoch jump.
        "linux-4.4.302",
    ];

    let base = match std::env::var_os("MARLINE_DATA_DIR") {
        Some(v) => std::path::PathBuf::from(v),
        None => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home).join(".marline")
        }
    };
    std::fs::create_dir_all(&base).expect("failed to create data directory");

    let mut ready = Vec::new();
    for &version in &KERNEL_VERSIONS {
        let dir = base.join(version);
        let needs_dl = if dir.is_dir() {
            let empty = dir.read_dir().map_or(true, |mut it| it.next().is_none());
            if empty {
                eprintln!("  {}: directory is empty, will re-download", version);
                true
            } else {
                false
            }
        } else {
            true
        };

        if needs_dl {
            eprintln!("Kernel dataset {} not found – downloading …", version);
            download_kernel(version, &base);
        }
        ready.push(dir);
    }
    ready
}

fn download_kernel(version: &str, base: &std::path::Path) {
    // kernel.org directories are per major version: v3.x/, v4.x/, v5.x/...
    let major = version.strip_prefix("linux-").and_then(|v| v.split('.').next()).unwrap_or("3");
    let url = format!("https://cdn.kernel.org/pub/linux/kernel/v{major}.x/{version}.tar.xz");
    let archive = base.join(format!("{}.tar.xz", version));

    // try curl, then wget
    let ok = std::process::Command::new("curl")
        .args(["-fL", "-o"])
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
    let kernel_dirs = ensure_datasets();

    // Report dataset sizes without keeping file contents in memory: only counts
    // and byte totals, then drop the buffers.
    for dir in &kernel_dirs {
        let mut files = Vec::new();
        collect_files(dir, &mut files);
        eprintln!(
            "  {}: {} files, {:.2} MB",
            dir.display(),
            files.len(),
            files.iter().map(|d| d.len()).sum::<usize>() as f64 / (1024.0 * 1024.0)
        );
        drop(files);
    }

    println!(
        "{:<50} {:>10} {:>12} {:>12} {:>12} {:>10} {:>10}",
        "tier | search_cfg", "cdc", "total_dedup", "stored_mb", "orig_mb", "elapsed_s", "mbps"
    );

    for cfg in configs() {
        for sc in search_config_grid() {
            let _sf_gen = if let Some(fn_val) = cfg.features_num_override {
                PalantirHasher::with_features_num(7, cfg.tier_list.clone(), fn_val)
            } else {
                PalantirHasher::new(7, cfg.tier_list.clone())
            };

            let name = format!("{} | {}", cfg.name, fmt_sc(&sc));

            run_sbc(&name, &kernel_dirs);
        }
    }
}
