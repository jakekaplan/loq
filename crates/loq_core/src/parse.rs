//! TOML configuration parsing with validation.
//!
//! Parses `loq.toml` files and detects unknown keys with suggestions.

use std::path::Path;

use crate::config::{ConfigError, LoqConfig};

/// Parses a `loq.toml` file and validates its structure.
///
/// Returns an error if the TOML is malformed or contains unknown keys.
/// Unknown keys trigger a suggestion if a similar valid key exists.
pub fn parse_config(path: &Path, text: &str) -> Result<LoqConfig, ConfigError> {
    let deserializer = toml::Deserializer::new(text);
    let mut unknown = Vec::new();
    let parsed: LoqConfig = serde_ignored::deserialize(deserializer, |path| {
        if let Some(key) = extract_key(&path) {
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

    Ok(parsed)
}

fn extract_key(path: &serde_ignored::Path) -> Option<String> {
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
        "respect_gitignore",
        "exclude",
        "exempt",
        "rules",
        "path",
        "max_lines",
        "severity",
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
        best.map(|s| s.to_string())
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
    use crate::config::Severity;

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
    fn rule_severity_defaults_to_error() {
        let text = "default_max_lines = 500\n[[rules]]\npath = \"**/*.rs\"\nmax_lines = 10\n";
        let config = parse_config(Path::new("loq.toml"), text).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].severity, Severity::Error);
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
    fn extract_key_with_array_index() {
        let path = serde_ignored::Path::Map {
            parent: &serde_ignored::Path::Root,
            key: "rules[0]".to_string(),
        };
        let key = extract_key(&path);
        assert_eq!(key, Some("rules".to_string()));
    }

    #[test]
    fn extract_key_empty_returns_none() {
        let path = serde_ignored::Path::Map {
            parent: &serde_ignored::Path::Root,
            key: "[0]".to_string(),
        };
        let key = extract_key(&path);
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
}
