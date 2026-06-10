//! A global `default_max_tokens` applies to files matching no rule.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn default_max_tokens_flags_unmatched_file() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("loq.toml"), "default_max_tokens = 4\n").unwrap();
    // 20 bytes -> ceil(20 / 4) = 5 tokens, over the budget of 4.
    std::fs::write(temp.path().join("big.txt"), "abcdefghijklmnopqrs\n").unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .arg("check")
        .assert()
        .failure()
        .stdout(predicate::str::contains("tokens"));
}

#[test]
fn both_default_budgets_is_an_error() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("loq.toml"),
        "default_max_lines = 100\ndefault_max_tokens = 4\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("a.txt"), "hi\n").unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .arg("check")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "only one of default_max_lines or default_max_tokens",
        ));
}
