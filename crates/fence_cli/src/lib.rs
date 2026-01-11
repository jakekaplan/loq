#![forbid(unsafe_code)]

mod cli;

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use fence_core::format::{format_finding, format_success};
use fence_core::report::{build_report, Finding, FindingKind, Summary};
use fence_fs::{CheckOptions, CheckOutput, FsError};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

pub use cli::{Cli, Command};

pub fn run_env() -> i32 {
    let args = std::env::args_os();
    let stdin = io::stdin();
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    run_with(args, stdin.lock(), &mut stdout, &mut stderr)
}

pub fn run_with<I, R, W1, W2>(args: I, mut stdin: R, stdout: &mut W1, stderr: &mut W2) -> i32
where
    I: IntoIterator<Item = OsString>,
    R: Read,
    W1: WriteColor,
    W2: WriteColor,
{
    let cli = Cli::parse_from(args);
    let mode = output_mode(&cli);

    let command = cli
        .command
        .clone()
        .unwrap_or(Command::Check(cli::CheckArgs { paths: vec![] }));
    match command {
        Command::Check(args) => run_check(args, &cli, &mut stdin, stdout, stderr, mode),
        Command::Init(args) => run_init(args, &cli, stdout, stderr),
    }
}

fn run_check<R: Read, W1: WriteColor, W2: WriteColor>(
    args: cli::CheckArgs,
    cli: &Cli,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
    mode: OutputMode,
) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let inputs = match collect_inputs(args.paths, stdin, &cwd) {
        Ok(paths) => paths,
        Err(err) => return print_error(stderr, &err),
    };

    let options = CheckOptions {
        config_path: cli.config.clone(),
        cwd: cwd.clone(),
    };

    let start = Instant::now();
    let output = match fence_fs::run_check(inputs, options) {
        Ok(output) => output,
        Err(err) => return handle_fs_error(err, stderr),
    };
    let duration_ms = start.elapsed().as_millis();

    handle_check_output(output, duration_ms, stdout, mode)
}

fn handle_fs_error<W: WriteColor>(err: FsError, stderr: &mut W) -> i32 {
    let message = format!("error: {err}");
    let _ = write_block(stderr, Some(Color::Red), &message);
    2
}

fn handle_check_output<W: WriteColor>(
    mut output: CheckOutput,
    duration_ms: u128,
    stdout: &mut W,
    mode: OutputMode,
) -> i32 {
    output
        .outcomes
        .sort_by(|a, b| a.display_path.cmp(&b.display_path));

    let report = build_report(&output.outcomes, duration_ms);

    match mode {
        OutputMode::Silent => {}
        OutputMode::Quiet => {
            for finding in &report.findings {
                if matches!(
                    &finding.kind,
                    FindingKind::Violation { severity, .. }
                        if *severity == fence_core::Severity::Error
                ) {
                    let line = format_finding(finding);
                    let _ = write_line(stdout, Some(Color::Red), &line);
                }
            }
        }
        _ => {
            if report.findings.is_empty() {
                let line = format_success(&report.summary);
                let _ = write_line(stdout, Some(Color::Green), &line);
            } else {
                for finding in &report.findings {
                    if mode != OutputMode::Verbose
                        && matches!(finding.kind, FindingKind::SkipWarning { .. })
                    {
                        continue;
                    }
                    let (color, line) = match &finding.kind {
                        FindingKind::Violation { severity, .. } => match severity {
                            fence_core::Severity::Error => {
                                (Some(Color::Red), format_finding(finding))
                            }
                            fence_core::Severity::Warning => {
                                (Some(Color::Yellow), format_finding(finding))
                            }
                        },
                        FindingKind::SkipWarning { .. } => {
                            (Some(Color::Yellow), format_finding(finding))
                        }
                    };
                    let _ = write_line(stdout, color, &line);
                    if mode == OutputMode::Verbose {
                        print_finding_verbose(finding, stdout);
                    }
                }
                let _ = writeln!(stdout);
                let _ = write_label(stdout, "Summary");
                let _ = write_summary(stdout, &report.summary);
            }
        }
    }

    if report.summary.errors > 0 {
        1
    } else {
        0
    }
}

