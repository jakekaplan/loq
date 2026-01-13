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
            use_cache: false,
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
            use_cache: false,
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
            use_cache: false,
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
        fix_guidance: None,
    };
    let compiled = loq_core::config::compile_config(
        loq_core::config::ConfigOrigin::BuiltIn,
        temp.path().to_path_buf(),
        config,
        None,
    )
    .unwrap();
    let file_cache = cache::Cache::empty();

    let binary = temp.path().join("binary.txt");
    std::fs::write(&binary, b"\0binary").unwrap();
    let binary_outcome = check_file(&binary, &compiled, temp.path(), &file_cache);
    assert!(matches!(binary_outcome.kind, OutcomeKind::Binary));

    let dir_outcome = check_file(temp.path(), &compiled, temp.path(), &file_cache);
    assert!(matches!(dir_outcome.kind, OutcomeKind::Unreadable { .. }));
}

#[test]
fn explicit_file_bypasses_gitignore() {
    // Following ruff's model: explicit file paths bypass gitignore
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "ignored.txt\n");
    let file = write_file(&temp, "ignored.txt", "a\n");

    let output = run_check(
        vec![file],
        CheckOptions {
            config_path: None,
            cwd: temp.path().to_path_buf(),
            use_cache: false,
        },
    )
    .unwrap();

    // Explicit file bypasses gitignore - it gets checked
    assert_eq!(output.outcomes.len(), 1);
    assert!(matches!(output.outcomes[0].kind, OutcomeKind::Pass { .. }));
}

#[test]
fn directory_walk_respects_gitignore() {
    // Directory walks still respect gitignore
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "ignored.txt\n");
    write_file(&temp, "ignored.txt", "a\n");
    write_file(&temp, "included.txt", "b\n");

    let output = run_check(
        vec![temp.path().to_path_buf()],
        CheckOptions {
            config_path: None,
            cwd: temp.path().to_path_buf(),
            use_cache: false,
        },
    )
    .unwrap();

    // Directory walk respects gitignore - ignored.txt is filtered out
    // Only included.txt and .gitignore should be in outcomes
    let file_names: Vec<_> = output
        .outcomes
        .iter()
        .map(|o| o.display_path.as_str())
        .collect();
    assert!(
        !file_names.iter().any(|p| p.contains("ignored.txt")),
        "ignored.txt should be filtered out, got: {file_names:?}"
    );
    assert!(
        file_names.iter().any(|p| p.contains("included.txt")),
        "included.txt should be present, got: {file_names:?}"
    );
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
            use_cache: false,
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
            use_cache: false,
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
            use_cache: false,
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
fn gitignore_negation_works_in_directory_walk() {
    // Gitignore with negation pattern - tested via directory walk
    let temp = TempDir::new().unwrap();
    // Ignore all .log files, but whitelist important.log
    write_file(&temp, ".gitignore", "*.log\n!important.log\n");

    write_file(&temp, "debug.log", "ignored\n");
    write_file(&temp, "important.log", "not ignored\n");

    // Walk directory instead of passing explicit files
    let output = run_check(
        vec![temp.path().to_path_buf()],
        CheckOptions {
            config_path: None,
            cwd: temp.path().to_path_buf(),
            use_cache: false,
        },
    )
    .unwrap();

    let file_names: Vec<_> = output
        .outcomes
        .iter()
        .map(|o| o.display_path.as_str())
        .collect();

    // debug.log should be filtered out by gitignore
    assert!(
        !file_names.iter().any(|p| p.contains("debug.log")),
        "debug.log should be filtered out, got: {file_names:?}"
    );

    // important.log should NOT be excluded (whitelisted by negation pattern)
    assert!(
        file_names.iter().any(|p| p.contains("important.log")),
        "important.log should pass (whitelisted), got: {file_names:?}"
    );
}

#[test]
fn explicit_files_bypass_gitignore_even_with_negation() {
    // Explicit files bypass gitignore entirely (following ruff's model)
    let temp = TempDir::new().unwrap();
    write_file(&temp, ".gitignore", "*.log\n!important.log\n");

    let ignored = write_file(&temp, "debug.log", "ignored\n");
    let whitelisted = write_file(&temp, "important.log", "not ignored\n");

    // Pass explicit files - both should be checked (gitignore bypassed)
    let output = run_check(
        vec![ignored, whitelisted],
        CheckOptions {
            config_path: None,
            cwd: temp.path().to_path_buf(),
            use_cache: false,
        },
    )
    .unwrap();

    // Both files should be present - explicit paths bypass gitignore
    assert_eq!(
        output.outcomes.len(),
        2,
        "Both explicit files should be checked"
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
            use_cache: false,
        },
    );

    match result {
        Err(FsError::ConfigRead { .. }) => {}
        Err(other) => panic!("expected ConfigRead error, got {other}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[test]
fn exclude_pattern_with_globstar() {
    // Tests that `**/.git/**` pattern works (user-reported workaround)
    let temp = TempDir::new().unwrap();
    write_file(
        &temp,
        "loq.toml",
        "default_max_lines = 10\nexclude = [\"**/.git/**\"]\n",
    );
    write_file(&temp, ".git/logs/HEAD", "a\n");
    write_file(&temp, "src/app.py", "b\n");

    let output = run_check(
        vec![temp.path().to_path_buf()],
        CheckOptions {
            config_path: Some(temp.path().join("loq.toml")),
            cwd: temp.path().to_path_buf(),
            use_cache: false,
        },
    )
    .unwrap();

    let file_names: Vec<_> = output
        .outcomes
        .iter()
        .map(|o| o.display_path.as_str())
        .collect();

    // .git/logs/HEAD should be excluded by pattern
    assert!(
        !file_names.iter().any(|p| p.contains(".git")),
        "**/.git/** should be excluded, got: {file_names:?}"
    );
    // src/app.py should be present
    assert!(
        file_names.iter().any(|p| p.contains("app.py")),
        "src/app.py should be present, got: {file_names:?}"
    );
}

#[test]
fn normalize_path_strips_leading_dot_slash() {
    // Verify normalize_path strips "./" prefix - key fix for exclude patterns
    assert_eq!(normalize_path(Path::new("./foo/bar")), "foo/bar");
    assert_eq!(
        normalize_path(Path::new("./.git/logs/HEAD")),
        ".git/logs/HEAD"
    );
    assert_eq!(normalize_path(Path::new("foo/bar")), "foo/bar");
    assert_eq!(normalize_path(Path::new(".")), ".");
}

#[cfg(windows)]
#[test]
fn relative_path_handles_verbatim_root() {
    let path = Path::new(r"C:\repo\project\generated\big.txt");
    let root = Path::new(r"\\?\C:\repo\project");
    let relative = relative_path_str(path, root);
    assert_eq!(relative, "generated/big.txt");
}
