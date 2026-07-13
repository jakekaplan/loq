mod json;

use std::io;

use loq_core::report::{Finding, FindingKind, SkipReason, Summary};
use loq_core::{Limit, Metric};
use loq_fs::walk::WalkError;
use termcolor::{Color, ColorSpec, WriteColor};

pub use json::write_json;

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

/// Color specs for change reports (baseline/tighten/relax).
pub struct ChangeStyle {
    /// Color spec for the "from" value.
    pub from: ColorSpec,
    /// Color spec for the "to" value.
    pub to: ColorSpec,
    /// Color spec for success markers.
    pub ok: ColorSpec,
    /// Color spec for dimmed text.
    pub dimmed: ColorSpec,
}

pub enum Change {
    Added {
        path: String,
        to: usize,
    },
    Updated {
        path: String,
        from: usize,
        to: usize,
    },
    Removed {
        path: String,
        from: usize,
    },
    Adjusted {
        path: String,
        from: usize,
        to: usize,
    },
}

impl Change {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Added { path, .. }
            | Self::Updated { path, .. }
            | Self::Removed { path, .. }
            | Self::Adjusted { path, .. } => path,
        }
    }

    #[must_use]
    pub const fn sort_value(&self) -> usize {
        match self {
            Self::Added { to, .. } | Self::Updated { to, .. } | Self::Adjusted { to, .. } => *to,
            Self::Removed { from, .. } => *from,
        }
    }

    #[must_use]
    pub const fn previous(&self) -> Option<usize> {
        match self {
            Self::Added { .. } => None,
            Self::Updated { from, .. }
            | Self::Removed { from, .. }
            | Self::Adjusted { from, .. } => Some(*from),
        }
    }
}

/// Builds color specs for change reports.
#[must_use]
pub fn change_style() -> ChangeStyle {
    let mut from = ColorSpec::new();
    from.set_fg(Some(Color::Red)).set_bold(true);
    let mut to = ColorSpec::new();
    to.set_fg(Some(Color::Green));
    let mut ok = ColorSpec::new();
    ok.set_fg(Some(Color::Green));
    let mut dimmed = ColorSpec::new();
    dimmed.set_dimmed(true);
    ChangeStyle {
        from,
        to,
        ok,
        dimmed,
    }
}

/// Computes a display width for change values (minimum 6).
pub fn change_width(changes: &[&Change]) -> usize {
    let mut width = 6;
    for change in changes {
        match change {
            Change::Added { to, .. } => {
                width = width.max(format_number(*to).len());
            }
            Change::Updated { from, to, .. } | Change::Adjusted { from, to, .. } => {
                width = width
                    .max(format_number(*from).len())
                    .max(format_number(*to).len());
            }
            Change::Removed { from, .. } => {
                width = width.max(format_number(*from).len());
            }
        }
    }
    width
}

/// Returns the plural suffix (`""` or `"s"`) for `count`.
#[must_use]
pub const fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// Writes a dimmed summary line prefixed with a green check mark.
pub fn write_ok_line<W: WriteColor>(
    writer: &mut W,
    style: &ChangeStyle,
    text: &str,
) -> io::Result<()> {
    writer.set_color(&style.ok)?;
    write!(writer, "✔ ")?;
    writer.reset()?;
    writer.set_color(&style.dimmed)?;
    write!(writer, "{text}")?;
    writer.reset()?;
    writeln!(writer)
}

pub fn write_change<W: WriteColor>(
    writer: &mut W,
    style: &ChangeStyle,
    width: usize,
    change: &Change,
) -> io::Result<()> {
    let (symbol, from, to, path) = match change {
        Change::Added { path, to } => (Some("+"), None, Some(*to), path),
        Change::Updated { path, from, to } => (Some("~"), Some(*from), Some(*to), path),
        Change::Removed { path, from } => (Some("-"), Some(*from), None, path),
        Change::Adjusted { path, from, to } => (None, Some(*from), Some(*to), path),
    };

    if let Some(symbol) = symbol {
        writer.set_color(&style.dimmed)?;
        write!(writer, "{symbol} ")?;
        writer.reset()?;
    }

    let from_str = from.map_or_else(|| "-".to_string(), format_number);
    let to_str = to.map_or_else(|| "-".to_string(), format_number);

    if from.is_some() {
        writer.set_color(&style.from)?;
    } else {
        writer.set_color(&style.dimmed)?;
    }
    write!(writer, "{from_str:>width$}")?;
    writer.reset()?;
    writer.set_color(&style.dimmed)?;
    write!(writer, " -> ")?;
    writer.reset()?;
    if to.is_some() {
        writer.set_color(&style.to)?;
    } else {
        writer.set_color(&style.dimmed)?;
    }
    write!(writer, "{to_str:<width$}")?;
    writer.reset()?;
    write!(writer, " ")?;
    write_path(writer, path)?;
    writeln!(writer)?;

    Ok(())
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

    writer.set_color(&fg(color))?;
    write!(writer, "{symbol} ")?;
    writer.reset()?;

    match &finding.kind {
        FindingKind::Violation {
            actual,
            limit,
            matched_by,
            ..
        } => {
            let actual_str = formatted_measurement(*actual, *limit);
            let limit_str = format_number(limit.max);
            writer.set_color(&fg(color).set_bold(true).clone())?;
            write!(writer, "{actual_str:>6}")?;
            writer.reset()?;
            if limit.metric == Metric::Tokens {
                writer.set_color(&dimmed())?;
                write!(writer, " tokens")?;
                writer.reset()?;
            }
            writer.set_color(&dimmed())?;
            write!(writer, " > ")?;
            writer.reset()?;
            writer.set_color(&fg(Color::Green))?;
            write!(writer, "{limit_str:<6}")?;
            writer.reset()?;

            write!(writer, " ")?;
            write_path(writer, &finding.path)?;
            writeln!(writer)?;

            if verbose {
                writer.set_color(&dimmed())?;
                let rule_str = match matched_by {
                    loq_core::MatchBy::Rule { pattern } => {
                        format!("{}={} (match: {pattern})", limit_key(*limit), limit.max)
                    }
                    loq_core::MatchBy::Default => {
                        format!("{}={} (default)", limit_key(*limit), limit.max)
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

pub(crate) fn write_path<W: WriteColor>(writer: &mut W, path: &str) -> io::Result<()> {
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

fn formatted_measurement(actual: usize, limit: Limit) -> String {
    let value = format_number(actual);
    if limit.is_approximate() {
        format!("~{value}")
    } else {
        value
    }
}

const fn limit_key(limit: Limit) -> &'static str {
    match limit.metric {
        Metric::Lines => "max-lines",
        Metric::Tokens => "max-tokens",
    }
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
        writeln!(writer, "{} {word}", summary.errors)?;
    } else {
        writer.set_color(&fg(Color::Green))?;
        write!(writer, "✔")?;
        writer.reset()?;
        let word = if summary.passed == 1 { "file" } else { "files" };
        writeln!(writer, " {} {word} ok", format_number(summary.passed))?;
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
            writeln!(writer, "  {}", error.message)?;
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