fn print_finding_verbose<W: WriteColor>(finding: &Finding, stdout: &mut W) {
    let config_line = format!("  config: {}", config_source_label(&finding.config_source));
    let _ = write_line(stdout, None, &config_line);

    if let FindingKind::Violation {
        limit,
        severity,
        matched_by,
        ..
    } = &finding.kind
    {
        let rule_line = match matched_by {
            fence_core::MatchBy::Rule { pattern } => format!(
                "  rule: max-lines={} severity={} (matched: {})",
                limit,
                severity_label(*severity),
                pattern
            ),
            fence_core::MatchBy::Default => format!(
                "  rule: default max-lines={} severity={}",
                limit,
                severity_label(*severity)
            ),
        };
        let _ = write_line(stdout, None, &rule_line);
    }
}

fn config_source_label(origin: &fence_core::ConfigOrigin) -> String {
    match origin {
        fence_core::ConfigOrigin::BuiltIn => "<built-in defaults>".to_string(),
        fence_core::ConfigOrigin::File(path) => path.display().to_string(),
    }
}

fn severity_label(severity: fence_core::Severity) -> &'static str {
    match severity {
        fence_core::Severity::Error => "error",
        fence_core::Severity::Warning => "warning",
    }
}

fn collect_inputs<R: Read>(
    mut paths: Vec<PathBuf>,
    stdin: &mut R,
    cwd: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut use_stdin = false;
    paths.retain(|path| {
        if path == Path::new("-") {
            use_stdin = true;
            false
        } else {
            true
        }
    });

    if use_stdin {
        let mut stdin_paths = fence_fs::stdin::read_paths(stdin, cwd)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
        paths.append(&mut stdin_paths);
    }

    if paths.is_empty() && !use_stdin {
        paths.push(PathBuf::from("."));
    }

    Ok(paths)
}

