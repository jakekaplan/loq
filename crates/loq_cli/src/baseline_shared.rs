//! Shared helpers for baseline-like commands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use loq_core::config::DEFAULT_RESPECT_GITIGNORE;
use loq_core::{FileOutcome, Metric, OutcomeKind};
use loq_fs::CheckOptions;
use termcolor::WriteColor;
use toml_edit::{DocumentMut, Item};

use crate::exact_limits::{extract_paths, is_exact_path};
use crate::output::print_error;
use crate::ExitStatus;

/// A change report produced by a baseline-style command.
pub(crate) trait ChangeReport {
    /// Returns true when the command made no changes.
    fn is_empty(&self) -> bool;
    /// Writes the human-readable change summary.
    fn write<W: WriteColor>(&self, writer: &mut W) -> std::io::Result<()>;
}

/// Prints a change report (or error) and returns the process exit status.
pub(crate) fn finish<W1: WriteColor, W2: WriteColor, R: ChangeReport>(
    result: Result<R>,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match result {
        Ok(report) if report.is_empty() => {
            let _ = writeln!(stdout, "✔ No changes needed");
            ExitStatus::Success
        }
        Ok(report) => {
            let _ = report.write(stdout);
            ExitStatus::Success
        }
        Err(err) => print_error(stderr, &format!("{err:#}")),
    }
}

/// Collects line-metric violations keyed by match key.
pub(crate) fn line_violations(outcomes: &[FileOutcome]) -> HashMap<String, usize> {
    outcomes.iter().filter_map(line_violation).collect()
}

fn line_violation(outcome: &FileOutcome) -> Option<(String, usize)> {
    if let OutcomeKind::Violation { actual, limit, .. } = &outcome.kind {
        if limit.metric == Metric::Lines {
            return Some((outcome.match_key.clone(), *actual));
        }
    }
    None
}

/// Find files under `scan_paths` that violate the given threshold.
///
/// The temporary config and match keys are rooted at `root` (the discovered
/// config's directory), while only `scan_paths` are walked — so keys stay
/// root-relative without widening the scan beyond what the caller asked for.
pub(crate) fn scan_violations_with_threshold(
    root: &Path,
    scan_paths: &[PathBuf],
    doc: &DocumentMut,
    threshold: usize,
    context: &'static str,
) -> Result<HashMap<String, usize>> {
    let temp_config = build_temp_config(doc, threshold);
    let temp_file = tempfile::NamedTempFile::new_in(root).context("failed to create temp file")?;
    std::io::Write::write_all(&mut &temp_file, temp_config.as_bytes())
        .context("failed to write temp config")?;

    let options = CheckOptions {
        config_path: Some(temp_file.path().to_path_buf()),
        cwd: root.to_path_buf(),
        use_cache: false,
    };

    let output = loq_fs::run_check(scan_paths.to_vec(), options).context(context)?;
    let temp_config_path = temp_file
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp_file.path().to_path_buf());

    let violations = output
        .outcomes
        .iter()
        .filter(|outcome| outcome.path != temp_config_path)
        .filter_map(line_violation)
        .collect();

    Ok(violations)
}

/// Build a temporary config for violation scanning.
/// Copies policy rules but not exact-path line rules managed by baseline.
fn build_temp_config(doc: &DocumentMut, threshold: usize) -> String {
    let mut temp_doc = DocumentMut::new();

    // In a token-default project, keep the token default so token-governed
    // files surface as token violations (which baseline ignores) rather than
    // being mis-scanned as line violations and grandfathered as max_lines
    // rules that would silently shadow the token budget.
    if let Some(tokens) = token_default(doc) {
        temp_doc["default_max_tokens"] = toml_edit::value(tokens);
    } else {
        let threshold_value = i64::try_from(threshold).unwrap_or(i64::MAX);
        temp_doc["default_max_lines"] = toml_edit::value(threshold_value);
    }

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
        let mut policy_rules = toml_edit::ArrayOfTables::new();
        for rule in rules_array {
            if let Some(path_value) = rule.get("path") {
                let paths = extract_paths(path_value);
                let has_token_limit = rule.get("max_tokens").is_some();
                let has_glob_path = paths.iter().any(|p| !is_exact_path(p));
                if has_glob_path || has_token_limit {
                    policy_rules.push(rule.clone());
                }
            }
        }
        if !policy_rules.is_empty() {
            temp_doc["rules"] = Item::ArrayOfTables(policy_rules);
        }
    }

    temp_doc.to_string()
}

