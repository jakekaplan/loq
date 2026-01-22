//! Shared helpers for baseline-like commands.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use loq_fs::CheckOptions;
use termcolor::WriteColor;
use toml_edit::{DocumentMut, Item};

use crate::config_edit::{extract_paths, is_exact_path, normalize_display_path};

/// Statistics about rule changes.
pub(crate) struct BaselineStats {
    pub(crate) added: usize,
    pub(crate) updated: usize,
    pub(crate) removed: usize,
}

impl BaselineStats {
    pub(crate) const fn has_no_changes(&self) -> bool {
        self.added == 0 && self.updated == 0 && self.removed == 0
    }
}

/// Write a summary line for rule updates.
pub(crate) fn write_stats<W: WriteColor>(
    writer: &mut W,
    stats: &BaselineStats,
) -> std::io::Result<()> {
    if stats.has_no_changes() {
        writeln!(writer, "No changes needed")?;
    } else {
        let mut parts = Vec::new();
        if stats.added > 0 {
            parts.push(format!(
                "added {} rule{}",
                stats.added,
                if stats.added == 1 { "" } else { "s" }
            ));
        }
        if stats.updated > 0 {
            parts.push(format!(
                "updated {} rule{}",
                stats.updated,
                if stats.updated == 1 { "" } else { "s" }
            ));
        }
        if stats.removed > 0 {
            parts.push(format!(
                "removed {} rule{}",
                stats.removed,
                if stats.removed == 1 { "" } else { "s" }
            ));
        }
        let output = capitalize_first(&parts.join(", "));
        writeln!(writer, "{output}")?;
    }
    Ok(())
}

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
#[allow(clippy::cast_possible_wrap)]
fn build_temp_config(doc: &DocumentMut, threshold: usize) -> String {
    let mut temp_doc = DocumentMut::new();

    temp_doc["default_max_lines"] = toml_edit::value(threshold as i64);

    let respect_gitignore = doc
        .get("respect_gitignore")
        .and_then(Item::as_bool)
        .unwrap_or(true);
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

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_has_no_changes() {
        let empty = BaselineStats {
            added: 0,
            updated: 0,
            removed: 0,
        };
        assert!(empty.has_no_changes());

        let not_empty = BaselineStats {
            added: 1,
            updated: 0,
            removed: 0,
        };
        assert!(!not_empty.has_no_changes());
    }

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
    fn capitalize_first_handles_empty() {
        assert_eq!(capitalize_first(""), "");
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
