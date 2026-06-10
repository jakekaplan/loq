//! Baseline command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use termcolor::WriteColor;
use toml_edit::DocumentMut;

use crate::baseline_shared::{finish, scan_violations_with_threshold, ChangeReport};
use crate::cli::BaselineArgs;
use crate::config_edit::{load_doc_or_default, locate_config, persist_doc, threshold_from_doc};
use crate::exact_limits::{self, ExactLimit, ExactLimits};
use crate::output::{
    change_style, max_formatted_width, plural, write_change_row, write_ok_line, ChangeKind,
    ChangeRow, ChangeStyle,
};
use crate::ExitStatus;

struct BaselineReport {
    changes: Vec<ChangeRow>,
}

impl ChangeReport for BaselineReport {
    fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    fn write<W: WriteColor>(&self, writer: &mut W) -> std::io::Result<()> {
        write_report(writer, self)
    }
}

pub fn run_baseline<W1: WriteColor, W2: WriteColor>(
    args: &BaselineArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    finish(run_baseline_inner(args), stdout, stderr)
}

fn run_baseline_inner(args: &BaselineArgs) -> Result<BaselineReport> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (config_path, root) = locate_config(&cwd);

    let (mut doc, config_exists) = load_doc_or_default(&config_path)?;
    let threshold = threshold_from_doc(&doc, args.threshold);
    let violations = scan_violations_with_threshold(
        &root,
        std::slice::from_ref(&cwd),
        &doc,
        threshold,
        "baseline check failed",
    )?;
    let existing_rules = ExactLimits::collect(&doc);
    let report = apply_baseline_changes(&mut doc, &violations, &existing_rules);

    persist_doc(&root, &config_path, &doc, config_exists)?;

    Ok(report)
}

/// Apply baseline changes to the document.
fn apply_baseline_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &ExactLimits,
) -> BaselineReport {
    let mut changes = Vec::new();
    let mut limits_to_remove: Vec<ExactLimit> = Vec::new();

    for (path, limit) in existing_rules.iter() {
        if let Some(&actual) = violations.get(path) {
            if actual != limit.max_lines {
                exact_limits::update_limit(doc, limit, actual);
                changes.push(ChangeRow {
                    path: path.to_string(),
                    from: Some(limit.max_lines),
                    to: Some(actual),
                    kind: ChangeKind::Updated,
                });
            }
        } else {
            limits_to_remove.push(limit);
            changes.push(ChangeRow {
                path: path.to_string(),
                from: Some(limit.max_lines),
                to: None,
                kind: ChangeKind::Removed,
            });
        }
    }

    exact_limits::remove_limits(doc, limits_to_remove);

    let mut new_violations: Vec<_> = violations
        .iter()
        .filter(|(path, _)| !existing_rules.contains_path(path))
        .collect();
    new_violations.sort_by_key(|(path, _)| *path);

    for (path, &actual) in new_violations {
        exact_limits::set_limit(doc, existing_rules, path, actual);
        changes.push(ChangeRow {
            path: (*path).clone(),
            from: None,
            to: Some(actual),
            kind: ChangeKind::Added,
        });
    }

    BaselineReport { changes }
}

fn write_report<W: WriteColor>(writer: &mut W, report: &BaselineReport) -> std::io::Result<()> {
    if report.changes.is_empty() {
        return Ok(());
    }

    let style = change_style();

    let mut changes: Vec<_> = report.changes.iter().collect();
    changes.sort_by_key(|change| (change_sort_value(change), change.path.as_str()));
    let width = max_formatted_width(
        changes
            .iter()
            .flat_map(|change| change.from.into_iter().chain(change.to)),
    );
    let counts = write_change_lines(writer, &changes, width, &style)?;

    if counts.added > 0 || counts.updated > 0 {
        write_ok_line(writer, &style, &capitalize_first(&change_summary(&counts)))?;
    }

    if counts.removed > 0 {
        write_ok_line(
            writer,
            &style,
            &format!(
                "Removed limits for {} file{}",
                counts.removed,
                plural(counts.removed)
            ),
        )?;
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

fn change_sort_value(change: &ChangeRow) -> usize {
    match change.kind {
        ChangeKind::Removed => change.from.unwrap_or(0),
        _ => change.to.or(change.from).unwrap_or(0),
    }
}

struct ChangeCounts {
    added: usize,
    updated: usize,
    removed: usize,
}

fn write_change_lines<W: WriteColor>(
    writer: &mut W,
    changes: &[&ChangeRow],
    width: usize,
    style: &ChangeStyle,
) -> std::io::Result<ChangeCounts> {
    let mut counts = ChangeCounts {
        added: 0,
        updated: 0,
        removed: 0,
    };

    for change in changes {
        match change.kind {
            ChangeKind::Added => counts.added += 1,
            ChangeKind::Updated => counts.updated += 1,
            ChangeKind::Removed => counts.removed += 1,
            ChangeKind::Adjusted => {}
        }

        let symbol = change.kind.symbol();
        write_change_row(
            writer,
            style,
            width,
            symbol,
            change.from,
            change.to,
            &change.path,
        )?;
    }

    Ok(counts)
}

fn change_summary(counts: &ChangeCounts) -> String {
    let mut parts = Vec::new();
    if counts.added > 0 {
        parts.push(format!(
            "added {} file{}",
            counts.added,
            plural(counts.added)
        ));
    }
    if counts.updated > 0 {
        parts.push(format!(
            "updated {} file{}",
            counts.updated,
            plural(counts.updated)
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
            changes: vec![ChangeRow {
                path: "src/lib.rs".into(),
                from: Some(10),
                to: Some(12),
                kind: ChangeKind::Updated,
            }],
        };
        assert!(!report.is_empty());

        let report = BaselineReport {
            changes: vec![ChangeRow {
                path: "src/old.rs".into(),
                from: Some(10),
                to: None,
                kind: ChangeKind::Removed,
            }],
        };
        assert!(!report.is_empty());
    }

    #[test]
    fn write_report_sorts_by_limit_and_summarizes() {
        let report = BaselineReport {
            changes: vec![
                ChangeRow {
                    path: "b.rs".into(),
                    from: Some(200),
                    to: Some(150),
                    kind: ChangeKind::Updated,
                },
                ChangeRow {
                    path: "a.rs".into(),
                    from: None,
                    to: Some(120),
                    kind: ChangeKind::Added,
                },
                ChangeRow {
                    path: "c.rs".into(),
                    from: Some(300),
                    to: None,
                    kind: ChangeKind::Removed,
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
            changes: vec![ChangeRow {
                path: "src/old.rs".into(),
                from: Some(10),
                to: None,
                kind: ChangeKind::Removed,
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
