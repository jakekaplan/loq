//! Integration tests for the --allow-growth flag.

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
fn updates_grown_files() {
    // Without --allow-growth, grown files are left unchanged
    // With --allow-growth, grown files get updated limits
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "grown.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    // File grew from 600 to 700
    write_file(&temp, "grown.txt", &repeat_lines(700));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline", "--allow-growth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("max_lines = 700"));
    assert!(!updated.contains("max_lines = 600"));
}

#[test]
fn handles_mixed_changes() {
    // Scenario with --allow-growth:
    // - file_a: shrinks (always updated)
    // - file_b: grows (updated because of flag)
    // - file_c: unchanged (no update needed)
    // - file_d: drops below threshold (removed)
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "file_a.txt"
max_lines = 800

[[rules]]
path = "file_b.txt"
max_lines = 600

[[rules]]
path = "file_c.txt"
max_lines = 700

[[rules]]
path = "file_d.txt"
max_lines = 550
"#;
    write_file(&temp, "loq.toml", config);

    // file_a shrinks
    write_file(&temp, "file_a.txt", &repeat_lines(650));
    // file_b grows
    write_file(&temp, "file_b.txt", &repeat_lines(750));
    // file_c stays at 700
    write_file(&temp, "file_c.txt", &repeat_lines(700));
    // file_d drops below threshold
    write_file(&temp, "file_d.txt", &repeat_lines(400));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline", "--allow-growth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 2 rules"))
        .stdout(predicate::str::contains("removed 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();

    // file_a: shrunk from 800 to 650
    assert!(updated.contains("max_lines = 650"));
    assert!(!updated.contains("max_lines = 800"));

    // file_b: grew from 600 to 750
    assert!(updated.contains("max_lines = 750"));

    // file_c: unchanged at 700
    assert!(updated.contains("max_lines = 700"));

    // file_d: removed
    assert!(!updated.contains("file_d.txt"));
    assert!(!updated.contains("max_lines = 550"));
}

#[test]
fn without_flag_grown_files_stay_unchanged() {
    // Verify that without the flag, grown files are NOT updated
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "grown.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    // File grew from 600 to 700
    write_file(&temp, "grown.txt", &repeat_lines(700));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"]) // No --allow-growth
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes needed"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    // Limit should still be 600, not 700
    assert!(updated.contains("max_lines = 600"));
    assert!(!updated.contains("max_lines = 700"));
}

#[test]
fn combined_with_threshold() {
    // --allow-growth works with --threshold
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "file.txt"
max_lines = 400
"#;
    write_file(&temp, "loq.toml", config);
    // File at 450 - above the old rule (400) but below default (500)
    write_file(&temp, "file.txt", &repeat_lines(450));

    // With --threshold 300, file is a violation (450 > 300)
    // With --allow-growth, the limit should update from 400 to 450
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline", "--threshold", "300", "--allow-growth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 1 rule"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("max_lines = 450"));
    assert!(!updated.contains("max_lines = 400"));
}

#[test]
fn only_affects_exact_path_rules() {
    // Glob rules should never be updated, even with --allow-growth
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

# Policy rule (glob)
[[rules]]
path = "**/*.txt"
max_lines = 300

# Baseline rule (exact)
[[rules]]
path = "specific.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    // specific.txt grew
    write_file(&temp, "specific.txt", &repeat_lines(700));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline", "--allow-growth"])
        .assert()
        .success();

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();

    // Glob rule should be unchanged
    assert!(updated.contains("path = \"**/*.txt\""));
    assert!(updated.contains("max_lines = 300"));

    // Exact path rule should be updated
    assert!(updated.contains("max_lines = 700"));
    assert!(!updated.contains("max_lines = 600"));
}

#[test]
fn preserves_comments_and_formatting() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

# Legacy monolith - needs refactoring
[[rules]]
path = "monolith.txt"
max_lines = 800
"#;
    write_file(&temp, "loq.toml", config);
    // File grew to 900
    write_file(&temp, "monolith.txt", &repeat_lines(900));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline", "--allow-growth"])
        .assert()
        .success();

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    // Comment should be preserved
    assert!(updated.contains("# Legacy monolith - needs refactoring"));
    // Value should be updated
    assert!(updated.contains("max_lines = 900"));
}
