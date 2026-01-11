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
fn excluded_files_are_skipped() {
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

    assert!(matches!(
        output.outcomes[0].kind,
        OutcomeKind::Excluded { .. }
    ));
}

#[test]
fn exempt_files_are_skipped() {
    let temp = TempDir::new().unwrap();
    write_file(
        &temp,
        "loq.toml",
        "default_max_lines = 1\nexempt = [\"a.txt\"]\n",
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

    assert!(matches!(
        output.outcomes[0].kind,
        OutcomeKind::Exempt { .. }
    ));
}

#[test]
fn no_default_skips_files() {
    let temp = TempDir::new().unwrap();
    write_file(&temp, "loq.toml", "exempt = []\n");
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
    write_file(&temp, "loq.toml", "default_max_lines = 1\nexempt = []\n");
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
        exempt: vec![],
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
    let binary_outcome = check_file(&binary, &compiled, temp.path(), None);
    assert!(matches!(binary_outcome.kind, OutcomeKind::Binary));

    let dir_outcome = check_file(temp.path(), &compiled, temp.path(), None);
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

    assert!(matches!(
        output.outcomes[0].kind,
        OutcomeKind::Excluded { .. }
    ));
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

    let outcome_ignored = output.outcomes.iter().find(|o| o.path == ignored).unwrap();
    let outcome_whitelisted = output
        .outcomes
        .iter()
        .find(|o| o.path == whitelisted)
        .unwrap();

    // debug.log should be excluded by gitignore
    assert!(
        matches!(outcome_ignored.kind, OutcomeKind::Excluded { .. }),
        "debug.log should be excluded, got {:?}",
        outcome_ignored.kind
    );

    // important.log should NOT be excluded (whitelisted by negation pattern)
    assert!(
        matches!(outcome_whitelisted.kind, OutcomeKind::Pass { .. }),
        "important.log should pass (whitelisted), got {:?}",
        outcome_whitelisted.kind
    );
}

#[test]
fn multiple_configs_in_different_directories() {
    let temp = TempDir::new().unwrap();

    // Subdirectory A with strict limit (2 lines)
    write_file(&temp, "dir_a/loq.toml", "default_max_lines = 2\n");
    let file_a = write_file(&temp, "dir_a/file.txt", "one\ntwo\nthree\n"); // 3 lines - violation

    // Subdirectory B with lenient limit (10 lines)
    write_file(&temp, "dir_b/loq.toml", "default_max_lines = 10\n");
    let file_b = write_file(&temp, "dir_b/file.txt", "one\ntwo\nthree\n"); // 3 lines - pass

    let output = run_check(
        vec![file_a.clone(), file_b.clone()],
        CheckOptions {
            config_path: None, // Use discovery
            cwd: temp.path().to_path_buf(),
        },
    )
    .unwrap();

    assert_eq!(output.outcomes.len(), 2);

    // Find outcomes by path
    let outcome_a = output.outcomes.iter().find(|o| o.path == file_a).unwrap();
    let outcome_b = output.outcomes.iter().find(|o| o.path == file_b).unwrap();

    // File A should violate (3 lines > 2 limit)
    match &outcome_a.kind {
        OutcomeKind::Violation { limit, actual, .. } => {
            assert_eq!(*limit, 2);
            assert_eq!(*actual, 3);
        }
        other => panic!("expected Violation for file_a, got {other:?}"),
    }

    // File B should pass (3 lines < 10 limit)
    match &outcome_b.kind {
        OutcomeKind::Pass { limit, actual, .. } => {
            assert_eq!(*limit, 10);
            assert_eq!(*actual, 3);
        }
        other => panic!("expected Pass for file_b, got {other:?}"),
    }
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
