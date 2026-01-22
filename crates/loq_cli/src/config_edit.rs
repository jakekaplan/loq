//! Shared helpers for editing `loq.toml` with `toml_edit`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use loq_core::config::{DEFAULT_MAX_LINES, DEFAULT_RESPECT_GITIGNORE};
use loq_fs::normalize_display_path;
use toml_edit::{DocumentMut, Item, Table};

use crate::init::add_to_gitignore;

/// Extract path strings from a path value (can be string or array).
pub(crate) fn extract_paths(value: &Item) -> Vec<String> {
    if let Some(s) = value.as_str() {
        vec![s.to_string()]
    } else if let Some(arr) = value.as_array() {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else {
        vec![]
    }
}

/// Check if a path is an exact path (no glob metacharacters).
pub(crate) fn is_exact_path(path: &str) -> bool {
    !path.contains('*') && !path.contains('?') && !path.contains('[') && !path.contains('{')
}

/// Collect existing exact-path rules (rules where path is a single literal path, not a glob).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn collect_exact_path_rules(doc: &DocumentMut) -> HashMap<String, (usize, usize)> {
    let mut rules = HashMap::new();

    if let Some(rules_array) = doc.get("rules").and_then(Item::as_array_of_tables) {
        for (idx, rule) in rules_array.iter().enumerate() {
            if let Some(path_value) = rule.get("path") {
                let paths = extract_paths(path_value);
                // Only consider single-path rules that look like exact paths (no glob chars)
                if paths.len() == 1 && is_exact_path(&paths[0]) {
                    if let Some(max_lines) = rule.get("max_lines").and_then(Item::as_integer) {
                        let normalized = normalize_display_path(&paths[0]);
                        rules.insert(normalized, (max_lines as usize, idx));
                    }
                }
            }
        }
    }

    rules
}

/// Update `max_lines` for a rule at the given index.
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn update_rule_max_lines(doc: &mut DocumentMut, idx: usize, new_max: usize) {
    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(|v| v.as_array_of_tables_mut())
    {
        if let Some(rule) = rules.get_mut(idx) {
            rule["max_lines"] = toml_edit::value(new_max as i64);
        }
    }
}

/// Remove a rule at the given index.
pub(crate) fn remove_rule(doc: &mut DocumentMut, idx: usize) {
    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(|v| v.as_array_of_tables_mut())
    {
        rules.remove(idx);
    }
}

