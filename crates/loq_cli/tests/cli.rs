use std::process::Command;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

fn write_file(dir: &TempDir, path: &str, contents: &str) {
    let full = dir.path().join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(full, contents).unwrap();
}

fn repeat_lines(count: usize) -> String {
    "line\n".repeat(count)
}

fn run_git(dir: &TempDir, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir.path())
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

fn init_git_repo(dir: &TempDir) {
    run_git(dir, &["init"]);
    run_git(dir, &["config", "user.name", "Loq Test"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
}

#[test]
fn default_check_success() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "src/main.rs", "fn main() {}\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("files ok"));
}

#[test]
fn check_explicit_files() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "a.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("files ok"));
}

#[test]
fn check_reads_stdin_list() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "-"])
        .write_stdin("a.txt\n")
        .assert()
        .success();
}

#[test]
fn check_allows_flags_after_paths() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "a.txt", "--output-format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"summary\""));
}

#[test]
fn check_staged_intersects_with_paths() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "src/staged.txt", "ok\n");
    write_file(&temp, "docs/staged.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "src/staged.txt", "a\nb\n");
    write_file(&temp, "docs/staged.txt", "a\nb\n");
    run_git(&temp, &["add", "src/staged.txt", "docs/staged.txt"]);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "src", "--staged"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("src/"))
        .stdout(predicate::str::contains("staged.txt"))
        .stdout(predicate::str::contains("docs/").not());
}

#[test]
fn check_diff_since_ref_only_checks_changed_files() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "changed.txt", "ok\n");
    write_file(&temp, "unchanged.txt", "a\nb\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "changed.txt", "a\nb\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--diff", "HEAD"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("changed.txt"))
        .stdout(predicate::str::contains("unchanged.txt").not());
}

#[test]
fn check_staged_from_subdir_resolves_repo_relative_paths() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/changed.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "sub/changed.txt", "a\nb\n");
    run_git(&temp, &["add", "sub/changed.txt"]);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--staged"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("changed.txt"));
}

#[test]
fn check_diff_from_subdir_resolves_repo_relative_paths() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "sub/changed.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "sub/changed.txt", "a\nb\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "--diff", "HEAD"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("changed.txt"));
}

#[test]
fn check_staged_parent_dir_scope_includes_matches() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "src/changed.txt", "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, "src/changed.txt", "a\nb\n");
    run_git(&temp, &["add", "src/changed.txt"]);
    std::fs::create_dir_all(temp.path().join("sub")).unwrap();

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path().join("sub"))
        .args(["check", "../src", "--staged", "--output-format", "json"])
        .assert()
        .failure();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["violations"][0]["path"], "../src/changed.txt");
}

#[test]
fn check_staged_errors_when_not_in_git_repo() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--staged"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--staged requires a git repository (run inside a repo)",
        ));
}

#[test]
fn check_diff_errors_when_not_in_git_repo() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--diff", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--diff requires a git repository (run inside a repo)",
        ));
}

#[test]
fn check_staged_handles_leading_space_filename() {
    let temp = TempDir::new().unwrap();
    init_git_repo(&temp);

    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    let odd_path = " odd name.txt";
    write_file(&temp, odd_path, "ok\n");
    run_git(&temp, &["add", "."]);
    run_git(&temp, &["commit", "-m", "initial"]);

    write_file(&temp, odd_path, "a\nb\n");
    run_git(&temp, &["add", odd_path]);

    let assert = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "--staged", "--output-format", "json"])
        .assert()
        .failure();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["violations"][0]["path"], odd_path);
}

#[test]
fn check_staged_errors_when_git_unavailable() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .env("PATH", "")
        .args(["check", "--staged"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--staged requires git, but git is not available",
        ));
}

#[test]
fn exit_code_error_on_violation() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "big.txt", &repeat_lines(501));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "big.txt"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("✖"))
        .stdout(predicate::str::contains("501"))
        .stdout(predicate::str::contains(">"))
        .stdout(predicate::str::contains("500"));
}

#[test]
fn missing_file_warns() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--verbose", "check", "missing.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("⚠"))
        .stdout(predicate::str::contains("file not found"));
}

#[test]
fn verbose_includes_rule() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    write_file(&temp, "a.txt", "a\nb\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--verbose", "check", "a.txt"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("rule:"));
}

#[test]
fn verbose_includes_skip_warnings() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--verbose", "check", "missing.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("⚠"))
        .stdout(predicate::str::contains("file not found"));
}

#[test]
fn init_writes_config() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("default_max_lines = 500"));
}

#[test]
fn init_fails_when_exists() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 10\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn init_accepts_verbosity_flags() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--verbose", "init"])
        .assert()
        .success();

    assert!(temp.path().join("loq.toml").exists());
}

#[test]
fn init_adds_cache_to_gitignore() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "node_modules/\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    let gitignore = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".loq_cache"));
    assert!(gitignore.contains("node_modules/"));
}

#[test]
fn init_does_not_duplicate_cache_in_gitignore() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", ".loq_cache\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    let gitignore = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
    assert_eq!(gitignore.matches(".loq_cache").count(), 1);
}

#[test]
fn config_error_is_reported() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "max_line = 10\n");
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "a.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown key"));
}

#[test]
fn verbose_shows_matched_rule_pattern() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 100
[[rules]]
path = "**/*.rs"
max_lines = 1
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "main.rs", "fn main() {}\nfn other() {}\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--verbose", "check", "main.rs"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("match: **/*.rs"));
}

#[test]
fn old_v1_cache_is_migrated_to_v2() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 500\n");
    write_file(&temp, "a.txt", "hello\n");

    // Write a v1 format cache file (old format with `lines` field)
    let v1_cache = r#"{"version":1,"config_hash":123,"entries":{"a.txt":{"mtime_secs":0,"mtime_nanos":0,"lines":1}}}"#;
    write_file(&temp, ".loq_cache", v1_cache);

    // Run check - should succeed and migrate the cache
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "a.txt"])
        .assert()
        .success();

    // Verify cache was rewritten as v2
    let cache_contents = std::fs::read_to_string(temp.path().join(".loq_cache")).unwrap();
    let cache: serde_json::Value = serde_json::from_str(&cache_contents).unwrap();
    assert_eq!(cache["version"], 2, "cache should be upgraded to v2");
    // v2 format uses `result` field with enum, not `lines`
    let entries = cache["entries"].as_object().unwrap();
    assert!(!entries.is_empty(), "cache should have entries");
    for (_key, entry) in entries {
        assert!(
            entry.get("result").is_some(),
            "v2 cache entries should have 'result' field"
        );
        assert!(
            entry.get("lines").is_none(),
            "v2 cache entries should not have 'lines' field"
        );
    }
}
