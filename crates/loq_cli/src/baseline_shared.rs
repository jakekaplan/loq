//! Shared helpers for baseline-like commands.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use loq_core::config::DEFAULT_RESPECT_GITIGNORE;
use loq_fs::CheckOptions;
use toml_edit::{DocumentMut, Item};

use crate::config_edit::{extract_paths, is_exact_path};
use loq_fs::normalize_display_path;

/// Find all files that violate the given threshold.
pub(crate) fn find_violations(
    cwd: &Path,
    doc: &DocumentMut,
    threshold: usize,
    context: &'static str,
) -> Result<HashMap<String, usize>> {
    let temp_config = build_temp_config(doc, threshold);
    let temp_file = tempfile::NamedTempFile::new_in(cwd).context("failed to create temp file")?;
    std::io::Write::write_all(&mut &temp_file, temp_config.as_bytes())
        .context("failed to write temp config")?;

    let options = CheckOptions {
        config_path: Some(temp_file.path().to_path_buf()),
        cwd: cwd.to_path_buf(),
        use_cache: false,
    };

    let output = loq_fs::run_check(vec![cwd.to_path_buf()], options).context(context)?;

    let mut violations = HashMap::new();
    for outcome in output.outcomes {
        if let loq_core::OutcomeKind::Violation { actual, .. } = outcome.kind {
            let path = normalize_display_path(&outcome.display_path);
            violations.insert(path, actual);
        }
    }

    Ok(violations)
}

/// Build a temporary config for violation scanning.
/// Copies glob rules (policy) but not exact-path rules (baseline).
fn build_temp_config(doc: &DocumentMut, threshold: usize) -> String {
    let mut temp_doc = DocumentMut::new();

    let threshold_value = i64::try_from(threshold).unwrap_or(i64::MAX);
    temp_doc["default_max_lines"] = toml_edit::value(threshold_value);

    let respect_gitignore = doc
        .get("respect_gitignore")
        .and_then(Item::as_bool)
        .unwrap_or(DEFAULT_RESPECT_GITIGNORE);
    temp_doc["respect_gitignore"] = toml_edit::value(respect_gitignore);

    if let Some(exclude_array) = doc.get("exclude").and_then(Item::as_array) {
        temp_doc["exclude"] = Item::Value(toml_edit::Value::Array(exclude_array.clone()));
    } else {
        temp_doc["exclude"] = Item::Value(toml_edit::Value::Array(toml_edit::Array::default()));
    }

    if let Some(rules_array) = doc.get("rules").and_then(Item::as_array_of_tables) {
        let mut glob_rules = toml_edit::ArrayOfTables::new();
        for rule in rules_array {
            if let Some(path_value) = rule.get("path") {
                let paths = extract_paths(path_value);
                let is_glob = paths.iter().any(|p| !is_exact_path(p));
                if is_glob {
                    glob_rules.push(rule.clone());
                }
            }
        }
        if !glob_rules.is_empty() {
            temp_doc["rules"] = Item::ArrayOfTables(glob_rules);
        }
    }

    temp_doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_temp_config_keeps_glob_rules_only() {
        let doc: DocumentMut = r#"
default_max_lines = 500

[[rules]]
path = "**/*.rs"
max_lines = 1000

[[rules]]
path = "src/main.rs"
max_lines = 200
"#
        .parse()
        .unwrap();
        let temp = build_temp_config(&doc, 123);
        assert!(temp.contains("path = \"**/*.rs\""));
        assert!(!temp.contains("path = \"src/main.rs\""));
        assert!(temp.contains("default_max_lines = 123"));
    }

    #[test]
    fn build_temp_config_ignores_rules_without_path() {
        let doc: DocumentMut = r"
default_max_lines = 500

[[rules]]
max_lines = 10
"
        .parse()
        .unwrap();
        let temp = build_temp_config(&doc, 500);
        assert!(!temp.contains("[[rules]]"));
    }
}
