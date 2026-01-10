use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "fence", version, about = "A fast file-size fence for LLM-friendly codebases")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,

    #[arg(long = "silent", global = true)]
    pub silent: bool,

    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    #[arg(long = "config", global = true)]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    Check(CheckArgs),
    Init(InitArgs),
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    #[arg(value_name = "PATH", allow_hyphen_values = true)]
    pub paths: Vec<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    #[arg(long = "baseline")]
    pub baseline: bool,
}
