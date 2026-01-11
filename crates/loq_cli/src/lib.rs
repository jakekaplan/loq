//! Command-line interface for loq.
//!
//! Provides the main entry point and CLI argument handling for the loq tool.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cli;
mod output;

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use loq_core::report::{build_report, FindingKind};
use loq_fs::{CheckOptions, CheckOutput, FsError};
use tempfile::NamedTempFile;
use termcolor::{Color, ColorChoice, StandardStream, WriteColor};

use output::{print_error, write_block, write_finding, write_summary, write_walk_errors};

pub use cli::{Cli, Command};

/// Runs the CLI using environment args and stdio.
///
/// Returns the exit code (0 for success, 1 for violations, 2 for errors).
pub fn run_env() -> i32 {
    let args = std::env::args_os();
    let stdin = io::stdin();
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    run_with(args, stdin.lock(), &mut stdout, &mut stderr)
}

/// Runs the CLI with custom args and streams (for testing).
pub fn run_with<I, R, W1, W2>(args: I, mut stdin: R, stdout: &mut W1, stderr: &mut W2) -> i32
where
    I: IntoIterator<Item = OsString>,
    R: Read,
    W1: WriteColor,
    W2: WriteColor,
{
    let cli = Cli::parse_from(args);
    let mode = output_mode(&cli);

    let command = cli
        .command
        .clone()
        .unwrap_or(Command::Check(cli::CheckArgs { paths: vec![] }));
    match command {
        Command::Check(args) => run_check(args, &cli, &mut stdin, stdout, stderr, mode),
        Command::Init(args) => run_init(args, &cli, stdout, stderr),
    }
}

fn run_check<R: Read, W1: WriteColor, W2: WriteColor>(
    args: cli::CheckArgs,
    cli: &Cli,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
    mode: OutputMode,
) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let inputs = match collect_inputs(args.paths, stdin, &cwd) {
        Ok(paths) => paths,
        Err(err) => return print_error(stderr, &format!("{err:#}")),
    };

    let options = CheckOptions {
        config_path: cli.config.clone(),
        cwd: cwd.clone(),
    };

    let start = Instant::now();
    let output = match loq_fs::run_check(inputs, options) {
        Ok(output) => output,
        Err(err) => return handle_fs_error(err, stderr),
    };
    let duration_ms = start.elapsed().as_millis();

    handle_check_output(output, duration_ms, stdout, mode)
}

fn handle_fs_error<W: WriteColor>(err: FsError, stderr: &mut W) -> i32 {
    let message = format!("error: {err}");
    let _ = write_block(stderr, Some(Color::Red), &message);
    2
}

fn handle_check_output<W: WriteColor>(
    mut output: CheckOutput,
    duration_ms: u128,
    stdout: &mut W,
    mode: OutputMode,
) -> i32 {
    output
        .outcomes
        .sort_by(|a, b| a.display_path.cmp(&b.display_path));

    let report = build_report(&output.outcomes, duration_ms);

    match mode {
        OutputMode::Silent => {}
        OutputMode::Quiet => {
            for finding in &report.findings {
                if matches!(
                    &finding.kind,
                    FindingKind::Violation { severity, .. }
                        if *severity == loq_core::Severity::Error
                ) {
                    let _ = write_finding(stdout, finding, false);
                }
            }
        }
        _ => {
            let verbose = mode == OutputMode::Verbose;
            for finding in &report.findings {
                if !verbose && matches!(finding.kind, FindingKind::SkipWarning { .. }) {
                    continue;
                }
                let _ = write_finding(stdout, finding, verbose);
            }
            let _ = write_summary(stdout, &report.summary);

            // Show walk errors if any
            if !output.walk_errors.is_empty() {
                let _ = write_walk_errors(stdout, &output.walk_errors, verbose);
            }
        }
    }

    if report.summary.errors > 0 {
        1
    } else {
        0
    }
}

