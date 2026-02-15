use std::io::{self, Read};

use super::*;

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("fail"))
    }
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
    let status = handle_check_output(
        output,
        &mut stdout,
        OutputMode::Default,
        OutputFormat::Text,
        None,
    );
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
    let status = handle_check_output(
        output,
        &mut stdout,
        OutputMode::Verbose,
        OutputFormat::Text,
        None,
    );
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
    let _code = handle_check_output(
        output,
        &mut stdout,
        OutputMode::Default,
        OutputFormat::Text,
        None,
    );
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
    let status = handle_check_output(
        output,
        &mut stdout,
        OutputMode::Default,
        OutputFormat::Json,
        Some(&JsonFilter::Staged),
    );
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
    assert_eq!(parsed["filter"]["type"], "staged");
}

#[test]
fn run_check_returns_error_for_stdin_and_git_filter() {
    use termcolor::NoColor;

    let args = CheckArgs {
        paths: vec![],
        stdin: true,
        no_cache: false,
        staged: true,
        diff_ref: None,
        output_format: OutputFormat::Text,
    };
    let mut stdin: &[u8] = b"a.rs\n";
    let mut stdout = NoColor::new(Vec::new());
    let mut stderr = NoColor::new(Vec::new());

    let status = run_check(
        &args,
        &mut stdin,
        &mut stdout,
        &mut stderr,
        OutputMode::Default,
    );

    assert_eq!(status, ExitStatus::Error);
    let err = String::from_utf8(stderr.into_inner()).unwrap();
    assert!(err.contains("cannot combine '-'"));
}

#[test]
fn run_check_reports_stdin_read_error() {
    use termcolor::NoColor;

    let args = CheckArgs {
        paths: vec![],
        stdin: true,
        no_cache: false,
        staged: false,
        diff_ref: None,
        output_format: OutputFormat::Text,
    };
    let mut stdin = FailingReader;
    let mut stdout = NoColor::new(Vec::new());
    let mut stderr = NoColor::new(Vec::new());

    let status = run_check(
        &args,
        &mut stdin,
        &mut stdout,
        &mut stderr,
        OutputMode::Default,
    );

    assert_eq!(status, ExitStatus::Error);
    let err = String::from_utf8(stderr.into_inner()).unwrap();
    assert!(err.contains("failed to read stdin"));
}
