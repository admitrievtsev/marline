//! Command-line argument definitions for the `palantir_metrics` benchmark runner.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// Benchmark regime.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Regime {
    Regime1,
    Regime2,
}

/// Delta encoder kind accepted on the command line.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum EncoderKind {
    Gdelta,
    Xdelta,
    Ddelta,
    Zdelta,
}

impl EncoderKind {
    /// Returns the canonical encoder name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Gdelta => "gdelta",
            Self::Xdelta => "xdelta",
            Self::Ddelta => "ddelta",
            Self::Zdelta => "zdelta",
        }
    }
}

/// Parsed arguments of the `palantir_metrics` command.
#[derive(Parser, Debug)]
#[command(
    name = "palantir_metrics",
    about = "Palantir kernel-dataset benchmark runner (regime1: full-tree windows, regime2: uniform slices)"
)]
pub struct Cli {
    /// Benchmark regime.
    #[arg(long, value_enum, default_value_t = Regime::Regime2)]
    pub regime: Regime,
    /// Dataset root directory.
    #[arg(long, default_value = "/home/maxllon/pornfolder/linuxes")]
    pub root: PathBuf,
    /// Selected kernel versions.
    #[arg(long, value_delimiter = ',')]
    pub versions: Vec<String>,
    /// Selected version windows.
    #[arg(long, value_delimiter = ',')]
    pub windows: Vec<String>,
    /// Selected tier configurations.
    #[arg(long, value_delimiter = ',')]
    pub tiers: Vec<String>,
    /// Selected delta encoders.
    #[arg(long, alias = "codecs", value_enum, value_delimiter = ',', default_value = "gdelta")]
    pub encoders: Vec<EncoderKind>,
    /// Sampling rate.
    #[arg(long, default_value_t = 7)]
    pub sampling_rate: u64,
    /// Minimum Jaccard thresholds.
    #[arg(long, value_delimiter = ',')]
    pub min_jaccard: Vec<f64>,
    /// Maximum document-frequency values.
    #[arg(long, value_delimiter = ',')]
    pub max_df: Vec<usize>,
    /// Rare-first flags.
    #[arg(long, value_delimiter = ',')]
    pub rare_first: Vec<bool>,
    /// Fingerprint metric thresholds.
    #[arg(long, value_delimiter = ',')]
    pub fp_metric_threshold: Vec<u8>,
    /// Minimum CDC chunk size.
    #[arg(long, default_value_t = 8192)]
    pub cdc_min: usize,
    /// Average CDC chunk size.
    #[arg(long, default_value_t = 32768)]
    pub cdc_avg: usize,
    /// Maximum CDC chunk size.
    #[arg(long, default_value_t = 65536)]
    pub cdc_max: usize,
    /// Random seed.
    #[arg(long, default_value = "0", value_parser = parse_seed)]
    pub seed: u64,
    /// Whether extraction is disabled.
    #[arg(long)]
    pub no_extract: bool,
    /// Output path.
    #[arg(long)]
    pub out: PathBuf,
}

/// Parses the command line into [`Cli`].
pub fn parse_cli() -> Cli {
    Cli::parse()
}

fn parse_seed(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}
