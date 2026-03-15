mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use common::{
    git_head, init_git_repo, json_output, run_git, run_git_in_dir, violation_paths, write_file,
};

fn setup_repo_with_sub_and_other_files() -> TempDir {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/inside.txt", "ok\n");
    write_file(&temp, "other/outside.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    temp
}

fn assert_repo_wide_paths_from_subdir(temp: &TempDir, args: &[&str]) {
    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(args)
        .assert()
        .failure();

    let output = json_output(&assert.get_output().stdout);
    let paths = violation_paths(&output);
    assert!(paths.iter().any(|path| path == "inside.txt"));
    assert!(paths.iter().any(|path| path == "../other/outside.txt"));
}

fn enable_relative_diff_paths(temp: &TempDir) {
    run_git(temp, &["config", "diff.relative", "true"]);
}

fn setup_repo_with_root_and_sub_configs() -> TempDir {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 2\n");
    write_file(&temp, "sub/loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "other/outside.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    temp
}

#[cfg(unix)]
fn write_fake_git_script(dir: &TempDir, body: &str) {
    let git_path = dir.path().join("git");
    std::fs::write(&git_path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = std::fs::metadata(&git_path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&git_path, permissions).unwrap();
}

#[cfg(unix)]
fn assert_spawn_error_from_git_diff(args: &[&str]) {
    let temp = TempDir::new().unwrap();

    let fake_git = TempDir::new().unwrap();
    write_fake_git_script(
        &fake_git,
        r#"if [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ]; then
  printf '%s\n' "$PWD"
  git_path="$(command -v git)"
  PATH="/bin:/usr/bin:$PATH" chmod 0644 "$git_path"
  exit 0
fi
exit 1"#,
    );

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .env("PATH", fake_git.path())
        .args(args)
        .assert()
        .failure();

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("failed to run git"));
}

#[test]
fn check_staged_from_subdir_without_scope_checks_repo_wide() {
    let temp = setup_repo_with_sub_and_other_files();

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");
    run_git(&temp, &["add", "sub/inside.txt", "other/outside.txt"]);

    assert_repo_wide_paths_from_subdir(&temp, &["check", "--staged", "--output-format", "json"]);
}

#[test]
fn check_staged_from_subdir_with_diff_relative_enabled_checks_repo_wide() {
    let temp = setup_repo_with_sub_and_other_files();
    enable_relative_diff_paths(&temp);

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");
    run_git(&temp, &["add", "sub/inside.txt", "other/outside.txt"]);

    assert_repo_wide_paths_from_subdir(&temp, &["check", "--staged", "--output-format", "json"]);
}

#[test]
fn check_diff_from_subdir_without_scope_checks_repo_wide() {
    let temp = setup_repo_with_sub_and_other_files();

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");

    assert_repo_wide_paths_from_subdir(
        &temp,
        &["check", "--diff", "HEAD", "--output-format", "json"],
    );
}

#[test]
fn check_diff_from_subdir_with_diff_relative_enabled_checks_repo_wide() {
    let temp = setup_repo_with_sub_and_other_files();
    enable_relative_diff_paths(&temp);

    write_file(&temp, "sub/inside.txt", "a\nb\n");
    write_file(&temp, "other/outside.txt", "a\nb\n");

    assert_repo_wide_paths_from_subdir(
        &temp,
        &["check", "--diff", "HEAD", "--output-format", "json"],
    );
}

#[test]
fn check_staged_from_subdir_with_no_staged_files_succeeds() {
    let temp = setup_repo_with_sub_and_other_files();

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
fn check_staged_from_subdir_uses_repo_root_config() {
    let temp = setup_repo_with_root_and_sub_configs();
    write_file(&temp, "other/outside.txt", "a\nb\n");
    run_git(&temp, &["add", "other/outside.txt"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .success();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(output["summary"]["files_checked"], 1);
    assert_eq!(output["summary"]["passed"], 1);
    assert_eq!(output["summary"]["violations"], 0);
}

#[test]
fn check_diff_from_subdir_uses_repo_root_config() {
    let temp = setup_repo_with_root_and_sub_configs();
    write_file(&temp, "other/outside.txt", "a\nb\n");

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--diff", "HEAD", "--output-format", "json"])
        .assert()
        .success();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(output["summary"]["files_checked"], 1);
    assert_eq!(output["summary"]["passed"], 1);
    assert_eq!(output["summary"]["violations"], 0);
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
fn check_staged_ignores_submodule_directory_entries() {
    let temp = TempDir::new().unwrap();
    let submodule_origin = TempDir::new().unwrap();

    init_git_repo(&temp);
    init_git_repo(&submodule_origin);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&submodule_origin, "nested/inside.txt", "a\nb\n");
    run_git(&submodule_origin, &["add", "."]);
    run_git(&submodule_origin, &["commit", "-m", "initial"]);

    run_git(
        &temp,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule_origin.path().to_string_lossy().as_ref(),
            "modules/sub",
        ],
    );
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&submodule_origin, "nested/inside.txt", "a\nb\nc\n");
    run_git(&submodule_origin, &["add", "nested/inside.txt"]);
    run_git(&submodule_origin, &["commit", "-m", "update"]);

    let submodule_checkout = temp.path().join("modules/sub");
    let updated_head = git_head(submodule_origin.path());
    run_git_in_dir(&submodule_checkout, &["fetch", "origin"]);
    run_git_in_dir(&submodule_checkout, &["checkout", &updated_head]);
    run_git(&temp, &["add", "modules/sub"]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .success();

    let output = json_output(&assert.get_output().stdout);
    assert_eq!(output["summary"]["files_checked"], 0);
    assert_eq!(output["summary"]["violations"], 0);
}

#[cfg(unix)]
#[test]
fn check_staged_reports_empty_repo_root_from_git() {
    let temp = TempDir::new().unwrap();

    let fake_git = TempDir::new().unwrap();
    write_fake_git_script(
        &fake_git,
        r#"if [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ]; then
  exit 0
fi
exit 1"#,
    );

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .env("PATH", fake_git.path())
        .args(["check", "--staged"])
        .assert()
        .failure();

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("failed to determine git repository root"));
}

#[cfg(unix)]
#[test]
fn check_staged_preserves_rev_parse_stderr_for_non_repo_failures() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join(".git"), "gitdir: /tmp/protected\n").unwrap();

    let fake_git = TempDir::new().unwrap();
    write_fake_git_script(
        &fake_git,
        r#"if [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ]; then
  echo 'fatal: detected dubious ownership in repository at /tmp/repro' >&2
  exit 128
fi
exit 1"#,
    );

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .env("PATH", fake_git.path())
        .args(["check", "--staged"])
        .assert()
        .failure();

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("git rev-parse failed: fatal: detected dubious ownership"));
    assert!(!stderr.contains("requires a git repository"));
}

#[cfg(unix)]
#[test]
fn check_staged_reports_spawn_error_from_git_diff() {
    assert_spawn_error_from_git_diff(&["check", "--staged"]);
}

#[cfg(unix)]
#[test]
fn check_diff_reports_spawn_error_from_git_diff() {
    assert_spawn_error_from_git_diff(&["check", "--diff", "HEAD"]);
}

#[cfg(unix)]
#[test]
fn check_staged_reports_git_diff_failure_without_stderr() {
    let temp = TempDir::new().unwrap();

    let fake_git = TempDir::new().unwrap();
    write_fake_git_script(
        &fake_git,
        r#"if [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ]; then
  printf '%s\n' "$PWD"
  exit 0
fi
if [ "$1" = "diff" ]; then
  exit 2
fi
exit 1"#,
    );

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .env("PATH", fake_git.path())
        .args(["check", "--staged"])
        .assert()
        .failure();

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("git diff failed with status"));
}
