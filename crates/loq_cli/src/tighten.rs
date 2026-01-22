//! Tighten command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use termcolor::WriteColor;
use toml_edit::DocumentMut;

use crate::baseline_shared::find_violations;
use crate::cli::TightenArgs;
use crate::config_edit::{
    collect_exact_path_rules, load_doc_or_default, persist_doc, remove_rule, threshold_from_doc,
    update_rule_max_lines,
};
use crate::output::{
    change_style, max_number_width, print_error, write_change_line, ChangeKind, ChangeRow,
};
use crate::ExitStatus;

struct TightenReport {
    changes: Vec<ChangeRow>,
    removed: usize,
}

impl TightenReport {
    fn is_empty(&self) -> bool {
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

    let (mut doc, config_exists) = load_doc_or_default(&config_path)?;
    let threshold = threshold_from_doc(&doc, args.threshold);

    let violations = find_violations(&cwd, &doc, threshold, "tighten check failed")?;
    let existing_rules = collect_exact_path_rules(&doc);
    let report = apply_tighten_changes(&mut doc, &violations, &existing_rules);

    persist_doc(&cwd, &config_path, &doc, config_exists)?;

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
                changes.push(ChangeRow {
                    path: path.clone(),
                    from: Some(*current_limit),
                    to: Some(actual),
                    kind: ChangeKind::Adjusted,
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
    let style = change_style();

    if !report.changes.is_empty() {
        let mut changes: Vec<_> = report.changes.iter().collect();
        changes.sort_by_key(|change| (change.to, change.path.as_str()));

        let width = max_number_width(
            changes
                .iter()
                .flat_map(|change| change.from.into_iter().chain(change.to)),
        );

        for change in changes {
            write_change_line(
                writer,
                &style,
                width,
                change.kind.symbol(),
                change.from,
                change.to,
                &change.path,
            )?;
        }

        let count = report.changes.len();
        writer.set_color(&style.ok)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&style.dimmed)?;
        write!(
            writer,
            "Tightened limits for {count} file{}",
            if count == 1 { "" } else { "s" }
        )?;
        writer.reset()?;
        writeln!(writer)?;
    }

    if report.removed > 0 {
        writer.set_color(&style.ok)?;
        write!(writer, "✔ ")?;
        writer.reset()?;
        writer.set_color(&style.dimmed)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use termcolor::NoColor;

    #[test]
    fn write_report_sorts_by_limit_and_summarizes() {
        let report = TightenReport {
            changes: vec![
                ChangeRow {
                    path: "b.rs".into(),
                    from: Some(200),
                    to: Some(150),
                    kind: ChangeKind::Adjusted,
                },
                ChangeRow {
                    path: "a.rs".into(),
                    from: Some(120),
                    to: Some(110),
                    kind: ChangeKind::Adjusted,
                },
            ],
            removed: 1,
        };

        let mut out = NoColor::new(Vec::new());
        write_report(&mut out, &report).unwrap();
        let output = String::from_utf8(out.into_inner()).unwrap();
        let lines: Vec<_> = output.lines().collect();

        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("120"));
        assert!(lines[0].contains("->"));
        assert!(lines[0].contains("110"));
        assert!(lines[0].contains("a.rs"));
        assert!(lines[1].contains("200"));
        assert!(lines[1].contains("150"));
        assert!(lines[1].contains("b.rs"));
        assert_eq!(lines[2], "✔ Tightened limits for 2 files");
        assert_eq!(lines[3], "✔ Removed limits for 1 file");
    }

    #[test]
    fn write_report_handles_removed_only() {
        let report = TightenReport {
            changes: Vec::new(),
            removed: 2,
        };

        let mut out = NoColor::new(Vec::new());
        write_report(&mut out, &report).unwrap();
        let output = String::from_utf8(out.into_inner()).unwrap();
        assert_eq!(
            output.lines().collect::<Vec<_>>(),
            vec!["✔ Removed limits for 2 files"]
        );
    }

    #[test]
    fn tighten_report_is_empty() {
        let report = TightenReport {
            changes: Vec::new(),
            removed: 0,
        };
        assert!(report.is_empty());

        let report = TightenReport {
            changes: vec![ChangeRow {
                path: "src/lib.rs".into(),
                from: Some(10),
                to: Some(9),
                kind: ChangeKind::Adjusted,
            }],
            removed: 0,
        };
        assert!(!report.is_empty());

        let report = TightenReport {
            changes: Vec::new(),
            removed: 1,
        };
        assert!(!report.is_empty());
    }
}
