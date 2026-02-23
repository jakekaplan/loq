mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

use common::{init_git_repo, run_git, write_file};

fn json_output(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).unwrap()
}

fn violation_paths(output: &serde_json::Value) -> Vec<String> {
    output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|violation| violation["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn check_staged_from_subdir_without_scope_checks_repo_wide() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/inside.txt", "ok\n");
    write_file(&temp, "other/outside.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");
    run_git(&temp, &["add", "sub/inside.txt", "other/outside.txt"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "inside.txt"));
    assert!(paths.iter().any(|path| path == "../other/outside.txt"));
}

#[test]
fn check_diff_from_subdir_without_scope_checks_repo_wide() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/inside.txt", "ok\n");
    write_file(&temp, "other/outside.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--diff", "HEAD", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "inside.txt"));
    assert!(paths.iter().any(|path| path == "../other/outside.txt"));
}

#[test]
fn check_staged_from_subdir_with_no_staged_files_succeeds() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/inside.txt", "ok\n");
    write_file(&temp, "other/outside.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "other/outside.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .success();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(output["summary"]["files_checked"], 0);
    assert_eq!(output["summary"]["violations"], 0);
}

#[test]
fn check_diff_from_subdir_with_scope_intersects_changed_paths() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/inside.txt", "ok\n");
    write_file(&temp, "other/outside.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", ".", "--diff", "HEAD", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "inside.txt"));
    assert!(paths.iter().all(|path| path != "../other/outside.txt"));
}

#[test]
fn check_staged_ignores_deleted_files() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "keep.txt", "ok\n");
    write_file(&temp, "delete.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "keep.txt", "a\nb\n");
    run_git(&temp, &["add", "keep.txt"]);
    run_git(&temp, &["rm", "delete.txt"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "keep.txt"));
    assert!(paths.iter().all(|path| path != "delete.txt"));
}

#[test]
fn check_staged_with_stdin_scope_intersects_git_paths() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "src/a.txt", "ok\n");
    write_file(&temp, "docs/b.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "src/a.txt", "a\nb\n");
    write_file(&temp, "docs/b.txt", "a\nb\n");
    run_git(&temp, &["add", "src/a.txt", "docs/b.txt"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--stdin", "--staged", "--output-format", "json"])
        .write_stdin("src\n")
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "src/a.txt"));
    assert!(paths.iter().all(|path| path != "docs/b.txt"));
}

#[test]
fn check_diff_with_stdin_scope_intersects_git_paths() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "src/a.txt", "ok\n");
    write_file(&temp, "docs/b.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "src/a.txt", "a\nb\n");
    write_file(&temp, "docs/b.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args([
            "check",
            "--stdin",
            "--diff",
            "HEAD",
            "--output-format",
            "json",
        ])
        .write_stdin("src\n")
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "src/a.txt"));
    assert!(paths.iter().all(|path| path != "docs/b.txt"));
}

#[test]
fn check_staged_with_empty_stdin_scope_checks_nothing() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "src/a.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "src/a.txt", "a\nb\n");
    run_git(&temp, &["add", "src/a.txt"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--stdin", "--staged", "--output-format", "json"])
        .write_stdin("")
        .assert()
        .success();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(output["summary"]["files_checked"], 0);
    assert_eq!(output["summary"]["violations"], 0);
}

#[test]
fn check_diff_with_empty_stdin_scope_checks_nothing() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "src/a.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "src/a.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args([
            "check",
            "--stdin",
            "--diff",
            "HEAD",
            "--output-format",
            "json",
        ])
        .write_stdin("")
        .assert()
        .success();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(output["summary"]["files_checked"], 0);
    assert_eq!(output["summary"]["violations"], 0);
}
