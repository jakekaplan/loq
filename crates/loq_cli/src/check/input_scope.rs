use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::cli::CheckArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
enum GitFilter {
    Staged,
    Diff(String),
}

impl GitFilter {
    const fn flag_name(&self) -> &'static str {
        match self {
            Self::Staged => "--staged",
            Self::Diff(_) => "--diff",
        }
    }

    fn unavailable_message(&self) -> String {
        format!(
            "{} requires git, but git is not available",
            self.flag_name()
        )
    }

    fn not_repo_message(&self) -> String {
        format!(
            "{} requires a git repository (run inside a repo)",
            self.flag_name()
        )
    }
}

fn git_filter_from_args(args: &CheckArgs) -> Option<GitFilter> {
    if args.staged {
        Some(GitFilter::Staged)
    } else {
        args.diff.clone().map(GitFilter::Diff)
    }
}

pub(super) fn resolve_check_inputs<R: Read>(
    args: &CheckArgs,
    stdin: &mut R,
    cwd: &Path,
) -> Result<Vec<PathBuf>> {
    match git_filter_from_args(args) {
        Some(filter) => list_git_paths(&filter, cwd),
        None => collect_inputs(args.paths.clone(), args.stdin, stdin, cwd),
    }
}

fn run_git(args: &[&str], cwd: &Path, unavailable_message: &str) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow!("{unavailable_message}")
            } else {
                anyhow::Error::new(error).context("failed to run git")
            }
        })
}

fn git_command_error(prefix: &str, output: &std::process::Output) -> anyhow::Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    if message.is_empty() {
        anyhow!("{prefix} failed with status {}", output.status)
    } else {
        anyhow!("{prefix} failed: {message}")
    }
}

fn git_repo_root(cwd: &Path, unavailable_message: &str, not_repo_message: &str) -> Result<PathBuf> {
    let output = run_git(&["rev-parse", "--show-toplevel"], cwd, unavailable_message)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        if message.contains("not a git repository") {
            bail!("{not_repo_message}");
        }
        return Err(git_command_error("git rev-parse", &output));
    }

    let root = strip_line_endings(&output.stdout);
    anyhow::ensure!(!root.is_empty(), "failed to determine git repository root");

    let root_path = path_from_git_bytes(root);
    Ok(dunce::canonicalize(&root_path).unwrap_or(root_path))
}

fn list_git_paths(filter: &GitFilter, cwd: &Path) -> Result<Vec<PathBuf>> {
    let unavailable_message = filter.unavailable_message();
    let not_repo_message = filter.not_repo_message();
    let repo_root = git_repo_root(cwd, &unavailable_message, &not_repo_message)?;

    let output = match filter {
        GitFilter::Staged => run_git(
            &[
                "-c",
                "diff.relative=false",
                "diff",
                "--name-only",
                "-z",
                "--cached",
                "--diff-filter=d",
            ],
            &repo_root,
            &unavailable_message,
        )?,
        GitFilter::Diff(reference) => run_git(
            &[
                "-c",
                "diff.relative=false",
                "diff",
                "--name-only",
                "-z",
                "--diff-filter=d",
                reference,
            ],
            &repo_root,
            &unavailable_message,
        )?,
    };

    if !output.status.success() {
        return Err(git_command_error("git diff", &output));
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
