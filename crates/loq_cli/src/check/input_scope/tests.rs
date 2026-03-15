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

fn check_args(staged: bool, diff: Option<&str>) -> CheckArgs {
    CheckArgs {
        paths: vec![],
        stdin: false,
        staged,
        diff: diff.map(str::to_owned),
        no_cache: false,
        output_format: crate::cli::OutputFormat::Text,
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
fn resolve_check_inputs_without_git_filter_uses_collect_inputs_behavior() {
    let args = CheckArgs {
        paths: vec![],
        stdin: false,
        staged: false,
        diff: None,
        no_cache: false,
        output_format: crate::cli::OutputFormat::Text,
    };
    let mut empty_stdin: &[u8] = b"";

    let result = resolve_check_inputs(&args, &mut empty_stdin, Path::new("/repo")).unwrap();
    assert_eq!(result.paths, vec![PathBuf::from(".")]);
    assert_eq!(result.config_path, None);
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
fn has_git_dir_ancestor_detects_repo_markers() {
    let temp = TempDir::new().unwrap();
    let repo_root = temp.path().join("repo");
    let nested = repo_root.join("nested/deep");

    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(repo_root.join(".git"), "gitdir: /tmp/worktree\n").unwrap();

    assert!(has_git_dir_ancestor(&nested));
    assert!(!has_git_dir_ancestor(temp.path()));
}

#[test]
fn git_filter_from_args_respects_staged_and_diff() {
    assert_eq!(
        git_filter_from_args(&check_args(true, None)),
        Some(GitFilter::Staged)
    );
    assert_eq!(
        git_filter_from_args(&check_args(false, Some("main"))),
        Some(GitFilter::Diff("main".into()))
    );
    assert_eq!(git_filter_from_args(&check_args(false, None)), None);
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

    let paths = list_git_paths(&GitFilter::Staged, temp.path(), "unavailable").unwrap();
    assert!(paths.iter().any(|path| path.ends_with("a.txt")));
}

#[test]
fn list_git_paths_diff() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());

    std::fs::write(temp.path().join("a.txt"), "ok\n").unwrap();
    exec_git(temp.path(), &["add", "."]);
    exec_git(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("a.txt"), "changed\n").unwrap();

    let paths =
        list_git_paths(&GitFilter::Diff("HEAD".into()), temp.path(), "unavailable").unwrap();
    assert!(paths.iter().any(|path| path.ends_with("a.txt")));
}

#[test]
fn git_diff_args_match_expected_flags() {
    assert_eq!(
        git_diff_args(&GitFilter::Staged),
        vec![
            "-c",
            "diff.relative=false",
            "diff",
            "--name-only",
            "-z",
            "--diff-filter=d",
            "--cached",
        ]
    );
    assert_eq!(
        git_diff_args(&GitFilter::Diff("HEAD~1..HEAD".into())),
        vec![
            "-c",
            "diff.relative=false",
            "diff",
            "--name-only",
            "-z",
            "--diff-filter=d",
            "HEAD~1..HEAD",
        ]
    );
}

#[test]
fn resolve_git_inputs_uses_repo_root_config() {
    let temp = TempDir::new().unwrap();
    init_test_repo(temp.path());
    let sub = temp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(temp.path().join("loq.toml"), "default_max_lines = 2\n").unwrap();
    std::fs::write(sub.join("loq.toml"), "default_max_lines = 1\n").unwrap();
    std::fs::write(temp.path().join("outside.txt"), "ok\n").unwrap();
    exec_git(temp.path(), &["add", "."]);
    exec_git(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("outside.txt"), "changed\n").unwrap();
    exec_git(temp.path(), &["add", "outside.txt"]);

    let resolved = resolve_git_inputs(&GitFilter::Staged, &sub).unwrap();
    let expected_config = dunce::canonicalize(temp.path().join("loq.toml"))
        .unwrap_or_else(|_| temp.path().join("loq.toml"));
    assert_eq!(resolved.config_path, Some(expected_config));
    assert!(resolved
        .paths
        .iter()
        .any(|path| path.ends_with("outside.txt")));
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
        "unavailable",
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

    let paths = list_git_paths(&GitFilter::Staged, temp.path(), "unavailable").unwrap();
    let names: Vec<_> = paths.iter().filter_map(|path| path.file_name()).collect();
    assert!(names.windows(2).all(|window| window[0] <= window[1]));
}
