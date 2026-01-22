//! Baseline command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use termcolor::WriteColor;
use toml_edit::DocumentMut;

use crate::baseline_shared::find_violations;
use crate::cli::BaselineArgs;
use crate::config_edit::{
    add_rule, collect_exact_path_rules, load_doc_or_default, persist_doc, remove_rule,
    threshold_from_doc, update_rule_max_lines,
};
use crate::output::{change_style, max_number_width, print_error, write_change_line};
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

    // Step 1: Read and parse the config file (or create defaults if missing)
    let (mut doc, config_exists) = load_doc_or_default(&config_path)?;

    // Step 2: Determine threshold (--threshold or default_max_lines from config)
    let threshold = threshold_from_doc(&doc, args.threshold);

    // Step 3: Run check to find violations (respects config's exclude and gitignore settings)
    let violations = find_violations(&cwd, &doc, threshold, "baseline check failed")?;

    // Step 4: Collect existing exact-path rules (baseline candidates)
    let existing_rules = collect_exact_path_rules(&doc);

    // Step 5: Compute changes
    let report = apply_baseline_changes(&mut doc, &violations, &existing_rules);

    // Step 6: Write config back
    persist_doc(&cwd, &config_path, &doc, config_exists)?;

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

    let style = change_style();

    let mut changes: Vec<_> = report.changes.iter().collect();
    changes.sort_by_key(|change| (change_sort_value(change), change.path.as_str()));
    let width = max_number_width(
        changes
            .iter()
            .flat_map(|change| display_from(change).into_iter().chain(change.to)),
    );
    let counts = write_change_lines(writer, &changes, width, &style)?;

    if counts.added > 0 || counts.updated > 0 {
        writer.set_color(&style.ok)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&style.dimmed)?;
        let output = capitalize_first(&change_summary(&counts));
        write!(writer, "{output}")?;
        writer.reset()?;
        writeln!(writer)?;
    }

    if counts.removed > 0 {
        writer.set_color(&style.ok)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&style.dimmed)?;
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

fn write_change_lines<W: WriteColor>(
    writer: &mut W,
    changes: &[&BaselineChange],
    width: usize,
    style: &crate::output::ChangeStyle,
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
        write_change_line(
            writer,
            style,
            width,
            Some(symbol),
            from_value,
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
