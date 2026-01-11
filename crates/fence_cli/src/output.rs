use std::io;

use fence_core::report::{Finding, FindingKind, SkipReason, Summary};
use fence_core::{ConfigOrigin, Severity};
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
                    fence_core::MatchBy::Rule { pattern } => {
                        format!(
                            "max-lines={} severity={} (match: {})",
                            limit,
                            severity_label(*severity),
                            pattern
                        )
                    }
                    fence_core::MatchBy::Default => {
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
            let msg = match reason {
                SkipReason::Binary => "binary file skipped",
                SkipReason::Unreadable(e) => {
                    return writeln!(writer, "   unreadable: {e}");
                }
                SkipReason::Missing => "file not found",
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
