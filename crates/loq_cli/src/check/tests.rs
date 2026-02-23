use super::*;
use std::io;
use std::process::Command as StdCommand;

use tempfile::TempDir;

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("fail"))
    }
}

fn exec_git(dir: &Path, args: &[&str]) {
    let output = StdCommand::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_test_repo(dir: &Path) {
    exec_git(dir, &["init"]);
    exec_git(dir, &["config", "user.name", "Test"]);
    exec_git(dir, &["config", "user.email", "test@test.com"]);
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
fn decode_git_path_preserves_leading_and_trailing_spaces() {
    assert_eq!(
        decode_git_path(b" leading/file.txt "),
        Some(PathBuf::from(" leading/file.txt "))
    );
}

#[cfg(unix)]
#[test]
fn decode_git_path_handles_non_utf8_bytes() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let expected = PathBuf::from(OsStr::from_bytes(b"invalid-\xFF.txt"));
    assert_eq!(decode_git_path(b"invalid-\xFF.txt"), Some(expected));
}

#[test]
fn strip_line_endings_only_removes_newline_chars() {
    assert_eq!(strip_line_endings(b"/repo/path\n"), b"/repo/path");
    assert_eq!(strip_line_endings(b"/repo/path\r\n"), b"/repo/path");
    assert_eq!(strip_line_endings(b"/repo/path "), b"/repo/path ");
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
    let status = handle_check_output(output, &mut stdout, OutputMode::Default, OutputFormat::Text);
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
    let status = handle_check_output(output, &mut stdout, OutputMode::Verbose, OutputFormat::Text);
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
    let _code = handle_check_output(output, &mut stdout, OutputMode::Default, OutputFormat::Text);
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
    let status = handle_check_output(output, &mut stdout, OutputMode::Default, OutputFormat::Json);
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

#[test]
fn git_filter_from_args_staged() {
    let args = CheckArgs {
        paths: vec![],
        stdin: false,
        staged: true,
        diff: None,
        no_cache: false,
        output_format: OutputFormat::Text,
    };
    assert_eq!(git_filter_from_args(&args), Some(GitFilter::Staged));
}

#[test]
fn git_filter_from_args_diff() {
    let args = CheckArgs {
        paths: vec![],
        stdin: false,
        staged: false,
        diff: Some("main".into()),
        no_cache: false,
        output_format: OutputFormat::Text,
    };
    assert_eq!(
        git_filter_from_args(&args),
        Some(GitFilter::Diff("main".into()))
    );
}

#[test]
fn git_filter_from_args_none() {
    let args = CheckArgs {
        paths: vec![],
        stdin: false,
        staged: false,
        diff: None,
        no_cache: false,
        output_format: OutputFormat::Text,
    };
    assert_eq!(git_filter_from_args(&args), None);
}

#[test]
fn run_git_returns_output_on_success() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());
    let output = run_git(&["status", "--short"], temp.path(), "unavailable").unwrap();
    assert!(output.status.success());
}

#[test]
fn git_repo_root_returns_root() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());
    let sub = temp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    let root = git_repo_root(&sub, "unavailable", "not a repo").unwrap();
    assert_eq!(
        std::fs::canonicalize(&root).unwrap(),
        std::fs::canonicalize(temp.path()).unwrap()
    );
}

#[test]
fn git_repo_root_fails_outside_repo() {
    let temp = TempDir::new().unwrap();
    let err = git_repo_root(temp.path(), "unavailable", "not a repo").unwrap_err();
    assert!(err.to_string().contains("not a repo"));
}

#[test]
fn list_git_paths_staged() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());

    std::fs::write(temp.path().join("a.txt"), "ok\n").unwrap();
    exec_git(temp.path(), &["add", "."]);
    exec_git(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("a.txt"), "changed\n").unwrap();
    exec_git(temp.path(), &["add", "a.txt"]);

    let paths = list_git_paths(&GitFilter::Staged, temp.path()).unwrap();
    assert!(paths.iter().any(|p| p.ends_with("a.txt")));
}

#[test]
fn list_git_paths_diff() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());

    std::fs::write(temp.path().join("a.txt"), "ok\n").unwrap();
    exec_git(temp.path(), &["add", "."]);
    exec_git(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("a.txt"), "changed\n").unwrap();

    let paths = list_git_paths(&GitFilter::Diff("HEAD".into()), temp.path()).unwrap();
    assert!(paths.iter().any(|p| p.ends_with("a.txt")));
}

#[test]
fn list_git_paths_invalid_ref_fails() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());

    std::fs::write(temp.path().join("a.txt"), "ok\n").unwrap();
    exec_git(temp.path(), &["add", "."]);
    exec_git(temp.path(), &["commit", "-m", "init"]);

    let err = list_git_paths(
        &GitFilter::Diff("nonexistent_ref_abc123".into()),
        temp.path(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("git diff failed"));
}

#[test]
fn list_git_paths_deduplicates_and_sorts() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());

    std::fs::write(temp.path().join("b.txt"), "ok\n").unwrap();
    std::fs::write(temp.path().join("a.txt"), "ok\n").unwrap();
    exec_git(temp.path(), &["add", "."]);
    exec_git(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("b.txt"), "changed\n").unwrap();
    std::fs::write(temp.path().join("a.txt"), "changed\n").unwrap();
    exec_git(temp.path(), &["add", "."]);

    let paths = list_git_paths(&GitFilter::Staged, temp.path()).unwrap();
    let names: Vec<_> = paths.iter().filter_map(|p| p.file_name()).collect();
    assert!(names.windows(2).all(|w| w[0] <= w[1]));
}
