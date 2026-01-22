//! Baseline command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use termcolor::{Color, ColorSpec, WriteColor};
use toml_edit::{DocumentMut, Item};

use crate::baseline_shared::find_violations;
use crate::cli::BaselineArgs;
use crate::config_edit::{
    add_rule, collect_exact_path_rules, default_document, remove_rule, update_rule_max_lines,
};
use crate::init::add_to_gitignore;
use crate::output::{format_number, print_error, write_path};
use crate::ExitStatus;

enum BaselineChangeKind {
    Added,
    Updated,
    Removed,
}

struct BaselineChange {
    path: String,
    from: usize,
    to: Option<usize>,
    kind: BaselineChangeKind,
}

struct BaselineReport {
    changes: Vec<BaselineChange>,
}

impl BaselineReport {
    fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

pub fn run_baseline<W1: WriteColor, W2: WriteColor>(
    args: &BaselineArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_baseline_inner(args) {
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

fn run_baseline_inner(args: &BaselineArgs) -> Result<BaselineReport> {
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
    let violations = find_violations(&cwd, &doc, threshold, "baseline check failed")?;

    // Step 4: Collect existing exact-path rules (baseline candidates)
    let existing_rules = collect_exact_path_rules(&doc);

    // Step 5: Compute changes
    let report = apply_baseline_changes(&mut doc, &violations, &existing_rules);

    // Step 6: Write config back
    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    if !config_exists {
        add_to_gitignore(&cwd);
    }

    Ok(report)
}

/// Apply baseline changes to the document.
fn apply_baseline_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
) -> BaselineReport {
    let mut changes = Vec::new();

    // Track which indices to remove (in reverse order to maintain correctness)
    let mut indices_to_remove: Vec<usize> = Vec::new();

    // Process existing exact-path rules
    for (path, (current_limit, idx)) in existing_rules {
        if let Some(&actual) = violations.get(path) {
            // File still violates - reset to current size if it changed
            if actual != *current_limit {
                update_rule_max_lines(doc, *idx, actual);
                changes.push(BaselineChange {
                    path: path.clone(),
                    from: *current_limit,
                    to: Some(actual),
                    kind: BaselineChangeKind::Updated,
                });
            }
        } else {
            // File is now compliant (under threshold) - remove the rule
            indices_to_remove.push(*idx);
            changes.push(BaselineChange {
                path: path.clone(),
                from: *current_limit,
                to: None,
                kind: BaselineChangeKind::Removed,
            });
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
        changes.push(BaselineChange {
            path: (*path).clone(),
            from: actual,
            to: Some(actual),
            kind: BaselineChangeKind::Added,
        });
    }

    BaselineReport { changes }
}

fn write_report<W: WriteColor>(writer: &mut W, report: &BaselineReport) -> std::io::Result<()> {
    if report.changes.is_empty() {
        return Ok(());
    }

    let mut from_spec = ColorSpec::new();
    from_spec.set_fg(Some(Color::Red)).set_bold(true);
    let mut to_spec = ColorSpec::new();
    to_spec.set_fg(Some(Color::Green));
    let mut green_spec = ColorSpec::new();
    green_spec.set_fg(Some(Color::Green));
    let mut dimmed_spec = ColorSpec::new();
    dimmed_spec.set_dimmed(true);

    let mut changes: Vec<_> = report.changes.iter().collect();
    changes.sort_by_key(|change| (change_sort_value(change), change.path.as_str()));
    let width = max_width(&changes);
    let counts = write_change_lines(writer, &changes, width, &from_spec, &to_spec, &dimmed_spec)?;

    if counts.added > 0 || counts.updated > 0 {
        writer.set_color(&green_spec)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&dimmed_spec)?;
        let output = capitalize_first(&change_summary(&counts));
        write!(writer, "{output}")?;
        writer.reset()?;
        writeln!(writer)?;
    }

    if counts.removed > 0 {
        writer.set_color(&green_spec)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&dimmed_spec)?;
        write!(
            writer,
            "Removed limits for {} file{}",
            counts.removed,
            if counts.removed == 1 { "" } else { "s" }
        )?;
        writer.reset()?;
        writeln!(writer)?;
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

fn change_sort_value(change: &BaselineChange) -> usize {
    match change.kind {
        BaselineChangeKind::Removed => change.from,
        _ => change.to.unwrap_or(change.from),
    }
}

const fn display_from(change: &BaselineChange) -> Option<usize> {
    match change.kind {
        BaselineChangeKind::Added => None,
        _ => Some(change.from),
    }
}

struct ChangeCounts {
    added: usize,
    updated: usize,
    removed: usize,
}

fn max_width(changes: &[&BaselineChange]) -> usize {
    changes.iter().fold(6, |current, change| {
        let from_len = display_from(change).map_or(1, |from| format_number(from).len());
        let to_len = change.to.map_or(1, |to| format_number(to).len());
        current.max(from_len).max(to_len)
    })
}

fn write_change_lines<W: WriteColor>(
    writer: &mut W,
    changes: &[&BaselineChange],
    width: usize,
    from_spec: &ColorSpec,
    to_spec: &ColorSpec,
    dimmed_spec: &ColorSpec,
) -> std::io::Result<ChangeCounts> {
    let mut counts = ChangeCounts {
        added: 0,
        updated: 0,
        removed: 0,
    };

    for change in changes {
        match change.kind {
            BaselineChangeKind::Added => counts.added += 1,
            BaselineChangeKind::Updated => counts.updated += 1,
            BaselineChangeKind::Removed => counts.removed += 1,
        }

        let symbol = match change.kind {
            BaselineChangeKind::Added => "+",
            BaselineChangeKind::Updated => "~",
            BaselineChangeKind::Removed => "-",
        };
        let from_value = display_from(change);
        let from_str = from_value.map_or_else(|| "-".to_string(), format_number);
        let to_str = change.to.map_or_else(|| "-".to_string(), format_number);

        writer.set_color(dimmed_spec)?;
        write!(writer, "{symbol} ")?;
        writer.reset()?;
        if from_value.is_some() {
            writer.set_color(from_spec)?;
        } else {
            writer.set_color(dimmed_spec)?;
        }
        write!(writer, "{from_str:>width$}")?;
        writer.reset()?;
        writer.set_color(dimmed_spec)?;
        write!(writer, " -> ")?;
        writer.reset()?;
        if change.to.is_some() {
            writer.set_color(to_spec)?;
        } else {
            writer.set_color(dimmed_spec)?;
        }
        write!(writer, "{to_str:<width$}")?;
        writer.reset()?;
        write!(writer, " ")?;
        write_path(writer, &change.path)?;
        writeln!(writer)?;
    }

    Ok(counts)
}

fn change_summary(counts: &ChangeCounts) -> String {
    let mut parts = Vec::new();
    if counts.added > 0 {
        parts.push(format!(
            "added {} file{}",
            counts.added,
            if counts.added == 1 { "" } else { "s" }
        ));
    }
    if counts.updated > 0 {
        parts.push(format!(
            "updated {} file{}",
            counts.updated,
            if counts.updated == 1 { "" } else { "s" }
        ));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use termcolor::NoColor;

    #[test]
    fn baseline_report_is_empty() {
        let report = BaselineReport {
            changes: Vec::new(),
        };
        assert!(report.is_empty());

        let report = BaselineReport {
            changes: vec![BaselineChange {
                path: "src/lib.rs".into(),
                from: 10,
                to: Some(12),
                kind: BaselineChangeKind::Updated,
            }],
        };
        assert!(!report.is_empty());

        let report = BaselineReport {
            changes: vec![BaselineChange {
                path: "src/old.rs".into(),
                from: 10,
                to: None,
                kind: BaselineChangeKind::Removed,
            }],
        };
        assert!(!report.is_empty());
    }

    #[test]
    fn write_report_sorts_by_limit_and_summarizes() {
        let report = BaselineReport {
            changes: vec![
                BaselineChange {
                    path: "b.rs".into(),
                    from: 200,
                    to: Some(150),
                    kind: BaselineChangeKind::Updated,
                },
                BaselineChange {
                    path: "a.rs".into(),
                    from: 120,
                    to: Some(120),
                    kind: BaselineChangeKind::Added,
                },
                BaselineChange {
                    path: "c.rs".into(),
                    from: 300,
                    to: None,
                    kind: BaselineChangeKind::Removed,
                },
            ],
        };

        let mut out = NoColor::new(Vec::new());
        write_report(&mut out, &report).unwrap();
        let output = String::from_utf8(out.into_inner()).unwrap();
        let lines: Vec<_> = output.lines().collect();

        assert_eq!(lines.len(), 5);
        let added = lines[0].split_whitespace().collect::<Vec<_>>();
        assert_eq!(added, vec!["+", "-", "->", "120", "a.rs"]);
        let updated = lines[1].split_whitespace().collect::<Vec<_>>();
        assert_eq!(updated, vec!["~", "200", "->", "150", "b.rs"]);
        let removed = lines[2].split_whitespace().collect::<Vec<_>>();
        assert_eq!(removed, vec!["-", "300", "->", "-", "c.rs"]);
        assert_eq!(lines[3], "✔ Added 1 file, updated 1 file");
        assert_eq!(lines[4], "✔ Removed limits for 1 file");
    }

    #[test]
    fn write_report_handles_removed_only() {
        let report = BaselineReport {
            changes: vec![BaselineChange {
                path: "src/old.rs".into(),
                from: 10,
                to: None,
                kind: BaselineChangeKind::Removed,
            }],
        };

        let mut out = NoColor::new(Vec::new());
        write_report(&mut out, &report).unwrap();
        let output = String::from_utf8(out.into_inner()).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        let removed = lines[0].split_whitespace().collect::<Vec<_>>();
        assert_eq!(removed, vec!["-", "10", "->", "-", "src/old.rs"]);
        assert_eq!(lines[1], "✔ Removed limits for 1 file");
    }

    #[test]
    fn capitalize_first_handles_empty() {
        assert_eq!(capitalize_first(""), "");
    }
}