fn run_init<W1: WriteColor, W2: WriteColor>(
    args: cli::InitArgs,
    cli: &Cli,
    stdout: &mut W1,
    stderr: &mut W2,
) -> i32 {
    if cli.config.is_some() || cli.quiet || cli.silent || cli.verbose {
        return print_error(stderr, "init does not accept output or config flags");
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(".fence.toml");
    if path.exists() {
        return print_error(stderr, ".fence.toml already exists");
    }

    let content = if args.baseline {
        match baseline_config(&cwd) {
            Ok(content) => content,
            Err(err) => return print_error(stderr, &err),
        }
    } else {
        default_config_text(&[])
    };

    if let Err(err) = std::fs::write(&path, content) {
        return print_error(stderr, &format!("failed to write .fence.toml: {err}"));
    }

    let _ = std::io::Write::flush(stdout);
    0
}

fn baseline_config(cwd: &Path) -> Result<String, String> {
    let temp_path = cwd.join(".fence.baseline.tmp.toml");
    let template = default_config_text(&[]);
    std::fs::write(&temp_path, template)
        .map_err(|err| format!("failed to create baseline config: {err}"))?;

    let options = CheckOptions {
        config_path: Some(temp_path.clone()),
        cwd: cwd.to_path_buf(),
    };

    let output = fence_fs::run_check(vec![cwd.to_path_buf()], options);
    let _ = std::fs::remove_file(&temp_path);
    let output = output.map_err(|err| format!("baseline check failed: {err}"))?;

    let mut exempt = Vec::new();
    for outcome in output.outcomes {
        if let fence_core::OutcomeKind::Violation {
            severity: fence_core::Severity::Error,
            ..
        } = outcome.kind
        {
            let mut path = outcome.display_path.replace('\\', "/");
            if path.starts_with("./") {
                path = path.trim_start_matches("./").to_string();
            }
            exempt.push(path);
        }
    }

    exempt.sort();
    exempt.dedup();

    Ok(default_config_text(&exempt))
}

fn default_config_text(exempt: &[String]) -> String {
    let mut output = String::new();
    output.push_str("# fence: an \"electric fence\" that keeps files small for humans and LLMs.\n");
    output.push_str("# Counted lines are wc -l style (includes blanks/comments).\n\n");
    output.push_str("default_max_lines = 400\n\n");
    output.push_str("respect_gitignore = true\n\n");
    let exclude = fence_core::FenceConfig::init_template().exclude;
    if exclude.is_empty() {
        output.push_str("exclude = []\n\n");
    } else {
        output.push_str("exclude = [\n");
        for pattern in exclude {
            output.push_str(&format!("  \"{pattern}\",\n"));
        }
        output.push_str("]\n\n");
    }

    if exempt.is_empty() {
        output.push_str("exempt = []\n\n");
    } else {
        output.push_str("exempt = [\n");
        for path in exempt {
            output.push_str(&format!("  \"{path}\",\n"));
        }
        output.push_str("]\n\n");
    }

    output.push_str("# Last match wins. Put general rules first and overrides later.\n");
    output.push_str("[[rules]]\n");
    output.push_str("path = \"**/*.tsx\"\n");
    output.push_str("max_lines = 300\n");
    output.push_str("severity = \"warning\"\n\n");
    output.push_str("[[rules]]\n");
    output.push_str("path = \"tests/**/*\"\n");
    output.push_str("max_lines = 500\n");
    output
}

fn write_line<W: WriteColor>(writer: &mut W, color: Option<Color>, line: &str) -> io::Result<()> {
    if let Some(color) = color {
        let mut spec = ColorSpec::new();
        spec.set_fg(Some(color));
        writer.set_color(&spec)?;
    }
    writeln!(writer, "{line}")?;
    writer.reset()?;
    Ok(())
}

fn write_block<W: WriteColor>(writer: &mut W, color: Option<Color>, block: &str) -> io::Result<()> {
    for (idx, line) in block.lines().enumerate() {
        if idx == 0 {
            write_line(writer, color, line)?;
        } else {
            write_line(writer, None, line)?;
        }
    }
    Ok(())
}

fn write_label<W: WriteColor>(writer: &mut W, label: &str) -> io::Result<()> {
    let mut spec = ColorSpec::new();
    spec.set_dimmed(true);
    writer.set_color(&spec)?;
    writeln!(writer, "{label}")?;
    writer.reset()?;
    Ok(())
}

fn write_summary<W: WriteColor>(writer: &mut W, summary: &Summary) -> io::Result<()> {
    let error_label = if summary.errors == 1 {
        "error"
    } else {
        "errors"
    };
    let warning_label = if summary.warnings == 1 {
        "warning"
    } else {
        "warnings"
    };

    write!(
        writer,
        "{} files checked ({} skipped): ",
        summary.total, summary.skipped
    )?;

    let mut spec = ColorSpec::new();
    spec.set_fg(Some(Color::Green));
    writer.set_color(&spec)?;
    write!(writer, "{} passed", summary.passed)?;
    writer.reset()?;

    write!(writer, " | ")?;
    spec.set_fg(Some(Color::Red));
    writer.set_color(&spec)?;
    write!(writer, "{} {}", summary.errors, error_label)?;
    writer.reset()?;

    write!(writer, " | ")?;
    spec.set_fg(Some(Color::Yellow));
    writer.set_color(&spec)?;
    write!(writer, "{} {}", summary.warnings, warning_label)?;
    writer.reset()?;

    writeln!(writer, " [{}ms]", summary.duration_ms)?;
    Ok(())
}

fn print_error<W: WriteColor>(stderr: &mut W, message: &str) -> i32 {
    let _ = write_line(stderr, Some(Color::Red), &format!("error: {message}"));
    2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Default,
    Quiet,
    Silent,
    Verbose,
}

fn output_mode(cli: &Cli) -> OutputMode {
    if cli.silent {
        OutputMode::Silent
    } else if cli.quiet {
        OutputMode::Quiet
    } else if cli.verbose {
        OutputMode::Verbose
    } else {
        OutputMode::Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fail"))
        }
    }

    #[test]
    fn collect_inputs_reports_stdin_error() {
        let err = collect_inputs(vec![PathBuf::from("-")], &mut FailingReader, Path::new("."))
            .unwrap_err();
        assert!(err.contains("failed to read stdin"));
    }

    #[test]
    fn output_mode_precedence() {
        let cli = Cli {
            command: None,
            quiet: true,
            silent: true,
            verbose: true,
            config: None,
        };
        assert_eq!(output_mode(&cli), OutputMode::Silent);
    }
}
