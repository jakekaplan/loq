//! TOML configuration parsing with validation.
//!
//! Parses `loq.toml` files and detects unknown keys with suggestions.

use std::path::Path;

use serde::{Deserialize, Deserializer};

use crate::config::{ConfigError, LoqConfig, Rule, DEFAULT_RESPECT_GITIGNORE};
use crate::Limit;

#[derive(Deserialize)]
struct RawConfig {
    default_max_lines: Option<usize>,
    default_max_tokens: Option<usize>,
    #[serde(default = "default_respect_gitignore")]
    respect_gitignore: bool,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    rules: Vec<RawRule>,
    #[serde(default)]
    fix_guidance: Option<String>,
}

#[derive(Deserialize)]
struct RawRule {
    #[serde(deserialize_with = "deserialize_string_or_vec")]
    path: Vec<String>,
    max_lines: Option<usize>,
    max_tokens: Option<usize>,
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(value) => Ok(vec![value]),
        StringOrVec::Vec(values) => Ok(values),
    }
}

const fn default_respect_gitignore() -> bool {
    DEFAULT_RESPECT_GITIGNORE
}

/// Parses a `loq.toml` file and validates its structure.
///
/// Returns an error if the TOML is malformed or contains unknown keys.
/// Unknown keys trigger a suggestion if a similar valid key exists.
pub fn parse_config(path: &Path, text: &str) -> Result<LoqConfig, ConfigError> {
    let deserializer = toml::Deserializer::new(text);
    let mut unknown = Vec::new();
    let raw: RawConfig = serde_ignored::deserialize(deserializer, |path| {
        if let Some(key) = extract_unknown_key_name(&path) {
            unknown.push(key);
        }
    })
    .map_err(|err| ConfigError::Toml {
        path: path.to_path_buf(),
        message: err.to_string(),
        line_col: err
            .span()
            .and_then(|span| line_col_from_offset(text, span.start)),
    })?;

    if let Some(key) = unknown.into_iter().next() {
        let line_col = find_key_location(text, &key);
        let suggestion = suggest_key(&key);
        return Err(ConfigError::UnknownKey {
            path: path.to_path_buf(),
            key,
            line_col,
            suggestion,
        });
    }

    let default_limit = match (raw.default_max_lines, raw.default_max_tokens) {
        (Some(_), Some(_)) => {
            return Err(ConfigError::InvalidLimit {
                path: path.to_path_buf(),
                message: "set only one of default_max_lines or default_max_tokens".to_string(),
            });
        }
        (Some(lines), None) => Some(Limit::lines(lines)),
        (None, Some(tokens)) => Some(Limit::tokens(tokens)),
        (None, None) => None,
    };

    let mut rules = Vec::with_capacity(raw.rules.len());
    for raw_rule in raw.rules {
        let limit = match (raw_rule.max_lines, raw_rule.max_tokens) {
            (Some(lines), None) => Limit::lines(lines),
            (None, Some(tokens)) => Limit::tokens(tokens),
            (Some(_), Some(_)) => {
                return Err(ConfigError::InvalidLimit {
                    path: path.to_path_buf(),
                    message: format!(
                        "rule for '{}' must set only one of max_lines or max_tokens",
                        raw_rule.path.join(", ")
                    ),
                });
            }
            (None, None) => {
                return Err(ConfigError::InvalidLimit {
                    path: path.to_path_buf(),
                    message: format!(
                        "rule for '{}' must set max_lines or max_tokens",
                        raw_rule.path.join(", ")
                    ),
                });
            }
        };
        rules.push(Rule {
            paths: raw_rule.path,
            limit,
        });
    }

    Ok(LoqConfig {
        default_limit,
        respect_gitignore: raw.respect_gitignore,
        exclude: raw.exclude,
        rules,
        fix_guidance: raw.fix_guidance,
    })
}

fn extract_unknown_key_name(path: &serde_ignored::Path) -> Option<String> {
    let path_str = path.to_string();
    let mut last = path_str.split('.').next_back().unwrap_or(&path_str);
    if let Some(pos) = last.find('[') {
        last = &last[..pos];
    }
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

fn find_key_location(text: &str, key: &str) -> Option<(usize, usize)> {
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(key) {
            if rest.trim_start().starts_with('=') {
                let leading = line.len().saturating_sub(trimmed.len());
                return Some((line_idx + 1, leading + 1));
            }
        }
    }
    None
}

