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
    #[arg(long, alias = "codecs", value_enum, value_delimiter = ',', default_value = "gdelta")]
    encoders: Vec<EncoderKind>,
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

impl Cli {
    /// Returns the selected benchmark regime.
    pub fn regime(&self) -> Regime {
        self.regime
    }
    /// Returns the dataset root directory.
    pub fn root(&self) -> &PathBuf {
        &self.root
    }
    /// Returns the selected kernel versions.
    pub fn versions(&self) -> &[String] {
        &self.versions
    }
    /// Returns the selected version windows.
    pub fn windows(&self) -> &[String] {
        &self.windows
    }
    /// Returns the selected tier configurations.
    pub fn tiers(&self) -> &[String] {
        &self.tiers
    }
    /// Returns the selected delta encoders.
    pub fn encoders(&self) -> &[EncoderKind] {
        &self.encoders
    }
    /// Returns the sampling rate.
    pub fn sampling_rate(&self) -> u64 {
        self.sampling_rate
    }
    /// Returns the minimum Jaccard thresholds.
    pub fn min_jaccard(&self) -> &[f64] {
        &self.min_jaccard
    }
    /// Returns the maximum document-frequency values.
    pub fn max_df(&self) -> &[usize] {
        &self.max_df
    }
    /// Returns the rare-first flags.
    pub fn rare_first(&self) -> &[bool] {
        &self.rare_first
    }
    /// Returns the fingerprint metric thresholds.
    pub fn fp_metric_threshold(&self) -> &[u8] {
        &self.fp_metric_threshold
    }
    /// Returns the minimum CDC chunk size.
    pub fn cdc_min(&self) -> usize {
        self.cdc_min
    }
    /// Returns the average CDC chunk size.
    pub fn cdc_avg(&self) -> usize {
        self.cdc_avg
    }
    /// Returns the maximum CDC chunk size.
    pub fn cdc_max(&self) -> usize {
        self.cdc_max
    }
    /// Returns the random seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// Returns whether extraction is disabled.
    pub fn no_extract(&self) -> bool {
        self.no_extract
    }
    /// Returns the output path.
    pub fn out(&self) -> &PathBuf {
        &self.out
    }
}

fn parse_seed(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}
