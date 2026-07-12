use super::*;
use std::path::PathBuf;

#[test]
fn default_config_has_expected_values() {
    let config = LoqConfig::default();
    assert_eq!(config.default_limit, Some(Limit::lines(DEFAULT_MAX_LINES)));
    assert!(config.respect_gitignore);
    assert!(config.exclude.is_empty());
    assert!(config.rules.is_empty());
}

#[test]
fn invalid_glob_reports_error() {
    let config = LoqConfig {
        default_limit: Some(Limit::lines(1)),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            paths: vec!["[[".to_string()],
            limit: Limit::lines(1),
        }],
        fix_guidance: None,
    };
    let err = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();
    assert!(matches!(err, ConfigError::Glob { .. }));
}

#[test]
fn glob_error_display_is_stable() {
    let config = LoqConfig {
        default_limit: Some(Limit::lines(1)),
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
        default_limit: None,
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            paths: vec!["src/*.rs".to_string()],
            limit: Limit::lines(1),
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
        default_limit: Some(Limit::lines(1)),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![Rule {
            paths: vec!["prompts/**/*.md".to_string()],
            limit: Limit::tokens(8000),
        }],
        fix_guidance: None,
    };
    let compiled = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap();

    assert_eq!(compiled.rules()[0].limit, Limit::tokens(8000));
}

#[test]
fn default_token_limit_compiles() {
    let config = LoqConfig {
        default_limit: Some(Limit::tokens(2000)),
        respect_gitignore: true,
        exclude: vec![],
        rules: vec![],
        fix_guidance: None,
    };
    let compiled = compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap();

    assert_eq!(compiled.default_limit, Some(Limit::tokens(2000)));
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
