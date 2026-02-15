use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use loq_fs::git;

use crate::cli::CheckArgs;
use crate::output::JsonFilter;

pub(crate) struct PlannedInputs {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) filter: Option<JsonFilter>,
}

pub(crate) fn plan_inputs<R: Read>(
    args: &CheckArgs,
    stdin: &mut R,
    cwd: &Path,
) -> Result<PlannedInputs> {
    let filter = check_filter(args);
    if args.stdin && filter.is_some() {
        bail!("cannot combine '-' (stdin path list) with --staged/--diff");
    }

    let git_paths = match filter.as_ref() {
        Some(filter) => Some(resolve_git_paths(filter, cwd)?),
        None => None,
    };

    let paths = collect_inputs(args.paths.clone(), args.stdin, stdin, cwd, git_paths)?;

    Ok(PlannedInputs { paths, filter })
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
    let git_filter = to_git_filter(filter);

    git::resolve_paths(cwd, &git_filter)
        .map_err(|error| anyhow::anyhow!(git_error_message(filter, error)))
}

fn to_git_filter(filter: &JsonFilter) -> git::GitFilter {
    match filter {
        JsonFilter::Staged => git::GitFilter::Staged,
        JsonFilter::Diff { git_ref } => git::GitFilter::Diff {
            git_ref: git_ref.clone(),
        },
    }
}

pub(crate) fn git_error_message(filter: &JsonFilter, error: git::GitError) -> String {
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

pub(crate) fn collect_inputs<R: Read>(
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

pub(crate) fn normalize_components(path: &Path) -> PathBuf {
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
