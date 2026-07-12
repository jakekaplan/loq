//! Line-limit violation collection.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use loq_core::config::{compile_config, LoqConfig};
use loq_core::{FileOutcome, Limit, Metric, OutcomeKind};
use loq_fs::{CheckConfig, CheckOptions};

use crate::exact_limits::is_exact_path;

/// Collects line-metric violations keyed by match key.
pub(crate) fn line_violations(outcomes: &[FileOutcome]) -> HashMap<String, usize> {
    outcomes
        .iter()
        .filter_map(|outcome| match &outcome.kind {
            OutcomeKind::Violation { actual, limit, .. } if limit.metric == Metric::Lines => {
                Some((outcome.match_key.clone(), *actual))
            }
            _ => None,
        })
        .collect()
}

/// Finds line-limit violations under `scan_path`, keyed relative to `root`.
pub(crate) fn scan_line_violations(
    root: &Path,
    scan_path: &Path,
    config_path: &Path,
    config: LoqConfig,
    threshold: usize,
) -> Result<HashMap<String, usize>> {
    let root = root
        .canonicalize()
        .context("failed to resolve config root")?;
    let mut config = config;
    if !matches!(config.default_limit, Some(limit) if limit.metric == Metric::Tokens) {
        config.default_limit = Some(Limit::lines(threshold));
    }
    config.rules.retain(|rule| {
        rule.limit.metric == Metric::Tokens || rule.paths.iter().any(|path| !is_exact_path(path))
    });
    let compiled = compile_config(root.clone(), config, Some(config_path))?;
    let options = CheckOptions {
        config: CheckConfig::Compiled(compiled),
        cwd: root,
        use_cache: false,
    };
    let output = loq_fs::run_check(vec![scan_path.to_path_buf()], options)?;
    Ok(line_violations(&output.outcomes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::config::Rule;
    use tempfile::TempDir;

    #[test]
    fn scan_finds_line_violations() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("violates.rs"), "a\nb\n").unwrap();

        let violations = scan_line_violations(
            temp.path(),
            temp.path(),
            &temp.path().join("loq.toml"),
            LoqConfig::default(),
            1,
        )
        .unwrap();

        assert_eq!(violations.get("violates.rs"), Some(&2));
    }

    #[test]
    fn scan_rejects_invalid_glob() {
        let temp = TempDir::new().unwrap();
        let config = LoqConfig {
            rules: vec![Rule {
                paths: vec!["[".into()],
                limit: Limit::lines(1),
            }],
            ..LoqConfig::default()
        };

        let error = scan_line_violations(
            temp.path(),
            temp.path(),
            &temp.path().join("loq.toml"),
            config,
            1,
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid glob"));
    }

    #[test]
    fn scan_ignores_token_violations() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("prompt.md"), "abcdefghijklmnopq\n").unwrap();
        let config = LoqConfig {
            rules: vec![Rule {
                paths: vec!["*.md".into()],
                limit: Limit::tokens(4),
            }],
            ..LoqConfig::default()
        };

        let violations = scan_line_violations(
            temp.path(),
            temp.path(),
            &temp.path().join("loq.toml"),
            config,
            1,
        )
        .unwrap();

        assert!(violations.is_empty());
    }
}
