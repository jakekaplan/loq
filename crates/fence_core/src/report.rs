use crate::config::Severity;
use crate::decide::MatchBy;

#[derive(Debug, Clone)]
pub struct FileOutcome {
    pub path: std::path::PathBuf,
    pub display_path: String,
    pub kind: OutcomeKind,
}

#[derive(Debug, Clone)]
pub enum OutcomeKind {
    Excluded {
        pattern: String,
    },
    Exempt {
        pattern: String,
    },
    NoLimit,
    Missing,
    Unreadable {
        error: String,
    },
    Binary,
    Violation {
        limit: usize,
        actual: usize,
        severity: Severity,
        matched_by: MatchBy,
    },
    Pass {
        limit: usize,
        actual: usize,
        severity: Severity,
        matched_by: MatchBy,
    },
}

#[derive(Debug, Clone)]
pub enum SkipReason {
    Binary,
    Unreadable(String),
    Missing,
}

#[derive(Debug, Clone)]
pub enum FindingKind {
    Violation {
        severity: Severity,
        limit: usize,
        actual: usize,
        over_by: usize,
    },
    SkipWarning {
        reason: SkipReason,
    },
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub path: String,
    pub kind: FindingKind,
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub total: usize,
    pub skipped: usize,
    pub passed: usize,
    pub errors: usize,
    pub warnings: usize,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub summary: Summary,
}

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
                    kind: FindingKind::SkipWarning {
                        reason: SkipReason::Missing,
                    },
                });
            }
            OutcomeKind::Unreadable { error } => {
                summary.skipped += 1;
                findings.push(Finding {
                    path: outcome.display_path.clone(),
                    kind: FindingKind::SkipWarning {
                        reason: SkipReason::Unreadable(error.clone()),
                    },
                });
            }
            OutcomeKind::Binary => {
                summary.skipped += 1;
                findings.push(Finding {
                    path: outcome.display_path.clone(),
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
                ..
            } => {
                let over_by = actual.saturating_sub(*limit);
                findings.push(Finding {
                    path: outcome.display_path.clone(),
                    kind: FindingKind::Violation {
                        severity: *severity,
                        limit: *limit,
                        actual: *actual,
                        over_by,
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
            ) => b_over.cmp(a_over).then_with(|| a.path.cmp(&b.path)),
            _ => a.path.cmp(&b.path),
        }
    });
}

fn finding_rank(kind: &FindingKind) -> u8 {
    match kind {
        FindingKind::Violation { severity, .. } => match severity {
            Severity::Error => 0,
            Severity::Warning => 1,
        },
        FindingKind::SkipWarning { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_each_file_once() {
        let outcomes = vec![
            FileOutcome {
                path: "a".into(),
                display_path: "a".into(),
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
                kind: OutcomeKind::Missing,
            },
            FileOutcome {
                path: "e".into(),
                display_path: "e".into(),
                kind: OutcomeKind::Binary,
            },
            FileOutcome {
                path: "f".into(),
                display_path: "f".into(),
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
                kind: FindingKind::Violation {
                    severity: Severity::Warning,
                    limit: 10,
                    actual: 12,
                    over_by: 2,
                },
            },
            Finding {
                path: "a".into(),
                kind: FindingKind::Violation {
                    severity: Severity::Error,
                    limit: 10,
                    actual: 20,
                    over_by: 10,
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
        assert_eq!(findings[0].path, "a");
        assert_eq!(findings[1].path, "b");
        assert_eq!(findings[2].path, "c");
    }
}
