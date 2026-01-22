//! Directory walking and file expansion.
//!
//! Expands paths (files and directories) into a list of files to check,
//! filtering out excluded files (gitignore, exclude patterns) at this layer.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ignore::WalkBuilder;
use loq_core::PatternList;
use thiserror::Error;

use crate::relative_path_for_match;

/// Files/directories that are always excluded regardless of configuration.
const HARDCODED_EXCLUDES: &[&str] = &[".loq_cache", "loq.toml"];

/// Check if a path matches any hardcoded exclude pattern.
fn is_hardcoded_exclude(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| HARDCODED_EXCLUDES.contains(&name))
}

/// Error encountered while walking a directory.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct WalkError {
    /// Error message for a skipped path.
    pub message: String,
}

/// Result of expanding paths.
pub struct WalkResult {
    /// All discovered file paths (already filtered).
    pub paths: Vec<PathBuf>,
    /// Errors encountered during walking.
    pub errors: Vec<WalkError>,
}

/// Options for directory walking and filtering.
pub struct WalkOptions<'a> {
    /// Whether to respect `.gitignore` files during walking.
    pub respect_gitignore: bool,
    /// Exclude patterns from config.
    pub exclude: &'a PatternList,
    /// Root directory for relative path matching.
    pub root_dir: &'a Path,
}

/// Expands paths into a flat list of files, filtering out excluded paths.
///
/// Directories are walked recursively. Non-existent paths are included
/// (to be reported as missing later). Uses parallel walking for performance.
///
/// **Gitignore behavior (matches ruff):**
/// - Explicit file paths bypass gitignore (if you name a file, you want it checked)
/// - Directory walks respect gitignore via the `ignore` crate
/// - Exclude patterns from config always apply to both
#[must_use]
pub fn expand_paths(paths: &[PathBuf], options: &WalkOptions) -> WalkResult {
    let mut files = Vec::new();
    let mut errors = Vec::new();

    for path in paths {
        if path.exists() {
            if path.is_dir() {
                let result = walk_directory(path, options);
                files.extend(result.paths);
                errors.extend(result.errors);
            } else {
                // Explicit file path - bypass gitignore (like ruff), but respect exclude patterns
                if should_skip_explicit_path(path, options) {
                    continue;
                }
                files.push(path.clone());
            }
        } else {
            // Non-existent path - include to report as missing
            files.push(path.clone());
        }
    }

    WalkResult {
        paths: files,
        errors,
    }
}

/// Checks if an explicit file path should be skipped (hardcoded or exclude pattern).
///
/// Explicit paths bypass gitignore (following ruff's model: if you name a file, you want it checked).
fn should_skip_explicit_path(path: &Path, options: &WalkOptions) -> bool {
    is_hardcoded_exclude(path)
        || options
            .exclude
            .matches(&relative_path_for_match(path, options.root_dir))
            .is_some()
}

fn walk_directory(path: &PathBuf, options: &WalkOptions) -> WalkResult {
    let (path_tx, path_rx) = mpsc::channel();
    let (error_tx, error_rx) = mpsc::channel();

    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .git_ignore(options.respect_gitignore)
        .git_global(false)
        .git_exclude(false);

    if options.respect_gitignore {
        builder.add_custom_ignore_filename(".gitignore");
    }

    let walker = builder.build_parallel();

    walker.run(|| {
        let path_tx = path_tx.clone();
        let error_tx = error_tx.clone();
        Box::new(move |entry| {
            match entry {
                Ok(e) => {
                    let entry_path = e.path();
                    // Skip hardcoded excludes (directories and files)
                    if is_hardcoded_exclude(entry_path) {
                        return if e.file_type().is_some_and(|t| t.is_dir()) {
                            ignore::WalkState::Skip
                        } else {
                            ignore::WalkState::Continue
                        };
                    }
                    if e.file_type().is_some_and(|t| t.is_file()) {
                        let _ = path_tx.send(e.into_path());
                    }
                }
                Err(e) => {
                    let _ = error_tx.send(WalkError {
                        message: e.to_string(),
                    });
                }
            }
            ignore::WalkState::Continue
        })
    });

    drop(path_tx);
    drop(error_tx);

    // Filter walked paths through exclude patterns
    // (gitignore is already handled by the walker)
    let paths: Vec<PathBuf> = path_rx
        .into_iter()
        .filter(|p| {
            let relative_path = relative_path_for_match(p, options.root_dir);
            options.exclude.matches(&relative_path).is_none()
        })
        .collect();

    WalkResult {
        paths,
        errors: error_rx.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests;
