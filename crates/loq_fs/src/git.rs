//! Git path discovery for `loq check` filters.
//!
//! Supports:
//! - `--staged`: files in the staging area
//! - `--diff <ref>`: files changed relative to a git ref

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

/// Git-backed path filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFilter {
    /// Files in the staging area.
    Staged,
    /// Files changed relative to the provided git ref.
    Diff {
        /// Git ref/range (for example: `main`, `HEAD~1`, `origin/main..HEAD`).
        git_ref: String,
    },
}

/// Errors from git path discovery.
#[derive(Debug, Error)]
pub enum GitError {
    /// Git executable not found.
    #[error("git is not available")]
    GitNotAvailable,
    /// Current directory is not inside a git repository.
    #[error("not inside a git repository")]
    NotRepository,
    /// Git command failed.
    #[error("git failed: {stderr}")]
    CommandFailed {
        /// Captured stderr/stdout from git.
        stderr: String,
    },
    /// I/O failure while launching git.
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Returns paths selected by the git filter.
///
/// Returned paths are absolute (resolved against `cwd`).
pub fn resolve_paths(cwd: &Path, filter: &GitFilter) -> Result<Vec<PathBuf>, GitError> {
    ensure_git_repository(cwd)?;

    let mut args = vec!["diff", "--name-only", "-z", "--diff-filter=ACMR"];
    match filter {
        GitFilter::Staged => args.push("--staged"),
        GitFilter::Diff { git_ref } => args.push(git_ref),
    }

    let output = run_git(cwd, &args)?;
    if !output.status.success() {
        return Err(GitError::CommandFailed {
            stderr: command_error_text(&output.stderr, &output.stdout),
        });
    }

    Ok(parse_paths(&output.stdout, cwd))
}

fn ensure_git_repository(cwd: &Path) -> Result<(), GitError> {
    let output = run_git(cwd, &["rev-parse", "--is-inside-work-tree"])?;
    if !output.status.success() {
        let error = command_error_text(&output.stderr, &output.stdout);
        if is_not_repository(&error) {
            return Err(GitError::NotRepository);
        }
        return Err(GitError::CommandFailed { stderr: error });
    }

    let inside = String::from_utf8_lossy(&output.stdout);
    if inside.trim() == "true" {
        Ok(())
    } else {
        Err(GitError::NotRepository)
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    match Command::new("git").current_dir(cwd).args(args).output() {
        Ok(output) => Ok(output),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(GitError::GitNotAvailable)
        }
        Err(error) => Err(GitError::Io(error)),
    }
}

fn parse_paths(stdout: &[u8], cwd: &Path) -> Vec<PathBuf> {
    stdout
        .split(|byte| *byte == b'\0')
        .filter(|chunk| !chunk.is_empty())
        .map(bytes_to_path)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .collect()
}

fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        PathBuf::from(OsStr::from_bytes(bytes))
    }

    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).to_string())
    }
}

fn command_error_text(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    "unknown git error".to_string()
}

fn is_not_repository(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("not a git repository")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_paths_resolves_relative_paths() {
        let cwd = Path::new("/repo");
        let output = b"src/main.rs\0README.md\0";
        let paths = parse_paths(output, cwd);
        assert_eq!(paths, vec![cwd.join("src/main.rs"), cwd.join("README.md")]);
    }

    #[test]
    fn parse_paths_preserves_absolute_paths() {
        let cwd = Path::new("/repo");
        let output = b"/tmp/a.rs\0";
        let paths = parse_paths(output, cwd);
        assert_eq!(paths, vec![PathBuf::from("/tmp/a.rs")]);
    }

    #[cfg(unix)]
    #[test]
    fn parse_paths_keeps_utf8_bytes_without_quoting() {
        let cwd = Path::new("/repo");
        let output = b"caf\xC3\xA9.rs\0";
        let paths = parse_paths(output, cwd);
        assert_eq!(paths, vec![cwd.join("café.rs")]);
    }

    #[test]
    fn command_error_prefers_stderr() {
        let text = command_error_text(b"bad ref\n", b"ignored\n");
        assert_eq!(text, "bad ref");
    }

    #[test]
    fn not_repository_detection_is_case_insensitive() {
        assert!(is_not_repository("FATAL: Not a git repository"));
    }
}
