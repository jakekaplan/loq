//! Check command implementation.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use loq_core::report::{build_report, FindingKind, Report};
use loq_fs::{CheckOptions, CheckOutput, FsError};
use termcolor::{Color, WriteColor};

use crate::cli::{CheckArgs, OutputFormat};
use crate::output::{
    print_error, write_block, write_finding, write_guidance, write_json, write_summary,
    write_walk_errors,
};
use crate::Cli;
use crate::ExitStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Default,
    Verbose,
}

pub const fn output_mode(cli: &Cli) -> OutputMode {
    if cli.verbose {
        OutputMode::Verbose
    } else {
        OutputMode::Default
    }
}

pub fn run_check<R: Read, W1: WriteColor + Write, W2: WriteColor>(
    args: &CheckArgs,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
    mode: OutputMode,
) -> ExitStatus {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let inputs = match collect_inputs(args.paths.clone(), args.stdin, stdin, &cwd) {
        Ok(paths) => paths,
        Err(err) => return print_error(stderr, &format!("{err:#}")),
    };

    let options = CheckOptions {
        config_path: None,
        cwd: cwd.clone(),
        use_cache: !args.no_cache,
    };

    let output = match loq_fs::run_check(inputs, options) {
        Ok(output) => output,
        Err(err) => return handle_fs_error(&err, stderr),
    };

    handle_check_output(output, stdout, mode, args.output_format)
}

fn handle_fs_error<W: WriteColor>(err: &FsError, stderr: &mut W) -> ExitStatus {
    let message = format!("error: {err}");
    let _ = write_block(stderr, Some(Color::Red), &message);
    ExitStatus::Error
}

fn handle_check_output<W: WriteColor + Write>(
    output: CheckOutput,
    stdout: &mut W,
    mode: OutputMode,
    format: OutputFormat,
) -> ExitStatus {
    let CheckOutput {
        outcomes,
        walk_errors,
        fix_guidance,
    } = output;
    let report = build_report(&outcomes, fix_guidance);

    match format {
        OutputFormat::Json => {
            let _ = write_json(stdout, &report, &walk_errors);
        }
        OutputFormat::Text => {
            write_text_output(stdout, &report, &walk_errors, mode);
        }
    }

    if report.summary.errors > 0 {
        ExitStatus::Failure
    } else {
        ExitStatus::Success
    }
}

fn write_text_output<W: WriteColor>(
    stdout: &mut W,
    report: &Report,
    walk_errors: &[loq_fs::walk::WalkError],
    mode: OutputMode,
) {
    let verbose = mode == OutputMode::Verbose;
    for finding in &report.findings {
        if !verbose && matches!(finding.kind, FindingKind::SkipWarning { .. }) {
            continue;
        }
        let _ = write_finding(stdout, finding, verbose);
    }
    let _ = write_summary(stdout, &report.summary);

    if let Some(guidance) = &report.fix_guidance {
        let _ = write_guidance(stdout, guidance);
    }

    if !walk_errors.is_empty() {
        let _ = write_walk_errors(stdout, walk_errors, verbose);
    }
}

fn collect_inputs<R: Read>(
    mut paths: Vec<PathBuf>,
    use_stdin: bool,
    stdin: &mut R,
    cwd: &Path,
) -> Result<Vec<PathBuf>> {
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
        let err = collect_inputs(vec![], true, &mut FailingReader, Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("failed to read stdin"));
    }

    #[test]
    fn collect_inputs_empty_defaults_to_cwd() {
        let mut empty_stdin: &[u8] = b"";
        let result = collect_inputs(vec![], false, &mut empty_stdin, Path::new("/repo")).unwrap();
        assert_eq!(result, vec![PathBuf::from(".")]);
    }

    #[test]
    fn collect_inputs_stdin_only_no_default() {
        let mut empty_stdin: &[u8] = b"";
        let result = collect_inputs(vec![], true, &mut empty_stdin, Path::new("/repo")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn collect_inputs_stdin_with_paths() {
        let mut stdin: &[u8] = b"file1.rs\nfile2.rs\n";
        let result = collect_inputs(vec![], true, &mut stdin, Path::new("/repo")).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], PathBuf::from("/repo/file1.rs"));
        assert_eq!(result[1], PathBuf::from("/repo/file2.rs"));
    }

    #[test]
    fn collect_inputs_mixed_paths_and_stdin() {
        let mut stdin: &[u8] = b"from_stdin.rs\n";
        let result = collect_inputs(
            vec![PathBuf::from("explicit.rs")],
            true,
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
            fix_guidance: None,
        };
        let status =
            handle_check_output(output, &mut stdout, OutputMode::Default, OutputFormat::Text);
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
            fix_guidance: None,
        };
        let status =
            handle_check_output(output, &mut stdout, OutputMode::Verbose, OutputFormat::Text);
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
            walk_errors: vec![WalkError {
                message: "permission denied".into(),
            }],
            fix_guidance: None,
        };
        let _code =
            handle_check_output(output, &mut stdout, OutputMode::Default, OutputFormat::Text);
        let output_str = String::from_utf8(stdout.into_inner()).unwrap();
        assert!(output_str.contains("skipped"));
    }

    #[test]
    fn handle_check_output_json_format() {
        use loq_core::report::{FileOutcome, OutcomeKind};
        use loq_core::ConfigOrigin;
        use loq_core::MatchBy;
        use loq_fs::walk::WalkError;
        use termcolor::NoColor;

        let mut stdout = NoColor::new(Vec::new());
        let output = loq_fs::CheckOutput {
            outcomes: vec![
                FileOutcome {
                    path: "big.rs".into(),
                    display_path: "big.rs".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Violation {
                        limit: 100,
                        actual: 150,
                        matched_by: MatchBy::Default,
                    },
                },
                FileOutcome {
                    path: "skipped.bin".into(),
                    display_path: "skipped.bin".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Binary,
                },
            ],
            walk_errors: vec![WalkError {
                message: "permission denied".into(),
            }],
            fix_guidance: Some("Split large files.".to_string()),
        };
        let status =
            handle_check_output(output, &mut stdout, OutputMode::Default, OutputFormat::Json);
        assert_eq!(status, ExitStatus::Failure);
        let output_str = String::from_utf8(stdout.into_inner()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output_str).unwrap();
        assert_eq!(parsed["violations"][0]["path"], "big.rs");
        assert_eq!(parsed["violations"][0]["lines"], 150);
        assert_eq!(parsed["violations"][0]["max_lines"], 100);
        assert_eq!(parsed["summary"]["violations"], 1);
        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["summary"]["walk_errors"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "skipped.bin");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "binary");
        assert_eq!(parsed["walk_errors"][0], "permission denied");
        assert_eq!(parsed["fix_guidance"], "Split large files.");
    }
}
