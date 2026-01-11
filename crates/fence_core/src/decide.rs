use crate::config::{CompiledConfig, Severity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchBy {
    Rule { pattern: String },
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Excluded {
        pattern: String,
    },
    Exempt {
        pattern: String,
    },
    Check {
        limit: usize,
        severity: Severity,
        matched_by: MatchBy,
    },
    SkipNoLimit,
}

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
    use crate::config::{compile_config, ConfigOrigin, FenceConfig, Rule};
    use std::path::PathBuf;

    fn compiled(config: FenceConfig) -> CompiledConfig {
        compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap()
    }

    #[test]
    fn rule_order_last_match_wins() {
        let config = FenceConfig {
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
        let config = FenceConfig {
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
        let config = FenceConfig {
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
        let config = FenceConfig {
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
}
