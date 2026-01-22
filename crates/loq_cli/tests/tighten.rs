//! Integration tests for the tighten command.

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
fn tightens_when_file_shrinks() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "big.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "big.txt", &repeat_lines(550));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["tighten"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("max_lines = 550"));
    assert!(!updated.contains("max_lines = 600"));
}

#[test]
fn does_not_increase_when_file_grows() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "grown.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "grown.txt", &repeat_lines(650));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["tighten"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes needed"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("max_lines = 600"));
    assert!(!updated.contains("max_lines = 650"));
}

#[test]
fn removes_rule_when_file_compliant() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "small.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "small.txt", &repeat_lines(400));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["tighten"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(!updated.contains("small.txt"));
}

#[test]
fn does_not_add_rules_for_new_violations() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 500\n");
    write_file(&temp, "new.txt", &repeat_lines(550));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["tighten"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes needed"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(!updated.contains("path = \"new.txt\""));
}

#[test]
fn creates_config_when_missing() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "target\n");
    write_file(&temp, "legacy.txt", &repeat_lines(520));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["tighten"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes needed"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("default_max_lines = 500"));
    assert!(!config.contains("legacy.txt"));

    let gitignore = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".loq_cache"));
}

#[test]
fn fails_on_invalid_config() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines =\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["tighten"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to parse"));
}
