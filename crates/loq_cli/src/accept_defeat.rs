//! Accept-defeat command implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use loq_fs::CheckOptions;
use termcolor::{Color, ColorSpec, WriteColor};
use toml_edit::DocumentMut;

use crate::cli::AcceptDefeatArgs;
use crate::config_edit::{
    add_rule, collect_exact_path_rules, normalize_display_path, update_rule_max_lines,
};
use crate::output::{format_number, print_error, write_path};
use crate::ExitStatus;

struct DefeatChange {
    path: String,
    actual: usize,
    new_limit: usize,
}

struct DefeatReport {
    changes: Vec<DefeatChange>,
}

impl DefeatReport {
    fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

pub fn run_accept_defeat<W1: WriteColor, W2: WriteColor>(
    args: &AcceptDefeatArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    match run_accept_defeat_inner(args) {
        Ok(report) => {
            if report.is_empty() {
                let _ = writeln!(stdout, "No violations to accept");
                return ExitStatus::Failure;
            }
            let _ = write_report(stdout, &report);
            ExitStatus::Success
        }
        Err(err) => print_error(stderr, &format!("{err:#}")),
    }
}

fn run_accept_defeat_inner(args: &AcceptDefeatArgs) -> Result<DefeatReport> {
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
        cwd,
        use_cache: false,
    };

    let output = loq_fs::run_check(paths, options).context("accept-defeat check failed")?;
    let violations = collect_violations(&output.outcomes);

    if violations.is_empty() {
        return Ok(DefeatReport {
            changes: Vec::new(),
        });
    }

    let mut doc = if config_exists {
        let config_text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        config_text
            .parse()
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    } else {
        default_document()
    };

    let existing_rules = collect_exact_path_rules(&doc);
    let changes = apply_defeat_changes(&mut doc, &violations, &existing_rules, args.buffer);

    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(DefeatReport { changes })
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

fn default_document() -> DocumentMut {
    let mut doc = DocumentMut::new();
    doc["default_max_lines"] = toml_edit::value(500_i64);
    doc
}

fn apply_defeat_changes(
    doc: &mut DocumentMut,
    violations: &HashMap<String, usize>,
    existing_rules: &HashMap<String, (usize, usize)>,
    buffer: usize,
) -> Vec<DefeatChange> {
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
        changes.push(DefeatChange {
            path: path.clone(),
            actual,
            new_limit,
        });
    }

    changes
}

fn write_report<W: WriteColor>(writer: &mut W, report: &DefeatReport) -> std::io::Result<()> {
    let count = report.changes.len();
    writeln!(
        writer,
        "Accepted defeat on {count} file{}:",
        if count == 1 { "" } else { "s" }
    )?;
    let mut actual_spec = ColorSpec::new();
    actual_spec.set_fg(Some(Color::Red)).set_bold(true);
    let mut limit_spec = ColorSpec::new();
    limit_spec.set_fg(Some(Color::Green)).set_bold(true);

    for change in &report.changes {
        write!(writer, "  ")?;
        write_path(writer, &change.path)?;
        write!(writer, ": ")?;
        writer.set_color(&actual_spec)?;
        write!(writer, "{}", format_number(change.actual))?;
        writer.reset()?;
        write!(writer, " lines -> limit ")?;
        writer.set_color(&limit_spec)?;
        write!(writer, "{}", format_number(change.new_limit))?;
        writer.reset()?;
        writeln!(writer)?;
    }
    Ok(())
}
