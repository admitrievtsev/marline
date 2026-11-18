use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use chunkfs::chunkers::{FastChunker, SizeParams};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::{DataContainer, FileSystem};
use marline_index::heuristic_index::SearchConfig;
use marline_palantir::encoder::GdeltaEncoder;
use marline_palantir::lifecycle_manager::{LifecycleManager, LifecycleTierConfig};
use marline_palantir::palantir_scrubber::PalantirScrubber;
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
        "{:<20} cdc={:<8.4} total_dedup={:<8.4} stored_mb={:<10.3} orig_mb={:<10.3} elapsed_s={:<10.2} mbps={:.2}",
        name, cdc_ratio, total_ratio, stored_mb, orig_mb, elapsed.as_secs_f64(), mbps,
    );
}

fn alpha_from_env() -> Result<Option<f64>, std::num::ParseFloatError> {
    std::env::var("MARLINE_ALPHA").ok().map(|value| value.parse::<f64>()).transpose()
}

fn run_sbc<const N: usize>(
    name: &str,
    kernel_files: &[Vec<Vec<u8>>],
    tier_list: &[u32],
    features_num_override: Option<usize>,
    search_config: &SearchConfig,
    alpha: Option<f64>,
    lifecycle_configs: [LifecycleTierConfig; N],
) {
    let tier_list: [u32; N] = tier_list.try_into().expect("tier count must match N");
    let sf_gen = if let Some(features_num) = features_num_override {
        PalantirHasher::with_features_num(7, tier_list.to_vec(), features_num)
    } else {
        PalantirHasher::new(7, tier_list.to_vec())
    };
    let tier_config = match features_num_override {
        Some(features_num) => TierConfig::<N>::with_features_num(tier_list, features_num),
        None => TierConfig::<N>::new(tier_list),
    };
    let mut scrubber = PalantirScrubber::new(
        sf_gen,
        GdeltaEncoder,
        tier_config,
        lifecycle_configs,
        search_config.clone(),
    );
    scrubber.set_delta_promote_threshold(alpha);
    run_metrics(&format!("{name} alpha={alpha:?}"), scrubber, kernel_files);
}

fn ensure_datasets() -> Vec<std::path::PathBuf> {
    const KERNEL_VERSIONS: [&str; 5] =
        //   ["linux-3.4.5", "linux-3.4.6", "linux-3.4.7", "linux-3.4.8", "linux-3.4.9"];
        //       ["linux-3.4.5", "linux-3.5.6", "linux-3.6.7", "linux-3.7.8", "linux-3.8.9"];
        ["linux-3.4.5", "linux-3.6.6", "linux-3.8.7", "linux-3.10.8", "linux-3.12.9"];

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
    let url = format!("https://cdn.kernel.org/pub/linux/kernel/v3.x/{}.tar.xz", version);
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
    let alpha = alpha_from_env()
        .expect("MARLINE_ALPHA must be a floating point number when set")
        .filter(|value| value.is_finite() && *value > 0.0);
    let kernel_dirs = ensure_datasets();

    let mut kernel_files: Vec<Vec<Vec<u8>>> = Vec::new();
    for dir in &kernel_dirs {
        let mut files = Vec::new();
        collect_files(Path::new(dir), &mut files);
        eprintln!(
            "  {}: {} files, {:.2} MB",
            dir.display(),
            files.len(),
            files.iter().map(|d| d.len()).sum::<usize>() as f64 / (1024.0 * 1024.0)
        );

        kernel_files.push(files);
    }

    println!(
        "{:<20} {:<30} {:<12} {:<21} {:<21} {:<21} {:<12}",
        "tier", "search_cfg", "cdc_ratio", "total_dedup", "stored_mb", "orig_mb", "mbps"
    );

    for cfg in configs() {
        for sc in search_config_grid() {
            let name = format!("{} | {}", cfg.name, fmt_sc(&sc));

            match cfg.tier_list.len() {
                1 => {
                    run_sbc::<1>(
                        &name,
                        &kernel_files,
                        &cfg.tier_list,
                        cfg.features_num_override,
                        &sc,
                        alpha,
                        LifecycleManager::<1>::default_configs(),
                    );
                }
                2 => {
                    run_sbc::<2>(
                        &name,
                        &kernel_files,
                        &cfg.tier_list,
                        cfg.features_num_override,
                        &sc,
                        alpha,
                        LifecycleManager::<2>::default_configs(),
                    );
                }
                3 => {
                    run_sbc::<3>(
                        &name,
                        &kernel_files,
                        &cfg.tier_list,
                        cfg.features_num_override,
                        &sc,
                        alpha,
                        LifecycleManager::<3>::default_configs(),
                    );
                }
                4 => {
                    run_sbc::<4>(
                        &name,
                        &kernel_files,
                        &cfg.tier_list,
                        cfg.features_num_override,
                        &sc,
                        alpha,
                        LifecycleManager::<4>::default_configs(),
                    );
                }
                5 => {
                    run_sbc::<5>(
                        &name,
                        &kernel_files,
                        &cfg.tier_list,
                        cfg.features_num_override,
                        &sc,
                        alpha,
                        LifecycleManager::<5>::default_configs(),
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}
