//! Integration tests for the relax command.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use regex::Regex;
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

fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

fn parse_relax_line(line: &str) -> Option<(usize, usize, &str)> {
    let mut parts = line.split_whitespace();
    let actual = parts.next()?;
    let arrow = parts.next()?;
    let limit = parts.next()?;
    let path = parts.next()?;
    if arrow != "->" {
        return None;
    }
    let actual = actual.replace('_', "").parse().ok()?;
    let limit = limit.replace('_', "").parse().ok()?;
    Some((actual, limit, path))
}

#[test]
fn creates_config_and_rule_when_missing() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "src/legacy.rs", &repeat_lines(523));

    let output = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["relax"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("Relaxed limits for 1 file"));
    assert!(stdout.lines().any(|line| {
        line.contains("src/legacy.rs")
            && line.contains("523")
            && line.contains("->")
            && line.contains("623")
    }));

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

    let output = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["relax", "--buffer", "50", "src/legacy.rs"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.lines().any(|line| {
        line.contains("src/legacy.rs")
            && line.contains("650")
            && line.contains("->")
            && line.contains("700")
    }));

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
        .args(["relax"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Relaxed limits for 1 file"));

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("path = \"**/*.rs\""));
    assert!(content.contains("max_lines = 700"));
    assert!(content.contains("path = \"src/big.rs\""));
    assert!(content.contains("max_lines = 850"));
}

#[test]
fn exits_zero_when_no_violations() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "src/small.rs", &repeat_lines(10));

    let output = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["relax"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("No violations to relax"));
}

#[test]
fn orders_paths_and_applies_buffer_for_multiple_files() {
    let temp = TempDir::new().unwrap();
    let config = "default_max_lines = 10\nexclude = []\n";
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "src/b.rs", &repeat_lines(12));
    write_file(&temp, "src/a.rs", &repeat_lines(15));

    let output = cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["relax", "--buffer", "3"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let mut lines = stdout.lines().filter(|line| line.contains("->"));
    let first = lines.next().and_then(parse_relax_line).unwrap();
    let second = lines.next().and_then(parse_relax_line).unwrap();
    assert_eq!(first, (12, 15, "src/b.rs"));
    assert_eq!(second, (15, 18, "src/a.rs"));

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("path = \"src/a.rs\""));
    assert!(content.contains("max_lines = 18"));
    assert!(content.contains("path = \"src/b.rs\""));
    assert!(content.contains("max_lines = 15"));
}