fn collect_inputs<R: Read>(
    mut paths: Vec<PathBuf>,
    stdin: &mut R,
    cwd: &Path,
) -> Result<Vec<PathBuf>> {
    let mut use_stdin = false;
    paths.retain(|path| {
        if path == Path::new("-") {
            use_stdin = true;
            false
        } else {
            true
        }
    });

    if use_stdin {
        let mut stdin_paths =
            loq_fs::stdin::read_paths(stdin, cwd).context("failed to read stdin")?;
        paths.append(&mut stdin_paths);
    }

    if paths.is_empty() && !use_stdin {
        paths.push(PathBuf::from("."));
    }

    Ok(paths)
}

fn run_init<W1: WriteColor, W2: WriteColor>(
    args: cli::InitArgs,
    _cli: &Cli,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join("loq.toml");
    if path.exists() {
        return print_error(stderr, "loq.toml already exists");
    }

    let content = if args.baseline {
        match baseline_config(&cwd) {
            Ok(content) => content,
            Err(err) => return print_error(stderr, &format!("{err:#}")),
        }
    } else {
        default_config_text(&[])
    };

    if let Err(err) = std::fs::write(&path, content) {
        return print_error(stderr, &format!("failed to write loq.toml: {err}"));
    }

    let _ = std::io::Write::flush(stdout);
    0
}

fn baseline_config(cwd: &Path) -> Result<String> {
    let template = default_config_text(&[]);
    let mut temp_file =
        NamedTempFile::new_in(cwd).context("failed to create baseline temp file")?;
    std::io::Write::write_all(&mut temp_file, template.as_bytes())
        .context("failed to write baseline config")?;

    let options = CheckOptions {
        config_path: Some(temp_file.path().to_path_buf()),
        cwd: cwd.to_path_buf(),
    };

    let output =
        loq_fs::run_check(vec![cwd.to_path_buf()], options).context("baseline check failed")?;

    let mut exempt = Vec::new();
    for outcome in output.outcomes {
        if let loq_core::OutcomeKind::Violation {
            severity: loq_core::Severity::Error,
            ..
        } = outcome.kind
        {
            let mut path = outcome.display_path.replace('\\', "/");
            if path.starts_with("./") {
                path = path.trim_start_matches("./").to_string();
            }
            exempt.push(path);
        }
    }

    exempt.sort();
    exempt.dedup();

    Ok(default_config_text(&exempt))
}

fn default_config_text(exempt: &[String]) -> String {
    let mut output = String::new();
    output.push_str("default_max_lines = 500\n\n");
    output.push_str("respect_gitignore = true\n\n");
    let exclude = loq_core::LoqConfig::init_template().exclude;
    if exclude.is_empty() {
        output.push_str("exclude = []\n\n");
    } else {
        output.push_str("exclude = [\n");
        for pattern in exclude {
            output.push_str(&format!("  \"{pattern}\",\n"));
        }
        output.push_str("]\n\n");
    }

    if exempt.is_empty() {
        output.push_str("exempt = []\n\n");
    } else {
        output.push_str("exempt = [\n");
        for path in exempt {
            output.push_str(&format!("  \"{path}\",\n"));
        }
        output.push_str("]\n\n");
    }

    output.push_str("# Last match wins. Put general rules first and overrides later.\n");
    output.push_str("[[rules]]\n");
    output.push_str("path = \"**/*.tsx\"\n");
    output.push_str("max_lines = 300\n");
    output.push_str("severity = \"warning\"\n\n");
    output.push_str("[[rules]]\n");
    output.push_str("path = \"tests/**/*\"\n");
    output.push_str("max_lines = 500\n");
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Default,
    Quiet,
    Silent,
    Verbose,
}

fn output_mode(cli: &Cli) -> OutputMode {
    if cli.silent {
        OutputMode::Silent
    } else if cli.quiet {
        OutputMode::Quiet
    } else if cli.verbose {
        OutputMode::Verbose
    } else {
        OutputMode::Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fail"))
        }
    }

    #[test]
    fn collect_inputs_reports_stdin_error() {
        let err = collect_inputs(vec![PathBuf::from("-")], &mut FailingReader, Path::new("."))
            .unwrap_err();
        assert!(err.to_string().contains("failed to read stdin"));
    }

    #[test]
    fn output_mode_precedence() {
        let cli = Cli {
            command: None,
            quiet: true,
            silent: true,
            verbose: true,
            config: None,
        };
        assert_eq!(output_mode(&cli), OutputMode::Silent);
    }
}