/// Returns the project's default token budget, if tokens are the default metric.
///
/// A line default takes precedence (and an explicit `default_max_lines` makes
/// the config line-governed), so this only fires when `default_max_tokens` is
/// set without `default_max_lines`.
fn token_default(doc: &DocumentMut) -> Option<i64> {
    if doc.get("default_max_lines").is_some() {
        return None;
    }
    doc.get("default_max_tokens").and_then(Item::as_integer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::{ConfigOrigin, Limit, MatchBy};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn violation(match_key: &str, limit: Limit, actual: usize) -> FileOutcome {
        FileOutcome {
            path: PathBuf::from(match_key),
            display_path: match_key.into(),
            match_key: match_key.into(),
            config_source: ConfigOrigin::BuiltIn,
            kind: OutcomeKind::Violation {
                limit,
                actual,
                matched_by: MatchBy::Default,
            },
        }
    }

    #[test]
    fn line_violations_keys_by_match_key() {
        let mut pass = violation("src/b.rs", Limit::lines(10), 9);
        pass.kind = OutcomeKind::Pass {
            limit: Limit::lines(10),
            actual: 9,
            matched_by: MatchBy::Default,
        };
        let outcomes = vec![violation("src/a.rs", Limit::lines(10), 12), pass];

        let violations = line_violations(&outcomes);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations.get("src/a.rs"), Some(&12));
    }

    #[test]
    fn line_violations_ignores_token_violations() {
        let outcomes = vec![violation("prompt.md", Limit::tokens(4), 5)];

        assert!(line_violations(&outcomes).is_empty());
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
    fn build_temp_config_keeps_exact_token_rules() {
        let doc: DocumentMut = r#"
default_max_lines = 500

[[rules]]
path = "prompts/build.md"
max_tokens = 4

[[rules]]
path = "src/main.rs"
max_lines = 200
"#
        .parse()
        .unwrap();

        let temp = build_temp_config(&doc, 123);

        assert!(temp.contains("path = \"prompts/build.md\""));
        assert!(temp.contains("max_tokens = 4"));
        assert!(!temp.contains("path = \"src/main.rs\""));
        assert!(!temp.contains("max_lines = 200"));
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

    #[test]
    fn build_temp_config_preserves_token_default() {
        let doc: DocumentMut = "default_max_tokens = 8000\n".parse().unwrap();

        let temp = build_temp_config(&doc, 500);

        assert!(temp.contains("default_max_tokens = 8000"));
        assert!(!temp.contains("default_max_lines"));
    }

    #[test]
    fn build_temp_config_uses_line_threshold_when_lines_are_the_default() {
        let doc: DocumentMut = "default_max_lines = 500\n".parse().unwrap();

        let temp = build_temp_config(&doc, 123);

        assert!(temp.contains("default_max_lines = 123"));
        assert!(!temp.contains("default_max_tokens"));
    }

    #[test]
    fn scan_violations_does_not_include_temp_config_file() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("violates.rs"), "a\nb\n").unwrap();
        let doc = DocumentMut::new();

        let violations = scan_violations_with_threshold(
            temp.path(),
            &[temp.path().to_path_buf()],
            &doc,
            1,
            "baseline scan should succeed",
        )
        .unwrap();

        assert_eq!(violations.len(), 1);
        assert_eq!(violations.get("violates.rs"), Some(&2));
    }

    #[test]
    fn scan_violations_ignores_token_rules() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("prompt.md"), "abcdefghijklmnopq\n").unwrap();
        let doc: DocumentMut = r#"
default_max_lines = 500

[[rules]]
path = "*.md"
max_tokens = 4
"#
        .parse()
        .unwrap();

        let violations = scan_violations_with_threshold(
            temp.path(),
            &[temp.path().to_path_buf()],
            &doc,
            1,
            "baseline scan should succeed",
        )
        .unwrap();

        assert!(violations.is_empty());
    }

    #[test]
    fn scan_violations_preserves_exact_token_rules() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("prompts")).unwrap();
        std::fs::write(temp.path().join("prompts/build.md"), "one\ntwo\n").unwrap();
        let doc: DocumentMut = r#"
default_max_lines = 500

[[rules]]
path = "prompts/build.md"
max_tokens = 4
"#
        .parse()
        .unwrap();

        let violations = scan_violations_with_threshold(
            temp.path(),
            &[temp.path().to_path_buf()],
            &doc,
            1,
            "baseline scan should succeed",
        )
        .unwrap();

        assert!(violations.is_empty());
    }
}
