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
    let err = collect_inputs(vec![], true, &mut FailingReader, Path::new("."), false).unwrap_err();
    assert!(err.to_string().contains("failed to read stdin"));
}

#[test]
fn collect_inputs_empty_defaults_to_cwd() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(vec![], false, &mut empty_stdin, Path::new("/repo"), true).unwrap();
    assert_eq!(result, vec![PathBuf::from(".")]);
}

#[test]
fn collect_inputs_stdin_only_no_default() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(vec![], true, &mut empty_stdin, Path::new("/repo"), false).unwrap();
    assert!(result.is_empty());
}

#[test]
fn collect_inputs_stdin_with_paths() {
    let mut stdin: &[u8] = b"file1.rs\nfile2.rs\n";
    let result = collect_inputs(vec![], true, &mut stdin, Path::new("/repo"), false).unwrap();
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
        false,
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

#[cfg(windows)]
#[test]
fn normalize_path_lexical_keeps_drive_prefix() {
    let input = PathBuf::from(r"C:\repo\sub\..\src");
    let expected = PathBuf::from(r"C:\repo\src");
    assert_eq!(normalize_path_lexical(&input), expected);
}

#[test]
fn strip_line_endings_only_removes_newline_chars() {
    assert_eq!(strip_line_endings(b"/repo/path\n"), b"/repo/path");
    assert_eq!(strip_line_endings(b"/repo/path\r\n"), b"/repo/path");
    assert_eq!(strip_line_endings(b"/repo/path "), b"/repo/path ");
}

#[test]
fn intersect_paths_with_scope_normalizes_parent_dirs() {
    let temp = TempDir::new().unwrap();
    let cwd = temp.path().join("sub");
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&src_dir).unwrap();

    let candidate = src_dir.join("file.rs");
    let filtered =
        intersect_paths_with_scope(vec![PathBuf::from("../src")], vec![candidate.clone()], &cwd);

    assert_eq!(filtered, vec![candidate]);
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

// --- git_filter_from_args ---

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

// --- apply_git_filter ---

#[test]
fn apply_git_filter_none_passes_through() {
    let paths = vec![PathBuf::from("a.rs")];
    let result = apply_git_filter(paths.clone(), None, Path::new(".")).unwrap();
    assert_eq!(result, paths);
}

#[test]
fn apply_git_filter_empty_scope_returns_git_paths() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());

    std::fs::write(temp.path().join("file.txt"), "hello\n").unwrap();
    exec_git(temp.path(), &["add", "file.txt"]);
    exec_git(temp.path(), &["commit", "-m", "initial"]);

    std::fs::write(temp.path().join("file.txt"), "changed\n").unwrap();
    exec_git(temp.path(), &["add", "file.txt"]);

    let filter = GitFilter::Staged;
    let result = apply_git_filter(vec![], Some(&filter), temp.path()).unwrap();
    assert!(!result.is_empty());
    assert!(result.iter().any(|p| p.ends_with("file.txt")));
}

// --- run_git ---

#[test]
fn run_git_returns_output_on_success() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());
    let output = run_git(
        &["rev-parse", "--is-inside-work-tree"],
        temp.path(),
        "unavailable",
    )
    .unwrap();
    assert!(output.status.success());
}

// --- git_repo_root ---

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

// --- list_git_paths ---

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

// --- normalize_path_lexical ---

#[test]
fn normalize_path_lexical_curdir() {
    let result = normalize_path_lexical(Path::new("./foo/bar"));
    assert_eq!(result, PathBuf::from("foo/bar"));
}

#[test]
fn normalize_path_lexical_parent_relative() {
    let result = normalize_path_lexical(Path::new("../../foo"));
    assert_eq!(result, PathBuf::from("../../foo"));
}

#[test]
fn normalize_path_lexical_double_parent_relative() {
    let result = normalize_path_lexical(Path::new("a/../../b"));
    assert_eq!(result, PathBuf::from("../b"));
}

#[test]
fn normalize_path_lexical_empty_path() {
    let result = normalize_path_lexical(Path::new(""));
    assert_eq!(result, PathBuf::new());
}

#[test]
fn normalize_path_lexical_parent_from_root_is_noop() {
    let result = normalize_path_lexical(Path::new("/a/../../b"));
    assert_eq!(result, PathBuf::from("/b"));
}

// --- normalize_scope_path ---

#[test]
fn normalize_scope_path_absolute_stays_absolute() {
    let result = normalize_scope_path(Path::new("/some/abs/path"), Path::new("/cwd"));
    assert_eq!(result, PathBuf::from("/some/abs/path"));
}

#[test]
fn normalize_scope_path_relative_joins_cwd() {
    let result = normalize_scope_path(Path::new("rel/path"), Path::new("/cwd"));
    assert_eq!(result, PathBuf::from("/cwd/rel/path"));
}

// --- candidate_in_scope ---

#[test]
fn candidate_in_scope_file_match() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("file.txt");
    std::fs::write(&file, "content").unwrap();
    assert!(candidate_in_scope(&file, &file));
}

#[test]
fn candidate_in_scope_file_no_match() {
    let temp = TempDir::new().unwrap();
    let a = temp.path().join("a.txt");
    let b = temp.path().join("b.txt");
    std::fs::write(&a, "").unwrap();
    std::fs::write(&b, "").unwrap();
    assert!(!candidate_in_scope(&a, &b));
}

#[test]
fn candidate_in_scope_dir_contains() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("file.txt");
    std::fs::write(&file, "content").unwrap();
    assert!(candidate_in_scope(&file, temp.path()));
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
