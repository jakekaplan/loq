mod common;

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use tempfile::TempDir;

use common::{init_git_repo, run_git, write_file};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();

    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let entry_type = entry.file_type().unwrap();
        let target = dst.join(entry.file_name());

        if entry_type.is_dir() {
            copy_dir_all(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn json_output(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).unwrap()
}

fn violation_paths(output: &Value) -> Vec<String> {
    let mut paths = output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|violation| violation["path"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn setup_examples_repo() -> TempDir {
    let temp = TempDir::new().unwrap();
    copy_dir_all(&fixture_path("check_scope_examples"), temp.path());
    init_git_repo(&temp);
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);
    temp
}

#[test]
fn default_check_still_walks_current_directory() {
    let temp = setup_examples_repo();

    write_file(&temp, "docs/guide.txt", "a\nb\n");
    write_file(&temp, "src/other.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(
        violation_paths(&output),
        vec!["docs/guide.txt", "src/other.txt"]
    );
    assert_eq!(output["summary"]["files_checked"], 4);
}

#[test]
fn explicit_path_scope_still_only_checks_requested_tree() {
    let temp = setup_examples_repo();

    write_file(&temp, "docs/guide.txt", "a\nb\n");
    write_file(&temp, "src/other.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "src", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(violation_paths(&output), vec!["src/other.txt"]);
    assert_eq!(output["summary"]["files_checked"], 2);
}

#[test]
fn stdin_scope_still_uses_only_listed_paths() {
    let temp = setup_examples_repo();

    write_file(&temp, "docs/guide.txt", "a\nb\n");
    write_file(&temp, "src/other.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("docs"))
        .args(["check", "-", "--output-format", "json"])
        .write_stdin("guide.txt\n")
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(violation_paths(&output), vec!["guide.txt"]);
    assert_eq!(output["summary"]["files_checked"], 1);
}

#[test]
fn staged_scope_from_subdir_checks_repo_wide() {
    let temp = setup_examples_repo();

    write_file(&temp, "nested/subdir/notes.txt", "a\nb\n");
    write_file(&temp, "src/other.txt", "a\nb\n");
    run_git(&temp, &["add", "nested/subdir/notes.txt", "src/other.txt"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("nested/subdir"))
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(
        violation_paths(&output),
        vec!["../../src/other.txt", "notes.txt"]
    );
    assert_eq!(output["summary"]["files_checked"], 2);
}

#[test]
fn diff_head_ignores_untracked_files_and_only_checks_tracked_changes() {
    let temp = setup_examples_repo();

    write_file(&temp, "src/other.txt", "a\nb\n");
    write_file(&temp, "scratch/untracked.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--diff", "HEAD", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(violation_paths(&output), vec!["src/other.txt"]);
    assert_eq!(output["summary"]["files_checked"], 1);
}

#[test]
fn diff_range_accepts_commit_ranges_like_main_dot_dot_head() {
    let temp = setup_examples_repo();

    write_file(&temp, "docs/guide.txt", "a\nb\n");
    run_git(&temp, &["add", "docs/guide.txt"]);
    run_git(&temp, &["commit", "-m", "docs change"]);

    write_file(&temp, "src/other.txt", "a\nb\n");
    run_git(&temp, &["add", "src/other.txt"]);
    run_git(&temp, &["commit", "-m", "src change"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--diff", "HEAD~1..HEAD", "--output-format", "json"])
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(violation_paths(&output), vec!["src/other.txt"]);
    assert_eq!(output["summary"]["files_checked"], 1);
}
