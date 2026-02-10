//! Integration tests for escaped exact paths in baseline/relax flows.

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

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn assert_baseline_then_relax_updates_same_rule(path: &str, escaped_path: &str) {
    let temp = TempDir::new().unwrap();
    let escaped_path_rule = format!("path = \"{escaped_path}\"");

    write_file(&temp, path, &repeat_lines(550));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 file"));

    let after_baseline = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(after_baseline.contains(&escaped_path_rule));
    assert_eq!(count_occurrences(&after_baseline, &escaped_path_rule), 1);
    assert!(after_baseline.contains("max_lines = 550"));

    write_file(&temp, path, &repeat_lines(560));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["relax", "--extra", "5", path])
        .assert()
        .success()
        .stdout(predicate::str::contains("Relaxed limits for 1 file"));

    let after_relax = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert_eq!(count_occurrences(&after_relax, &escaped_path_rule), 1);
    assert!(after_relax.contains("max_lines = 565"));
}

#[test]
fn baseline_updates_existing_escaped_rule_without_duplicates() {
    let temp = TempDir::new().unwrap();
    let path = "routes/[id]/page.svelte";
    let escaped_path = "routes/[[]id[]]/page.svelte";

    write_file(&temp, path, &repeat_lines(550));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1 file"));

    let after_first = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    let escaped_path_rule = format!("path = \"{escaped_path}\"");
    assert!(after_first.contains(&escaped_path_rule));
    assert_eq!(count_occurrences(&after_first, &escaped_path_rule), 1);
    assert!(after_first.contains("max_lines = 550"));

    write_file(&temp, path, &repeat_lines(575));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["baseline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated 1 file"));

    let after_second = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert_eq!(count_occurrences(&after_second, &escaped_path_rule), 1);
    assert!(after_second.contains("max_lines = 575"));
}

#[test]
fn relax_matches_existing_escaped_rule_and_updates_it() {
    let temp = TempDir::new().unwrap();
    let path = "routes/[id]/page.svelte";
    let escaped_path = "routes/[[]id[]]/page.svelte";

    let config = format!(
        "default_max_lines = 500\n\n[[rules]]\npath = \"{escaped_path}\"\nmax_lines = 550\n"
    );
    write_file(&temp, "loq.toml", &config);
    write_file(&temp, path, &repeat_lines(560));

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["relax", "--extra", "10", path])
        .assert()
        .success()
        .stdout(predicate::str::contains("Relaxed limits for 1 file"));

    let updated = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    let escaped_path_rule = format!("path = \"{escaped_path}\"");
    assert_eq!(count_occurrences(&updated, &escaped_path_rule), 1);
    assert!(updated.contains("max_lines = 570"));
}

#[test]
fn baseline_and_relax_roundtrip_with_literal_right_bracket_path() {
    assert_baseline_then_relax_updates_same_rule(
        "routes/id]/page.svelte",
        "routes/id[]]/page.svelte",
    );
}

#[test]
fn baseline_and_relax_roundtrip_with_literal_braces_path() {
    assert_baseline_then_relax_updates_same_rule(
        "routes/{slug}/page.svelte",
        "routes/[{]slug[}]/page.svelte",
    );
}
