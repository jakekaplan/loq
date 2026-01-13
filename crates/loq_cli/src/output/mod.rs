use std::io;

use loq_core::report::{Finding, FindingKind, SkipReason, Summary};
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
    let (symbol, color) = match &finding.kind {
        FindingKind::Violation { .. } => ("✖", Color::Red),
        FindingKind::SkipWarning { .. } => ("⚠", Color::Yellow),
    };

    // Symbol
    writer.set_color(&fg(color))?;
    write!(writer, "{symbol} ")?;
    writer.reset()?;

    // Details first (fixed-width), then path (variable-width)
    match &finding.kind {
        FindingKind::Violation {
            actual,
            limit,
            matched_by,
            ..
        } => {
            // Format: ✖ 1,427 > 500  path/to/file.rs
            // Right-align actual within 6 chars (handles up to 99,999)
            let actual_str = format_number(*actual);
            let limit_str = format_number(*limit);
            writer.set_color(&fg(color).set_bold(true).clone())?;
            write!(writer, "{actual_str:>6}")?;
            writer.reset()?;
            writer.set_color(&dimmed())?;
            write!(writer, " > ")?;
            writer.reset()?;
            writer.set_color(&fg(Color::Green))?;
            write!(writer, "{limit_str:<6}")?;
            writer.reset()?;

            // Path (directory dimmed, filename bold)
            write!(writer, " ")?;
            write_path(writer, &finding.path)?;
            writeln!(writer)?;

            if verbose {
                writer.set_color(&dimmed())?;
                let rule_str = match matched_by {
                    loq_core::MatchBy::Rule { pattern } => {
                        format!("max-lines={limit} (match: {pattern})")
                    }
                    loq_core::MatchBy::Default => {
                        format!("max-lines={limit} (default)")
                    }
                };
                writeln!(writer, "                  └─ rule: {rule_str}")?;
                writer.reset()?;
            }
        }
        FindingKind::SkipWarning { reason } => {
            let msg = match reason {
                SkipReason::Binary => "binary file skipped",
                SkipReason::Unreadable(e) => {
                    write_path(writer, &finding.path)?;
                    return writeln!(writer, "  unreadable: {e}");
                }
                SkipReason::Missing => "file not found",
            };
            write_path(writer, &finding.path)?;
            writeln!(writer, "  {msg}")?;
        }
    }

    Ok(())
}

fn write_path<W: WriteColor>(writer: &mut W, path: &str) -> io::Result<()> {
    if let Some(pos) = path.rfind('/') {
        let (dir, file) = path.split_at(pos + 1);
        writer.set_color(&dimmed())?;
        write!(writer, "{dir}")?;
        writer.reset()?;
        writer.set_color(&bold())?;
        write!(writer, "{file}")?;
    } else {
        writer.set_color(&bold())?;
        write!(writer, "{path}")?;
    }
    writer.reset()
}

pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, '_');
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
    if summary.errors > 0 {
        let word = if summary.errors == 1 {
            "violation"
        } else {
            "violations"
        };
        writer.set_color(&fg(Color::Red))?;
        write!(writer, "{} {word}", summary.errors)?;
        writer.reset()?;
        writer.set_color(&dimmed())?;
        writeln!(writer, " ({}ms)", summary.duration_ms)?;
    } else {
        writer.set_color(&fg(Color::Green))?;
        write!(writer, "✔")?;
        writer.reset()?;
        write!(writer, " {} files ok", format_number(summary.passed))?;
        writer.set_color(&dimmed())?;
        writeln!(writer, " ({}ms)", summary.duration_ms)?;
    }
    writer.reset()
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

/// Writes fix guidance text when violations exist.
///
/// Outputs a blank line followed by the guidance text exactly as configured.
pub fn write_guidance<W: WriteColor>(writer: &mut W, guidance: &str) -> io::Result<()> {
    writeln!(writer)?;
    write!(writer, "{guidance}")?;
    if !guidance.ends_with('\n') {
        writeln!(writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
