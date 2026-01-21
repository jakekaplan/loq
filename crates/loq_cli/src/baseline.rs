//! Baseline command implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use loq_fs::CheckOptions;
use termcolor::WriteColor;
use toml_edit::{DocumentMut, Item};

use crate::cli::BaselineArgs;
use crate::config_edit::{
    add_rule, collect_exact_path_rules, default_document, extract_paths, is_exact_path,
    normalize_display_path, remove_rule, update_rule_max_lines,
};
use crate::init::add_to_gitignore;
use crate::output::print_error;
use crate::ExitStatus;

/// Statistics about baseline changes.
struct BaselineStats {
    added: usize,
    updated: usize,
    removed: usize,
}

impl BaselineStats {
    const fn has_no_changes(&self) -> bool {
        self.added == 0 && self.updated == 0 && self.removed == 0
    }
}

pub fn run_baseline<W1: WriteColor, W2: WriteColor>(
    args: &BaselineArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_baseline_inner(args) {
        Ok(stats) => {
            let _ = write_stats(stdout, &stats);
            ExitStatus::Success
        }
        Err(err) => print_error(stderr, &format!("{err:#}")),
    }
}

fn run_baseline_inner(args: &BaselineArgs) -> Result<BaselineStats> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config_path = cwd.join("loq.toml");

    let config_exists = config_path.exists();
    // Step 1: Read and parse the config file (or create defaults if missing)
    let mut doc: DocumentMut = if config_exists {
        let config_text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        config_text
            .parse()
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    } else {
        default_document()
    };

    // Step 2: Determine threshold (--threshold or default_max_lines from config)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let threshold = args.threshold.unwrap_or_else(|| {
        doc.get("default_max_lines")
            .and_then(Item::as_integer)
            .map_or(500, |v| v as usize)
    });

    // Step 3: Run check to find violations (respects config's exclude and gitignore settings)
    let violations = find_violations(&cwd, &doc, threshold)?;

    // Step 4: Collect existing exact-path rules (baseline candidates)
    let existing_rules = collect_exact_path_rules(&doc);

    // Step 5: Compute changes
    let stats = apply_baseline_changes(&mut doc, &violations, &existing_rules);

    // Step 6: Write config back
    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    if !config_exists {
        add_to_gitignore(&cwd);
    }

    Ok(stats)
}

/// Find all files that violate the given threshold.
fn find_violations(
    cwd: &Path,
    doc: &DocumentMut,
    threshold: usize,
) -> Result<HashMap<String, usize>> {
    // Build temp config using toml_edit to ensure proper escaping
    let temp_config = build_temp_config(doc, threshold);
    let temp_file = tempfile::NamedTempFile::new_in(cwd).context("failed to create temp file")?;
    std::io::Write::write_all(&mut &temp_file, temp_config.as_bytes())
        .context("failed to write temp config")?;

    let options = CheckOptions {
        config_path: Some(temp_file.path().to_path_buf()),
        cwd: cwd.to_path_buf(),
        use_cache: false,
    };

    let output =
        loq_fs::run_check(vec![cwd.to_path_buf()], options).context("baseline check failed")?;

    let mut violations = HashMap::new();
    for outcome in output.outcomes {
        if let loq_core::OutcomeKind::Violation { actual, .. } = outcome.kind {
            // display_path is already normalized (forward slashes, relative to cwd)
            let path = normalize_display_path(&outcome.display_path);
            violations.insert(path, actual);
        }
    }

    Ok(violations)
}

/// Build a temporary config for violation scanning.
/// Copies glob rules (policy) but not exact-path rules (baseline).
/// This ensures files covered by glob policy rules are properly evaluated,
/// while baselined files are evaluated against the threshold.
#[allow(clippy::cast_possible_wrap)]
fn build_temp_config(doc: &DocumentMut, threshold: usize) -> String {
    let mut temp_doc = DocumentMut::new();

    // Set threshold
    temp_doc["default_max_lines"] = toml_edit::value(threshold as i64);

    // Copy respect_gitignore (defaults to true)
    let respect_gitignore = doc
        .get("respect_gitignore")
        .and_then(Item::as_bool)
        .unwrap_or(true);
    temp_doc["respect_gitignore"] = toml_edit::value(respect_gitignore);

    // Copy exclude array with proper escaping
    if let Some(exclude_array) = doc.get("exclude").and_then(Item::as_array) {
        temp_doc["exclude"] = Item::Value(toml_edit::Value::Array(exclude_array.clone()));
    } else {
        temp_doc["exclude"] = Item::Value(toml_edit::Value::Array(toml_edit::Array::default()));
    }

    // Copy only glob rules (policy), not exact-path rules (baseline)
    if let Some(rules_array) = doc.get("rules").and_then(Item::as_array_of_tables) {
        let mut glob_rules = toml_edit::ArrayOfTables::new();
        for rule in rules_array {
            if let Some(path_value) = rule.get("path") {
                let paths = extract_paths(path_value);
                // Only copy rules with glob patterns (not exact paths)
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

/// Apply baseline changes to the document.
fn apply_baseline_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
) -> BaselineStats {
    let mut stats = BaselineStats {
        added: 0,
        updated: 0,
        removed: 0,
    };

    // Track which indices to remove (in reverse order to maintain correctness)
    let mut indices_to_remove: Vec<usize> = Vec::new();

    // Process existing exact-path rules
    for (path, (current_limit, idx)) in existing_rules {
        if let Some(&actual) = violations.get(path) {
            // File still violates - reset to current size if it changed
            if actual != *current_limit {
                update_rule_max_lines(doc, *idx, actual);
                stats.updated += 1;
            }
        } else {
            // File is now compliant (under threshold) - remove the rule
            indices_to_remove.push(*idx);
            stats.removed += 1;
        }
    }

    // Remove rules in reverse order to maintain index validity
    indices_to_remove.sort_by(|a, b| b.cmp(a));
    for idx in indices_to_remove {
        remove_rule(doc, idx);
    }

    // Add new rules for violations not already covered (sorted for deterministic output)
    let mut new_violations: Vec<_> = violations
        .iter()
        .filter(|(path, _)| !existing_rules.contains_key(*path))
        .collect();
    new_violations.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (path, &actual) in new_violations {
        add_rule(doc, path, actual);
        stats.added += 1;
    }

    stats
}

fn write_stats<W: WriteColor>(writer: &mut W, stats: &BaselineStats) -> std::io::Result<()> {
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
        // Capitalize first letter of the output
        let output = parts.join(", ");
        let output = capitalize_first(&output);
        writeln!(writer, "{output}")?;
    }
    Ok(())
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
}
