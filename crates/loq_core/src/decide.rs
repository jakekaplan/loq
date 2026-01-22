//! Rule matching and decision logic.
//!
//! Determines what limit applies to a file based on configuration.
//! Priority: rules (last match wins) → default.
//!
//! Note: Exclusion filtering (gitignore, exclude patterns) is handled
//! at the walk layer, not here.

use crate::config::CompiledConfig;

/// How a file's limit was determined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchBy {
    /// Matched a specific rule pattern.
    Rule {
        /// The glob pattern that matched.
        pattern: String,
    },
    /// Used the default limit.
    Default,
}

/// The decision for how to handle a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// File should be checked against a limit.
    Check {
        /// Maximum allowed lines.
        limit: usize,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
    /// No default limit and no matching rule; skip.
    SkipNoLimit,
}

/// Decides what limit applies to a file path.
///
/// Checks rules (last match wins), then falls back to default.
#[must_use]
pub fn decide(config: &CompiledConfig, path: &str) -> Decision {
    for rule in config.rules().iter().rev() {
        if let Some(pattern) = rule.matches(path) {
            return Decision::Check {
                limit: rule.max_lines,
                matched_by: MatchBy::Rule {
                    pattern: pattern.to_string(),
                },
            };
        }
    }

    if let Some(default_max) = config.default_max_lines {
        Decision::Check {
            limit: default_max,
            matched_by: MatchBy::Default,
        }
    } else {
        Decision::SkipNoLimit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{compile_config, ConfigOrigin, LoqConfig, Rule};
    use std::path::PathBuf;

    fn compiled(config: LoqConfig) -> CompiledConfig {
        compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap()
    }

    #[test]
    fn rule_order_last_match_wins() {
        let config = LoqConfig {
            default_max_lines: Some(500),
            respect_gitignore: true,
            exclude: vec![],
            rules: vec![
                Rule {
                    path: vec!["**/*.rs".to_string()],
                    max_lines: 100,
                },
                Rule {
                    path: vec!["**/*.rs".to_string()],
                    max_lines: 200,
                },
            ],
            fix_guidance: None,
        };
        let decision = decide(&compiled(config), "src/main.rs");
        match decision {
            Decision::Check { limit, .. } => {
                assert_eq!(limit, 200);
            }
            Decision::SkipNoLimit => panic!("expected check"),
        }
    }

    #[test]
    fn default_fallback_when_no_rule() {
        let config = LoqConfig {
            default_max_lines: Some(123),
            respect_gitignore: true,
            exclude: vec![],
            rules: vec![],
            fix_guidance: None,
        };
        let decision = decide(&compiled(config), "src/file.txt");
        match decision {
            Decision::Check { limit, matched_by } => {
                assert_eq!(limit, 123);
                assert_eq!(matched_by, MatchBy::Default);
            }
            Decision::SkipNoLimit => panic!("expected default"),
        }
    }

    #[test]
    fn skip_when_no_default_and_no_rule() {
        let config = LoqConfig {
            default_max_lines: None,
            respect_gitignore: true,
            exclude: vec![],
            rules: vec![],
            fix_guidance: None,
        };
        let decision = decide(&compiled(config), "src/file.txt");
        assert_eq!(decision, Decision::SkipNoLimit);
    }

    #[test]
    fn multi_path_rule_matches_any() {
        let config = LoqConfig {
            default_max_lines: Some(500),
            respect_gitignore: true,
            exclude: vec![],
            rules: vec![Rule {
                path: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
                max_lines: 100,
            }],
            fix_guidance: None,
        };
        let compiled = compiled(config);

        // First pattern matches
        let decision_a = decide(&compiled, "src/a.rs");
        match decision_a {
            Decision::Check { limit, matched_by } => {
                assert_eq!(limit, 100);
                assert_eq!(
                    matched_by,
                    MatchBy::Rule {
                        pattern: "src/a.rs".to_string()
                    }
                );
            }
            Decision::SkipNoLimit => panic!("expected check for a.rs"),
        }

        // Second pattern matches
        let decision_b = decide(&compiled, "src/b.rs");
        match decision_b {
            Decision::Check { matched_by, .. } => {
                assert_eq!(
                    matched_by,
                    MatchBy::Rule {
                        pattern: "src/b.rs".to_string()
                    }
                );
            }
            Decision::SkipNoLimit => panic!("expected check for b.rs"),
        }

        // Neither pattern matches - falls back to default
        let decision_c = decide(&compiled, "src/c.rs");
        match decision_c {
            Decision::Check { limit, matched_by } => {
                assert_eq!(limit, 500);
                assert_eq!(matched_by, MatchBy::Default);
            }
            Decision::SkipNoLimit => panic!("expected default for c.rs"),
        }
    }
}
