use super::*;
use loq_core::report::{Finding, FindingKind, SkipReason, Summary};
use loq_core::{ConfigOrigin, MatchBy, Severity};
use termcolor::NoColor;

fn output_string<F>(f: F) -> String
where
    F: FnOnce(&mut NoColor<Vec<u8>>) -> io::Result<()>,
{
    let mut buf = NoColor::new(Vec::new());
    f(&mut buf).unwrap();
    String::from_utf8(buf.into_inner()).unwrap()
}

#[test]
fn colorspec_helpers_build_correctly() {
    let fg_spec = fg(Color::Red);
    assert_eq!(fg_spec.fg(), Some(&Color::Red));
    assert!(!fg_spec.bold());
    assert!(!fg_spec.dimmed());

    let bold_spec = bold();
    assert!(bold_spec.bold());
    assert!(bold_spec.fg().is_none());

    let dimmed_spec = dimmed();
    assert!(dimmed_spec.dimmed());
    assert!(dimmed_spec.fg().is_none());
}

#[test]
fn write_count_line_singular() {
    let out = output_string(|w| write_count_line(w, "✔", Color::Green, 1, "Error", "Errors"));
    assert!(out.contains("1 Error"));
    assert!(!out.contains("Errors"));
}

#[test]
fn write_count_line_plural() {
    let out = output_string(|w| write_count_line(w, "✔", Color::Green, 5, "Error", "Errors"));
    assert!(out.contains("5 Errors"));
}

#[test]
fn write_count_line_passed_no_plural() {
    let out = output_string(|w| write_count_line(w, "✔", Color::Green, 5, "Passed", "Passed"));
    assert!(out.contains("5 Passed"));
    assert!(!out.contains("Passeds"));
}

#[test]
fn severity_label_error() {
    assert_eq!(severity_label(Severity::Error), "error");
}

#[test]
fn severity_label_warning() {
    assert_eq!(severity_label(Severity::Warning), "warning");
}

#[test]
fn write_line_with_color() {
    let out = output_string(|w| write_line(w, Some(Color::Red), "hello"));
    assert_eq!(out, "hello\n");
}

#[test]
fn write_line_without_color() {
    let out = output_string(|w| write_line(w, None, "hello"));
    assert_eq!(out, "hello\n");
}

#[test]
fn format_number_small() {
    assert_eq!(format_number(42), "42");
}

#[test]
fn format_number_hundreds() {
    assert_eq!(format_number(999), "999");
}

#[test]
fn format_number_thousands() {
    assert_eq!(format_number(1234), "1,234");
}

#[test]
fn format_number_millions() {
    assert_eq!(format_number(1_234_567), "1,234,567");
}

#[test]
fn write_block_multiline() {
    let out = output_string(|w| write_block(w, Some(Color::Red), "line1\nline2\nline3"));
    assert_eq!(out, "line1\nline2\nline3\n");
}

#[test]
fn write_block_single_line() {
    let out = output_string(|w| write_block(w, Some(Color::Red), "single"));
    assert_eq!(out, "single\n");
}

#[test]
fn write_finding_violation_error() {
    let finding = Finding {
        path: "src/main.rs".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::Violation {
            severity: Severity::Error,
            limit: 100,
            actual: 150,
            over_by: 50,
            matched_by: MatchBy::Default,
        },
    };
    let out = output_string(|w| write_finding(w, &finding, false));
    assert!(out.contains("✖"));
    assert!(out.contains("main.rs"));
    assert!(out.contains("150 lines"));
    assert!(out.contains("+50 over limit"));
}

#[test]
fn write_finding_violation_warning() {
    let finding = Finding {
        path: "warn.txt".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::Violation {
            severity: Severity::Warning,
            limit: 10,
            actual: 15,
            over_by: 5,
            matched_by: MatchBy::Default,
        },
    };
    let out = output_string(|w| write_finding(w, &finding, false));
    assert!(out.contains("⚠"));
    assert!(out.contains("15 lines"));
    assert!(out.contains("+5 over limit"));
}

#[test]
fn write_finding_violation_verbose_default_match() {
    let finding = Finding {
        path: "src/lib.rs".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::Violation {
            severity: Severity::Error,
            limit: 100,
            actual: 200,
            over_by: 100,
            matched_by: MatchBy::Default,
        },
    };
    let out = output_string(|w| write_finding(w, &finding, true));
    assert!(out.contains("rule:"));
    assert!(out.contains("(default)"));
    assert!(out.contains("config:"));
    assert!(out.contains("<built-in>"));
}

