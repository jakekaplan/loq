//! Outcome aggregation and report generation.
//!
//! Collects file check outcomes and generates structured reports
//! with findings sorted by severity.

use crate::config::{ConfigOrigin, Severity};
use crate::decide::MatchBy;

/// The result of checking a single file.
#[derive(Debug, Clone)]
pub struct FileOutcome {
    /// Absolute path to the file.
    pub path: std::path::PathBuf,
    /// Path relative to working directory for display.
    pub display_path: String,
    /// Which config was used for this file.
    pub config_source: ConfigOrigin,
    /// What happened when checking the file.
    pub kind: OutcomeKind,
}

/// What happened when checking a file.
#[derive(Debug, Clone)]
pub enum OutcomeKind {
    /// File was excluded by pattern.
    Excluded {
        /// The pattern that matched.
        pattern: String,
    },
    /// File was exempted by pattern.
    Exempt {
        /// The pattern that matched.
        pattern: String,
    },
    /// No limit configured for this file.
    NoLimit,
    /// File does not exist.
    Missing,
    /// File could not be read.
    Unreadable {
        /// The error message.
        error: String,
    },
    /// File appears to be binary (contains null bytes).
    Binary,
    /// File exceeds its line limit.
    Violation {
        /// The configured limit.
        limit: usize,
        /// Actual line count.
        actual: usize,
        /// Severity of the violation.
        severity: Severity,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
    /// File is within its line limit.
    Pass {
        /// The configured limit.
        limit: usize,
        /// Actual line count.
        actual: usize,
        /// Severity that would apply if over.
        severity: Severity,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
}

/// Why a file was skipped (for warnings).
#[derive(Debug, Clone)]
pub enum SkipReason {
    /// Binary file (contains null bytes).
    Binary,
    /// Could not read the file.
    Unreadable(String),
    /// File does not exist.
    Missing,
}

/// A reportable finding (violation or skip warning).
#[derive(Debug, Clone)]
pub enum FindingKind {
    /// File exceeded its line limit.
    Violation {
        /// Severity of the violation.
        severity: Severity,
        /// The configured limit.
        limit: usize,
        /// Actual line count.
        actual: usize,
        /// How many lines over the limit.
        over_by: usize,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
    /// File was skipped with a warning.
    SkipWarning {
        /// Why the file was skipped.
        reason: SkipReason,
    },
}

/// A single finding to report.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Display path for the file.
    pub path: String,
    /// Which config was used.
    pub config_source: ConfigOrigin,
    /// What kind of finding this is.
    pub kind: FindingKind,
}

/// Summary statistics for a check run.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    /// Total files processed.
    pub total: usize,
    /// Files skipped (excluded, exempt, no limit, etc.).
    pub skipped: usize,
    /// Files that passed their limit.
    pub passed: usize,
    /// Files with error-severity violations.
    pub errors: usize,
    /// Files with warning-severity violations.
    pub warnings: usize,
    /// Time taken in milliseconds.
    pub duration_ms: u128,
}

/// The complete report from a check run.
#[derive(Debug, Clone)]
pub struct Report {
    /// All findings, sorted by severity.
    pub findings: Vec<Finding>,
    /// Summary statistics.
    pub summary: Summary,
}

/// Builds a report from file outcomes.
///
/// Aggregates outcomes into findings and summary statistics.
/// Findings are sorted by severity (skip warnings, then warnings, then errors).
pub fn build_report(outcomes: &[FileOutcome], duration_ms: u128) -> Report {
    let mut findings = Vec::new();
    let mut summary = Summary {
        total: outcomes.len(),
        duration_ms,
        ..Summary::default()
    };

    for outcome in outcomes {
        match &outcome.kind {
            OutcomeKind::Excluded { .. } | OutcomeKind::Exempt { .. } | OutcomeKind::NoLimit => {
                summary.skipped += 1;
            }
            OutcomeKind::Missing => {
                summary.skipped += 1;
                findings.push(Finding {
                    path: outcome.display_path.clone(),
                    config_source: outcome.config_source.clone(),
                    kind: FindingKind::SkipWarning {
                        reason: SkipReason::Missing,
                    },
                });
            }
            OutcomeKind::Unreadable { error } => {
                summary.skipped += 1;
                findings.push(Finding {
                    path: outcome.display_path.clone(),
                    config_source: outcome.config_source.clone(),
                    kind: FindingKind::SkipWarning {
                        reason: SkipReason::Unreadable(error.clone()),
                    },
                });
            }
            OutcomeKind::Binary => {
                summary.skipped += 1;
                findings.push(Finding {
                    path: outcome.display_path.clone(),
                    config_source: outcome.config_source.clone(),
                    kind: FindingKind::SkipWarning {
                        reason: SkipReason::Binary,
                    },
                });
            }
            OutcomeKind::Pass { .. } => {
                summary.passed += 1;
            }
            OutcomeKind::Violation {
                severity,
                limit,
                actual,
                matched_by,
            } => {
                let over_by = actual.saturating_sub(*limit);
                findings.push(Finding {
                    path: outcome.display_path.clone(),
                    config_source: outcome.config_source.clone(),
                    kind: FindingKind::Violation {
                        severity: *severity,
                        limit: *limit,
                        actual: *actual,
                        over_by,
                        matched_by: matched_by.clone(),
                    },
                });
                match severity {
                    Severity::Error => summary.errors += 1,
                    Severity::Warning => summary.warnings += 1,
                }
            }
        }
    }

    sort_findings(&mut findings);

    Report { findings, summary }
}

/// Sorts findings by severity (skip warnings first, errors last).
///
/// Within each severity, violations are sorted by how much they're over the limit.
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        let rank_a = finding_rank(&a.kind);
        let rank_b = finding_rank(&b.kind);
        if rank_a != rank_b {
            return rank_a.cmp(&rank_b);
        }
        match (&a.kind, &b.kind) {
            (
                FindingKind::Violation {
                    over_by: a_over, ..
                },
                FindingKind::Violation {
                    over_by: b_over, ..
                },
            ) => a_over.cmp(b_over).then_with(|| a.path.cmp(&b.path)),
            _ => a.path.cmp(&b.path),
        }
    });
}

