//! Baseline command implementation.

use std::collections::HashMap;

use anyhow::{Context, Result};
use termcolor::WriteColor;
use toml_edit::DocumentMut;

use crate::cli::BaselineArgs;
use crate::config_edit::{config_path_and_root, line_threshold, load_doc_or_default, persist_doc};
use crate::exact_limits::{self, ExactLimit, ExactLimits};
use crate::line_violations::scan_line_violations;
use crate::output::{
    change_style, change_width, plural, print_error, write_change, write_ok_line, Change,
    ChangeStyle,
};
use crate::ExitStatus;

struct BaselineReport {
    changes: Vec<Change>,
}

pub fn run_baseline<W1: WriteColor, W2: WriteColor>(
    args: &BaselineArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_baseline_inner(args) {
        Ok(report) if report.changes.is_empty() => {
            let _ = writeln!(stdout, "✔ No changes needed");
            ExitStatus::Success
        }
        Ok(report) => {
            let _ = write_report(stdout, &report);
            ExitStatus::Success
        }
        Err(err) => print_error(stderr, &format!("{err:#}")),
    }
}

fn run_baseline_inner(args: &BaselineArgs) -> Result<BaselineReport> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let (config_path, root) = config_path_and_root(&cwd)?;

    let (mut doc, config_exists) = load_doc_or_default(&config_path)?;
    let config = loq_core::parse_config(&config_path, &doc.to_string())?;
    let threshold = line_threshold(&config, args.threshold);
    let violations = scan_line_violations(&root, &cwd, &config_path, config, threshold)
        .context("baseline check failed")?;
    let existing_rules = ExactLimits::collect(&doc);
    let scope = loq_fs::PathIdentity::new(&cwd, &root, &root).match_key;
    let report = apply_baseline_changes(&mut doc, &violations, &existing_rules, &scope);

    persist_doc(&root, &config_path, &doc, config_exists)?;

    Ok(report)
}

fn apply_baseline_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &ExactLimits,
    scope: &str,
) -> BaselineReport {
    let mut changes = Vec::new();
    let mut limits_to_remove: Vec<ExactLimit> = Vec::new();

    for (path, limit) in existing_rules.within(scope) {
        if let Some(&actual) = violations.get(path) {
            if actual != limit.max_lines {
                exact_limits::update_limit(doc, limit, actual);
                changes.push(Change::Updated {
                    path: path.to_string(),
                    from: limit.max_lines,
                    to: actual,
                });
            }
        } else {
            limits_to_remove.push(limit);
            changes.push(Change::Removed {
                path: path.to_string(),
                from: limit.max_lines,
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
        changes.push(Change::Added {
            path: (*path).clone(),
            to: actual,
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
    changes.sort_by_key(|change| (change.sort_value(), change.path()));
    let width = change_width(&changes);
    let counts = write_change_lines(writer, &changes, width, &style)?;

    if counts.added > 0 || counts.updated > 0 {
        write_ok_line(writer, &style, &change_summary(&counts))?;
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

struct ChangeCounts {
    added: usize,
    updated: usize,
    removed: usize,
}

fn write_change_lines<W: WriteColor>(
    writer: &mut W,
    changes: &[&Change],
    width: usize,
    style: &ChangeStyle,
) -> std::io::Result<ChangeCounts> {
    let mut counts = ChangeCounts {
        added: 0,
        updated: 0,
        removed: 0,
    };

    for change in changes {
        match change {
            Change::Added { .. } => counts.added += 1,
            Change::Updated { .. } => counts.updated += 1,
            Change::Removed { .. } => counts.removed += 1,
            Change::Adjusted { .. } => {}
        }

        write_change(writer, style, width, change)?;
    }

    Ok(counts)
}

fn change_summary(counts: &ChangeCounts) -> String {
    if counts.added == 0 {
        return format!("Updated {} file{}", counts.updated, plural(counts.updated));
    }
    if counts.updated == 0 {
        return format!("Added {} file{}", counts.added, plural(counts.added));
    }
    format!(
        "Added {} file{}, updated {} file{}",
        counts.added,
        plural(counts.added),
        counts.updated,
        plural(counts.updated)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use termcolor::NoColor;

    #[test]
    fn write_report_sorts_by_limit_and_summarizes() {
        let report = BaselineReport {
            changes: vec![
                Change::Updated {
                    path: "b.rs".into(),
                    from: 200,
                    to: 150,
                },
                Change::Added {
                    path: "a.rs".into(),
                    to: 120,
                },
                Change::Removed {
                    path: "c.rs".into(),
                    from: 300,
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
            changes: vec![Change::Removed {
                path: "src/old.rs".into(),
                from: 10,
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
}
