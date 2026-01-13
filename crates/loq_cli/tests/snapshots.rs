use assert_cmd::cargo::cargo_bin_cmd;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Strip ANSI escape codes from output
fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

/// Normalize output for snapshot comparison:
/// - Strip ANSI codes
/// - Replace timing with placeholder
fn normalize(s: &str) -> String {
    let s = strip_ansi(s);
    let re = regex::Regex::new(r"\(\d+m?s\)").unwrap();
    re.replace_all(&s, "(TIME)").to_string()
}

fn run_loq(fixture: &str) -> (String, String, bool) {
    let output = cargo_bin_cmd!("loq")
        .current_dir(fixture_path(fixture))
        .output()
        .unwrap();

    let stdout = normalize(&String::from_utf8_lossy(&output.stdout));
    let stderr = normalize(&String::from_utf8_lossy(&output.stderr));
    (stdout, stderr, output.status.success())
}

fn run_loq_with_args(fixture: &str, args: &[&str]) -> (String, String, bool) {
    let output = cargo_bin_cmd!("loq")
        .current_dir(fixture_path(fixture))
        .args(args)
        .output()
        .unwrap();

    let stdout = normalize(&String::from_utf8_lossy(&output.stdout));
    let stderr = normalize(&String::from_utf8_lossy(&output.stderr));
    (stdout, stderr, output.status.success())
}

fn run_loq_with_stdin(fixture: &str, args: &[&str], stdin: &str) -> (String, String, bool) {
    let output = cargo_bin_cmd!("loq")
        .current_dir(fixture_path(fixture))
        .args(args)
        .write_stdin(stdin)
        .output()
        .unwrap();

    let stdout = normalize(&String::from_utf8_lossy(&output.stdout));
    let stderr = normalize(&String::from_utf8_lossy(&output.stderr));
    (stdout, stderr, output.status.success())
}

#[test]
fn empty_directory() {
    let (stdout, stderr, success) = run_loq("empty_directory");

    assert!(success, "should succeed with no files");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn all_ok() {
    let (stdout, stderr, success) = run_loq("all_ok");

    assert!(success, "should succeed when all files are ok");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn one_violation() {
    let (stdout, stderr, success) = run_loq("one_violation");

    assert!(!success, "should fail with violation");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn pass_and_fail() {
    let (stdout, stderr, success) = run_loq("pass_and_fail");

    assert!(!success, "should fail when there's an error");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn no_config_uses_defaults() {
    let (stdout, stderr, success) = run_loq("no_config");

    assert!(success, "should succeed with defaults");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn multiple_rules() {
    let (stdout, stderr, success) = run_loq("multiple_rules");

    assert!(!success, "should fail when rule is violated");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn baselined_file_passes() {
    let (stdout, stderr, success) = run_loq("baselined");

    assert!(success, "baselined file should pass");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn nested_directories() {
    let (stdout, stderr, success) = run_loq("nested");

    assert!(!success, "should find violation in nested dir");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn gitignore_respected() {
    let (stdout, stderr, success) = run_loq("gitignore");

    assert!(success, "ignored files should not cause failure");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn binary_file_skipped() {
    let (stdout, stderr, success) = run_loq("binary_file");

    assert!(success, "binary files should be skipped");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn missing_file_explicit_arg() {
    let (stdout, stderr, success) = run_loq_with_args("all_ok", &["check", "nonexistent.rs"]);

    assert!(success, "missing file is a warning, not error");
    insta::assert_snapshot!("missing_file_stdout", stdout);
    insta::assert_snapshot!("missing_file_stderr", stderr);
}

#[test]
fn stdin_file_list() {
    let (stdout, stderr, success) = run_loq_with_stdin("stdin", &["check", "-"], "a.rs\nb.rs\n");

    assert!(success, "stdin files should be checked");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}

#[test]
fn fix_guidance_shown_on_violation() {
    let (stdout, stderr, success) = run_loq("fix_guidance");

    assert!(!success, "should fail with violation");
    assert!(stderr.is_empty(), "stderr should be empty");
    insta::assert_snapshot!(stdout);
}
