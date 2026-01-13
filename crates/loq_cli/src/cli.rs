//! CLI argument definitions.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Parsed command-line arguments.
#[derive(Parser, Debug)]
#[command(name = "loq", version, about = "Enforce file size constraints")]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Show extra information.
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,
}

/// Available commands.
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Check file line counts.
    Check(CheckArgs),
    /// Create a loq.toml config file.
    Init(InitArgs),
    /// Update baseline rules for files exceeding the limit.
    Baseline(BaselineArgs),
}

/// Arguments for the check command.
#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Paths to check (files, directories, or - for stdin).
    #[arg(value_name = "PATH", allow_hyphen_values = true)]
    pub paths: Vec<PathBuf>,

    /// Disable file caching.
    #[arg(long = "no-cache")]
    pub no_cache: bool,
}

/// Arguments for the init command.
#[derive(Args, Debug, Clone)]
pub struct InitArgs {}

/// Arguments for the baseline command.
#[derive(Args, Debug, Clone)]
pub struct BaselineArgs {
    /// Line threshold for baseline (defaults to `default_max_lines` from config).
    #[arg(long = "threshold")]
    pub threshold: Option<usize>,

    /// Allow increasing limits for files that grew beyond their baseline.
    #[arg(long = "allow-growth")]
    pub allow_growth: bool,
}
