use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Instant;

use chunkfs::chunkers::{FastChunker, SizeParams};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::{Data, DataContainer, FileSystem, Scrub, ScrubMeasurements};
use clap::{Parser, ValueEnum};
use marline_index::heuristic_index::SearchConfig;
use marline_palantir::codec::{AnyCodec, DdeltaCodec, GdeltaCodec, XdeltaCodec};
use marline_palantir::lifecycle_manager::{LifecycleManager, LifecycleTierConfig};
use marline_palantir::mock_rocksdb::MockRocksDBMap;
use marline_palantir::palantir_scrubber::PalantirScrubber;
use marline_palantir::sf_generator::PalantirHasher;
use marline_palantir::types::TierConfig;
use marline_palantir::utils::lcm_vec;
use serde::Serialize;

const MIB: f64 = 1024.0 * 1024.0;

const ALL_VERSIONS: [&str; 15] = [
    "4.19", "5.4", "5.10", "5.15", "6.1", "6.2", "6.3", "6.4", "6.5", "6.6", "6.6.7", "6.6.14",
    "6.6.21", "6.6.28", "6.6-dup",
];

const DEFAULT_WINDOWS: [(&str, &str); 14] = [
    ("4.19", "5.4"),
    ("5.4", "5.10"),
    ("5.10", "5.15"),
    ("5.15", "6.1"),
    ("6.1", "6.2"),
    ("6.2", "6.3"),
    ("6.3", "6.4"),
    ("6.4", "6.5"),
    ("6.5", "6.6"),
    ("6.6", "6.6.7"),
    ("6.6.7", "6.6.14"),
    ("6.6.14", "6.6.21"),
    ("6.6.21", "6.6.28"),
    ("6.6.28", "6.6-dup"),
];

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Regime {
    Regime1,
    Regime2,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CodecKind {
    Gdelta,
    Xdelta,
    Ddelta,
}

impl CodecKind {
    fn build(self) -> AnyCodec {
        match self {
            Self::Gdelta => AnyCodec::Gdelta(GdeltaCodec),
            Self::Xdelta => AnyCodec::Xdelta(XdeltaCodec),
            Self::Ddelta => AnyCodec::Ddelta(DdeltaCodec),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Gdelta => "gdelta",
            Self::Xdelta => "xdelta",
            Self::Ddelta => "ddelta",
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "palantir_metrics",
    about = "Palantir kernel-dataset benchmark runner (regime1: full-tree windows, regime2: uniform slices)"
)]
struct Cli {
    #[arg(long, value_enum, default_value_t = Regime::Regime2)]
    regime: Regime,

    #[arg(long, default_value = "/home/maxllon/pornfolder/linuxes")]
    root: PathBuf,

    #[arg(long, value_delimiter = ',')]
    versions: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    windows: Vec<String>,

    #[arg(long, value_delimiter = ',')]
    tiers: Vec<String>,

    #[arg(long, value_enum, value_delimiter = ',', default_value = "gdelta")]
    codecs: Vec<CodecKind>,

    #[arg(long, default_value_t = 7)]
    sampling_rate: u64,

    #[arg(long, value_delimiter = ',')]
    min_jaccard: Vec<f64>,

    #[arg(long, value_delimiter = ',')]
    max_df: Vec<usize>,

    #[arg(long, value_delimiter = ',')]
    rare_first: Vec<bool>,

    #[arg(long, value_delimiter = ',')]
    fp_metric_threshold: Vec<u8>,

    #[arg(long, default_value_t = 8192)]
    cdc_min: usize,

    #[arg(long, default_value_t = 32768)]
    cdc_avg: usize,

    #[arg(long, default_value_t = 65536)]
    cdc_max: usize,

    #[arg(long, default_value = "0", value_parser = parse_seed)]
    seed: u64,

    #[arg(long)]
    no_extract: bool,

    #[arg(long)]
    out: PathBuf,
}

fn parse_seed(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}

#[derive(Clone)]
struct TierSpec {
    name: String,
    tiers: Vec<u32>,
    features_num: Option<usize>,
}

fn builtin_tiers() -> Vec<TierSpec> {
    vec![
        TierSpec { name: "ODESS".into(), tiers: vec![4], features_num: Some(12) },
        TierSpec { name: "N2_G3-2".into(), tiers: vec![3, 2], features_num: None },
        TierSpec { name: "N3_G4-3-2".into(), tiers: vec![4, 3, 2], features_num: None },
        TierSpec { name: "N3_G8-4-2".into(), tiers: vec![8, 4, 2], features_num: None },
        TierSpec { name: "N4_G6-4-3-2".into(), tiers: vec![6, 4, 3, 2], features_num: None },
        TierSpec { name: "N5_G12-6-4-3-2".into(), tiers: vec![12, 6, 4, 3, 2], features_num: None },
        TierSpec { name: "N1_G12".into(), tiers: vec![12], features_num: Some(12) },
        TierSpec { name: "N3_G12-4-3-2".into(), tiers: vec![12, 4, 3, 2], features_num: None },
        TierSpec { name: "N3_G6-4-2".into(), tiers: vec![6, 4, 2], features_num: None },
        TierSpec { name: "N2_G6-4".into(), tiers: vec![6, 4], features_num: None },
        TierSpec { name: "N3_G8-6-2".into(), tiers: vec![8, 6, 2], features_num: None },
        TierSpec { name: "N4_G12-8-4-2".into(), tiers: vec![12, 8, 4, 2], features_num: None },
        TierSpec { name: "N3_G4-3-2-fn24".into(), tiers: vec![4, 3, 2], features_num: Some(24) },
        TierSpec { name: "N4_G4-3-2-1".into(), tiers: vec![4, 3, 2, 1], features_num: None },
    ]
}

fn resolve_tiers(selected: &[String]) -> Result<Vec<TierSpec>, Box<dyn Error>> {
    let all = builtin_tiers();
    if selected.is_empty() {
        return Ok(all);
    }
    let mut out = Vec::new();
    for want in selected {
        let spec = all
            .iter()
            .find(|t| t.name == *want)
            .ok_or_else(|| format!("unknown tier config `{want}`"))?
            .clone();
        out.push(spec);
    }
    Ok(out)
}

fn build_search_configs(cli: &Cli) -> Vec<SearchConfig> {
    let mjs: Vec<Option<f64>> = if cli.min_jaccard.is_empty() {
        vec![None]
    } else {
        cli.min_jaccard.iter().map(|v| Some(*v)).collect()
    };
    let mds: Vec<Option<usize>> = if cli.max_df.is_empty() {
        vec![None]
    } else {
        cli.max_df.iter().map(|v| Some(*v)).collect()
    };
    let rfs: Vec<bool> =
        if cli.rare_first.is_empty() { vec![false] } else { cli.rare_first.clone() };
    let fps: Vec<Option<u8>> = if cli.fp_metric_threshold.is_empty() {
        vec![None]
    } else {
        cli.fp_metric_threshold.iter().map(|v| Some(*v)).collect()
    };

    let mut configs = Vec::new();
    for &mj in &mjs {
        for &md in &mds {
            for &rf in &rfs {
                for &fp in &fps {
                    configs.push(SearchConfig {
                        min_jaccard: mj,
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

enum CaseKind {
    Window { a: String, b: String },
    Slices { versions: Vec<String> },
}

struct Case {
    label: String,
    kind: CaseKind,
}

fn resolve_cases(cli: &Cli) -> Result<Vec<Case>, Box<dyn Error>> {
    match cli.regime {
        Regime::Regime2 => {
            let versions: Vec<String> = if cli.versions.is_empty() {
                ALL_VERSIONS.iter().map(|v| (*v).to_string()).collect()
            } else {
                cli.versions.clone()
            };
            let label = if versions.len() == ALL_VERSIONS.len()
                && versions.iter().zip(ALL_VERSIONS.iter()).all(|(a, b)| a == b)
            {
                "all-slices".to_string()
            } else {
                versions.join("|")
            };
            Ok(vec![Case { label, kind: CaseKind::Slices { versions } }])
        }
        Regime::Regime1 => {
            let windows: Vec<(String, String)> = if !cli.windows.is_empty() {
                let mut parsed = Vec::new();
                for w in &cli.windows {
                    let (a, b) = w
                        .split_once(':')
                        .ok_or_else(|| format!("window `{w}` must be of form <a>:<b>"))?;
                    parsed.push((a.to_string(), b.to_string()));
                }
                parsed
            } else if !cli.versions.is_empty() {
                cli.versions.windows(2).map(|pair| (pair[0].clone(), pair[1].clone())).collect()
            } else {
                DEFAULT_WINDOWS.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect()
            };
            Ok(windows
                .into_iter()
                .map(|(a, b)| Case { label: format!("{a}|{b}"), kind: CaseKind::Window { a, b } })
                .collect())
        }
    }
}

fn materialize(case: &Case, cli: &Cli) -> Result<Vec<(String, PathBuf)>, Box<dyn Error>> {
    match &case.kind {
        CaseKind::Window { a, b } => {
            if !cli.no_extract {
                let script = cli.root.join("extract_to_work.sh");
                eprintln!("  extract {a} {b} ...");
                let status = Command::new("bash").arg(&script).arg(a).arg(b).status()?;
                if !status.success() {
                    return Err(format!("extract_to_work.sh failed for window {a}|{b}").into());
                }
            }
            let dirs = vec![
                (a.clone(), cli.root.join("work").join(format!("linux-{a}"))),
                (b.clone(), cli.root.join("work").join(format!("linux-{b}"))),
            ];
            Ok(dirs)
        }
        CaseKind::Slices { versions } => Ok(versions
            .iter()
            .map(|v| (v.clone(), cli.root.join("slices").join(format!("linux-{v}"))))
            .collect()),
    }
}

type Db = HashMap<[u8; 32], DataContainer<[u8; 32]>>;
type Palantir<const N: usize> = PalantirScrubber<PalantirHasher, [u8; 32], AnyCodec, N>;

struct SharedScrubber<const N: usize> {
    inner: Rc<RefCell<Palantir<N>>>,
}

impl<const N: usize> Scrub<[u8; 32], Db, [u8; 32], MockRocksDBMap> for SharedScrubber<N> {
    fn scrub<'a>(
        &mut self,
        database: &mut Db,
        target_map: &mut MockRocksDBMap,
    ) -> io::Result<ScrubMeasurements>
    where
        [u8; 32]: 'a,
    {
        self.inner.borrow_mut().scrub(database, target_map)
    }
}

struct RunParams {
    sampling_rate: u64,
    cdc_min: usize,
    cdc_avg: usize,
    cdc_max: usize,
}

struct RunOutcome {
    cdc_ratio: f64,
    total_dedup: f64,
    stored_mb: f64,
    orig_mb: f64,
    elapsed_s: f64,
    mbps: f64,
    delta_stored: u64,
    fp_table_size: usize,
    sf_table_size: usize,
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut visited = HashSet::new();
    walk_inner(dir, out, &mut visited)
}

fn walk_inner(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> io::Result<()> {
    let canon = fs::canonicalize(dir)?;
    if !visited.insert(canon) {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        let meta = fs::metadata(&path)?;
        if meta.is_dir() {
            walk_inner(&path, out, visited)?;
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn run_timeline<const N: usize>(
    tier: &TierSpec,
    lifecycle: [LifecycleTierConfig; N],
    params: &RunParams,
    sc: &SearchConfig,
    codec: CodecKind,
    version_dirs: &[(String, PathBuf)],
) -> io::Result<RunOutcome> {
    let hasher = match tier.features_num {
        Some(fn_val) => {
            PalantirHasher::with_features_num(params.sampling_rate, tier.tiers.clone(), fn_val)
        }
        None => PalantirHasher::new(params.sampling_rate, tier.tiers.clone()),
    };
    let arr: [u32; N] = tier.tiers.as_slice().try_into().expect("tier width mismatch");
    let tier_config = match tier.features_num {
        Some(fn_val) => TierConfig::with_features_num(arr, fn_val),
        None => TierConfig::new(arr),
    };
    let scrubber = PalantirScrubber::new(hasher, codec.build(), tier_config, lifecycle, sc.clone());
    let shared = Rc::new(RefCell::new(scrubber));

    let database: Db = HashMap::new();
    let target_map = MockRocksDBMap::new();
    let mut fs = FileSystem::new_with_scrubber(
        database,
        target_map,
        Box::new(SharedScrubber { inner: Rc::clone(&shared) }),
        Sha256Hasher::default(),
    );

    let sizes = SizeParams::new(params.cdc_min, params.cdc_avg, params.cdc_max);
    let start = Instant::now();
    let mut orig_bytes: u64 = 0;
    let mut unique_cdc: HashMap<[u8; 32], usize> = HashMap::new();

    for (version, dir) in version_dirs {
        let mut files = Vec::new();
        walk_files(dir, &mut files)?;
        files.sort();

        for path in &files {
            orig_bytes += fs::metadata(path)?.len();
            let name = format!("{version}|{}", path.display());
            let mut handle = fs.create_file(name, FastChunker::new(sizes))?;
            let mut reader = io::BufReader::new(fs::File::open(path)?);
            fs.write_from_stream(&mut handle, &mut reader)?;
            fs.close_file(handle)?;
        }

        for (hash, container) in fs.storage_iterator() {
            if let Data::Chunk(chunk) = container.extract() {
                unique_cdc.entry(*hash).or_insert(chunk.len());
            }
        }

        fs.scrub()?;
        shared
            .borrow()
            .update()
            .map_err(|e| io::Error::other(format!("finish_version failed: {e}")))?;
    }

    let elapsed = start.elapsed();
    let total_dedup = fs.total_dedup_ratio();
    let unique_cdc_bytes: usize = unique_cdc.values().sum();
    let cdc_ratio =
        if unique_cdc_bytes > 0 { orig_bytes as f64 / unique_cdc_bytes as f64 } else { 0.0 };
    let orig_mb = orig_bytes as f64 / MIB;
    let stored_mb = if total_dedup > 0.0 { orig_mb / total_dedup } else { 0.0 };
    let elapsed_s = elapsed.as_secs_f64();
    let mbps = if elapsed_s > 0.0 { orig_mb / elapsed_s } else { 0.0 };

    let (delta_stored, fp_table_size, sf_table_size) = {
        let guard = shared.borrow();
        (guard.delta_stored(), guard.fp_table_size(), guard.sf_table_size())
    };

    Ok(RunOutcome {
        cdc_ratio,
        total_dedup,
        stored_mb,
        orig_mb,
        elapsed_s,
        mbps,
        delta_stored,
        fp_table_size,
        sf_table_size,
    })
}

fn run_for_tier(
    tier: &TierSpec,
    params: &RunParams,
    sc: &SearchConfig,
    codec: CodecKind,
    version_dirs: &[(String, PathBuf)],
) -> io::Result<RunOutcome> {
    match tier.tiers.len() {
        1 => run_timeline::<1>(
            tier,
            LifecycleManager::<1>::default_configs(),
            params,
            sc,
            codec,
            version_dirs,
        ),
        2 => run_timeline::<2>(
            tier,
            LifecycleManager::<2>::default_configs(),
            params,
            sc,
            codec,
            version_dirs,
        ),
        3 => run_timeline::<3>(
            tier,
            LifecycleManager::<3>::default_configs(),
            params,
            sc,
            codec,
            version_dirs,
        ),
        4 => run_timeline::<4>(
            tier,
            LifecycleManager::<4>::default_configs(),
            params,
            sc,
            codec,
            version_dirs,
        ),
        5 => run_timeline::<5>(
            tier,
            LifecycleManager::<5>::default_configs(),
            params,
            sc,
            codec,
            version_dirs,
        ),
        n => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported tier width {n} (supported 1..=5)"),
        )),
    }
}

#[derive(Serialize)]
struct Row {
    regime: String,
    case: String,
    versions: String,
    n_versions: usize,
    synthetic_slice: bool,
    tier: String,
    codec: String,
    tier_list: String,
    features_num: usize,
    tier_n: usize,
    sampling_rate: u64,
    min_jaccard: Option<f64>,
    max_df: Option<usize>,
    rare_first: bool,
    fp_metric_threshold: Option<u8>,
    cdc_min: usize,
    cdc_avg: usize,
    cdc_max: usize,
    seed: u64,
    cdc_ratio: f64,
    total_dedup: f64,
    delta_gain: f64,
    stored_mb: f64,
    orig_mb: f64,
    elapsed_s: f64,
    mbps: f64,
    delta_stored: u64,
    fp_table_size: usize,
    sf_table_size: usize,
}

fn features_num_of(tier: &TierSpec) -> usize {
    match tier.features_num {
        Some(fn_val) => fn_val,
        None => lcm_vec(&tier.tiers).expect("tier_list LCM overflow") as usize,
    }
}

fn build_row(
    cli: &Cli,
    case: &Case,
    tier: &TierSpec,
    codec: CodecKind,
    sc: &SearchConfig,
    outcome: &RunOutcome,
) -> Row {
    let versions: Vec<String> = match &case.kind {
        CaseKind::Window { a, b } => vec![a.clone(), b.clone()],
        CaseKind::Slices { versions } => versions.clone(),
    };
    let regime = match cli.regime {
        Regime::Regime1 => "regime1",
        Regime::Regime2 => "regime2",
    };
    Row {
        regime: regime.to_string(),
        case: case.label.clone(),
        versions: versions.join("|"),
        n_versions: versions.len(),
        synthetic_slice: matches!(cli.regime, Regime::Regime2),
        tier: tier.name.clone(),
        codec: codec.name().to_string(),
        tier_list: tier.tiers.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("-"),
        features_num: features_num_of(tier),
        tier_n: tier.tiers.len(),
        sampling_rate: cli.sampling_rate,
        min_jaccard: sc.min_jaccard,
        max_df: sc.max_df,
        rare_first: sc.rare_first,
        fp_metric_threshold: sc.fp_metric_threshold,
        cdc_min: cli.cdc_min,
        cdc_avg: cli.cdc_avg,
        cdc_max: cli.cdc_max,
        seed: cli.seed,
        cdc_ratio: outcome.cdc_ratio,
        total_dedup: outcome.total_dedup,
        delta_gain: outcome.total_dedup - outcome.cdc_ratio,
        stored_mb: outcome.stored_mb,
        orig_mb: outcome.orig_mb,
        elapsed_s: outcome.elapsed_s,
        mbps: outcome.mbps,
        delta_stored: outcome.delta_stored,
        fp_table_size: outcome.fp_table_size,
        sf_table_size: outcome.sf_table_size,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let tiers = resolve_tiers(&cli.tiers)?;
    if cli.codecs.is_empty() {
        return Err("at least one codec must be selected".into());
    }
    let search_configs = build_search_configs(&cli);
    let cases = resolve_cases(&cli)?;

    if let Some(parent) = cli.out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut writer = csv::Writer::from_path(&cli.out)?;

    let params = RunParams {
        sampling_rate: cli.sampling_rate,
        cdc_min: cli.cdc_min,
        cdc_avg: cli.cdc_avg,
        cdc_max: cli.cdc_max,
    };

    eprintln!(
        "regime={:?} cases={} tiers={} codecs={} search_configs={} out={}",
        cli.regime,
        cases.len(),
        tiers.len(),
        cli.codecs.iter().map(|codec| codec.name()).collect::<Vec<_>>().join(","),
        search_configs.len(),
        cli.out.display()
    );

    for case in &cases {
        let version_dirs = materialize(case, &cli)?;
        eprintln!("== case {} ==", case.label);
        for tier in &tiers {
            for codec in &cli.codecs {
                for sc in &search_configs {
                    eprintln!("  codec={} tier={}", codec.name(), tier.name);
                    let outcome = run_for_tier(tier, &params, sc, *codec, &version_dirs)?;
                    let row = build_row(&cli, case, tier, *codec, sc, &outcome);
                    eprintln!(
                        "  {:<7} {:<16} cdc={:.4} total={:.4} gain={:+.4} stored_mb={:.3} elapsed_s={:.2} mbps={:.2} deltas={}",
                        row.codec,
                        tier.name,
                        row.cdc_ratio,
                        row.total_dedup,
                        row.delta_gain,
                        row.stored_mb,
                        row.elapsed_s,
                        row.mbps,
                        row.delta_stored,
                    );
                    writer.serialize(&row)?;
                    writer.flush()?;
                }
            }
        }
    }

    eprintln!("wrote {}", cli.out.display());
    Ok(())
}
