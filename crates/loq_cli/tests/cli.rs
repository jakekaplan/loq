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
    let mut out = String::new();
    for _ in 0..count {
        out.push_str("line\n");
    }
    out
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
fn exit_code_error_on_violation() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(501);
    write_file(&temp, "big.txt", &contents);

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

    // Missing files are only shown in verbose mode
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
    let config = "default_max_lines = 1\n";
    write_file(&temp, "loq.toml", config);
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
    assert!(
        gitignore.contains(".loq_cache"),
        "should add .loq_cache to .gitignore"
    );
    assert!(
        gitignore.contains("node_modules/"),
        "should preserve existing entries"
    );
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
    assert_eq!(
        gitignore.matches(".loq_cache").count(),
        1,
        "should not duplicate .loq_cache"
    );
}

#[test]
fn init_baseline_locks_at_current_size() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(501);
    write_file(&temp, "src/legacy.txt", &contents);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init", "--baseline"])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    // Should have the file in a baseline rule
    assert!(content.contains("\"src/legacy.txt\""));
    // Should be locked at exact line count (501 lines)
    assert!(content.contains("max_lines = 501"));
    assert!(content.contains("# Baseline:"));
    // Should NOT have warning severity (error is default)
    assert!(!content.contains("severity = \"warning\"\nmax_lines = 501"));
}

#[test]
fn baseline_rules_are_respected_after_init() {
    let temp = TempDir::new().unwrap();
    // Create file with 501 lines (over default 500 limit)
    let contents = repeat_lines(501);
    write_file(&temp, "legacy.txt", &contents);

    // Step 1: Run init --baseline to create config with baseline rule
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["init", "--baseline"])
        .assert()
        .success();

    // Verify baseline rule was created
    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(
        config.contains("max_lines = 501"),
        "baseline should lock at 501"
    );

    // Step 2: Run loq - should PASS because baseline rule matches exactly
    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .assert()
        .success();

    // Step 3: Add one more line - now it should FAIL (502 > 501)
    let over_baseline = repeat_lines(502);
    write_file(&temp, "legacy.txt", &over_baseline);

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .assert()
        .failure();
}

#[test]
fn config_error_is_reported() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "bad.toml", "max_line = 10\n");
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--config", "bad.toml", "check", "a.txt"])
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
fn warning_severity_shows_yellow() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 100
[[rules]]
path = "**/*.txt"
max_lines = 1
severity = "warning"
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "warn.txt", "a\nb\nc\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["check", "warn.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("⚠"))
        .stdout(predicate::str::contains("3"))
        .stdout(predicate::str::contains(">"));
}

#[test]
fn verbose_shows_warning_severity_label() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 100
[[rules]]
path = "**/*.txt"
max_lines = 1
severity = "warning"
"#;
    write_file(&temp, "loq.toml", config);
    write_file(&temp, "warn.txt", "a\nb\nc\n");

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .args(["--verbose", "check", "warn.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("severity=warning"));
}
