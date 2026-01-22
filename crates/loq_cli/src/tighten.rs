//! Tighten command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use termcolor::WriteColor;
use toml_edit::{DocumentMut, Item};

use crate::baseline_shared::{find_violations, write_stats, BaselineStats};
use crate::cli::TightenArgs;
use crate::config_edit::{
    add_rule, collect_exact_path_rules, default_document, remove_rule, update_rule_max_lines,
};
use crate::init::add_to_gitignore;
use crate::output::print_error;
use crate::ExitStatus;

pub fn run_tighten<W1: WriteColor, W2: WriteColor>(
    args: &TightenArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_tighten_inner(args) {
        Ok(stats) => {
            let _ = write_stats(stdout, &stats);
            ExitStatus::Success
        }
        Err(err) => print_error(stderr, &format!("{err:#}")),
    }
}

fn run_tighten_inner(args: &TightenArgs) -> Result<BaselineStats> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config_path = cwd.join("loq.toml");

    let config_exists = config_path.exists();
    let mut doc: DocumentMut = if config_exists {
        let config_text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        config_text
            .parse()
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    } else {
        default_document()
    };

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let threshold = args.threshold.unwrap_or_else(|| {
        doc.get("default_max_lines")
            .and_then(Item::as_integer)
            .map_or(500, |v| v as usize)
    });

    let violations = find_violations(&cwd, &doc, threshold, "tighten check failed")?;
    let existing_rules = collect_exact_path_rules(&doc);
    let stats = apply_tighten_changes(&mut doc, &violations, &existing_rules);

    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    if !config_exists {
        add_to_gitignore(&cwd);
    }

    Ok(stats)
}

fn apply_tighten_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
) -> BaselineStats {
    let mut stats = BaselineStats {
        added: 0,
        updated: 0,
        removed: 0,
    };

    let mut indices_to_remove: Vec<usize> = Vec::new();

    for (path, (current_limit, idx)) in existing_rules {
        if let Some(&actual) = violations.get(path) {
            if actual < *current_limit {
                update_rule_max_lines(doc, *idx, actual);
                stats.updated += 1;
            }
        } else {
            indices_to_remove.push(*idx);
            stats.removed += 1;
        }
    }

    indices_to_remove.sort_by(|a, b| b.cmp(a));
    for idx in indices_to_remove {
        remove_rule(doc, idx);
    }

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
