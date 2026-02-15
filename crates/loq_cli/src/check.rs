//! Check command implementation.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use loq_core::report::{build_report, FindingKind, Report};
use loq_fs::{git, CheckOptions, CheckOutput, FsError};
use termcolor::{Color, WriteColor};

use crate::cli::{CheckArgs, OutputFormat};
use crate::output::{
    print_error, write_block, write_finding, write_guidance, write_json, write_summary,
    write_walk_errors, JsonFilter,
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
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let filter = check_filter(args);
    if args.stdin && filter.is_some() {
        return print_error(
            stderr,
            "cannot combine '-' (stdin path list) with --staged/--diff",
        );
    }

    let git_paths = match filter.as_ref() {
        Some(filter) => match resolve_git_paths(filter, &cwd) {
            Ok(paths) => Some(paths),
            Err(err) => return print_error(stderr, &format!("{err:#}")),
        },
        None => None,
    };

    let inputs = match collect_inputs(args.paths.clone(), args.stdin, stdin, &cwd, git_paths) {
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

    handle_check_output(output, stdout, mode, args.output_format, filter.as_ref())
}

fn check_filter(args: &CheckArgs) -> Option<JsonFilter> {
    if args.staged {
        Some(JsonFilter::Staged)
    } else {
        args.diff_ref
            .clone()
            .map(|git_ref| JsonFilter::Diff { git_ref })
    }
}

fn resolve_git_paths(filter: &JsonFilter, cwd: &Path) -> Result<Vec<PathBuf>> {
    let git_filter = match filter {
        JsonFilter::Staged => git::GitFilter::Staged,
        JsonFilter::Diff { git_ref } => git::GitFilter::Diff {
            git_ref: git_ref.clone(),
        },
    };

    git::resolve_paths(cwd, &git_filter)
        .map_err(|error| anyhow::anyhow!(git_error_message(filter, error)))
}

fn git_error_message(filter: &JsonFilter, error: git::GitError) -> String {
    let flag = match filter {
        JsonFilter::Staged => "--staged",
        JsonFilter::Diff { .. } => "--diff",
    };

    match error {
        git::GitError::GitNotAvailable => format!("{flag} requires git, but git is not available"),
        git::GitError::NotRepository => format!("{flag} requires a git repository"),
        git::GitError::CommandFailed { stderr } => format!("git failed: {stderr}"),
        git::GitError::Io(error) => format!("git failed: {error}"),
    }
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
    filter: Option<&JsonFilter>,
) -> ExitStatus {
    let CheckOutput {
        outcomes,
        walk_errors,
        fix_guidance,
    } = output;
    let report = build_report(&outcomes, fix_guidance);

    match format {
        OutputFormat::Json => {
            let _ = write_json(stdout, &report, &walk_errors, filter);
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

fn collect_inputs<R: Read>(
    mut paths: Vec<PathBuf>,
    use_stdin: bool,
    stdin: &mut R,
    cwd: &Path,
    git_paths: Option<Vec<PathBuf>>,
) -> Result<Vec<PathBuf>> {
    if use_stdin {
        let mut stdin_paths =
            loq_fs::stdin::read_paths(stdin, cwd).context("failed to read stdin")?;
        paths.append(&mut stdin_paths);
    }

    if let Some(git_paths) = git_paths {
        if paths.is_empty() {
            return Ok(git_paths);
        }
        return Ok(intersect_paths(git_paths, &paths, cwd));
    }

    if paths.is_empty() && !use_stdin {
        paths.push(PathBuf::from("."));
    }

    Ok(paths)
}

fn intersect_paths(
    git_paths: Vec<PathBuf>,
    selected_paths: &[PathBuf],
    cwd: &Path,
) -> Vec<PathBuf> {
    let prefixes: Vec<PathBuf> = selected_paths
        .iter()
        .map(|path| normalize_for_prefix(path, cwd))
        .collect();

    git_paths
        .into_iter()
        .filter(|git_path| {
            let git_path = normalize_for_prefix(git_path, cwd);
            prefixes
                .iter()
                .any(|prefix| git_path == *prefix || git_path.starts_with(prefix))
        })
        .collect()
}

fn normalize_for_prefix(path: &Path, cwd: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    normalize_components(&absolute)
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests;
