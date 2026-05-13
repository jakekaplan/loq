use super::*;
use std::path::PathBuf;

#[test]
fn default_config_has_expected_values() {
    let config = LoqConfig::default();
    assert_eq!(config.default_max_lines, Some(DEFAULT_MAX_LINES));
    assert!(config.respect_gitignore);
    assert!(config.exclude.is_empty());
    assert!(config.rules.is_empty());
}

#[test]
fn built_in_defaults_matches_default() {
    let default = LoqConfig::default();
    let built_in = LoqConfig::built_in_defaults();
    assert_eq!(default.default_max_lines, built_in.default_max_lines);
    assert_eq!(default.respect_gitignore, built_in.respect_gitignore);
}

#[test]
fn init_template_has_rules() {
    let template = LoqConfig::init_template();
    assert_eq!(template.default_max_lines, Some(DEFAULT_MAX_LINES));
    assert_eq!(template.rules.len(), 2);
    assert_eq!(template.rules[0].path, vec!["**/*.tsx"]);
    assert_eq!(template.rules[1].path, vec!["tests/**/*"]);
}

#[test]
fn invalid_glob_reports_error() {
    let config = LoqConfig {
        default_max_lines: Some(1),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            path: vec!["[[".to_string()],
            max_lines: Some(1),
            max_tokens: None,
        }],
        fix_guidance: None,
    };
    let err = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();
    match err {
        ConfigError::Glob { .. } => {}
        _ => panic!("expected glob error"),
    }
}

#[test]
fn glob_error_display_is_stable() {
    let config = LoqConfig {
        default_max_lines: Some(1),
        respect_gitignore: true,
        exclude: vec!["[[".to_string()],
        rules: vec![],
        fix_guidance: None,
    };
    let err = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn glob_star_does_not_cross_directories() {
    let config = LoqConfig {
        default_max_lines: None,
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            path: vec!["src/*.rs".to_string()],
            max_lines: Some(1),
            max_tokens: None,
        }],
        fix_guidance: None,
    };
    let compiled = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap();
    let rule = &compiled.rules()[0];

    assert!(rule.matches("src/lib.rs").is_some());
    assert!(rule.matches("src/nested/lib.rs").is_none());
}

#[test]
fn token_rule_compiles_to_token_limit() {
    let config = LoqConfig {
        default_max_lines: Some(1),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            path: vec!["prompts/**/*.md".to_string()],
            max_lines: None,
            max_tokens: Some(8000),
        }],
        fix_guidance: None,
    };
    let compiled = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap();

    assert_eq!(compiled.rules()[0].limit, Limit::tokens(8000));
}

#[test]
fn rule_with_both_budgets_is_invalid() {
    let config = LoqConfig {
        default_max_lines: Some(1),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            path: vec!["**/*.md".to_string()],
            max_lines: Some(100),
            max_tokens: Some(1000),
        }],
        fix_guidance: None,
    };
    let err = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();

    assert!(matches!(err, ConfigError::InvalidLimit { .. }));
    assert!(err
        .to_string()
        .contains("only one of max_lines or max_tokens"));
}

#[test]
fn rule_without_budget_is_invalid() {
    let config = LoqConfig {
        default_max_lines: Some(1),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            path: vec!["**/*.md".to_string()],
            max_lines: None,
            max_tokens: None,
        }],
        fix_guidance: None,
    };
    let err = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();

    assert!(matches!(err, ConfigError::InvalidLimit { .. }));
    assert!(err.to_string().contains("must set max_lines or max_tokens"));
}

#[test]
fn pattern_list_no_match_returns_none() {
    let patterns = vec![PatternMatcher {
        pattern: "*.rs".to_string(),
        matcher: globset::GlobBuilder::new("*.rs")
            .literal_separator(true)
            .build()
            .unwrap()
            .compile_matcher(),
    }];
    let list = PatternList::new(patterns);
    assert!(list.matches("foo.txt").is_none());
}

#[test]
fn format_toml_error_without_location() {
    let msg = format_toml_error(Path::new("test.toml"), &None, "parse error");
    assert_eq!(msg, "test.toml - parse error");
}

#[test]
fn format_unknown_key_error_without_suggestion() {
    let msg = format_unknown_key_error(Path::new("test.toml"), "xyz", &Some((1, 1)), &None);
    assert!(msg.contains("unknown key 'xyz'"));
    assert!(!msg.contains("did you mean"));
}
