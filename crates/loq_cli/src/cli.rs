//! CLI argument definitions.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Reset baseline rules to match current file sizes.
    Baseline(BaselineArgs),
    /// Tighten baseline rules without raising limits.
    Tighten(TightenArgs),
    /// Optionally add a buffer to current violations.
    Relax(RelaxArgs),
}

/// Output format for check results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable colored output.
    #[default]
    Text,
    /// Machine-readable JSON output.
    Json,
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

    /// Output format.
    #[arg(long = "output-format", value_enum, default_value_t = OutputFormat::Text)]
    pub output_format: OutputFormat,
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
}

/// Arguments for the tighten command.
#[derive(Args, Debug, Clone)]
pub struct TightenArgs {
    /// Line threshold for tightening (defaults to `default_max_lines` from config).
    #[arg(long = "threshold")]
    pub threshold: Option<usize>,
}

/// Arguments for the relax command.
#[derive(Args, Debug, Clone)]
pub struct RelaxArgs {
    /// Specific files to relax limits for.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Extra lines to add above the current line count.
    #[arg(long = "extra", visible_alias = "buffer", default_value_t = 0)]
    pub extra: usize,
}
