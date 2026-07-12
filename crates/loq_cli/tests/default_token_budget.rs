use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn default_max_tokens_flags_unmatched_file() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("loq.toml"), "default_max_tokens = 4\n").unwrap();
    std::fs::write(temp.path().join("big.txt"), "abcdefghijklmnopqrs\n").unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .arg("check")
        .assert()
        .failure()
        .stdout(predicate::str::contains("tokens"));
}

#[test]
fn baseline_does_not_shadow_a_token_default_with_a_line_rule() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("loq.toml"), "default_max_tokens = 4\n").unwrap();
    // Also exceeds the fallback 500-line baseline threshold.
    std::fs::write(temp.path().join("big.txt"), "line\n".repeat(600)).unwrap();

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .arg("baseline")
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes needed"));

    let config = std::fs::read_to_string(temp.path().join("loq.toml")).unwrap();
    assert!(
        !config.contains("max_lines"),
        "baseline added a max_lines rule that shadows the token budget: {config}"
    );

    cargo_bin_cmd!("loq")
        .current_dir(temp.path())
        .arg("check")
        .assert()
        .failure()
        .stdout(predicate::str::contains("tokens"));
}

#[test]
fn all_commands_reject_both_default_budgets() {
    let temp = TempDir::new().unwrap();
    std::fs::write(
        temp.path().join("loq.toml"),
        "default_max_lines = 100\ndefault_max_tokens = 4\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("a.txt"), "hi\n").unwrap();

    for command in ["check", "baseline", "tighten", "relax"] {
        cargo_bin_cmd!("loq")
            .current_dir(temp.path())
            .arg(command)
            .assert()
            .code(2)
            .stderr(predicate::str::contains(
                "only one of default_max_lines or default_max_tokens",
            ));
    }
}