fn suggest_key(key: &str) -> Option<String> {
    let candidates = [
        "default_max_lines",
        "default_max_tokens",
        "respect_gitignore",
        "exclude",
        "rules",
        "path",
        "max_lines",
        "max_tokens",
        "fix_guidance",
    ];
    let mut best = None;
    let mut best_score = usize::MAX;
    for candidate in candidates {
        let score = strsim::levenshtein(key, candidate);
        if score < best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    if best_score <= 3 {
        best.map(ToString::to_string)
    } else {
        None
    }
}

fn line_col_from_offset(text: &str, offset: usize) -> Option<(usize, usize)> {
    if offset > text.len() {
        return None;
    }
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in text.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    Some((line, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_detection() {
        let text = "default_max_lines = 500\nmax_line = 10\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        match err {
            ConfigError::UnknownKey {
                key, suggestion, ..
            } => {
                assert_eq!(key, "max_line");
                assert_eq!(suggestion, Some("max_lines".to_string()));
            }
            _ => panic!("expected unknown key"),
        }
    }

    #[test]
    fn rule_parsed_correctly() {
        let text = "default_max_lines = 500\n[[rules]]\npath = \"**/*.rs\"\nmax_lines = 10\n";
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].limit, Limit::lines(10));
    }

    #[test]
    fn token_rule_parsed_correctly() {
        let text =
            "default_max_lines = 500\n[[rules]]\npath = \"prompts/**/*.md\"\nmax_tokens = 8000\n";
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].limit, Limit::tokens(8000));
    }

    #[test]
    fn both_default_budgets_are_invalid() {
        let text = "default_max_lines = 500\ndefault_max_tokens = 2000\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        assert!(err
            .to_string()
            .contains("only one of default_max_lines or default_max_tokens"));
    }

    #[test]
    fn rule_with_both_budgets_is_invalid() {
        let text = "[[rules]]\npath = \"**/*.md\"\nmax_lines = 100\nmax_tokens = 1000\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        assert!(err
            .to_string()
            .contains("only one of max_lines or max_tokens"));
    }

    #[test]
    fn rule_without_budget_is_invalid() {
        let text = "[[rules]]\npath = \"**/*.md\"\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        assert!(err.to_string().contains("must set max_lines or max_tokens"));
    }

    #[test]
    fn respect_gitignore_defaults_true() {
        let text = "default_max_lines = 500\n";
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert!(config.respect_gitignore);
    }

    #[test]
    fn invalid_toml_reports_error() {
        let text = "default_max_lines =\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        match err {
            ConfigError::Toml { .. } => {}
            _ => panic!("expected toml error"),
        }
    }

    #[test]
    fn unknown_key_without_location() {
        let text = "rules = [{ path = \"src/*.rs\", max_lines = 10, max_line = 20 }]\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        match err {
            ConfigError::UnknownKey { line_col, .. } => {
                assert!(line_col.is_none());
            }
            _ => panic!("expected unknown key"),
        }
    }

    #[test]
    fn unknown_key_without_suggestion() {
        let text = "banana = 1\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        match err {
            ConfigError::UnknownKey { suggestion, .. } => {
                assert!(suggestion.is_none());
            }
            _ => panic!("expected unknown key"),
        }
    }

    #[test]
    fn line_col_from_offset_handles_newlines() {
        let text = "line1\nline2\nline3";
        let (line, col) = line_col_from_offset(text, 6).unwrap();
        assert_eq!(line, 2);
        assert_eq!(col, 1);
    }

    #[test]
    fn line_col_from_offset_out_of_bounds() {
        let text = "short";
        assert!(line_col_from_offset(text, 100).is_none());
    }

    #[test]
    fn extract_unknown_key_name_with_array_index() {
        let path = serde_ignored::Path::Map {
            parent: &serde_ignored::Path::Root,
            key: "rules[0]".to_string(),
        };
        let key = extract_unknown_key_name(&path);
        assert_eq!(key, Some("rules".to_string()));
    }

    #[test]
    fn extract_unknown_key_name_empty_returns_none() {
        let path = serde_ignored::Path::Map {
            parent: &serde_ignored::Path::Root,
            key: "[0]".to_string(),
        };
        let key = extract_unknown_key_name(&path);
        assert!(key.is_none());
    }

    #[test]
    fn find_key_location_finds_key() {
        let text = "  typo_key = 1\n";
        let loc = find_key_location(text, "typo_key");
        assert_eq!(loc, Some((1, 3)));
    }

    #[test]
    fn find_key_location_not_found() {
        let text = "other = 1\n";
        let loc = find_key_location(text, "missing");
        assert!(loc.is_none());
    }

    #[test]
    fn negative_max_lines_reports_error() {
        let text = "default_max_lines = -1\n";
        let err = parse_config(Path::new("loq.toml"), text).unwrap_err();
        match err {
            ConfigError::Toml { .. } => {}
            _ => panic!("expected Toml error, got {err:?}"),
        }
    }

    #[test]
    fn rule_path_accepts_string() {
        let text = r#"
[[rules]]
path = "**/*.rs"
max_lines = 100
"#;
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].paths, vec!["**/*.rs"]);
    }

    #[test]
    fn rule_path_accepts_array() {
        let text = r#"
[[rules]]
path = ["src/a.rs", "src/b.rs"]
max_lines = 100
"#;
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].paths, vec!["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn rule_path_array_single_element() {
        let text = r#"
[[rules]]
path = ["only_one.rs"]
max_lines = 100
"#;
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(config.rules[0].paths, vec!["only_one.rs"]);
    }

    #[test]
    fn fix_guidance_parsed_correctly() {
        let text = r#"
default_max_lines = 500
fix_guidance = "Split large files into smaller modules."
"#;
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(
            config.fix_guidance,
            Some("Split large files into smaller modules.".to_string())
        );
    }

    #[test]
    fn fix_guidance_multiline_string() {
        let text = r#"
default_max_lines = 500
fix_guidance = """
Consider splitting large files:
- Extract functions into modules
- Move tests to test files
"""
"#;
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert!(config.fix_guidance.is_some());
        let guidance = config.fix_guidance.unwrap();
        assert!(guidance.contains("Consider splitting large files:"));
        assert!(guidance.contains("Extract functions into modules"));
    }

    #[test]
    fn fix_guidance_defaults_to_none() {
        let text = "default_max_lines = 500\n";
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert!(config.fix_guidance.is_none());
    }
}
