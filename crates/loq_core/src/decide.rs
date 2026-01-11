//! Rule matching and decision logic.
//!
//! Determines what action to take for each file based on configuration.
//! Priority: exclude → exempt → rules (last match wins) → default.

use crate::config::{CompiledConfig, Severity};

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
    /// File matches an exclude pattern; skip entirely.
    Excluded {
        /// The pattern that matched.
        pattern: String,
    },
    /// File matches an exempt pattern; count but don't check.
    Exempt {
        /// The pattern that matched.
        pattern: String,
    },
    /// File should be checked against a limit.
    Check {
        /// Maximum allowed lines.
        limit: usize,
        /// Severity if limit is exceeded.
        severity: Severity,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
    /// No default limit and no matching rule; skip.
    SkipNoLimit,
}

/// Decides what action to take for a file path.
///
/// Checks patterns in order: exclude, exempt, rules (last match wins), default.
pub fn decide(config: &CompiledConfig, path: &str) -> Decision {
    if let Some(pattern) = config.exclude_patterns().matches(path) {
        return Decision::Excluded {
            pattern: pattern.to_string(),
        };
    }
    if let Some(pattern) = config.exempt_patterns().matches(path) {
        return Decision::Exempt {
            pattern: pattern.to_string(),
        };
    }

    let mut matched_rule = None;
    for rule in config.rules() {
        if rule.is_match(path) {
            matched_rule = Some(rule);
        }
    }

    if let Some(rule) = matched_rule {
        return Decision::Check {
            limit: rule.max_lines,
            severity: rule.severity,
            matched_by: MatchBy::Rule {
                pattern: rule.pattern.clone(),
            },
        };
    }

    if let Some(default_max) = config.default_max_lines {
        Decision::Check {
            limit: default_max,
            severity: Severity::Error,
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
            exempt: vec![],
            rules: vec![
                Rule {
                    path: "**/*.rs".to_string(),
                    max_lines: 100,
                    severity: Severity::Error,
                },
                Rule {
                    path: "**/*.rs".to_string(),
                    max_lines: 200,
                    severity: Severity::Warning,
                },
            ],
        };
        let decision = decide(&compiled(config), "src/main.rs");
        match decision {
            Decision::Check {
                limit, severity, ..
            } => {
                assert_eq!(limit, 200);
                assert_eq!(severity, Severity::Warning);
            }
            _ => panic!("expected check"),
        }
    }

    #[test]
    fn default_fallback_when_no_rule() {
        let config = LoqConfig {
            default_max_lines: Some(123),
            respect_gitignore: true,
            exclude: vec![],
            exempt: vec![],
            rules: vec![],
        };
        let decision = decide(&compiled(config), "src/file.txt");
        match decision {
            Decision::Check {
                limit, matched_by, ..
            } => {
                assert_eq!(limit, 123);
                assert_eq!(matched_by, MatchBy::Default);
            }
            _ => panic!("expected default"),
        }
    }

    #[test]
    fn skip_when_no_default_and_no_rule() {
        let config = LoqConfig {
            default_max_lines: None,
            respect_gitignore: true,
            exclude: vec![],
            exempt: vec![],
            rules: vec![],
        };
        let decision = decide(&compiled(config), "src/file.txt");
        assert_eq!(decision, Decision::SkipNoLimit);
    }

    #[test]
    fn exclude_beats_rules() {
        let config = LoqConfig {
            default_max_lines: Some(10),
            respect_gitignore: true,
            exclude: vec!["**/*.txt".to_string()],
            exempt: vec![],
            rules: vec![Rule {
                path: "**/*.txt".to_string(),
                max_lines: 1,
                severity: Severity::Error,
            }],
        };
        let decision = decide(&compiled(config), "notes.txt");
        match decision {
            Decision::Excluded { pattern } => assert_eq!(pattern, "**/*.txt"),
            _ => panic!("expected excluded"),
        }
    }

    #[test]
    fn exempt_beats_rules() {
        let config = LoqConfig {
            default_max_lines: Some(10),
            respect_gitignore: true,
            exclude: vec![],
            exempt: vec!["legacy.rs".to_string()],
            rules: vec![Rule {
                path: "**/*.rs".to_string(),
                max_lines: 1,
                severity: Severity::Error,
            }],
        };
        let decision = decide(&compiled(config), "legacy.rs");
        match decision {
            Decision::Exempt { pattern } => assert_eq!(pattern, "legacy.rs"),
            _ => panic!("expected exempt, got {decision:?}"),
        }
    }

    #[test]
    fn exclude_beats_exempt() {
        // When a file matches both exclude and exempt, exclude wins
        let config = LoqConfig {
            default_max_lines: Some(10),
            respect_gitignore: true,
            exclude: vec!["**/*.gen.rs".to_string()],
            exempt: vec!["**/*.gen.rs".to_string()],
            rules: vec![],
        };
        let decision = decide(&compiled(config), "output.gen.rs");
        match decision {
            Decision::Excluded { pattern } => assert_eq!(pattern, "**/*.gen.rs"),
            _ => panic!("expected excluded, got {decision:?}"),
        }
    }
}
