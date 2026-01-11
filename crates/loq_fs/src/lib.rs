//! Filesystem operations for loq.
//!
//! This crate handles file discovery, walking directories, counting lines,
//! and orchestrating checks across multiple files with parallel processing.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod count;
pub mod discover;
pub mod stdin;
pub mod walk;

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use loq_core::config::{compile_config, CompiledConfig, ConfigOrigin, LoqConfig};
use loq_core::decide::{decide, Decision};
use loq_core::report::{FileOutcome, OutcomeKind};
use rayon::prelude::*;
use thiserror::Error;

/// Filesystem operation errors.
#[derive(Debug, Error)]
pub enum FsError {
    /// Configuration parsing or compilation error.
    #[error("{0}")]
    Config(#[from] loq_core::config::ConfigError),
    /// General I/O error.
    #[error("{0}")]
    Io(std::io::Error),
    /// Failed to read a config file.
    #[error("failed to read config '{}': {}", path.display(), error)]
    ConfigRead {
        /// Path to the config file.
        path: PathBuf,
        /// The underlying I/O error.
        error: std::io::Error,
    },
    /// Gitignore parsing error.
    #[error("{0}")]
    Gitignore(String),
}

/// Options for running a check.
pub struct CheckOptions {
    /// Explicit config file path (overrides discovery).
    pub config_path: Option<PathBuf>,
    /// Current working directory for relative paths.
    pub cwd: PathBuf,
}

/// Output from a check run.
pub struct CheckOutput {
    /// Results for each file checked.
    pub outcomes: Vec<FileOutcome>,
    /// Errors encountered during directory walking.
    pub walk_errors: Vec<walk::WalkError>,
}

fn load_config_from_path(path: PathBuf, fallback_cwd: &Path) -> Result<CompiledConfig, FsError> {
    let root_dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| fallback_cwd.to_path_buf());
    let text = std::fs::read_to_string(&path).map_err(|error| FsError::ConfigRead {
        path: path.clone(),
        error,
    })?;
    let config = loq_core::parse_config(&path, &text)?;
    let compiled = compile_config(
        ConfigOrigin::File(path.clone()),
        root_dir,
        config,
        Some(&path),
    )?;
    Ok(compiled)
}

/// Runs a check on the given paths.
///
/// Expands directories, discovers configs, and checks all files in parallel.
/// Files are grouped by their applicable config for efficient processing.
pub fn run_check(paths: Vec<PathBuf>, options: CheckOptions) -> Result<CheckOutput, FsError> {
    let walk_options = walk::WalkOptions {
        respect_gitignore: false,
    };
    let walk_result = walk::expand_paths(&paths, &walk_options);
    let mut file_list = walk_result.paths;
    let walk_errors = walk_result.errors;
    file_list.sort();
    file_list.dedup();

    let root_gitignore = load_gitignore(&options.cwd)?;
    let mut outcomes = Vec::new();

    if let Some(config_path) = options.config_path {
        let compiled = load_config_from_path(config_path, &options.cwd)?;
        let group_outcomes =
            check_group(&file_list, &compiled, &options.cwd, root_gitignore.as_ref());
        outcomes.extend(group_outcomes);
        return Ok(CheckOutput {
            outcomes,
            walk_errors,
        });
    }

    let mut discovery = discover::ConfigDiscovery::new();
    let mut groups: std::collections::HashMap<Option<PathBuf>, Vec<PathBuf>> =
        std::collections::HashMap::new();

    for path in &file_list {
        let config_path = discover::find_config(path, &mut discovery)?;
        groups.entry(config_path).or_default().push(path.clone());
    }

    for (config_path, group_paths) in groups {
        let compiled = match config_path {
            Some(path) => load_config_from_path(path, &options.cwd)?,
            None => {
                let config = LoqConfig::built_in_defaults();
                compile_config(ConfigOrigin::BuiltIn, options.cwd.clone(), config, None)?
            }
        };

        let group_outcomes = check_group(
            &group_paths,
            &compiled,
            &options.cwd,
            root_gitignore.as_ref(),
        );
        outcomes.extend(group_outcomes);
    }

    Ok(CheckOutput {
        outcomes,
        walk_errors,
    })
}

fn check_group(
    paths: &[PathBuf],
    compiled: &loq_core::config::CompiledConfig,
    cwd: &Path,
    gitignore: Option<&Gitignore>,
) -> Vec<FileOutcome> {
    paths
        .par_iter()
        .map(|path| check_file(path, compiled, cwd, gitignore))
        .collect()
}

fn check_file(
    path: &Path,
    compiled: &loq_core::config::CompiledConfig,
    cwd: &Path,
    gitignore: Option<&Gitignore>,
) -> FileOutcome {
    let display_path = pathdiff::diff_paths(path, cwd)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let config_source = compiled.origin.clone();

    if compiled.respect_gitignore {
        if let Some(gitignore) = gitignore {
            if is_gitignored(gitignore, path, cwd) {
                return FileOutcome {
                    path: path.to_path_buf(),
                    display_path,
                    config_source,
                    kind: OutcomeKind::Excluded {
                        pattern: ".gitignore".to_string(),
                    },
                };
            }
        }
    }

    let relative =
        pathdiff::diff_paths(path, &compiled.root_dir).unwrap_or_else(|| path.to_path_buf());
    let relative_str = normalize_path(&relative);

    let decision = decide(compiled, &relative_str);

    let kind = match &decision {
        Decision::Excluded { pattern } => OutcomeKind::Excluded {
            pattern: pattern.clone(),
        },
        Decision::Exempt { pattern } => OutcomeKind::Exempt {
            pattern: pattern.clone(),
        },
        Decision::SkipNoLimit => OutcomeKind::NoLimit,
        Decision::Check {
            limit,
            severity,
            matched_by,
        } => match count::inspect_file(path) {
            Ok(count::FileInspection::Binary) => OutcomeKind::Binary,
            Ok(count::FileInspection::Text { lines }) => {
                if lines > *limit {
                    OutcomeKind::Violation {
                        limit: *limit,
                        actual: lines,
                        severity: *severity,
                        matched_by: matched_by.clone(),
                    }
                } else {
                    OutcomeKind::Pass {
                        limit: *limit,
                        actual: lines,
                        severity: *severity,
                        matched_by: matched_by.clone(),
                    }
                }
            }
            Err(count::CountError::Missing) => OutcomeKind::Missing,
            Err(count::CountError::Unreadable(error)) => OutcomeKind::Unreadable {
                error: error.to_string(),
            },
        },
    };

    FileOutcome {
        path: path.to_path_buf(),
        display_path,
        config_source,
        kind,
    }
}

#[cfg(windows)]
fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(not(windows))]
fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn load_gitignore(root: &Path) -> Result<Option<Gitignore>, FsError> {
    let path = root.join(".gitignore");
    if !path.is_file() {
        return Ok(None);
    }
    let mut builder = GitignoreBuilder::new(root);
    builder.add(path);
    let gitignore = builder
        .build()
        .map_err(|err| FsError::Gitignore(err.to_string()))?;
    Ok(Some(gitignore))
}

fn is_gitignored(gitignore: &Gitignore, path: &Path, root: &Path) -> bool {
    let relative = pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf());
    // We know path is a file (from walker), so pass false instead of calling is_dir()
    let matched = gitignore.matched_path_or_any_parents(&relative, false);
    matched.is_ignore() && !matched.is_whitelist()
}

#[cfg(test)]
mod tests;
