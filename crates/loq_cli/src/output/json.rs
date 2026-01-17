//! JSON output format for check results.

use std::io::{self, Write};

use loq_core::report::OutcomeKind;
use loq_core::MatchBy;
use loq_fs::CheckOutput;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct JsonOutput {
    version: &'static str,
    violations: Vec<JsonViolation>,
    skip_warnings: Vec<JsonSkipWarning>,
    walk_errors: Vec<String>,
    summary: JsonSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix_guidance: Option<String>,
}

#[derive(Debug, Serialize)]
struct JsonViolation {
    path: String,
    lines: usize,
    max_lines: usize,
    rule: String,
}

#[derive(Debug, Serialize)]
struct JsonSummary {
    files_checked: usize,
    skipped: usize,
    passed: usize,
    violations: usize,
    walk_errors: usize,
}

#[derive(Debug, Serialize)]
struct JsonSkipWarning {
    path: String,
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub fn write_json<W: Write>(writer: &mut W, output: &CheckOutput) -> io::Result<()> {
    let mut summary = JsonSummary {
        files_checked: output.outcomes.len(),
        skipped: 0,
        passed: 0,
        violations: 0,
        walk_errors: output.walk_errors.len(),
    };
    let mut violations = Vec::new();
    let mut skip_warnings = Vec::new();

    for outcome in &output.outcomes {
        match &outcome.kind {
            OutcomeKind::NoLimit => {
                summary.skipped += 1;
            }
            OutcomeKind::Missing => {
                summary.skipped += 1;
                skip_warnings.push(JsonSkipWarning {
                    path: outcome.display_path.clone(),
                    reason: "missing",
                    detail: None,
                });
            }
            OutcomeKind::Unreadable { error } => {
                summary.skipped += 1;
                skip_warnings.push(JsonSkipWarning {
                    path: outcome.display_path.clone(),
                    reason: "unreadable",
                    detail: Some(error.clone()),
                });
            }
            OutcomeKind::Binary => {
                summary.skipped += 1;
                skip_warnings.push(JsonSkipWarning {
                    path: outcome.display_path.clone(),
                    reason: "binary",
                    detail: None,
                });
            }
            OutcomeKind::Pass { .. } => {
                summary.passed += 1;
            }
            OutcomeKind::Violation {
                limit,
                actual,
                matched_by,
            } => {
                summary.violations += 1;
                let rule = match matched_by {
                    MatchBy::Rule { pattern } => pattern.clone(),
                    MatchBy::Default => "default".to_string(),
                };
                violations.push(JsonViolation {
                    path: outcome.display_path.clone(),
                    lines: *actual,
                    max_lines: *limit,
                    rule,
                });
            }
        }
    }

    violations.sort_by(|a, b| a.path.cmp(&b.path));
    skip_warnings.sort_by(|a, b| a.path.cmp(&b.path));

    let mut walk_errors: Vec<String> = output
        .walk_errors
        .iter()
        .map(|error| error.0.clone())
        .collect();
    walk_errors.sort();

    let fix_guidance = if summary.violations > 0 {
        output.fix_guidance.clone()
    } else {
        None
    };

    let output = JsonOutput {
        version: env!("CARGO_PKG_VERSION"),
        violations,
        skip_warnings,
        walk_errors,
        summary,
        fix_guidance,
    };

    serde_json::to_writer_pretty(&mut *writer, &output)?;
    writeln!(writer)
}
