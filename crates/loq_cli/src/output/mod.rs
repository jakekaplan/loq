use std::io;

use loq_core::report::{Finding, FindingKind, SkipReason, Summary};
use loq_core::{ConfigOrigin, Severity};
use loq_fs::walk::WalkError;
use termcolor::{Color, ColorSpec, WriteColor};

fn fg(color: Color) -> ColorSpec {
    let mut spec = ColorSpec::new();
    spec.set_fg(Some(color));
    spec
}

fn bold() -> ColorSpec {
    let mut spec = ColorSpec::new();
    spec.set_bold(true);
    spec
}

fn dimmed() -> ColorSpec {
    let mut spec = ColorSpec::new();
    spec.set_dimmed(true);
    spec
}

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
        writer.set_color(&fg(color))?;
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
    writer.set_color(&fg(color))?;
    write!(writer, "{symbol}  ")?;
    writer.reset()?;

    let path = &finding.path;
    if let Some(pos) = path.rfind('/') {
        let (dir, file) = path.split_at(pos + 1);
        writer.set_color(&dimmed())?;
        write!(writer, "{dir}")?;
        writer.reset()?;
        writer.set_color(&bold())?;
        writeln!(writer, "{file}")?;
    } else {
        writer.set_color(&bold())?;
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
            writer.set_color(&fg(color))?;
            writeln!(writer, "(+{} over limit)", format_number(over))?;
            writer.reset()?;

            if verbose {
                writer.set_color(&dimmed())?;
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
                writeln!(
                    writer,
                    "   └─ config: {}",
                    relative_config_path(&finding.config_source)
                )?;
                writer.reset()?;
            }
        }
        FindingKind::SkipWarning { reason } => {
            let msg = match reason {
                SkipReason::Binary => "binary file skipped",
                SkipReason::Unreadable(e) => return writeln!(writer, "   unreadable: {e}\n"),
                SkipReason::Missing => "file not found",
            };
            writeln!(writer, "   {msg}")?;
        }
    }

    writeln!(writer)
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
    let violations = summary.errors + summary.warnings;
    let total = summary.passed + violations;

    if violations > 0 {
        let violation_word = if violations == 1 {
            "violation"
        } else {
            "violations"
        };
        let files_word = if total == 1 { "file" } else { "files" };
        writeln!(
            writer,
            "Found {violations} {violation_word} in {} checked {files_word}.",
            format_number(total)
        )?;
    } else {
        writeln!(
            writer,
            "All {} files passed.",
            format_number(summary.passed)
        )?;
    }
    writeln!(writer)?;

    write_count_line(writer, "✖", Color::Red, summary.errors, "Error", "Errors")?;
    write_count_line(
        writer,
        "⚠",
        Color::Yellow,
        summary.warnings,
        "Warning",
        "Warnings",
    )?;
    write_count_line(
        writer,
        "✔",
        Color::Green,
        summary.passed,
        "Passed",
        "Passed",
    )?;
    writeln!(writer)?;

    writer.set_color(&dimmed())?;
    writeln!(writer, "  Time: {}ms", summary.duration_ms)?;
    writer.reset()
}

fn write_count_line<W: WriteColor>(
    writer: &mut W,
    symbol: &str,
    color: Color,
    count: usize,
    singular: &str,
    plural: &str,
) -> io::Result<()> {
    writer.set_color(&fg(color))?;
    write!(writer, "  {symbol}  ")?;
    writer.reset()?;
    let label = if count == 1 { singular } else { plural };
    writeln!(writer, "{} {label}", format_number(count))
}

pub fn print_error<W: WriteColor>(stderr: &mut W, message: &str) -> crate::ExitStatus {
    let _ = write_line(stderr, Some(Color::Red), &format!("error: {message}"));
    crate::ExitStatus::Error
}

pub fn write_walk_errors<W: WriteColor>(
    writer: &mut W,
    errors: &[WalkError],
    verbose: bool,
) -> io::Result<()> {
    writer.set_color(&dimmed())?;
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
    writer.reset()
}

#[cfg(test)]
mod tests;
