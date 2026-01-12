//! Filesystem operations for loq.
//!
//! This crate handles file discovery, walking directories, counting lines,
//! and orchestrating checks across multiple files with parallel processing.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cache;
pub mod count;
pub mod discover;
pub mod stdin;
pub mod walk;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    /// Whether to use file caching (default: true).
    pub use_cache: bool,
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

    // Step 2: Load cache (if enabled) - cache lives at config root
    let config_hash = cache::hash_config(&compiled);
    let file_cache = if options.use_cache {
        cache::Cache::load(&compiled.root_dir, config_hash)
    } else {
        cache::Cache::empty()
    };
    let file_cache = Mutex::new(file_cache);

    // Step 3: Load gitignore once (if enabled)
    let gitignore = if compiled.respect_gitignore {
        load_gitignore(&options.cwd)?
    } else {
        None
    };

    // Step 4: Walk paths, filtering through gitignore + exclude patterns
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

    // Step 5: Check all files in parallel
    let outcomes = check_group(&file_list, &compiled, &options.cwd, &file_cache);

    // Step 6: Save cache (if enabled) - cache lives at config root
    if options.use_cache {
        if let Ok(cache) = file_cache.into_inner() {
            cache.save(&compiled.root_dir);
        }
    }

    Ok(CheckOutput {
        outcomes,
        walk_errors,
    })
}

fn check_group(
    paths: &[PathBuf],
    compiled: &CompiledConfig,
    cwd: &Path,
    file_cache: &Mutex<cache::Cache>,
) -> Vec<FileOutcome> {
    paths
        .par_iter()
        .map(|path| check_file(path, compiled, cwd, file_cache))
        .collect()
}

fn check_file(
    path: &Path,
    compiled: &CompiledConfig,
    cwd: &Path,
    file_cache: &Mutex<cache::Cache>,
) -> FileOutcome {
    // Canonicalize to get absolute path for consistent matching.
    // Falls back to joining with cwd for non-existent files.
    let abs_path = path.canonicalize().unwrap_or_else(|_| cwd.join(path));

    let display_path = pathdiff::diff_paths(&abs_path, cwd)
        .unwrap_or_else(|| abs_path.clone())
        .to_string_lossy()
        .to_string();
    let config_source = compiled.origin.clone();

    let make_outcome = |kind| FileOutcome {
        path: abs_path.clone(),
        display_path: display_path.clone(),
        config_source: config_source.clone(),
        kind,
    };

    let relative =
        pathdiff::diff_paths(&abs_path, &compiled.root_dir).unwrap_or_else(|| abs_path.clone());
    let relative_str = normalize_path(&relative);

    let kind = match decide(compiled, &relative_str) {
        Decision::SkipNoLimit => OutcomeKind::NoLimit,
        Decision::Check {
            limit,
            severity,
            matched_by,
        } => check_file_lines(path, &relative_str, limit, severity, matched_by, file_cache),
    };

    make_outcome(kind)
}

fn check_file_lines(
    path: &Path,
    cache_key: &str,
    limit: usize,
    severity: loq_core::Severity,
    matched_by: loq_core::MatchBy,
    file_cache: &Mutex<cache::Cache>,
) -> OutcomeKind {
    // Get file mtime for cache lookup
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    // Try cache first (using relative path as key for consistency across directories)
    if let Some(mt) = mtime {
        if let Ok(cache) = file_cache.lock() {
            if let Some(lines) = cache.get(cache_key, mt) {
                return if lines > limit {
                    OutcomeKind::Violation {
                        limit,
                        actual: lines,
                        severity,
                        matched_by,
                    }
                } else {
                    OutcomeKind::Pass {
                        limit,
                        actual: lines,
                        severity,
                        matched_by,
                    }
                };
            }
        }
    }

    // Cache miss - read file
    match count::inspect_file(path) {
        Ok(count::FileInspection::Binary) => OutcomeKind::Binary,
        Ok(count::FileInspection::Text { lines }) => {
            // Update cache on successful read
            if let Some(mt) = mtime {
                if let Ok(mut cache) = file_cache.lock() {
                    cache.insert(cache_key.to_string(), mt, lines);
                }
            }

            if lines > limit {
                OutcomeKind::Violation {
                    limit,
                    actual: lines,
                    severity,
                    matched_by,
                }
            } else {
                OutcomeKind::Pass {
                    limit,
                    actual: lines,
                    severity,
                    matched_by,
                }
            }
        }
        Err(count::CountError::Missing) => OutcomeKind::Missing,
        Err(count::CountError::Unreadable(error)) => OutcomeKind::Unreadable {
            error: error.to_string(),
        },
    }
}

/// Normalizes a path to use forward slashes on all platforms.
#[cfg(windows)]
pub(crate) fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Normalizes a path to use forward slashes on all platforms.
#[cfg(not(windows))]
pub(crate) fn normalize_path(path: &Path) -> String {
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
