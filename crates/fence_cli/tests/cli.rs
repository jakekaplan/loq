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

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("All checks passed!"));
}

#[test]
fn check_explicit_files() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["check", "a.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("All checks passed!"));
}

#[test]
fn check_reads_stdin_list() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["check", "-"])
        .write_stdin("a.txt\n")
        .assert()
        .success();
}

#[test]
fn exit_code_error_on_violation() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(401);
    write_file(&temp, "big.txt", &contents);

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["check", "big.txt"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error[max-lines]"));
}

#[test]
fn quiet_suppresses_warnings_and_summary() {
    let temp = TempDir::new().unwrap();
    let config = r#"default_max_lines = 400
[[rules]]
path = "**/*.txt"
max_lines = 1
severity = "warning"
"#;
    write_file(&temp, ".fence.toml", config);
    write_file(&temp, "warn.txt", "a\nb\n");

    let output = cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["--quiet", "check", "warn.txt"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn silent_prints_nothing() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(401);
    write_file(&temp, "big.txt", &contents);

    let output = cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["--silent", "check", "big.txt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_file_warns() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["check", "missing.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skipped"));
}

#[test]
fn verbose_includes_config_and_rule() {
    let temp = TempDir::new().unwrap();
    let config = "default_max_lines = 1\n";
    write_file(&temp, ".fence.toml", config);
    write_file(&temp, "a.txt", "a\nb\n");

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["--verbose", "check", "a.txt"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("verbose: config"))
        .stdout(predicate::str::contains("verbose: rule"));
}

#[test]
fn verbose_includes_skip_warnings() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["--verbose", "check", "missing.txt"])
        .assert()
        .success()
        .stdout(predicate::str::contains("warning[skip-missing]"));
}

#[test]
fn init_writes_config() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join(".fence.toml")).unwrap();
    assert!(content.contains("default_max_lines = 400"));
}

#[test]
fn init_fails_when_exists() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".fence.toml", "default_max_lines = 10\n");

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn init_rejects_flags() {
    let temp = TempDir::new().unwrap();

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["--quiet", "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "init does not accept output or config flags",
        ));
}

#[test]
fn init_baseline_adds_exempt() {
    let temp = TempDir::new().unwrap();
    let contents = repeat_lines(401);
    write_file(&temp, "src/legacy.txt", &contents);

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["init", "--baseline"])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join(".fence.toml")).unwrap();
    assert!(content.contains("\"src/legacy.txt\""));
}

#[test]
fn config_error_is_reported() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "bad.toml", "max_line = 10\n");
    write_file(&temp, "a.txt", "a\n");

    cargo_bin_cmd!("fence")
        .current_dir(temp.path())
        .args(["--config", "bad.toml", "check", "a.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown key"));
}
