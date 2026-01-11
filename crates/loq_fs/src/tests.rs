use super::*;
use tempfile::TempDir;

fn write_file(dir: &TempDir, path: &str, contents: &str) -> PathBuf {
    let full = dir.path().join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&full, contents).unwrap();
    full
}

#[test]
fn excluded_files_are_filtered_out() {
    let temp = TempDir::new().unwrap();
    write_file(
        &temp,
        "loq.toml",
        "default_max_lines = 1\nexclude = [\"**/*.txt\"]\n",
    );
    let file = write_file(&temp, "a.txt", "a\nb\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    // Excluded files are silently filtered out - no outcome at all
    assert!(output.outcomes.is_empty());
}

#[test]
fn no_default_skips_files() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "");
    let file = write_file(&temp, "a.txt", "a\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    assert!(matches!(output.outcomes[0].kind, OutcomeKind::NoLimit));
}

#[test]
fn missing_files_reported() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 1\n");
    let missing = temp.path().join("missing.txt");

    let output = run_check(
        vec![missing],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    assert!(matches!(output.outcomes[0].kind, OutcomeKind::Missing));
}

#[test]
fn binary_and_unreadable_are_reported() {
    let temp = TempDir::new().unwrap();
    let config = loq_core::config::LoqConfig {
        default_max_lines: Some(1),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![],
    };
    let compiled = loq_core::config::compile_config(
        loq_core::config::ConfigOrigin::BuiltIn,
        temp.path().to_path_buf(),
        config,
        None,
    )
    .unwrap();

    let binary = temp.path().join("binary.txt");
    std::fs::write(&binary, b"\0binary").unwrap();
    let binary_outcome = check_file(&binary, &compiled, temp.path());
    assert!(matches!(binary_outcome.kind, OutcomeKind::Binary));

    let dir_outcome = check_file(temp.path(), &compiled, temp.path());
    assert!(matches!(dir_outcome.kind, OutcomeKind::Unreadable { .. }));
}

#[test]
fn gitignore_is_respected_by_default() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "ignored.txt\n");
    let file = write_file(&temp, "ignored.txt", "a\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: None,
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    // Gitignored files are silently filtered out - no outcome at all
    assert!(output.outcomes.is_empty());
}

#[test]
fn gitignore_can_be_disabled() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "ignored.txt\n");
    write_file(
        &temp,
        "loq.toml",
        "default_max_lines = 10\nrespect_gitignore = false\n",
    );
    let file = write_file(&temp, "ignored.txt", "a\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    assert!(matches!(output.outcomes[0].kind, OutcomeKind::Pass { .. }));
}

#[test]
fn exactly_at_limit_passes() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 3\n");
    // Exactly 3 lines
    let file = write_file(&temp, "exact.txt", "one\ntwo\nthree\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    match &output.outcomes[0].kind {
        OutcomeKind::Pass { limit, actual, .. } => {
            assert_eq!(*limit, 3);
            assert_eq!(*actual, 3);
        }
        other => panic!("expected Pass, got {other:?}"),
    }
}

#[test]
fn one_over_limit_violates() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "default_max_lines = 3\n");
    // 4 lines - one over
    let file = write_file(&temp, "over.txt", "one\ntwo\nthree\nfour\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    match &output.outcomes[0].kind {
        OutcomeKind::Violation { limit, actual, .. } => {
            assert_eq!(*limit, 3);
            assert_eq!(*actual, 4);
        }
        other => panic!("expected Violation, got {other:?}"),
    }
}

#[test]
fn gitignore_negation_pattern_whitelists_file() {
    let temp = TempDir::new().unwrap();
    // Ignore all .log files, but whitelist important.log
    write_file(&temp, ".gitignore", "*.log\n!important.log\n");

    let ignored = write_file(&temp, "debug.log", "ignored\n");
    let whitelisted = write_file(&temp, "important.log", "not ignored\n");

    let output = run_check(
        vec![ignored.clone(), whitelisted.clone()],
        CheckOptions {
            config_path: None,
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    // debug.log should be filtered out (not in outcomes)
    let outcome_ignored = output.outcomes.iter().find(|o| o.path == ignored);
    assert!(
        outcome_ignored.is_none(),
        "debug.log should be filtered out, but found {outcome_ignored:?}"
    );

    // important.log should NOT be excluded (whitelisted by negation pattern)
    let outcome_whitelisted = output
        .outcomes
        .iter()
        .find(|o| o.path == whitelisted)
        .unwrap();
    assert!(
        matches!(outcome_whitelisted.kind, OutcomeKind::Pass { .. }),
        "important.log should pass (whitelisted), got {:?}",
        outcome_whitelisted.kind
    );
}

#[test]
fn missing_config_file_returns_error() {
    let temp = TempDir::new().unwrap();
    let file = write_file(&temp, "test.txt", "content\n");

    let result = run_check(
        vec![file],
        CheckOptions {
            config_path: Some(temp.path().join("nonexistent.toml")),
            cwd: temp.path().to_path_buf(),
        },
    );

    match result {
        Err(FsError::ConfigRead { .. }) => {}
        Err(other) => panic!("expected ConfigRead error, got {other}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}
