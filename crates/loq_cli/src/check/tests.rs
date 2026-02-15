use std::io;
use std::path::{Path, PathBuf};

use super::*;

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("fail"))
    }
}

#[test]
fn collect_inputs_reports_stdin_error() {
    let err = collect_inputs(vec![], true, &mut FailingReader, Path::new("."), None).unwrap_err();
    assert!(err.to_string().contains("failed to read stdin"));
}

#[test]
fn collect_inputs_empty_defaults_to_cwd() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(vec![], false, &mut empty_stdin, Path::new("/repo"), None).unwrap();
    assert_eq!(result, vec![PathBuf::from(".")]);
}

#[test]
fn collect_inputs_stdin_only_no_default() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(vec![], true, &mut empty_stdin, Path::new("/repo"), None).unwrap();
    assert!(result.is_empty());
}

#[test]
fn collect_inputs_stdin_with_paths() {
    let mut stdin: &[u8] = b"file1.rs\nfile2.rs\n";
    let result = collect_inputs(vec![], true, &mut stdin, Path::new("/repo"), None).unwrap();
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
        None,
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&PathBuf::from("explicit.rs")));
    assert!(result.contains(&PathBuf::from("/repo/from_stdin.rs")));
}

#[test]
fn collect_inputs_uses_git_paths_when_no_path_filters() {
    let mut empty_stdin: &[u8] = b"";
    let git_paths = vec![PathBuf::from("/repo/src/a.rs")];
    let result = collect_inputs(
        vec![],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(git_paths.clone()),
    )
    .unwrap();
    assert_eq!(result, git_paths);
}

#[test]
fn collect_inputs_intersects_git_paths_with_selected_paths() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(
        vec![PathBuf::from("src")],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(vec![
            PathBuf::from("/repo/src/a.rs"),
            PathBuf::from("/repo/lib/b.rs"),
        ]),
    )
    .unwrap();

    assert_eq!(result, vec![PathBuf::from("/repo/src/a.rs")]);
}

#[test]
fn collect_inputs_git_intersection_can_be_empty() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(
        vec![PathBuf::from("src")],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(vec![PathBuf::from("/repo/lib/b.rs")]),
    )
    .unwrap();

    assert!(result.is_empty());
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
fn git_error_message_for_not_repository() {
    let message = git_error_message(&JsonFilter::Staged, git::GitError::NotRepository);
    assert_eq!(message, "--staged requires a git repository");
}

#[test]
fn git_error_message_for_git_not_available() {
    let message = git_error_message(&JsonFilter::Staged, git::GitError::GitNotAvailable);
    assert_eq!(message, "--staged requires git, but git is not available");
}