fn finding_rank(kind: &FindingKind) -> u8 {
    match kind {
        FindingKind::SkipWarning { .. } => 0,
        FindingKind::Violation { severity, .. } => match severity {
            Severity::Warning => 1,
            Severity::Error => 2,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigOrigin;

    #[test]
    fn summary_counts_each_file_once() {
        let outcomes = vec![
            FileOutcome {
                path: "a".into(),
                display_path: "a".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Pass {
                    limit: 10,
                    actual: 5,
                    severity: Severity::Error,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                path: "b".into(),
                display_path: "b".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Violation {
                    limit: 10,
                    actual: 20,
                    severity: Severity::Error,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                path: "c".into(),
                display_path: "c".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Violation {
                    limit: 10,
                    actual: 12,
                    severity: Severity::Warning,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                path: "d".into(),
                display_path: "d".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Missing,
            },
            FileOutcome {
                path: "e".into(),
                display_path: "e".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Binary,
            },
            FileOutcome {
                path: "f".into(),
                display_path: "f".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Unreadable {
                    error: "denied".into(),
                },
            },
        ];
        let report = build_report(&outcomes, 0);
        assert_eq!(report.summary.total, 6);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.summary.warnings, 1);
        assert_eq!(report.summary.skipped, 3);
    }

    #[test]
    fn findings_sorted_by_severity_and_overage() {
        let mut findings = vec![
            Finding {
                path: "b".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: FindingKind::Violation {
                    severity: Severity::Warning,
                    limit: 10,
                    actual: 12,
                    over_by: 2,
                    matched_by: MatchBy::Default,
                },
            },
            Finding {
                path: "a".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: FindingKind::Violation {
                    severity: Severity::Error,
                    limit: 10,
                    actual: 20,
                    over_by: 10,
                    matched_by: MatchBy::Default,
                },
            },
            Finding {
                path: "c".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: FindingKind::SkipWarning {
                    reason: SkipReason::Missing,
                },
            },
        ];
        sort_findings(&mut findings);
        // Skip warnings first, then warnings, then errors (biggest at bottom near summary)
        assert_eq!(findings[0].path, "c");
        assert_eq!(findings[1].path, "b");
        assert_eq!(findings[2].path, "a");
    }

    #[test]
    fn excluded_exempt_nolimit_are_skipped() {
        let outcomes = vec![
            FileOutcome {
                path: "excluded.txt".into(),
                display_path: "excluded.txt".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Excluded {
                    pattern: "*.txt".to_string(),
                },
            },
            FileOutcome {
                path: "exempt.rs".into(),
                display_path: "exempt.rs".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::Exempt {
                    pattern: "exempt.rs".to_string(),
                },
            },
            FileOutcome {
                path: "nolimit.js".into(),
                display_path: "nolimit.js".into(),
                config_source: ConfigOrigin::BuiltIn,
                kind: OutcomeKind::NoLimit,
            },
        ];
        let report = build_report(&outcomes, 0);
        assert_eq!(report.summary.total, 3);
        assert_eq!(report.summary.skipped, 3);
        assert_eq!(report.summary.passed, 0);
        assert_eq!(report.summary.errors, 0);
        assert_eq!(report.summary.warnings, 0);
        // No findings for excluded/exempt/nolimit
        assert!(report.findings.is_empty());
    }
}