/// Add a new exact-path rule at the end.
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn add_rule(doc: &mut DocumentMut, path: &str, max_lines: usize) {
    // Ensure rules array exists
    if doc.get("rules").is_none() {
        doc["rules"] = Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(|v| v.as_array_of_tables_mut())
    {
        let mut rule = Table::new();
        rule["path"] = toml_edit::value(path);
        rule["max_lines"] = toml_edit::value(max_lines as i64);
        rules.push(rule);
    }
}

/// Create a default document for initializing `loq.toml`.
pub(crate) fn default_document() -> DocumentMut {
    let mut doc = DocumentMut::new();
    doc["default_max_lines"] = toml_edit::value(default_max_lines_i64());
    doc["respect_gitignore"] = toml_edit::value(DEFAULT_RESPECT_GITIGNORE);
    doc["exclude"] = Item::Value(toml_edit::Value::Array(toml_edit::Array::default()));
    doc
}

fn default_max_lines_i64() -> i64 {
    i64::try_from(DEFAULT_MAX_LINES).unwrap_or(i64::MAX)
}

pub(crate) fn load_doc_or_default(config_path: &Path) -> Result<(DocumentMut, bool)> {
    if config_path.exists() {
        let config_text = std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let doc = config_text
            .parse()
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        Ok((doc, true))
    } else {
        Ok((default_document(), false))
    }
}

pub(crate) fn write_doc(config_path: &Path, doc: &DocumentMut) -> Result<()> {
    std::fs::write(config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(())
}

pub(crate) fn persist_doc(
    cwd: &Path,
    config_path: &Path,
    doc: &DocumentMut,
    config_existed: bool,
) -> Result<()> {
    write_doc(config_path, doc)?;
    if !config_existed {
        add_to_gitignore(cwd);
    }
    Ok(())
}

pub(crate) fn threshold_from_doc(doc: &DocumentMut, explicit: Option<usize>) -> usize {
    explicit.unwrap_or_else(|| {
        doc.get("default_max_lines")
            .and_then(Item::as_integer)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(DEFAULT_MAX_LINES)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::{Array, Formatted, Value};

    #[test]
    fn is_exact_path_detects_globs() {
        assert!(is_exact_path("src/main.rs"));
        assert!(is_exact_path("foo/bar/baz.txt"));
        assert!(!is_exact_path("**/*.rs"));
        assert!(!is_exact_path("src/*.rs"));
        assert!(!is_exact_path("src/[ab].rs"));
        assert!(!is_exact_path("src/{a,b}.rs"));
        assert!(!is_exact_path("src/?.rs"));
    }

    #[test]
    fn extract_paths_from_string() {
        let item = Item::Value(Value::String(Formatted::new("src/main.rs".into())));
        assert_eq!(extract_paths(&item), vec!["src/main.rs"]);
    }

    #[test]
    fn extract_paths_from_array() {
        let mut arr = Array::new();
        arr.push("a.rs");
        arr.push("b.rs");
        let item = Item::Value(Value::Array(arr));
        assert_eq!(extract_paths(&item), vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn normalize_display_path_strips_dot_slash() {
        assert_eq!(normalize_display_path("./src/main.rs"), "src/main.rs");
        assert_eq!(normalize_display_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn collect_exact_path_rules_filters_non_exact_rules() {
        let doc: DocumentMut = r#"
[[rules]]
path = "src/a.rs"
max_lines = 10

[[rules]]
path = ["src/b.rs", "src/c.rs"]
max_lines = 20

[[rules]]
path = "**/*.rs"
max_lines = 30
"#
        .parse()
        .unwrap();

        let rules = collect_exact_path_rules(&doc);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules["src/a.rs"].0, 10);
        assert_eq!(rules["src/a.rs"].1, 0);
    }

    #[test]
    fn add_update_remove_rule_flow() {
        let mut doc = DocumentMut::new();

        add_rule(&mut doc, "src/a.rs", 10);
        add_rule(&mut doc, "src/b.rs", 12);

        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        assert_eq!(rules.len(), 2);
        let first = rules.get(0).unwrap();
        assert_eq!(first.get("max_lines").and_then(Item::as_integer), Some(10));

        update_rule_max_lines(&mut doc, 0, 15);
        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        let first = rules.get(0).unwrap();
        assert_eq!(first.get("max_lines").and_then(Item::as_integer), Some(15));

        remove_rule(&mut doc, 1);
        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        assert_eq!(rules.len(), 1);
        let first = rules.get(0).unwrap();
        assert_eq!(first.get("path").and_then(Item::as_str), Some("src/a.rs"));
    }

    #[test]
    fn collect_exact_path_rules_normalizes_dot_slash() {
        let doc: DocumentMut = r#"
[[rules]]
path = "./src/a.rs"
max_lines = 10
"#
        .parse()
        .unwrap();

        let rules = collect_exact_path_rules(&doc);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules["src/a.rs"].0, 10);
    }

    #[test]
    fn default_document_has_expected_defaults() {
        let doc = default_document();
        assert_eq!(
            doc.get("default_max_lines").and_then(Item::as_integer),
            Some(default_max_lines_i64())
        );
        assert_eq!(
            doc.get("respect_gitignore").and_then(Item::as_bool),
            Some(DEFAULT_RESPECT_GITIGNORE)
        );
        let exclude = doc.get("exclude").and_then(Item::as_array);
        assert!(exclude.is_some());
        assert_eq!(exclude.unwrap().len(), 0);
    }
}
