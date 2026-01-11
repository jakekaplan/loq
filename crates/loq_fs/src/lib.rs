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

fn load_config_from_path(path: &Path, fallback_cwd: &Path) -> Result<CompiledConfig, FsError> {
    let root_dir = path
        .parent()
        .map_or_else(|| fallback_cwd.to_path_buf(), Path::to_path_buf);
    let text = std::fs::read_to_string(path).map_err(|error| FsError::ConfigRead {
        path: path.to_path_buf(),
        error,
    })?;
    let config = loq_core::parse_config(path, &text)?;
    let compiled = compile_config(
        ConfigOrigin::File(path.to_path_buf()),
        root_dir,
        config,
        Some(path),
    )?;
    Ok(compiled)
}

/// Runs a check on the given paths.
///
/// Loads a single config (from --config, cwd discovery, or built-in defaults),
/// then checks all files against that config in parallel.
///
/// Exclusion filtering (gitignore + exclude patterns) happens at the walk layer.
pub fn run_check(paths: Vec<PathBuf>, options: CheckOptions) -> Result<CheckOutput, FsError> {
    // Step 1: Load config (explicit path, discovered, or built-in defaults)
    let compiled = if let Some(ref config_path) = options.config_path {
        load_config_from_path(config_path, &options.cwd)?
    } else if let Some(config_path) = discover::find_config(&options.cwd) {
        load_config_from_path(&config_path, &options.cwd)?
    } else {
        let config = LoqConfig::built_in_defaults();
        compile_config(ConfigOrigin::BuiltIn, options.cwd.clone(), config, None)?
    };

    // Step 2: Load gitignore once (if enabled)
    let gitignore = if compiled.respect_gitignore {
        load_gitignore(&options.cwd)?
    } else {
        None
    };

    // Step 3: Walk paths, filtering through gitignore + exclude patterns
    let walk_options = walk::WalkOptions {
        respect_gitignore: compiled.respect_gitignore,
        gitignore: gitignore.as_ref(),
        exclude: compiled.exclude_patterns(),
        root_dir: &compiled.root_dir,
    };
    let walk_result = walk::expand_paths(&paths, &walk_options);
    let mut file_list = walk_result.paths;
    let walk_errors = walk_result.errors;
    file_list.sort();
    file_list.dedup();

    // Step 4: Check all files in parallel (no more exclusion checks here)
    let outcomes = check_group(&file_list, &compiled, &options.cwd);

    Ok(CheckOutput {
        outcomes,
        walk_errors,
    })
}

fn check_group(paths: &[PathBuf], compiled: &CompiledConfig, cwd: &Path) -> Vec<FileOutcome> {
    paths
        .par_iter()
        .map(|path| check_file(path, compiled, cwd))
        .collect()
}

fn check_file(path: &Path, compiled: &CompiledConfig, cwd: &Path) -> FileOutcome {
    let display_path = pathdiff::diff_paths(path, cwd)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let config_source = compiled.origin.clone();

    let make_outcome = |kind| FileOutcome {
        path: path.to_path_buf(),
        display_path: display_path.clone(),
        config_source: config_source.clone(),
        kind,
    };

    let relative =
        pathdiff::diff_paths(path, &compiled.root_dir).unwrap_or_else(|| path.to_path_buf());
    let relative_str = normalize_path(&relative);

    let kind = match decide(compiled, &relative_str) {
        Decision::SkipNoLimit => OutcomeKind::NoLimit,
        Decision::Check {
            limit,
            severity,
            matched_by,
        } => check_file_lines(path, limit, severity, matched_by),
    };

    make_outcome(kind)
}

fn check_file_lines(
    path: &Path,
    limit: usize,
    severity: loq_core::Severity,
    matched_by: loq_core::MatchBy,
) -> OutcomeKind {
    match count::inspect_file(path) {
        Ok(count::FileInspection::Binary) => OutcomeKind::Binary,
        Ok(count::FileInspection::Text { lines }) if lines > limit => OutcomeKind::Violation {
            limit,
            actual: lines,
            severity,
            matched_by,
        },
        Ok(count::FileInspection::Text { lines }) => OutcomeKind::Pass {
            limit,
            actual: lines,
            severity,
            matched_by,
        },
        Err(count::CountError::Missing) => OutcomeKind::Missing,
        Err(count::CountError::Unreadable(error)) => OutcomeKind::Unreadable {
            error: error.to_string(),
        },
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

#[cfg(test)]
mod tests;
