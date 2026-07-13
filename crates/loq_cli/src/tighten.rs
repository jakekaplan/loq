//! Tighten command implementation.

use std::collections::HashMap;

use anyhow::{Context, Result};
use termcolor::WriteColor;
use toml_edit::DocumentMut;

use crate::cli::TightenArgs;
use crate::config_edit::{config_path_and_root, line_threshold, load_doc_or_default, persist_doc};
use crate::exact_limits::{self, ExactLimit, ExactLimits};
use crate::line_violations::scan_line_violations;
use crate::output::{
    change_style, change_width, plural, print_error, write_change, write_ok_line, Change,
};
use crate::ExitStatus;

struct TightenReport {
    changes: Vec<Change>,
    removed: usize,
}

pub fn run_tighten<W1: WriteColor, W2: WriteColor>(
    args: &TightenArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_tighten_inner(args) {
        Ok(report) if report.changes.is_empty() && report.removed == 0 => {
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

fn run_tighten_inner(args: &TightenArgs) -> Result<TightenReport> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let (config_path, root) = config_path_and_root(&cwd)?;

    let (mut doc, config_exists) = load_doc_or_default(&config_path)?;
    let config = loq_core::parse_config(&config_path, &doc.to_string())?;
    let threshold = line_threshold(&config, args.threshold);
    let violations = scan_line_violations(&root, &cwd, &config_path, config, threshold)
        .context("tighten check failed")?;
    let existing_rules = ExactLimits::collect(&doc);
    let scope = loq_fs::PathIdentity::new(&cwd, &root, &root).match_key;
    let report = apply_tighten_changes(&mut doc, &violations, &existing_rules, &scope);

    persist_doc(&root, &config_path, &doc, config_exists)?;

    Ok(report)
}

fn apply_tighten_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &ExactLimits,
    scope: &str,
) -> TightenReport {
    let mut changes = Vec::new();
    let mut limits_to_remove: Vec<ExactLimit> = Vec::new();

    for (path, limit) in existing_rules.within(scope) {
        if let Some(&actual) = violations.get(path) {
            if actual < limit.max_lines {
                exact_limits::update_limit(doc, limit, actual);
                changes.push(Change::Adjusted {
                    path: path.to_string(),
                    from: limit.max_lines,
                    to: actual,
                });
            }
        } else {
            limits_to_remove.push(limit);
        }
    }

    let removed = limits_to_remove.len();
    exact_limits::remove_limits(doc, limits_to_remove);

    TightenReport { changes, removed }
}

fn write_report<W: WriteColor>(writer: &mut W, report: &TightenReport) -> std::io::Result<()> {
    let style = change_style();

    if !report.changes.is_empty() {
        let mut changes: Vec<_> = report.changes.iter().collect();
        changes.sort_by_key(|change| (change.sort_value(), change.path()));

        let width = change_width(&changes);

        for change in changes {
            write_change(writer, &style, width, change)?;
        }

        let count = report.changes.len();
        write_ok_line(
            writer,
            &style,
            &format!("Tightened limits for {count} file{}", plural(count)),
        )?;
    }

    if report.removed > 0 {
        write_ok_line(
            writer,
            &style,
            &format!(
                "Removed limits for {} file{}",
                report.removed,
                plural(report.removed)
            ),
        )?;
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
                Change::Adjusted {
                    path: "b.rs".into(),
                    from: 200,
                    to: 150,
                },
                Change::Adjusted {
                    path: "a.rs".into(),
                    from: 120,
                    to: 110,
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
}
