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

    /// Suppress summary output.
    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,

    /// Suppress all output.
    #[arg(long = "silent", global = true)]
    pub silent: bool,

    /// Show extra information.
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    /// Path to loq.toml config file.
    #[arg(long = "config", value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,
}

/// Available commands.
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Validate files against configured constraints.
    Check(CheckArgs),
    /// Create a loq.toml config file.
    Init(InitArgs),
}

/// Arguments for the check command.
#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Paths to check (files, directories, or - for stdin).
    #[arg(value_name = "PATH", allow_hyphen_values = true)]
    pub paths: Vec<PathBuf>,
}

/// Arguments for the init command.
#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Generate config that exempts current violations.
    #[arg(long = "baseline")]
    pub baseline: bool,
}
