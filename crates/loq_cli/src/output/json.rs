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

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::config::ConfigOrigin;
    use loq_core::report::FileOutcome;
    use loq_fs::walk;

    fn json_output_string(output: &CheckOutput) -> String {
        let mut buf = Vec::new();
        write_json(&mut buf, output).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn all_outcomes_counted() {
        let output = CheckOutput {
            outcomes: vec![
                FileOutcome {
                    path: "a.rs".into(),
                    display_path: "a.rs".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::NoLimit,
                },
                FileOutcome {
                    path: "b.rs".into(),
                    display_path: "b.rs".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Pass {
                        limit: 100,
                        actual: 50,
                        matched_by: MatchBy::Default,
                    },
                },
                FileOutcome {
                    path: "c.rs".into(),
                    display_path: "c.rs".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Violation {
                        limit: 100,
                        actual: 150,
                        matched_by: MatchBy::Default,
                    },
                },
            ],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["files_checked"], 3);
        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["summary"]["passed"], 1);
        assert_eq!(parsed["summary"]["violations"], 1);
    }

    #[test]
    fn missing_file_warning() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "missing.rs".into(),
                display_path: "missing.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Missing,
            }],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "missing.rs");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "missing");
        assert!(parsed["skip_warnings"][0]["detail"].is_null());
    }

    #[test]
    fn unreadable_file_warning() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "locked.rs".into(),
                display_path: "locked.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Unreadable {
                    error: "permission denied".into(),
                },
            }],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "locked.rs");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "unreadable");
        assert_eq!(parsed["skip_warnings"][0]["detail"], "permission denied");
    }

    #[test]
    fn binary_file_warning() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "image.png".into(),
                display_path: "image.png".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Binary,
            }],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["skipped"], 1);
        assert_eq!(parsed["skip_warnings"][0]["path"], "image.png");
        assert_eq!(parsed["skip_warnings"][0]["reason"], "binary");
        assert!(parsed["skip_warnings"][0]["detail"].is_null());
    }

    #[test]
    fn violation_with_rule_match() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "big.rs".into(),
                display_path: "big.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Violation {
                    limit: 100,
                    actual: 200,
                    matched_by: MatchBy::Rule {
                        pattern: "**/*.rs".into(),
                    },
                },
            }],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["path"], "big.rs");
        assert_eq!(parsed["violations"][0]["lines"], 200);
        assert_eq!(parsed["violations"][0]["max_lines"], 100);
        assert_eq!(parsed["violations"][0]["rule"], "**/*.rs");
    }

    #[test]
    fn violation_with_default_match() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "big.rs".into(),
                display_path: "big.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Violation {
                    limit: 100,
                    actual: 200,
                    matched_by: MatchBy::Default,
                },
            }],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["rule"], "default");
    }

    #[test]
    fn walk_errors_included() {
        let output = CheckOutput {
            outcomes: vec![],
            walk_errors: vec![
                walk::WalkError("path/to/error1".into()),
                walk::WalkError("path/to/error2".into()),
            ],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["summary"]["walk_errors"], 2);
        let errors = parsed["walk_errors"].as_array().unwrap();
        assert!(errors.contains(&serde_json::json!("path/to/error1")));
        assert!(errors.contains(&serde_json::json!("path/to/error2")));
    }

    #[test]
    fn fix_guidance_included_with_violations() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "big.rs".into(),
                display_path: "big.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Violation {
                    limit: 100,
                    actual: 200,
                    matched_by: MatchBy::Default,
                },
            }],
            walk_errors: vec![],
            fix_guidance: Some("Split large files.".into()),
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["fix_guidance"], "Split large files.");
    }

    #[test]
    fn fix_guidance_excluded_without_violations() {
        let output = CheckOutput {
            outcomes: vec![FileOutcome {
                path: "small.rs".into(),
                display_path: "small.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Pass {
                    limit: 100,
                    actual: 50,
                    matched_by: MatchBy::Default,
                },
            }],
            walk_errors: vec![],
            fix_guidance: Some("Split large files.".into()),
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["fix_guidance"].is_null());
    }

    #[test]
    fn violations_sorted_by_path() {
        let output = CheckOutput {
            outcomes: vec![
                FileOutcome {
                    path: "z.rs".into(),
                    display_path: "z.rs".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Violation {
                        limit: 100,
                        actual: 200,
                        matched_by: MatchBy::Default,
                    },
                },
                FileOutcome {
                    path: "a.rs".into(),
                    display_path: "a.rs".into(),
                    config_source: ConfigOrigin::BuiltIn,
                    kind: OutcomeKind::Violation {
                        limit: 100,
                        actual: 200,
                        matched_by: MatchBy::Default,
                    },
                },
            ],
            walk_errors: vec![],
            fix_guidance: None,
        };

        let json = json_output_string(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["violations"][0]["path"], "a.rs");
        assert_eq!(parsed["violations"][1]["path"], "z.rs");
    }
}
