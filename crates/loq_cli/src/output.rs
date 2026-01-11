use std::io;

use loq_core::report::{Finding, FindingKind, SkipReason, Summary};
use loq_core::{ConfigOrigin, Severity};
use loq_fs::walk::WalkError;
use termcolor::{Color, ColorSpec, WriteColor};

pub fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

pub fn write_line<W: WriteColor>(
    writer: &mut W,
    color: Option<Color>,
    line: &str,
) -> io::Result<()> {
    if let Some(color) = color {
        let mut spec = ColorSpec::new();
        spec.set_fg(Some(color));
        writer.set_color(&spec)?;
    }
    writeln!(writer, "{line}")?;
    writer.reset()?;
    Ok(())
}

pub fn write_finding<W: WriteColor>(
    writer: &mut W,
    finding: &Finding,
    verbose: bool,
) -> io::Result<()> {
    let (symbol, color, over_by) = match &finding.kind {
        FindingKind::Violation {
            severity, over_by, ..
        } => match severity {
            Severity::Error => ("✖", Color::Red, Some(*over_by)),
            Severity::Warning => ("⚠", Color::Yellow, Some(*over_by)),
        },
        FindingKind::SkipWarning { .. } => ("⚠", Color::Yellow, None),
    };

    // Line 1: symbol + path (directory dimmed, filename bold)
    let mut spec = ColorSpec::new();
    spec.set_fg(Some(color));
    writer.set_color(&spec)?;
    write!(writer, "{symbol}  ")?;
    writer.reset()?;

    let path = &finding.path;
    if let Some(pos) = path.rfind('/') {
        let (dir, file) = path.split_at(pos + 1);
        spec.set_dimmed(true);
        spec.set_fg(None);
        writer.set_color(&spec)?;
        write!(writer, "{dir}")?;
        writer.reset()?;
        spec.set_dimmed(false);
        spec.set_bold(true);
        writer.set_color(&spec)?;
        writeln!(writer, "{file}")?;
    } else {
        spec.set_bold(true);
        spec.set_fg(None);
        writer.set_color(&spec)?;
        writeln!(writer, "{path}")?;
    }
    writer.reset()?;

    // Line 2: indented details
    match &finding.kind {
        FindingKind::Violation {
            actual,
            limit,
            severity,
            matched_by,
            ..
        } => {
            let over = over_by.unwrap_or(0);
            write!(writer, "   {} lines   ", format_number(*actual))?;
            spec.set_fg(Some(color));
            spec.set_bold(false);
            writer.set_color(&spec)?;
            writeln!(writer, "(+{} over limit)", format_number(over))?;
            writer.reset()?;

            // Verbose: tree structure with rule and config
            if verbose {
                spec.set_dimmed(true);
                writer.set_color(&spec)?;

                let rule_str = match matched_by {
                    loq_core::MatchBy::Rule { pattern } => {
                        format!(
                            "max-lines={} severity={} (match: {})",
                            limit,
                            severity_label(*severity),
                            pattern
                        )
                    }
                    loq_core::MatchBy::Default => {
                        format!(
                            "max-lines={} severity={} (default)",
                            limit,
                            severity_label(*severity)
                        )
                    }
                };
                writeln!(writer, "   ├─ rule:   {rule_str}")?;

                let config_str = relative_config_path(&finding.config_source);
                writeln!(writer, "   └─ config: {config_str}")?;
                writer.reset()?;
            }
        }
        FindingKind::SkipWarning { reason } => {
            let msg: std::borrow::Cow<'static, str> = match reason {
                SkipReason::Binary => "binary file skipped".into(),
                SkipReason::Unreadable(e) => format!("unreadable: {e}").into(),
                SkipReason::Missing => "file not found".into(),
            };
            writeln!(writer, "   {msg}")?;
        }
    }

    writeln!(writer)?; // blank line between findings
    Ok(())
}

fn relative_config_path(origin: &ConfigOrigin) -> String {
    match origin {
        ConfigOrigin::BuiltIn => "<built-in>".to_string(),
        ConfigOrigin::File(path) => {
            // Just show the filename
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        }
    }
}

pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}

pub fn write_block<W: WriteColor>(
    writer: &mut W,
    color: Option<Color>,
    block: &str,
) -> io::Result<()> {
    for (idx, line) in block.lines().enumerate() {
        if idx == 0 {
            write_line(writer, color, line)?;
        } else {
            write_line(writer, None, line)?;
        }
    }
    Ok(())
}

pub fn write_summary<W: WriteColor>(writer: &mut W, summary: &Summary) -> io::Result<()> {
    let mut spec = ColorSpec::new();
    let violations = summary.errors + summary.warnings;

    // Header line
    if violations > 0 {
        let files_word = if summary.passed + violations == 1 {
            "file"
        } else {
            "files"
        };
        writeln!(
            writer,
            "Found {} violations in {} checked {}.",
            violations,
            format_number(summary.passed + violations),
            files_word
        )?;
    } else {
        writeln!(
            writer,
            "All {} files passed.",
            format_number(summary.passed)
        )?;
    }
    writeln!(writer)?;

    // Errors line
    spec.set_fg(Some(Color::Red));
    writer.set_color(&spec)?;
    write!(writer, "  ✖  ")?;
    writer.reset()?;
    let error_label = if summary.errors == 1 {
        "Error"
    } else {
        "Errors"
    };
    writeln!(writer, "{} {}", format_number(summary.errors), error_label)?;

    // Warnings line
    spec.set_fg(Some(Color::Yellow));
    writer.set_color(&spec)?;
    write!(writer, "  ⚠  ")?;
    writer.reset()?;
    let warning_label = if summary.warnings == 1 {
        "Warning"
    } else {
        "Warnings"
    };
    writeln!(
        writer,
        "{} {}",
        format_number(summary.warnings),
        warning_label
    )?;

    // Passed line
    spec.set_fg(Some(Color::Green));
    writer.set_color(&spec)?;
    write!(writer, "  ✔  ")?;
    writer.reset()?;
    writeln!(writer, "{} Passed", format_number(summary.passed))?;

    writeln!(writer)?;

    // Footer (dimmed)
    spec.set_dimmed(true);
    spec.set_fg(None);
    writer.set_color(&spec)?;
    writeln!(writer, "  Time: {}ms", summary.duration_ms)?;
    writer.reset()?;

    Ok(())
}

pub fn print_error<W: WriteColor>(stderr: &mut W, message: &str) -> i32 {
    let _ = write_line(stderr, Some(Color::Red), &format!("error: {message}"));
    2
}

pub fn write_walk_errors<W: WriteColor>(
    writer: &mut W,
    errors: &[WalkError],
    verbose: bool,
) -> io::Result<()> {
    let mut spec = ColorSpec::new();
    spec.set_dimmed(true);
    writer.set_color(&spec)?;

    if verbose {
        writeln!(writer, "Skipped paths ({}):", errors.len())?;
        for error in errors {
            writeln!(writer, "  {}", error.0)?;
        }
    } else {
        writeln!(
            writer,
            "Note: {} path(s) skipped due to errors. Use --verbose for details.",
            errors.len()
        )?;
    }

    writer.reset()?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(format_number(1234567), "1,234,567");
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
    fn print_error_returns_exit_code() {
        let mut buf = NoColor::new(Vec::new());
        let code = print_error(&mut buf, "something went wrong");
        assert_eq!(code, 2);
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
}
