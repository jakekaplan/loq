//! Tighten command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use termcolor::{Color, ColorSpec, WriteColor};
use toml_edit::{DocumentMut, Item};

use crate::baseline_shared::find_violations;
use crate::cli::TightenArgs;
use crate::config_edit::{
    collect_exact_path_rules, default_document, remove_rule, update_rule_max_lines,
};
use crate::init::add_to_gitignore;
use crate::output::{format_number, print_error, write_path};
use crate::ExitStatus;

struct TightenChange {
    path: String,
    from: usize,
    to: usize,
}

impl TightenChange {
    const fn delta(&self) -> usize {
        self.from.saturating_sub(self.to)
    }
}

struct TightenReport {
    changes: Vec<TightenChange>,
    removed: usize,
}

impl TightenReport {
    const fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.removed == 0
    }
}

pub fn run_tighten<W1: WriteColor, W2: WriteColor>(
    args: &TightenArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_tighten_inner(args) {
        Ok(report) => {
            if report.is_empty() {
                let _ = writeln!(stdout, "✔ No changes needed");
                return ExitStatus::Success;
            }
            let _ = write_report(stdout, &report);
            ExitStatus::Success
        }
        Err(err) => print_error(stderr, &format!("{err:#}")),
    }
}

fn run_tighten_inner(args: &TightenArgs) -> Result<TightenReport> {
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
    let report = apply_tighten_changes(&mut doc, &violations, &existing_rules);

    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    if !config_exists {
        add_to_gitignore(&cwd);
    }

    Ok(report)
}

fn apply_tighten_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
) -> TightenReport {
    let mut changes = Vec::new();
    let mut removed = 0;

    let mut indices_to_remove: Vec<usize> = Vec::new();

    for (path, (current_limit, idx)) in existing_rules {
        if let Some(&actual) = violations.get(path) {
            if actual < *current_limit {
                update_rule_max_lines(doc, *idx, actual);
                changes.push(TightenChange {
                    path: path.clone(),
                    from: *current_limit,
                    to: actual,
                });
            }
        } else {
            indices_to_remove.push(*idx);
            removed += 1;
        }
    }

    indices_to_remove.sort_by(|a, b| b.cmp(a));
    for idx in indices_to_remove {
        remove_rule(doc, idx);
    }

    TightenReport { changes, removed }
}

fn write_report<W: WriteColor>(writer: &mut W, report: &TightenReport) -> std::io::Result<()> {
    let mut from_spec = ColorSpec::new();
    from_spec.set_fg(Some(Color::Red)).set_bold(true);
    let mut to_spec = ColorSpec::new();
    to_spec.set_fg(Some(Color::Green));
    let mut green_spec = ColorSpec::new();
    green_spec.set_fg(Some(Color::Green));
    let mut dimmed_spec = ColorSpec::new();
    dimmed_spec.set_dimmed(true);

    if !report.changes.is_empty() {
        let mut changes: Vec<_> = report.changes.iter().collect();
        changes.sort_by_key(|change| (change.delta(), change.path.as_str()));

        let width = changes.iter().fold(6, |current, change| {
            let from_len = format_number(change.from).len();
            let to_len = format_number(change.to).len();
            current.max(from_len).max(to_len)
        });

        for change in changes {
            let from_str = format_number(change.from);
            let to_str = format_number(change.to);
            writer.set_color(&from_spec)?;
            write!(writer, "{from_str:>width$}")?;
            writer.reset()?;
            writer.set_color(&dimmed_spec)?;
            write!(writer, " -> ")?;
            writer.reset()?;
            writer.set_color(&to_spec)?;
            write!(writer, "{to_str:<width$}")?;
            writer.reset()?;
            write!(writer, " ")?;
            write_path(writer, &change.path)?;
            writeln!(writer)?;
        }

        let count = report.changes.len();
        writer.set_color(&green_spec)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&dimmed_spec)?;
        write!(
            writer,
            "Tightened limits for {count} file{}",
            if count == 1 { "" } else { "s" }
        )?;
        writer.reset()?;
        writeln!(writer)?;
    }

    if report.removed > 0 {
        writer.set_color(&green_spec)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&dimmed_spec)?;
        write!(
            writer,
            "Removed limits for {} file{}",
            report.removed,
            if report.removed == 1 { "" } else { "s" }
        )?;
        writer.reset()?;
        writeln!(writer)?;
    }

    Ok(())
}
