//! Integration tests for the baseline command - core functionality.

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
fn adds_rules_for_violations() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(501);
    write_file(&temp, "src/legacy.txt", &contents);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(content.contains("\"src/legacy.txt\""));
    assert!(content.contains("max_lines = 501"));
}

#[test]
fn rules_are_respected_after_baseline() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(501);
    write_file(&temp, "legacy.txt", &contents);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success();

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 501"));

    // Should pass - rule matches exactly
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .assert()
        .success();

    // Add one more line - should fail (502 > 501)
    write_file(&temp, "legacy.txt", &repeat_lines(502));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .assert()
        .failure();
}

#[test]
fn creates_config_when_missing() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "target\n");
    write_file(&temp, "file.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("default_max_lines = 500"));
    assert!(config.contains("\"file.txt\""));
    assert!(config.contains("max_lines = 600"));

    let gitignore = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".loq_cache"));
}

#[test]
fn updates_rule_if_file_shrunk() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "big.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success();

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 600"));

    // Shrink file
    write_file(&temp, "big.txt", &repeat_lines(550));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 1 rule"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 550"));
}

#[test]
fn removes_rule_if_file_compliant() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "file.txt", &repeat_lines(501));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success();

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 501"));

    // Shrink below threshold
    write_file(&temp, "file.txt", &repeat_lines(400));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 rule"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(!config.contains("max_lines = 501"));
    assert!(!config.contains("file.txt"));
}

#[test]
fn threshold_flag() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "file.txt", &repeat_lines(350));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline", "--threshold", "300"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 350"));
}

#[test]
fn preserves_glob_rules() {
    // Glob rules should be preserved when baseline adds new rules
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

# Important policy rule
[[rules]]
path = "**/*.rs"
max_lines = 1000
"#;
    write_file(&temp, "loq.toml", config);
    // .txt file not covered by glob rule, should be baselined
    write_file(&temp, "src/big.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    // Glob rule preserved
    assert!(updated.contains("path = \"**/*.rs\""));
    assert!(updated.contains("max_lines = 1000"));
    assert!(updated.contains("# Important policy rule"));
    // New baseline rule added
    assert!(updated.contains("src/big.txt"));
    assert!(updated.contains("max_lines = 600"));
}

#[test]
fn no_changes_when_all_compliant() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    write_file(&temp, "small.txt", "just a few lines\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes needed"));
}

#[test]
fn handles_multiple_violations() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    write_file(&temp, "a.txt", &repeat_lines(501));
    write_file(&temp, "b.txt", &repeat_lines(600));
    write_file(&temp, "c.txt", &repeat_lines(700));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 3 rules"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 501"));
    assert!(config.contains("max_lines = 600"));
    assert!(config.contains("max_lines = 700"));
}

#[test]
fn updates_rule_if_file_grew() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "big.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "big.txt", &repeat_lines(650));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("max_lines = 650"));
    assert!(!updated.contains("max_lines = 600"));
}

#[test]
fn respects_exclude_patterns() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500
exclude = ["generated/**"]
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "generated/big.txt", &repeat_lines(600));
    write_file(&temp, "src/big.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("src/big.txt"));
    assert!(!updated.contains("generated/big.txt"));
}

#[test]
fn respects_gitignore() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 500\n");
    write_file(&temp, ".gitignore", "build/\n");
    write_file(&temp, "build/output.txt", &repeat_lines(600));
    write_file(&temp, "src/main.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("src/main.txt"));
    assert!(!updated.contains("build/output.txt"));
}

#[test]
fn can_disable_gitignore() {
    let temp = TempDir::new().unwrap();
    write_file(
        &temp,
        "loq.toml",
        "default_max_lines = 500\nrespect_gitignore = false\n",
    );
    write_file(&temp, ".gitignore", "build/\n");
    write_file(&temp, "build/output.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("build/output.txt"));
}

#[test]
fn removes_rule_for_deleted_file() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "deleted.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(!updated.contains("deleted.txt"));
}

#[test]
fn respects_multiple_exclude_patterns() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500
exclude = ["generated/**", "vendor/**", "*.gen.rs"]
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "generated/code.txt", &repeat_lines(600));
    write_file(&temp, "vendor/lib.txt", &repeat_lines(600));
    write_file(&temp, "foo.gen.rs", &repeat_lines(600));
    write_file(&temp, "src/main.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("src/main.txt"));
    assert!(!updated.contains("generated/code.txt"));
    assert!(!updated.contains("vendor/lib.txt"));
    assert!(!updated.contains("foo.gen.rs"));
}

#[test]
fn skips_files_covered_by_glob_rules() {
    // Files that pass their effective limit (from glob rules) should NOT be baselined
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

# Policy: test files can be longer
[[rules]]
path = "tests/**"
max_lines = 800
"#;
    write_file(&temp, "loq.toml", config);

    // test file at 750 lines - above default (500) but below glob rule (800)
    write_file(&temp, "tests/big_test.txt", &repeat_lines(750));
    // src file at 600 - above default, should be baselined
    write_file(&temp, "src/main.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();

    // src/main.txt should be baselined (no glob rule covers it)
    assert!(updated.contains("\"src/main.txt\""));
    assert!(updated.contains("max_lines = 600"));

    // tests/big_test.txt should NOT be baselined (covered by glob rule, passes)
    assert!(!updated.contains("big_test.txt"));
}
