//! Check command implementation.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use loq_core::report::{build_report, FindingKind, Report};
use loq_fs::{CheckOptions, CheckOutput, FsError};
use termcolor::{Color, WriteColor};

use crate::cli::{CheckArgs, OutputFormat};
use crate::output::{
    print_error, write_block, write_finding, write_guidance, write_json, write_summary,
    write_walk_errors,
};
use crate::Cli;
use crate::ExitStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Default,
    Verbose,
}

pub const fn output_mode(cli: &Cli) -> OutputMode {
    if cli.verbose {
        OutputMode::Verbose
    } else {
        OutputMode::Default
    }
}

pub fn run_check<R: Read, W1: WriteColor + Write, W2: WriteColor>(
    args: &CheckArgs,
    stdin: &mut R,
    stdout: &mut W1,
    stderr: &mut W2,
    mode: OutputMode,
) -> ExitStatus {
    let cwd = std::env::current_dir()
        .and_then(dunce::canonicalize)
        .unwrap_or_else(|_| PathBuf::from("."));

    let inputs = match git_filter_from_args(args) {
        Some(filter) => list_git_paths(&filter, &cwd),
        None => collect_inputs(args.paths.clone(), args.stdin, stdin, &cwd),
    };

    let inputs = match inputs {
        Ok(paths) => paths,
        Err(err) => return print_error(stderr, &format!("{err:#}")),
    };

    let options = CheckOptions {
        config_path: None,
        cwd: cwd.clone(),
        use_cache: !args.no_cache,
    };

    let output = match loq_fs::run_check(inputs, options) {
        Ok(output) => output,
        Err(err) => return handle_fs_error(&err, stderr),
    };

    handle_check_output(output, stdout, mode, args.output_format)
}

fn handle_fs_error<W: WriteColor>(err: &FsError, stderr: &mut W) -> ExitStatus {
    let message = format!("error: {err}");
    let _ = write_block(stderr, Some(Color::Red), &message);
    ExitStatus::Error
}

fn handle_check_output<W: WriteColor + Write>(
    output: CheckOutput,
    stdout: &mut W,
    mode: OutputMode,
    format: OutputFormat,
) -> ExitStatus {
    let CheckOutput {
        outcomes,
        walk_errors,
        fix_guidance,
    } = output;
    let report = build_report(&outcomes, fix_guidance);

    match format {
        OutputFormat::Json => {
            let _ = write_json(stdout, &report, &walk_errors);
        }
        OutputFormat::Text => {
            write_text_output(stdout, &report, &walk_errors, mode);
        }
    }

    if report.summary.errors > 0 {
        ExitStatus::Failure
    } else {
        ExitStatus::Success
    }
}

fn write_text_output<W: WriteColor>(
    stdout: &mut W,
    report: &Report,
    walk_errors: &[loq_fs::walk::WalkError],
    mode: OutputMode,
) {
    let verbose = mode == OutputMode::Verbose;
    for finding in &report.findings {
        if !verbose && matches!(finding.kind, FindingKind::SkipWarning { .. }) {
            continue;
        }
        let _ = write_finding(stdout, finding, verbose);
    }
    let _ = write_summary(stdout, &report.summary);

    if let Some(guidance) = &report.fix_guidance {
        let _ = write_guidance(stdout, guidance);
    }

    if !walk_errors.is_empty() {
        let _ = write_walk_errors(stdout, walk_errors, verbose);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GitFilter {
    Staged,
    Diff(String),
}

impl GitFilter {
    const fn unavailable_message(&self) -> &'static str {
        match self {
            Self::Staged => "--staged requires git, but git is not available",
            Self::Diff(_) => "--diff requires git, but git is not available",
        }
    }

    const fn not_repo_message(&self) -> &'static str {
        match self {
            Self::Staged => "--staged requires a git repository (run inside a repo)",
            Self::Diff(_) => "--diff requires a git repository (run inside a repo)",
        }
    }
}

fn git_filter_from_args(args: &CheckArgs) -> Option<GitFilter> {
    if args.staged {
        Some(GitFilter::Staged)
    } else {
        args.diff.clone().map(GitFilter::Diff)
    }
}

fn run_git(args: &[&str], cwd: &Path, unavailable_message: &str) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("{unavailable_message}")
            } else {
                anyhow::Error::new(error).context("failed to run git")
            }
        })
}

fn git_repo_root(cwd: &Path, unavailable_message: &str, not_repo_message: &str) -> Result<PathBuf> {
    let output = run_git(&["rev-parse", "--show-toplevel"], cwd, unavailable_message)?;
    if !output.status.success() {
        bail!("{not_repo_message}");
    }

    let root = strip_line_endings(&output.stdout);
    anyhow::ensure!(!root.is_empty(), "failed to determine git repository root");

    let root_path = path_from_git_bytes(root);
    Ok(dunce::canonicalize(&root_path).unwrap_or(root_path))
}

fn list_git_paths(filter: &GitFilter, cwd: &Path) -> Result<Vec<PathBuf>> {
    let unavailable_message = filter.unavailable_message();
    let not_repo_message = filter.not_repo_message();
    let repo_root = git_repo_root(cwd, unavailable_message, not_repo_message)?;

    let output = match filter {
        GitFilter::Staged => run_git(
            &["diff", "--name-only", "-z", "--cached", "--diff-filter=d"],
            cwd,
            unavailable_message,
        )?,
        GitFilter::Diff(reference) => run_git(
            &["diff", "--name-only", "-z", "--diff-filter=d", reference],
            cwd,
            unavailable_message,
        )?,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        if message.is_empty() {
            bail!("git diff failed with status {}", output.status);
        }
        bail!("git diff failed: {message}");
    }

    let mut paths = output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter_map(decode_git_path)
        .map(|path| repo_root.join(path))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();

    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn strip_line_endings(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

fn decode_git_path(line: &[u8]) -> Option<PathBuf> {
    if line.is_empty() {
        return None;
    }
    Some(path_from_git_bytes(line))
}

fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).as_ref())
    }
}

fn collect_inputs<R: Read>(
    mut paths: Vec<PathBuf>,
    use_stdin: bool,
    stdin: &mut R,
    cwd: &Path,
) -> Result<Vec<PathBuf>> {
    if use_stdin {
        let mut stdin_paths =
            loq_fs::stdin::read_paths(stdin, cwd).context("failed to read stdin")?;
        paths.append(&mut stdin_paths);
    }

    if paths.is_empty() && !use_stdin {
        paths.push(PathBuf::from("."));
    }

    Ok(paths)
}

#[cfg(test)]
mod tests;
