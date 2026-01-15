//! Integration tests for the accept-defeat command.

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

#[test]
fn creates_config_and_rule_when_missing() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "src/legacy.rs", &repeat_lines(523));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["accept-defeat"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Accepted defeat on 1 file"))
        .stdout(predicate::str::contains(
            "src/legacy.rs: 523 lines -> limit 623",
        ));

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("default_max_lines = 500"));
    assert!(content.contains("path = \"src/legacy.rs\""));
    assert!(content.contains("max_lines = 623"));
}

#[test]
fn updates_existing_exact_rule() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "src/legacy.rs"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "src/legacy.rs", &repeat_lines(650));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["accept-defeat", "--buffer", "50", "src/legacy.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "src/legacy.rs: 650 lines -> limit 700",
        ));

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("max_lines = 700"));
}

#[test]
fn adds_exact_override_for_glob() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "**/*.rs"
max_lines = 700
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "src/big.rs", &repeat_lines(750));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["accept-defeat"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Accepted defeat on 1 file"));

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("path = \"**/*.rs\""));
    assert!(content.contains("max_lines = 700"));
    assert!(content.contains("path = \"src/big.rs\""));
    assert!(content.contains("max_lines = 850"));
}

#[test]
fn exits_one_when_no_violations() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "src/small.rs", &repeat_lines(10));

    let output = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["accept-defeat"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No violations to accept"));
}
