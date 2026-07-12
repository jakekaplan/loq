//! Outcome aggregation and report generation.
//!
//! Collects file check outcomes and generates structured reports.

use crate::decide::MatchBy;
use crate::Limit;

/// The result of checking a single file.
#[derive(Debug)]
pub struct FileOutcome {
    /// Path relative to working directory for display.
    pub display_path: String,
    /// Path relative to config root for rule matching and managed exact-path rules.
    pub match_key: String,
    /// What happened when checking the file.
    pub kind: OutcomeKind,
}

/// What happened when checking a file.
#[derive(Debug)]
pub enum OutcomeKind {
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
    /// File exceeds its configured budget.
    Violation {
        /// The configured budget.
        limit: Limit,
        /// Actual measured value.
        actual: usize,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
    /// File is within its configured budget.
    Pass {
        /// The configured budget.
        limit: Limit,
        /// Actual measured value.
        actual: usize,
        /// How the limit was determined.
        matched_by: MatchBy,
    },
}

/// Why a file was skipped (for warnings).
#[derive(Debug)]
pub enum SkipReason {
    /// Binary file (contains null bytes).
    Binary,
    /// Could not read the file.
    Unreadable(String),
    /// File does not exist.
    Missing,
}

/// A reportable finding (violation or skip warning).
#[derive(Debug)]
pub enum FindingKind {
    /// File exceeded its configured budget.
    Violation {
        /// The configured budget.
        limit: Limit,
        /// Actual measured value.
        actual: usize,
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
#[derive(Debug)]
pub struct Finding {
    /// Display path for the file.
    pub path: String,
    /// What kind of finding this is.
    pub kind: FindingKind,
}

/// Summary statistics for a check run.
#[derive(Debug, Default)]
pub struct Summary {
    /// Total files processed.
    pub total: usize,
    /// Files skipped (excluded, no limit, etc.).
    pub skipped: usize,
    /// Files that passed their limit.
    pub passed: usize,
    /// Files with violations.
    pub errors: usize,
}

/// The complete report from a check run.
#[derive(Debug)]
pub struct Report {
    /// All findings (skip warnings first, then violations by overage).
    pub findings: Vec<Finding>,
    /// Summary statistics.
    pub summary: Summary,
    /// Guidance text to show when violations exist.
    pub fix_guidance: Option<String>,
}

/// Builds a report from file outcomes.
///
/// Aggregates outcomes into findings and summary statistics.
/// Findings are sorted with skip warnings first, then violations by overage.
/// If `fix_guidance` is provided and there are violations, it will be included in the report.
#[must_use]
pub fn build_report(outcomes: &[FileOutcome], fix_guidance: Option<String>) -> Report {
    let mut findings = Vec::new();
    let mut summary = Summary {
        total: outcomes.len(),
        ..Summary::default()
    };

    for outcome in outcomes {
        match &outcome.kind {
            OutcomeKind::NoLimit => {
                summary.skipped += 1;
            }
            OutcomeKind::Missing => {
                summary.skipped += 1;
                push_skip_warning(&mut findings, outcome, SkipReason::Missing);
            }
            OutcomeKind::Unreadable { error } => {
                summary.skipped += 1;
                push_skip_warning(
                    &mut findings,
                    outcome,
                    SkipReason::Unreadable(error.clone()),
                );
            }
            OutcomeKind::Binary => {
                summary.skipped += 1;
                push_skip_warning(&mut findings, outcome, SkipReason::Binary);
            }
            OutcomeKind::Pass { .. } => {
                summary.passed += 1;
            }
            OutcomeKind::Violation {
                limit,
                actual,
                matched_by,
            } => {
                push_violation(&mut findings, outcome, *limit, *actual, matched_by.clone());
                summary.errors += 1;
            }
        }
    }

    sort_findings(&mut findings);

    // Only include guidance if there are violations
    let fix_guidance = if summary.errors > 0 {
        fix_guidance
    } else {
        None
    };

    Report {
        findings,
        summary,
        fix_guidance,
    }
}

fn push_skip_warning(findings: &mut Vec<Finding>, outcome: &FileOutcome, reason: SkipReason) {
    findings.push(Finding {
        path: outcome.display_path.clone(),
        kind: FindingKind::SkipWarning { reason },
    });
}

fn push_violation(
    findings: &mut Vec<Finding>,
    outcome: &FileOutcome,
    limit: Limit,
    actual: usize,
    matched_by: MatchBy,
) {
    findings.push(Finding {
        path: outcome.display_path.clone(),
        kind: FindingKind::Violation {
            limit,
            actual,
            matched_by,
        },
    });
}

/// Sorts findings with skip warnings first, then violations by overage.
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
                    limit: a_limit,
                    actual: a_actual,
                    ..
                },
                FindingKind::Violation {
                    limit: b_limit,
                    actual: b_actual,
                    ..
                },
            ) => (a_actual - a_limit.max)
                .cmp(&(b_actual - b_limit.max))
                .then_with(|| a.path.cmp(&b.path)),
            _ => a.path.cmp(&b.path),
        }
    });
}

