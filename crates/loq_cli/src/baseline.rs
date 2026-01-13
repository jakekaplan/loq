//! Baseline command implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use loq_fs::CheckOptions;
use termcolor::WriteColor;
use toml_edit::{DocumentMut, Item, Table};

use crate::cli::BaselineArgs;
use crate::output::print_error;
use crate::ExitStatus;

/// Statistics about baseline changes.
struct BaselineStats {
    added: usize,
    updated: usize,
    removed: usize,
}

impl BaselineStats {
    const fn is_empty(&self) -> bool {
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

    // Require existing config
    if !config_path.exists() {
        anyhow::bail!("loq.toml not found. Run `loq init` first.");
    }

    // Step 1: Read and parse the config file
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let mut doc: DocumentMut = config_text
        .parse()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

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
    let stats = apply_baseline_changes(&mut doc, &violations, &existing_rules, args.allow_growth);

    // Step 6: Write config back
    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;

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

/// Normalize display path for consistent comparison.
/// Strips leading "./" if present.
fn normalize_display_path(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).to_string()
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

/// Collect existing exact-path rules (rules where path is a single literal path, not a glob).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn collect_exact_path_rules(doc: &DocumentMut) -> HashMap<String, (usize, usize)> {
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

/// Extract path strings from a path value (can be string or array).
fn extract_paths(value: &Item) -> Vec<String> {
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
fn is_exact_path(path: &str) -> bool {
    !path.contains('*') && !path.contains('?') && !path.contains('[') && !path.contains('{')
}

/// Apply baseline changes to the document.
fn apply_baseline_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
    allow_growth: bool,
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
            // File still violates - update if it changed size
            if actual < *current_limit {
                // File shrunk - always tighten the limit
                update_rule_max_lines(doc, *idx, actual);
                stats.updated += 1;
            } else if actual > *current_limit && allow_growth {
                // File grew - only update if --allow-growth is set
                update_rule_max_lines(doc, *idx, actual);
                stats.updated += 1;
            }
            // If actual == current_limit, or grew without --allow-growth, leave unchanged
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

/// Update `max_lines` for a rule at the given index.
#[allow(clippy::cast_possible_wrap)]
fn update_rule_max_lines(doc: &mut DocumentMut, idx: usize, new_max: usize) {
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
fn remove_rule(doc: &mut DocumentMut, idx: usize) {
    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(|v| v.as_array_of_tables_mut())
    {
        rules.remove(idx);
    }
}

/// Add a new baseline rule at the end.
#[allow(clippy::cast_possible_wrap)]
fn add_rule(doc: &mut DocumentMut, path: &str, max_lines: usize) {
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

fn write_stats<W: WriteColor>(writer: &mut W, stats: &BaselineStats) -> std::io::Result<()> {
    if stats.is_empty() {
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
    fn stats_is_empty() {
        let empty = BaselineStats {
            added: 0,
            updated: 0,
            removed: 0,
        };
        assert!(empty.is_empty());

        let not_empty = BaselineStats {
            added: 1,
            updated: 0,
            removed: 0,
        };
        assert!(!not_empty.is_empty());
    }
}
