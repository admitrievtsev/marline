use std::path::PathBuf;

use clap::{value_parser, Arg, ArgMatches, Command};

pub fn add_data_dir_arg(command: Command) -> Command {
    command.arg(
        Arg::new("data-dir")
            .long("data-dir")
            .value_name("DIR")
            .help("Directory containing the extracted kernel datasets (default: ~/.marline)")
            .value_parser(value_parser!(PathBuf)),
    )
}

#[allow(dead_code)] // Not used by palantir_metrics.
pub fn add_benchmark_args(command: Command) -> Command {
    add_data_dir_arg(command).arg(
        Arg::new("output")
            .long("output")
            .value_name("FILE")
            .help("Write the CSV report to this file instead of stdout")
            .value_parser(value_parser!(PathBuf)),
    )
}

pub fn data_dir(matches: &ArgMatches) -> PathBuf {
    matches.get_one::<PathBuf>("data-dir").cloned().unwrap_or_else(default_data_dir)
}

#[allow(dead_code)] // Not used by palantir_metrics.
pub fn output(matches: &ArgMatches) -> Option<PathBuf> {
    matches.get_one::<PathBuf>("output").cloned()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".marline")
}