const fn finding_rank(kind: &FindingKind) -> u8 {
    match kind {
        FindingKind::SkipWarning { .. } => 0,
        FindingKind::Violation { .. } => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limit;

    #[test]
    fn summary_counts_each_file_once() {
        let outcomes = vec![
            FileOutcome {
                display_path: "a".into(),
                match_key: "a".into(),
                kind: OutcomeKind::Pass {
                    limit: Limit::lines(10),
                    actual: 5,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                display_path: "b".into(),
                match_key: "b".into(),
                kind: OutcomeKind::Violation {
                    limit: Limit::lines(10),
                    actual: 20,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                display_path: "c".into(),
                match_key: "c".into(),
                kind: OutcomeKind::Violation {
                    limit: Limit::lines(10),
                    actual: 12,
                    matched_by: MatchBy::Default,
                },
            },
            FileOutcome {
                display_path: "d".into(),
                match_key: "d".into(),
                kind: OutcomeKind::Missing,
            },
            FileOutcome {
                display_path: "e".into(),
                match_key: "e".into(),
                kind: OutcomeKind::Binary,
            },
            FileOutcome {
                display_path: "f".into(),
                match_key: "f".into(),
                kind: OutcomeKind::Unreadable {
                    error: "denied".into(),
                },
            },
        ];
        let report = build_report(&outcomes, None);
        assert_eq!(report.summary.total, 6);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.errors, 2);
        assert_eq!(report.summary.skipped, 3);
    }

    #[test]
    fn findings_sorted_by_overage() {
        let mut findings = vec![
            Finding {
                path: "b".into(),
                kind: FindingKind::Violation {
                    limit: Limit::lines(10),
                    actual: 12,
                    matched_by: MatchBy::Default,
                },
            },
            Finding {
                path: "a".into(),
                kind: FindingKind::Violation {
                    limit: Limit::lines(10),
                    actual: 20,
                    matched_by: MatchBy::Default,
                },
            },
            Finding {
                path: "c".into(),
                kind: FindingKind::SkipWarning {
                    reason: SkipReason::Missing,
                },
            },
        ];
        sort_findings(&mut findings);
        // Skip warnings first, then violations sorted by overage (smallest first)
        assert_eq!(findings[0].path, "c");
        assert_eq!(findings[1].path, "b");
        assert_eq!(findings[2].path, "a");
    }

    #[test]
    fn nolimit_is_skipped() {
        let outcomes = vec![FileOutcome {
            display_path: "nolimit.js".into(),
            match_key: "nolimit.js".into(),
            kind: OutcomeKind::NoLimit,
        }];
        let report = build_report(&outcomes, None);
        assert_eq!(report.summary.total, 1);
        assert_eq!(report.summary.skipped, 1);
        assert_eq!(report.summary.passed, 0);
        assert_eq!(report.summary.errors, 0);
        // No findings for nolimit
        assert!(report.findings.is_empty());
    }

    #[test]
    fn fix_guidance_included_when_violations_exist() {
        let outcomes = vec![FileOutcome {
            display_path: "big.rs".into(),
            match_key: "big.rs".into(),
            kind: OutcomeKind::Violation {
                limit: Limit::lines(100),
                actual: 150,
                matched_by: MatchBy::Default,
            },
        }];
        let guidance = Some("Split large files into smaller modules.".to_string());
        let report = build_report(&outcomes, guidance);
        assert_eq!(report.summary.errors, 1);
        assert!(report.fix_guidance.is_some());
        assert_eq!(
            report.fix_guidance.unwrap(),
            "Split large files into smaller modules."
        );
    }

    #[test]
    fn fix_guidance_excluded_when_no_violations() {
        let outcomes = vec![FileOutcome {
            display_path: "small.rs".into(),
            match_key: "small.rs".into(),
            kind: OutcomeKind::Pass {
                limit: Limit::lines(100),
                actual: 50,
                matched_by: MatchBy::Default,
            },
        }];
        let guidance = Some("Split large files into smaller modules.".to_string());
        let report = build_report(&outcomes, guidance);
        assert_eq!(report.summary.errors, 0);
        assert!(report.fix_guidance.is_none());
    }
}
