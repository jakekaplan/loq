//! Check command implementation.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use loq_core::report::{build_report, FindingKind};
use loq_fs::{CheckOptions, CheckOutput, FsError};
use termcolor::{Color, WriteColor};

use crate::cli::{CheckArgs, Cli};
use crate::output::{print_error, write_block, write_finding, write_summary, write_walk_errors};
use crate::ExitStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Default,
    Quiet,
    Silent,
    Verbose,
}

pub const fn output_mode(cli: &Cli) -> OutputMode {
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

pub fn run_check<R: Read, W1: WriteColor, W2: WriteColor>(
    args: &CheckArgs,
    cli: &Cli,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
    mode: OutputMode,
) -> ExitStatus {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let inputs = match collect_inputs(args.paths.clone(), stdin, &cwd) {
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
        Err(err) => return handle_fs_error(&err, stderr),
    };
    let duration_ms = start.elapsed().as_millis();

    handle_check_output(output, duration_ms, stdout, mode)
}

fn handle_fs_error<W: WriteColor>(err: &FsError, stderr: &mut W) -> ExitStatus {
    let message = format!("error: {err}");
    let _ = write_block(stderr, Some(Color::Red), &message);
    ExitStatus::Error
}

fn handle_check_output<W: WriteColor>(
    mut output: CheckOutput,
    duration_ms: u128,
    stdout: &mut W,
    mode: OutputMode,
) -> ExitStatus {
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
        OutputMode::Default | OutputMode::Verbose => {
            let verbose = mode == OutputMode::Verbose;
            for finding in &report.findings {
                if !verbose && matches!(finding.kind, FindingKind::SkipWarning { .. }) {
                    continue;
                }
                let _ = write_finding(stdout, finding, verbose);
            }
            let _ = write_summary(stdout, &report.summary);

            if !output.walk_errors.is_empty() {
                let _ = write_walk_errors(stdout, &output.walk_errors, verbose);
            }
        }
    }

    if report.summary.errors > 0 {
        ExitStatus::Failure
    } else {
        ExitStatus::Success
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

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

    #[test]
    fn collect_inputs_empty_defaults_to_cwd() {
        let mut empty_stdin: &[u8] = b"";
        let result = collect_inputs(vec![], &mut empty_stdin, Path::new("/repo")).unwrap();
        assert_eq!(result, vec![PathBuf::from(".")]);
    }

    #[test]
    fn collect_inputs_stdin_only_no_default() {
        let mut empty_stdin: &[u8] = b"";
        let result = collect_inputs(
            vec![PathBuf::from("-")],
            &mut empty_stdin,
            Path::new("/repo"),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn collect_inputs_stdin_with_paths() {
        let mut stdin: &[u8] = b"file1.rs\nfile2.rs\n";
        let result =
            collect_inputs(vec![PathBuf::from("-")], &mut stdin, Path::new("/repo")).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], PathBuf::from("/repo/file1.rs"));
        assert_eq!(result[1], PathBuf::from("/repo/file2.rs"));
    }

    #[test]
    fn collect_inputs_mixed_paths_and_stdin() {
        let mut stdin: &[u8] = b"from_stdin.rs\n";
        let result = collect_inputs(
            vec![PathBuf::from("explicit.rs"), PathBuf::from("-")],
            &mut stdin,
            Path::new("/repo"),
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&PathBuf::from("explicit.rs")));
        assert!(result.contains(&PathBuf::from("/repo/from_stdin.rs")));
    }

    #[test]
    fn handle_fs_error_returns_error_status() {
        use termcolor::NoColor;
        let mut stderr = NoColor::new(Vec::new());
        let err = FsError::Io(std::io::Error::other("test error"));
        let status = handle_fs_error(&err, &mut stderr);
        assert_eq!(status, ExitStatus::Error);
        let output = String::from_utf8(stderr.into_inner()).unwrap();
        assert!(output.contains("error:"));
    }

    #[test]
    fn handle_check_output_silent_mode() {
        use termcolor::NoColor;
        let mut stdout = NoColor::new(Vec::new());
        let output = loq_fs::CheckOutput {
            outcomes: vec![],
            walk_errors: vec![],
        };
        let status = handle_check_output(output, 0, &mut stdout, OutputMode::Silent);
        assert_eq!(status, ExitStatus::Success);
        assert!(stdout.into_inner().is_empty());
    }

    #[test]
    fn handle_check_output_quiet_mode_shows_errors_only() {
        use loq_core::report::{FileOutcome, OutcomeKind};
        use loq_core::{ConfigOrigin, MatchBy, Severity};
        use termcolor::NoColor;

        let mut stdout = NoColor::new(Vec::new());
        let output = loq_fs::CheckOutput {
            outcomes: vec![
                FileOutcome {
                    path: "error.txt".into(),
                    display_path: "error.txt".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Violation {
                        limit: 10,
                        actual: 20,
                        severity: Severity::Error,
                        matched_by: MatchBy::Default,
                    },
                },
                FileOutcome {
                    path: "warning.txt".into(),
                    display_path: "warning.txt".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Violation {
                        limit: 10,
                        actual: 15,
                        severity: Severity::Warning,
                        matched_by: MatchBy::Default,
                    },
                },
            ],
            walk_errors: vec![],
        };
        let status = handle_check_output(output, 0, &mut stdout, OutputMode::Quiet);
        assert_eq!(status, ExitStatus::Failure);
        let output_str = String::from_utf8(stdout.into_inner()).unwrap();
        assert!(output_str.contains("error.txt"));
        assert!(!output_str.contains("warning.txt"));
    }

    #[test]
    fn handle_check_output_default_mode_skips_skip_warnings() {
        use loq_core::report::{FileOutcome, OutcomeKind};
        use loq_core::ConfigOrigin;
        use termcolor::NoColor;

        let mut stdout = NoColor::new(Vec::new());
        let output = loq_fs::CheckOutput {
            outcomes: vec![FileOutcome {
                path: "missing.txt".into(),
                display_path: "missing.txt".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Missing,
            }],
            walk_errors: vec![],
        };
        let status = handle_check_output(output, 0, &mut stdout, OutputMode::Default);
        assert_eq!(status, ExitStatus::Success);
        let output_str = String::from_utf8(stdout.into_inner()).unwrap();
        assert!(!output_str.contains("missing.txt") || output_str.contains("passed"));
    }

    #[test]
    fn handle_check_output_verbose_mode_shows_skip_warnings() {
        use loq_core::report::{FileOutcome, OutcomeKind};
        use loq_core::ConfigOrigin;
        use termcolor::NoColor;

        let mut stdout = NoColor::new(Vec::new());
        let output = loq_fs::CheckOutput {
            outcomes: vec![FileOutcome {
                path: "missing.txt".into(),
                display_path: "missing.txt".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Missing,
            }],
            walk_errors: vec![],
        };
        let status = handle_check_output(output, 0, &mut stdout, OutputMode::Verbose);
        assert_eq!(status, ExitStatus::Success);
        let output_str = String::from_utf8(stdout.into_inner()).unwrap();
        assert!(output_str.contains("missing.txt"));
    }

    #[test]
    fn handle_check_output_with_walk_errors() {
        use loq_fs::walk::WalkError;
        use termcolor::NoColor;

        let mut stdout = NoColor::new(Vec::new());
        let output = loq_fs::CheckOutput {
            outcomes: vec![],
            walk_errors: vec![WalkError("permission denied".into())],
        };
        let _code = handle_check_output(output, 0, &mut stdout, OutputMode::Default);
        let output_str = String::from_utf8(stdout.into_inner()).unwrap();
        assert!(output_str.contains("skipped"));
    }
}
