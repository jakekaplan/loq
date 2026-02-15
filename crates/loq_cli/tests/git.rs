use std::path::Path;
use std::process::Command;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn repeat_lines(count: usize) -> String {
    "line\n".repeat(count)
}

struct TempGitRepo {
    dir: TempDir,
}

impl TempGitRepo {
    fn new() -> Self {
        let repo = Self {
            dir: TempDir::new().unwrap(),
        };
        repo.git(&["init"]);
        repo.git(&["config", "user.name", "Loq Test"]);
        repo.git(&["config", "user.email", "loq@example.com"]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write_file(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(self.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", message]);
    }
}

#[test]
fn staged_checks_only_staged_files() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");
    repo.write_file("staged.rs", &repeat_lines(12));
    repo.write_file("unstaged.rs", &repeat_lines(12));
    repo.git(&["add", "staged.rs"]);

    cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "--staged"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("staged.rs"))
        .stdout(predicate::str::contains("unstaged.rs").not());
}

#[test]
fn staged_with_no_staged_files_checks_zero_files() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");
    repo.write_file("a.rs", "fn main() {}\n");

    cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "--staged"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 files ok"));
}

#[test]
fn diff_checks_only_changed_files() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");
    repo.write_file("changed.rs", "fn a() {}\n");
    repo.write_file("untouched.rs", &repeat_lines(12));
    repo.commit_all("initial");

    repo.write_file("changed.rs", &repeat_lines(12));

    cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "--diff", "HEAD"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("changed.rs"))
        .stdout(predicate::str::contains("untouched.rs").not());
}

#[test]
fn staged_respects_path_intersection() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");
    repo.write_file("src/over.rs", &repeat_lines(12));
    repo.write_file("lib/over.rs", &repeat_lines(12));
    repo.git(&["add", "src/over.rs", "lib/over.rs"]);

    cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "src", "--staged"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("src/"))
        .stdout(predicate::str::contains("lib/").not());
}

#[test]
fn diff_invalid_ref_is_reported() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");

    cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "--diff", "does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("git failed:"));
}

#[test]
fn staged_outside_repository_reports_error() {
    let dir = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(dir.path())
        .args(["check", "--staged"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--staged requires a git repository",
        ));
}

#[test]
fn staged_cannot_be_combined_with_stdin_path_list() {
    let dir = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(dir.path())
        .args(["check", "-", "--staged"])
        .write_stdin("a.rs\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot combine '-'"));
}

#[test]
fn json_output_includes_filter_metadata() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");
    repo.write_file("staged.rs", "fn main() {}\n");
    repo.git(&["add", "staged.rs"]);

    let output = cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "--staged", "--output-format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["filter"]["type"], "staged");
}

#[test]
fn staged_handles_non_ascii_paths() {
    let repo = TempGitRepo::new();
    repo.write_file("loq.toml", "default_max_lines = 10\n");
    repo.write_file("café.rs", "fn main() {}\n");
    repo.git(&["add", "café.rs"]);

    let output = cargo_bin_cmd!("loq")
        .current_dir(repo.path())
        .args(["check", "--staged", "--output-format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["summary"]["skipped"], 0);
    assert_eq!(parsed["summary"]["passed"], 1);
    assert_eq!(parsed["summary"]["files_checked"], 1);
}
