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
    let git_filter = git_filter_from_args(args);
    let has_scope_inputs = !args.paths.is_empty() || args.stdin;
    let default_to_cwd = !has_scope_inputs && git_filter.is_none();
    let inputs = match collect_inputs(args.paths.clone(), args.stdin, stdin, &cwd, default_to_cwd)
        .and_then(|paths| apply_git_filter(paths, git_filter.as_ref(), &cwd, has_scope_inputs))
    {
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

fn apply_git_filter(
    paths: Vec<PathBuf>,
    filter: Option<&GitFilter>,
    cwd: &Path,
    scope_was_provided: bool,
) -> Result<Vec<PathBuf>> {
    let Some(filter) = filter else {
        return Ok(paths);
    };

    let git_paths = list_git_paths(filter, cwd)?;
    if paths.is_empty() {
        return if scope_was_provided {
            Ok(Vec::new())
        } else {
            Ok(git_paths)
        };
    }

    Ok(intersect_paths_with_scope(paths, git_paths, cwd))
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
    let check = run_git(
        &["rev-parse", "--is-inside-work-tree"],
        cwd,
        unavailable_message,
    )?;
    if !check.status.success() || strip_line_endings(&check.stdout) != b"true" {
        bail!("{not_repo_message}");
    }

    let output = run_git(&["rev-parse", "--show-toplevel"], cwd, unavailable_message)?;
    let root = strip_line_endings(&output.stdout);
    anyhow::ensure!(
        output.status.success() && !root.is_empty(),
        "failed to determine git repository root"
    );

    let root_path = path_from_git_bytes(root);
    Ok(dunce::canonicalize(&root_path).unwrap_or(root_path))
}

fn list_git_paths(filter: &GitFilter, cwd: &Path) -> Result<Vec<PathBuf>> {
    let unavailable_message = filter.unavailable_message();
    let not_repo_message = filter.not_repo_message();
    let repo_root = git_repo_root(cwd, unavailable_message, not_repo_message)?;

    let output = match filter {
        GitFilter::Staged => run_git(
            &["diff", "--name-only", "-z", "--cached"],
            cwd,
            unavailable_message,
        )?,
        GitFilter::Diff(reference) => run_git(
            &["diff", "--name-only", "-z", reference],
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

fn normalize_path_lexical(path: &Path) -> PathBuf {
    use std::ffi::{OsStr, OsString};
    use std::path::Component;

    let mut prefix: Option<OsString> = None;
    let mut has_root = false;
    let mut parts: Vec<OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix_component) => {
                prefix = Some(prefix_component.as_os_str().to_owned());
                parts.clear();
                has_root = false;
            }
            Component::RootDir => {
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(last) = parts.pop() {
                    if last.as_os_str() == OsStr::new("..") {
                        parts.push(last);
                        if !has_root {
                            parts.push(OsString::from(".."));
                        }
                    }
                } else if !has_root {
                    parts.push(OsString::from(".."));
                }
            }
            Component::Normal(part) => {
                parts.push(part.to_owned());
            }
        }
    }

    let mut normalized = match (prefix, has_root) {
        (Some(prefix), true) => {
            let mut base = OsString::new();
            base.push(prefix);
            base.push(std::path::MAIN_SEPARATOR.to_string());
            PathBuf::from(base)
        }
        (Some(prefix), false) => PathBuf::from(prefix),
        (None, true) => PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
        (None, false) => PathBuf::new(),
    };

    for part in parts {
        normalized.push(part);
    }
    normalized
}

fn intersect_paths_with_scope(
    scope_paths: Vec<PathBuf>,
    candidate_paths: Vec<PathBuf>,
    cwd: &Path,
) -> Vec<PathBuf> {
    let scope_paths = scope_paths
        .into_iter()
        .map(|path| normalize_scope_path(&path, cwd))
        .collect::<Vec<_>>();

    candidate_paths
        .into_iter()
        .filter_map(|candidate| {
            let candidate = normalize_path_lexical(&candidate);
            if scope_paths
                .iter()
                .any(|scope| candidate_in_scope(&candidate, scope))
            {
                Some(candidate)
            } else {
                None
            }
        })
        .collect()
}

fn normalize_scope_path(path: &Path, cwd: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    normalize_path_lexical(&absolute)
}

fn candidate_in_scope(candidate: &Path, scope: &Path) -> bool {
    candidate.starts_with(scope)
}

fn collect_inputs<R: Read>(
    mut paths: Vec<PathBuf>,
    use_stdin: bool,
    stdin: &mut R,
    cwd: &Path,
    default_to_cwd: bool,
) -> Result<Vec<PathBuf>> {
    if use_stdin {
        let mut stdin_paths =
            loq_fs::stdin::read_paths(stdin, cwd).context("failed to read stdin")?;
        paths.append(&mut stdin_paths);
    }

    if paths.is_empty() && default_to_cwd {
        paths.push(PathBuf::from("."));
    }

    Ok(paths)
}

#[cfg(test)]
mod tests;