#[test]
fn write_finding_violation_verbose_rule_match() {
    let finding = Finding {
        path: "src/lib.rs".into(),
        config_source: ConfigOrigin::File(std::path::PathBuf::from("/project/loq.toml")),
        kind: FindingKind::Violation {
            severity: Severity::Warning,
            limit: 50,
            actual: 75,
            over_by: 25,
            matched_by: MatchBy::Rule {
                pattern: "**/*.rs".into(),
            },
        },
    };
    let out = output_string(|w| write_finding(w, &finding, true));
    assert!(out.contains("rule:"));
    assert!(out.contains("match: **/*.rs"));
    assert!(out.contains("severity=warning"));
    assert!(out.contains("config:"));
    assert!(out.contains("loq.toml"));
}

#[test]
fn write_finding_skip_binary() {
    let finding = Finding {
        path: "image.png".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::SkipWarning {
            reason: SkipReason::Binary,
        },
    };
    let out = output_string(|w| write_finding(w, &finding, false));
    assert!(out.contains("⚠"));
    assert!(out.contains("binary file skipped"));
}

#[test]
fn write_finding_skip_missing() {
    let finding = Finding {
        path: "missing.txt".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::SkipWarning {
            reason: SkipReason::Missing,
        },
    };
    let out = output_string(|w| write_finding(w, &finding, false));
    assert!(out.contains("file not found"));
}

#[test]
fn write_finding_skip_unreadable() {
    let finding = Finding {
        path: "locked.txt".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::SkipWarning {
            reason: SkipReason::Unreadable("permission denied".into()),
        },
    };
    let out = output_string(|w| write_finding(w, &finding, false));
    assert!(out.contains("unreadable:"));
    assert!(out.contains("permission denied"));
}

#[test]
fn write_finding_path_without_directory() {
    let finding = Finding {
        path: "file.txt".into(),
        config_source: ConfigOrigin::BuiltIn,
        kind: FindingKind::Violation {
            severity: Severity::Error,
            limit: 10,
            actual: 20,
            over_by: 10,
            matched_by: MatchBy::Default,
        },
    };
    let out = output_string(|w| write_finding(w, &finding, false));
    assert!(out.contains("file.txt"));
}

#[test]
fn write_summary_with_violations() {
    let summary = Summary {
        total: 10,
        skipped: 2,
        passed: 5,
        errors: 2,
        warnings: 1,
        duration_ms: 42,
    };
    let out = output_string(|w| write_summary(w, &summary));
    assert!(out.contains("Found 3 violations"));
    assert!(out.contains("8 checked files"));
    assert!(out.contains("✖"));
    assert!(out.contains("2 Errors"));
    assert!(out.contains("⚠"));
    assert!(out.contains("1 Warning"));
    assert!(out.contains("✔"));
    assert!(out.contains("5 Passed"));
    assert!(out.contains("Time: 42ms"));
}

#[test]
fn write_summary_all_passed() {
    let summary = Summary {
        total: 5,
        skipped: 0,
        passed: 5,
        errors: 0,
        warnings: 0,
        duration_ms: 10,
    };
    let out = output_string(|w| write_summary(w, &summary));
    assert!(out.contains("All 5 files passed"));
    assert!(out.contains("0 Errors"));
    assert!(out.contains("0 Warnings"));
}

#[test]
fn write_summary_single_file() {
    let summary = Summary {
        total: 1,
        skipped: 0,
        passed: 0,
        errors: 1,
        warnings: 0,
        duration_ms: 5,
    };
    let out = output_string(|w| write_summary(w, &summary));
    assert!(out.contains("1 checked file."));
    assert!(out.contains("1 Error"));
}

#[test]
fn print_error_returns_error_status() {
    use crate::ExitStatus;
    let mut buf = NoColor::new(Vec::new());
    let status = print_error(&mut buf, "something went wrong");
    assert_eq!(status, ExitStatus::Error);
    let out = String::from_utf8(buf.into_inner()).unwrap();
    assert!(out.contains("error:"));
    assert!(out.contains("something went wrong"));
}

#[test]
fn write_walk_errors_verbose() {
    let errors = vec![
        WalkError("path/to/bad".into()),
        WalkError("another/error".into()),
    ];
    let out = output_string(|w| write_walk_errors(w, &errors, true));
    assert!(out.contains("Skipped paths (2):"));
    assert!(out.contains("path/to/bad"));
    assert!(out.contains("another/error"));
}

#[test]
fn write_walk_errors_non_verbose() {
    let errors = vec![WalkError("path/to/bad".into())];
    let out = output_string(|w| write_walk_errors(w, &errors, false));
    assert!(out.contains("1 path(s) skipped"));
    assert!(out.contains("--verbose"));
}

#[test]
fn relative_config_path_builtin() {
    let result = relative_config_path(&ConfigOrigin::BuiltIn);
    assert_eq!(result, "<built-in>");
}

#[test]
fn relative_config_path_file() {
    let result = relative_config_path(&ConfigOrigin::File("/some/path/loq.toml".into()));
    assert_eq!(result, "loq.toml");
}
