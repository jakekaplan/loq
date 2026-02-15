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
