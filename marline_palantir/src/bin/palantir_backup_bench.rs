use std::path::{Path, PathBuf};
use std::time::Instant;

use marline_palantir::encoder::GdeltaEncoder;
use marline_palantir::lifecycle_manager::LifecycleManager;
use marline_palantir::palantir_online::{OnlineSBC, PalantirOnline};
use marline_palantir::sf_generator::PalantirHasher;
use marline_palantir::types::TierConfig;

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

fn load_kernel(dir: &Path) -> Vec<u8> {
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    let mut all = Vec::new();
    for f in &files {
        all.extend_from_slice(f);
    }
    all
}

fn main() {
    let base = match std::env::var_os("MARLINE_DATA_DIR") {
        Some(v) => PathBuf::from(v),
        None => {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".marline")
        }
    };

    let scenario1 = ["linux-3.4.5", "linux-3.6.6"];
    let scenario2 = ["linux-3.10.8"];

    let mut kernels: Vec<(&str, &str)> = Vec::new();
    for k in &scenario1 {
        kernels.push(("A", k));
    }
    for k in &scenario2 {
        kernels.push(("B", k));
    }

    for &(group, version) in &kernels {
        let dir = base.join(version);
        assert!(
            dir.is_dir(),
            "kernel dataset missing: {} (place it in {})",
            version,
            base.display()
        );
        let data = load_kernel(&dir);
        println!("{} {}: {:.2} MiB", group, version, data.len() as f64 / (1024.0 * 1024.0));
    }

    let out = std::env::var_os("OUT").map(PathBuf::from);
    let runs: usize = std::env::var("RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut csv =
        String::from("run,group,mode,kernel,kernel_mib,elapsed_s,mbps,stored_mib,dedup_ratio\n");

    const TIERS: [u32; 3] = [4, 3, 2];

    for run in 0..runs {
        for &(mode, naive) in &[("opt", false), ("naive", true)] {
            let sf_gen = PalantirHasher::new(7, TIERS.to_vec());
            let mut online = PalantirOnline::new(
                sf_gen,
                GdeltaEncoder {},
                TierConfig { tier_list: TIERS, features_num: None },
                LifecycleManager::<3>::default_configs(),
                Default::default(),
            );
            online.set_naive(naive);

            let mut orig_total: usize = 0;
            for &(group, version) in &kernels {
                let data = load_kernel(&base.join(version));
                orig_total += data.len();

                let start = Instant::now();
                online.write(&data).unwrap();
                let elapsed = start.elapsed();

                let stored = online.bytes_stored();
                let mbps = data.len() as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
                let dedup = orig_total as f64 / stored as f64;
                let stored_mib = stored as f64 / (1024.0 * 1024.0);
                let kernel_mib = data.len() as f64 / (1024.0 * 1024.0);

                println!(
                    "run={} {} {:<5} {:<12} {:<9.2}MiB  {:<7.2}s  {:<8.2}MiB/s  stored={:<9.2}MiB  dedup={:.4}",
                    run, group, mode, version, kernel_mib, elapsed.as_secs_f64(), mbps, stored_mib, dedup
                );
                csv.push_str(&format!(
                    "{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                    run,
                    group,
                    mode,
                    version,
                    kernel_mib,
                    elapsed.as_secs_f64(),
                    mbps,
                    stored_mib,
                    dedup
                ));
            }
        }
    }

    if let Some(path) = out {
        std::fs::write(&path, &csv).unwrap();
        println!("CSV written to {}", path.display());
    } else {
        print!("{}", csv);
    }
}
