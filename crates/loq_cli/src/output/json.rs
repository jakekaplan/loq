//! JSON output format for check results.

use std::io::{self, Write};

use loq_core::report::{FindingKind, Report, SkipReason};
use loq_core::{Limit, MatchBy, Metric};
use loq_fs::walk::WalkError;
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
    metric: &'static str,
    approximate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
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

pub fn write_json<W: Write>(
    writer: &mut W,
    report: &Report,
    walk_errors: &[WalkError],
) -> io::Result<()> {
    let summary = JsonSummary {
        files_checked: report.summary.total,
        skipped: report.summary.skipped,
        passed: report.summary.passed,
        violations: report.summary.errors,
        walk_errors: walk_errors.len(),
    };
    let mut violations = Vec::new();
    let mut skip_warnings = Vec::new();

    for finding in &report.findings {
        match &finding.kind {
            FindingKind::Violation {
                limit,
                actual,
                matched_by,
                ..
            } => {
                let rule = match matched_by {
                    MatchBy::Rule { pattern } => pattern.clone(),
                    MatchBy::Default => "default".to_string(),
                };
                violations.push(JsonViolation {
                    path: finding.path.clone(),
                    metric: limit.metric.as_str(),
                    approximate: limit.is_approximate(),
                    lines: metric_value(*actual, *limit, Metric::Lines),
                    max_lines: metric_value(limit.max, *limit, Metric::Lines),
                    tokens: metric_value(*actual, *limit, Metric::Tokens),
                    max_tokens: metric_value(limit.max, *limit, Metric::Tokens),
                    rule,
                });
            }
            FindingKind::SkipWarning { reason } => {
                let (reason, detail) = match reason {
                    SkipReason::Missing => ("missing", None),
                    SkipReason::Binary => ("binary", None),
                    SkipReason::Unreadable(error) => ("unreadable", Some(error.clone())),
                };
                skip_warnings.push(JsonSkipWarning {
                    path: finding.path.clone(),
                    reason,
                    detail,
                });
            }
        }
    }

    violations.sort_by(|a, b| a.path.cmp(&b.path));
    skip_warnings.sort_by(|a, b| a.path.cmp(&b.path));

    let mut walk_errors: Vec<String> = walk_errors
        .iter()
        .map(|error| error.message.clone())
        .collect();
    walk_errors.sort();

    let output = JsonOutput {
        version: env!("CARGO_PKG_VERSION"),
        violations,
        skip_warnings,
        walk_errors,
        summary,
        fix_guidance: report.fix_guidance.clone(),
    };

    serde_json::to_writer_pretty(&mut *writer, &output)?;
    writeln!(writer)
}

