use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use marline_palantir::encoder::{
    DdeltaEncoder, Encoder, GdeltaEncoder, XdeltaEncoder, ZdeltaEncoder,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum Regime {
    Regime1,
    Regime2,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum EncoderKind {
    Gdelta,
    Xdelta,
    Ddelta,
    Zdelta,
}

impl EncoderKind {
    pub(super) fn build(self) -> Box<dyn Encoder> {
        match self {
            Self::Gdelta => Box::new(GdeltaEncoder),
            Self::Xdelta => Box::new(XdeltaEncoder),
            Self::Ddelta => Box::new(DdeltaEncoder),
            Self::Zdelta => Box::new(ZdeltaEncoder),
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Gdelta => "gdelta",
            Self::Xdelta => "xdelta",
            Self::Ddelta => "ddelta",
            Self::Zdelta => "zdelta",
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "palantir_metrics",
    about = "Palantir kernel-dataset benchmark runner (regime1: full-tree windows, regime2: uniform slices)"
)]
pub(super) struct Cli {
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
    pub(super) fn regime(&self) -> Regime {
        self.regime
    }
    pub(super) fn root(&self) -> &PathBuf {
        &self.root
    }
    pub(super) fn versions(&self) -> &[String] {
        &self.versions
    }
    pub(super) fn windows(&self) -> &[String] {
        &self.windows
    }
    pub(super) fn tiers(&self) -> &[String] {
        &self.tiers
    }
    pub(super) fn encoders(&self) -> &[EncoderKind] {
        &self.encoders
    }
    pub(super) fn sampling_rate(&self) -> u64 {
        self.sampling_rate
    }
    pub(super) fn min_jaccard(&self) -> &[f64] {
        &self.min_jaccard
    }
    pub(super) fn max_df(&self) -> &[usize] {
        &self.max_df
    }
    pub(super) fn rare_first(&self) -> &[bool] {
        &self.rare_first
    }
    pub(super) fn fp_metric_threshold(&self) -> &[u8] {
        &self.fp_metric_threshold
    }
    pub(super) fn cdc_min(&self) -> usize {
        self.cdc_min
    }
    pub(super) fn cdc_avg(&self) -> usize {
        self.cdc_avg
    }
    pub(super) fn cdc_max(&self) -> usize {
        self.cdc_max
    }
    pub(super) fn seed(&self) -> u64 {
        self.seed
    }
    pub(super) fn no_extract(&self) -> bool {
        self.no_extract
    }
    pub(super) fn out(&self) -> &PathBuf {
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
