//! Command-line interface for loq.
//!
//! Provides the main entry point and CLI argument handling for the loq tool.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod baseline;
mod check;
mod cli;
mod init;
mod output;

use std::ffi::OsString;
use std::io::{self, Read};
use std::process::ExitCode;

use clap::Parser;
use termcolor::{ColorChoice, StandardStream, WriteColor};

use baseline::run_baseline;
use check::{output_mode, run_check};
use init::run_init;

pub use cli::{Cli, Command};

/// Exit status for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// All checks passed.
    Success,
    /// Violations found (errors).
    Failure,
    /// Runtime error occurred.
    Error,
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Success => Self::from(0),
            ExitStatus::Failure => Self::from(1),
            ExitStatus::Error => Self::from(2),
        }
    }
}

/// Runs the CLI using environment args and stdio.
#[must_use]
pub fn run_env() -> ExitStatus {
    let args = std::env::args_os();
    let stdin = io::stdin();
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    run_with(args, stdin.lock(), &mut stdout, &mut stderr)
}

/// Runs the CLI with custom args and streams (for testing).
pub fn run_with<I, R, W1, W2>(args: I, mut stdin: R, stdout: &mut W1, stderr: &mut W2) -> ExitStatus
where
    I: IntoIterator<Item = OsString>,
    R: Read,
    W1: WriteColor,
    W2: WriteColor,
{
    let cli = Cli::parse_from(args);
    let mode = output_mode(&cli);

    let default_check = Command::Check(cli::CheckArgs {
        paths: vec![],
        no_cache: false,
    });
    match cli.command.as_ref().unwrap_or(&default_check) {
        Command::Check(args) => run_check(args, &cli, &mut stdin, stdout, stderr, mode),
        Command::Init(args) => run_init(args, stdout, stderr),
        Command::Baseline(args) => run_baseline(args, stdout, stderr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_to_exit_code() {
        assert_eq!(ExitCode::from(ExitStatus::Success), ExitCode::from(0));
        assert_eq!(ExitCode::from(ExitStatus::Failure), ExitCode::from(1));
        assert_eq!(ExitCode::from(ExitStatus::Error), ExitCode::from(2));
    }
}
