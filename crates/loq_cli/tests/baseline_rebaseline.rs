//! Integration tests for re-baselining: combined add/update/remove operations.

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
fn adds_updates_and_removes_in_single_pass() {
    // Scenario: Start with 3 baselined files, then:
    // - file_a.txt: shrinks (should UPDATE rule)
    // - file_b.txt: stays same (no change)
    // - file_c.txt: drops below threshold (should REMOVE rule)
    // - file_d.txt: new file above threshold (should ADD rule)
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "file_a.txt"
max_lines = 700

[[rules]]
path = "file_b.txt"
max_lines = 600

[[rules]]
path = "file_c.txt"
max_lines = 550
"#;
    write_file(&temp, "loq.toml", config);

    // file_a: shrinks from 700 to 650 (still above 500)
    write_file(&temp, "file_a.txt", &repeat_lines(650));
    // file_b: stays at 600
    write_file(&temp, "file_b.txt", &repeat_lines(600));
    // file_c: drops to 400 (below 500 threshold)
    write_file(&temp, "file_c.txt", &repeat_lines(400));
    // file_d: new file at 520
    write_file(&temp, "file_d.txt", &repeat_lines(520));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 file"))
        .stdout(predicate::str::contains("updated 1 file"))
        .stdout(predicate::str::contains("removed 1 file"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();

    // file_a: updated from 700 to 650
    assert!(updated.contains("\"file_a.txt\""));
    assert!(updated.contains("max_lines = 650"));
    assert!(!updated.contains("max_lines = 700"));

    // file_b: unchanged at 600
    assert!(updated.contains("\"file_b.txt\""));
    assert!(updated.contains("max_lines = 600"));

    // file_c: removed (no longer in config)
    assert!(!updated.contains("file_c.txt"));
    assert!(!updated.contains("max_lines = 550"));

    // file_d: added at 520
    assert!(updated.contains("\"file_d.txt\""));
    assert!(updated.contains("max_lines = 520"));
}

#[test]
fn handles_deleted_files_alongside_new_violations() {
    // Start with baselined file that gets deleted, while a new file appears
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "old_file.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);
    // old_file.txt doesn't exist (was deleted)
    // new_file.txt is a new violation
    write_file(&temp, "new_file.txt", &repeat_lines(550));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 file"))
        .stdout(predicate::str::contains("removed 1 file"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(!updated.contains("old_file.txt"));
    assert!(updated.contains("\"new_file.txt\""));
    assert!(updated.contains("max_lines = 550"));
}

#[test]
fn preserves_glob_rules_and_comments() {
    // Glob rules and comments should be preserved when re-baselining
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

# Policy: all test files can be longer
[[rules]]
path = "tests/**"
max_lines = 800

# Baseline: specific legacy file
[[rules]]
path = "legacy.txt"
max_lines = 600
"#;
    write_file(&temp, "loq.toml", config);

    // legacy.txt shrinks
    write_file(&temp, "legacy.txt", &repeat_lines(550));
    // new violation
    write_file(&temp, "src/new.txt", &repeat_lines(510));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success();

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();

    // Glob rule unchanged
    assert!(updated.contains("path = \"tests/**\""));
    assert!(updated.contains("max_lines = 800"));
    assert!(updated.contains("# Policy: all test files can be longer"));

    // legacy.txt updated
    assert!(updated.contains("\"legacy.txt\""));
    assert!(updated.contains("max_lines = 550"));
    assert!(!updated.contains("max_lines = 600"));

    // new.txt added
    assert!(updated.contains("\"src/new.txt\""));
    assert!(updated.contains("max_lines = 510"));
}

#[test]
fn multiple_files_shrink_to_different_sizes() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "a.txt"
max_lines = 800

[[rules]]
path = "b.txt"
max_lines = 900

[[rules]]
path = "c.txt"
max_lines = 1000
"#;
    write_file(&temp, "loq.toml", config);

    // All files shrink
    write_file(&temp, "a.txt", &repeat_lines(550));
    write_file(&temp, "b.txt", &repeat_lines(600));
    write_file(&temp, "c.txt", &repeat_lines(650));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 3 files"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("max_lines = 550"));
    assert!(updated.contains("max_lines = 600"));
    assert!(updated.contains("max_lines = 650"));
    assert!(!updated.contains("max_lines = 800"));
    assert!(!updated.contains("max_lines = 900"));
    assert!(!updated.contains("max_lines = 1000"));
}

#[test]
fn idempotent_when_no_changes() {
    // Running baseline twice with no file changes should produce same result
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 500\n");
    write_file(&temp, "big.txt", &repeat_lines(600));

    // First baseline
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 file"));

    let after_first = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();

    // Second baseline - should be idempotent
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("✔ No changes needed"));

    let after_second = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert_eq!(after_first, after_second);
}

#[test]
fn with_nested_directories() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 500

[[rules]]
path = "src/deep/nested/file.txt"
max_lines = 700
"#;
    write_file(&temp, "loq.toml", config);

    // Nested file shrinks
    write_file(&temp, "src/deep/nested/file.txt", &repeat_lines(550));
    // New nested violation
    write_file(&temp, "lib/utils/helper.txt", &repeat_lines(520));
    // Another nested file drops below threshold
    write_file(&temp, "test/unit/small.txt", &repeat_lines(100));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success();

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(updated.contains("\"src/deep/nested/file.txt\""));
    assert!(updated.contains("max_lines = 550"));
    assert!(updated.contains("\"lib/utils/helper.txt\""));
    assert!(updated.contains("max_lines = 520"));
    // small.txt not added (below threshold)
    assert!(!updated.contains("small.txt"));
}