fn metric_value(value: usize, limit: Limit, metric: Metric) -> Option<usize> {
    (limit.metric == metric).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::report::{build_report, FileOutcome, OutcomeKind};
    use loq_fs::walk;

    fn json_output_string(
        outcomes: Vec<FileOutcome>,
        walk_errors: Vec<walk::WalkError>,
        fix_guidance: Option<String>,
    ) -> String {
        let report = build_report(&outcomes, fix_guidance);
        let mut buf = Vec::new();
        write_json(&mut buf, &report, &walk_errors).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn all_outcomes_counted() {
        let outcomes = vec![
            FileOutcome {
                display_path: "a.rs".into(),
                match_key: "a.rs".into(),
                kind: OutcomeKind::NoLimit,
            },
            FileOutcome {
                display_path: "b.rs".into(),
                match_key: "b.rs".into(),
                kind: OutcomeKind::Pass {
                    limit: loq_core::Limit::lines(100),
                    actual: 50,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                display_path: "c.rs".into(),
                match_key: "c.rs".into(),
                kind: OutcomeKind::Violation {
                    limit: loq_core::Limit::lines(100),
                    actual: 150,
                    matched_by: MatchBy::Default,
                },
            },
        ];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["files_checked"], 3);
        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["summary"]["passed"], 1);
        assert_eq!(parsed["summary"]["violations"], 1);
    }

    #[test]
    fn missing_file_warning() {
        let outcomes = vec![FileOutcome {
            display_path: "missing.rs".into(),
            match_key: "missing.rs".into(),
            kind: OutcomeKind::Missing,
        }];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "missing.rs");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "missing");
        assert!(parsed["skip_warnings"][0]["detail"].is_null());
    }

    #[test]
    fn unreadable_file_warning() {
        let outcomes = vec![FileOutcome {
            display_path: "locked.rs".into(),
            match_key: "locked.rs".into(),
            kind: OutcomeKind::Unreadable {
                error: "permission denied".into(),
            },
        }];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "locked.rs");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "unreadable");
        assert_eq!(parsed["skip_warnings"][0]["detail"], "permission denied");
    }

    #[test]
    fn binary_file_warning() {
        let outcomes = vec![FileOutcome {
            display_path: "image.png".into(),
            match_key: "image.png".into(),
            kind: OutcomeKind::Binary,
        }];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "image.png");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "binary");
        assert!(parsed["skip_warnings"][0]["detail"].is_null());
    }

    #[test]
    fn violation_with_rule_match() {
        let outcomes = vec![FileOutcome {
            display_path: "big.rs".into(),
            match_key: "big.rs".into(),
            kind: OutcomeKind::Violation {
                limit: loq_core::Limit::lines(100),
                actual: 200,
                matched_by: MatchBy::Rule {
                    pattern: "**/*.rs".into(),
                },
            },
        }];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["path"], "big.rs");
        assert_eq!(parsed["violations"][0]["lines"], 200);
        assert_eq!(parsed["violations"][0]["max_lines"], 100);
        assert_eq!(parsed["violations"][0]["rule"], "**/*.rs");
    }

    #[test]
    fn token_violation_uses_token_fields() {
        let outcomes = vec![FileOutcome {
            display_path: "prompt.md".into(),
            match_key: "prompt.md".into(),
            kind: OutcomeKind::Violation {
                limit: loq_core::Limit::tokens(4),
                actual: 5,
                matched_by: MatchBy::Rule {
                    pattern: "prompts/**/*.md".into(),
                },
            },
        }];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["metric"], "tokens");
        assert_eq!(parsed["violations"][0]["approximate"], true);
        assert_eq!(parsed["violations"][0]["tokens"], 5);
        assert_eq!(parsed["violations"][0]["max_tokens"], 4);
        assert!(parsed["violations"][0]["lines"].is_null());
        assert!(parsed["violations"][0]["max_lines"].is_null());
    }

    #[test]
    fn violation_with_default_match() {
        let outcomes = vec![FileOutcome {
            display_path: "big.rs".into(),
            match_key: "big.rs".into(),
            kind: OutcomeKind::Violation {
                limit: loq_core::Limit::lines(100),
                actual: 200,
                matched_by: MatchBy::Default,
            },
        }];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["rule"], "default");
    }

    #[test]
    fn walk_errors_included() {
        let json = json_output_string(
            vec![],
            vec![
                walk::WalkError {
                    message: "path/to/error1".into(),
                },
                walk::WalkError {
                    message: "path/to/error2".into(),
                },
            ],
            None,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["walk_errors"], 2);
        let errors = parsed["walk_errors"].as_array().unwrap();
        assert!(errors.contains(&serde_json::json!("path/to/error1")));
        assert!(errors.contains(&serde_json::json!("path/to/error2")));
    }

    #[test]
    fn fix_guidance_included_with_violations() {
        let outcomes = vec![FileOutcome {
            display_path: "big.rs".into(),
            match_key: "big.rs".into(),
            kind: OutcomeKind::Violation {
                limit: loq_core::Limit::lines(100),
                actual: 200,
                matched_by: MatchBy::Default,
            },
        }];

        let json = json_output_string(outcomes, vec![], Some("Split large files.".into()));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["fix_guidance"], "Split large files.");
    }

    #[test]
    fn fix_guidance_excluded_without_violations() {
        let outcomes = vec![FileOutcome {
            display_path: "small.rs".into(),
            match_key: "small.rs".into(),
            kind: OutcomeKind::Pass {
                limit: loq_core::Limit::lines(100),
                actual: 50,
                matched_by: MatchBy::Default,
            },
        }];

        let json = json_output_string(outcomes, vec![], Some("Split large files.".into()));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["fix_guidance"].is_null());
    }

    #[test]
    fn violations_sorted_by_path() {
        let outcomes = vec![
            FileOutcome {
                display_path: "z.rs".into(),
                match_key: "z.rs".into(),
                kind: OutcomeKind::Violation {
                    limit: loq_core::Limit::lines(100),
                    actual: 200,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                display_path: "a.rs".into(),
                match_key: "a.rs".into(),
                kind: OutcomeKind::Violation {
                    limit: loq_core::Limit::lines(100),
                    actual: 200,
                    matched_by: MatchBy::Default,
                },
            },
        ];

        let json = json_output_string(outcomes, vec![], None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["path"], "a.rs");
        assert_eq!(parsed["violations"][1]["path"], "z.rs");
    }
}
