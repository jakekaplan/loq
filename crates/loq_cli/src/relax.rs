//! Relax command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use loq_fs::CheckOptions;
use termcolor::WriteColor;
use toml_edit::DocumentMut;

use crate::cli::RelaxArgs;
use crate::config_edit::{
    add_rule, collect_exact_path_rules, load_doc_or_default, persist_doc, update_rule_max_lines,
};
use crate::output::{
    change_style, max_number_width, print_error, write_change_line, ChangeKind, ChangeRow,
};
use crate::ExitStatus;
use loq_fs::normalize_display_path;

struct RelaxReport {
    changes: Vec<ChangeRow>,
}

impl RelaxReport {
    fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

pub fn run_relax<W1: WriteColor, W2: WriteColor>(
    args: &RelaxArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_relax_inner(args) {
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

fn run_relax_inner(args: &RelaxArgs) -> Result<RelaxReport> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config_path = cwd.join("loq.toml");
    let config_exists = config_path.exists();

    let paths = if args.files.is_empty() {
        vec![cwd.clone()]
    } else {
        args.files.clone()
    };

    let options = CheckOptions {
        config_path: config_exists.then(|| config_path.clone()),
        cwd: cwd.clone(),
        use_cache: false,
    };

    let output = loq_fs::run_check(paths, options).context("relax check failed")?;
    let violations = collect_violations(&output.outcomes);

    if violations.is_empty() {
        return Ok(RelaxReport {
            changes: Vec::new(),
        });
    }

    let (mut doc, config_exists_for_write) = load_doc_or_default(&config_path)?;

    let existing_rules = collect_exact_path_rules(&doc);
    let changes = apply_relax_changes(&mut doc, &violations, &existing_rules, args.extra);

    persist_doc(&cwd, &config_path, &doc, config_exists_for_write)?;

    Ok(RelaxReport { changes })
}

fn collect_violations(outcomes: &[loq_core::FileOutcome]) -> HashMap<String, usize> {
    let mut violations = HashMap::new();
    for outcome in outcomes {
        if let loq_core::OutcomeKind::Violation { actual, .. } = outcome.kind {
            let path = normalize_display_path(&outcome.display_path);
            violations.insert(path, actual);
        }
    }
    violations
}

fn apply_relax_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
    buffer: usize,
) -> Vec<ChangeRow> {
    let mut paths: Vec<_> = violations.iter().collect();
    paths.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut changes = Vec::new();
    for (path, &actual) in paths {
        let new_limit = actual.saturating_add(buffer);
        if let Some((_current_limit, idx)) = existing_rules.get(path) {
            update_rule_max_lines(doc, *idx, new_limit);
        } else {
            add_rule(doc, path, new_limit);
        }
        changes.push(ChangeRow {
            path: path.clone(),
            from: Some(actual),
            to: Some(new_limit),
            kind: ChangeKind::Adjusted,
        });
    }

    changes
}

fn write_report<W: WriteColor>(writer: &mut W, report: &RelaxReport) -> std::io::Result<()> {
    let count = report.changes.len();
    let style = change_style();

    let mut changes: Vec<_> = report.changes.iter().collect();
    changes.sort_by_key(|change| (change.to, change.from, change.path.as_str()));
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
    writer.set_color(&style.ok)?;
    write!(writer, "✔ ")?;
    writer.reset()?;
    writer.set_color(&style.dimmed)?;
    write!(
        writer,
        "Relaxed limits for {count} file{}",
        if count == 1 { "" } else { "s" }
    )?;
    writer.reset()?;
    writeln!(writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::report::OutcomeKind;
    use loq_core::{ConfigOrigin, MatchBy};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use termcolor::NoColor;
    use toml_edit::Item;

    #[test]
    fn collect_violations_filters_and_normalizes() {
        let outcomes = vec![
            loq_core::FileOutcome {
                path: PathBuf::from("/tmp/a"),
                display_path: "./src/a.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Violation {
                    limit: 10,
                    actual: 12,
                    matched_by: MatchBy::Default,
                },
            },
            loq_core::FileOutcome {
                path: PathBuf::from("/tmp/b"),
                display_path: "src/b.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Pass {
                    limit: 10,
                    actual: 9,
                    matched_by: MatchBy::Default,
                },
            },
        ];

        let violations = collect_violations(&outcomes);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations.get("src/a.rs"), Some(&12));
    }

    #[test]
    fn apply_relax_changes_updates_and_adds_rules() {
        let mut doc: DocumentMut = r#"
[[rules]]
path = "src/a.rs"
max_lines = 10
"#
        .parse()
        .unwrap();

        let mut violations = HashMap::new();
        violations.insert("src/a.rs".to_string(), 12);
        violations.insert("src/b.rs".to_string(), 20);

        let existing_rules = collect_exact_path_rules(&doc);
        let changes = apply_relax_changes(&mut doc, &violations, &existing_rules, 5);
        assert_eq!(changes.len(), 2);

        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        assert_eq!(rules.len(), 2);
        let first = rules.get(0).unwrap();
        let second = rules.get(1).unwrap();
        assert_eq!(first.get("max_lines").and_then(Item::as_integer), Some(17));
        assert_eq!(second.get("max_lines").and_then(Item::as_integer), Some(25));
    }

    #[test]
    fn write_report_formats_output() {
        let report = RelaxReport {
            changes: vec![ChangeRow {
                path: "src/file.rs".into(),
                from: Some(1_000),
                to: Some(1_050),
                kind: ChangeKind::Adjusted,
            }],
        };

        let mut out = NoColor::new(Vec::new());
        write_report(&mut out, &report).unwrap();
        let output = String::from_utf8(out.into_inner()).unwrap();
        let mut lines = output.lines();
        let change_line = lines.next().unwrap();
        assert!(change_line.contains("1_000"));
        assert!(change_line.contains("->"));
        assert!(change_line.contains("1_050"));
        assert!(change_line.contains("src/file.rs"));
        assert_eq!(lines.next(), Some("✔ Relaxed limits for 1 file"));
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_report_formats_plural_summary() {
        let report = RelaxReport {
            changes: vec![
                ChangeRow {
                    path: "src/a.rs".into(),
                    from: Some(10),
                    to: Some(20),
                    kind: ChangeKind::Adjusted,
                },
                ChangeRow {
                    path: "src/b.rs".into(),
                    from: Some(30),
                    to: Some(40),
                    kind: ChangeKind::Adjusted,
                },
            ],
        };

        let mut out = NoColor::new(Vec::new());
        write_report(&mut out, &report).unwrap();
        let output = String::from_utf8(out.into_inner()).unwrap();
        assert_eq!(output.lines().last(), Some("✔ Relaxed limits for 2 files"));
    }
}
