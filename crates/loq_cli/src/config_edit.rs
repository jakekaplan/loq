//! Shared helpers for editing `loq.toml` with `toml_edit`.

use std::collections::HashMap;

use toml_edit::{DocumentMut, Item, Table};

/// Normalize display path for consistent comparison.
/// Strips leading "./" if present.
pub(crate) fn normalize_display_path(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).to_string()
}

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
                        rules.insert(paths[0].clone(), (max_lines as usize, idx));
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
}
