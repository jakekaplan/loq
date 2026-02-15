//! Command-line interface for loq.
//!
//! Provides the main entry point and CLI argument handling for the loq tool.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod baseline;
mod baseline_shared;
mod check;
mod cli;
mod config_edit;
mod init;
mod output;
mod relax;
mod tighten;

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use clap::Parser;
use termcolor::{ColorChoice, StandardStream, WriteColor};

use baseline::run_baseline;
use check::{output_mode, run_check};
use init::run_init;
use relax::run_relax;
use tighten::run_tighten;

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

fn normalize_args<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut iter = args.into_iter();
    let mut normalized = Vec::new();

    if let Some(program) = iter.next() {
        normalized.push(program);
    }

    let mut subcommand_seen = false;
    let mut in_check = false;
    let mut rewrite_enabled = true;

    for arg in iter {
        if arg.as_os_str() == "--" {
            rewrite_enabled = false;
            normalized.push(arg);
            continue;
        }

        let arg_str = arg.to_string_lossy();
        if !subcommand_seen {
            if arg_str == "check" {
                subcommand_seen = true;
                in_check = true;
            } else if !arg_str.starts_with('-') {
                subcommand_seen = true;
            }
            normalized.push(arg);
            continue;
        }

        if rewrite_enabled && in_check && arg.as_os_str() == "-" {
            normalized.push(OsString::from("--stdin"));
        } else {
            normalized.push(arg);
        }
    }

    normalized
}

/// Runs the CLI with custom args and streams (for testing).
pub fn run_with<I, R, W1, W2>(args: I, mut stdin: R, stdout: &mut W1, stderr: &mut W2) -> ExitStatus
where
    I: IntoIterator<Item = OsString>,
    R: Read,
    W1: WriteColor + Write,
    W2: WriteColor,
{
    let cli = Cli::parse_from(normalize_args(args));
    let mode = output_mode(&cli);

    let default_check = Command::Check(cli::CheckArgs {
        paths: vec![],
        stdin: false,
        no_cache: false,
        output_format: cli::OutputFormat::Text,
    });
    match cli.command.as_ref().unwrap_or(&default_check) {
        Command::Check(args) => run_check(args, &mut stdin, stdout, stderr, mode),
        Command::Init(args) => run_init(args, stdout, stderr),
        Command::Baseline(args) => run_baseline(args, stdout, stderr),
        Command::Tighten(args) => run_tighten(args, stdout, stderr),
        Command::Relax(args) => run_relax(args, stdout, stderr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn check_command_details(cli: Cli) -> Option<(bool, Vec<PathBuf>)> {
        if let Some(Command::Check(check)) = cli.command {
            Some((check.stdin, check.paths))
        } else {
            None
        }
    }

    #[test]
    fn exit_status_to_exit_code() {
        assert_eq!(ExitCode::from(ExitStatus::Success), ExitCode::from(0));
        assert_eq!(ExitCode::from(ExitStatus::Failure), ExitCode::from(1));
        assert_eq!(ExitCode::from(ExitStatus::Error), ExitCode::from(2));
    }

    #[test]
    fn normalize_args_converts_stdin_dash_for_check() {
        let args = vec!["loq", "check", "-", "src"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let normalized = normalize_args(args);
        let cli = Cli::parse_from(normalized);

        let details = check_command_details(cli);
        assert_eq!(details, Some((true, vec![PathBuf::from("src")])));
    }

    #[test]
    fn normalize_args_preserves_literal_dash_after_double_dash() {
        let args = vec!["loq", "check", "--", "-"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let normalized = normalize_args(args);
        let cli = Cli::parse_from(normalized);

        let details = check_command_details(cli);
        assert_eq!(details, Some((false, vec![PathBuf::from("-")])));
    }

    #[test]
    fn normalize_args_converts_stdin_dash_with_global_flags() {
        let args = vec!["loq", "--verbose", "check", "-"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let normalized = normalize_args(args);
        let cli = Cli::parse_from(normalized);

        let details = check_command_details(cli);
        assert_eq!(details, Some((true, Vec::new())));
    }

    #[test]
    fn check_command_details_returns_none_for_non_check_commands() {
        let cli = Cli::parse_from(["loq", "init"]);
        assert_eq!(check_command_details(cli), None);
    }
}
