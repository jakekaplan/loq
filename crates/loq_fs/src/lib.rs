//! Filesystem operations for loq.
//!
//! This crate handles file discovery, walking directories, counting lines,
//! and orchestrating checks across multiple files with parallel processing.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cache;
pub mod count;
pub mod discover;
pub mod path_identity;
pub mod stdin;
pub mod walk;

pub use path_identity::PathIdentity;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    /// Guidance text to show when violations exist.
    pub fix_guidance: Option<String>,
}

fn load_config_from_path(path: &Path, fallback_cwd: &Path) -> Result<CompiledConfig, FsError> {
    let root_dir = path
        .parent()
        .map_or_else(|| fallback_cwd.to_path_buf(), Path::to_path_buf);
    // Canonicalize root_dir so pathdiff works correctly with canonicalized file paths.
    // On Windows, canonicalize returns extended-length paths (\\?\C:\...).
    let root_dir = root_dir.canonicalize().unwrap_or(root_dir);
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
/// Loads a single config (explicit path, cwd discovery, or built-in defaults),
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
        let root_dir = options
            .cwd
            .canonicalize()
            .unwrap_or_else(|_| options.cwd.clone());
        compile_config(ConfigOrigin::BuiltIn, root_dir, config, None)?
    };

    // Step 2: Load cache (if enabled) - cache lives at config root
    let config_hash = cache::hash_config(&compiled);
    let file_cache = if options.use_cache {
        cache::Cache::load(&compiled.root_dir, config_hash)
    } else {
        cache::Cache::empty()
    };
    let file_cache = Mutex::new(file_cache);

    // Step 3: Canonicalize cwd once (instead of per-file)
    let cwd_abs = options
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| options.cwd.clone());

    // Step 4: Walk paths, filtering through gitignore + exclude patterns
    let walk_options = walk::WalkOptions {
        respect_gitignore: compiled.respect_gitignore,
        exclude: compiled.exclude_patterns(),
        cwd: &cwd_abs,
        root_dir: &compiled.root_dir,
    };
    let walk_result = walk::expand_paths(&paths, &walk_options);
    let mut file_list = walk_result.paths;
    let walk_errors = walk_result.errors;
    file_list.sort();
    file_list.dedup();

    // Step 5: Check all files in parallel
    let outcomes = check_group(&file_list, &compiled, &cwd_abs, &file_cache);

    // Step 6: Save cache (if enabled) - cache lives at config root
    if options.use_cache {
        if let Ok(cache) = file_cache.into_inner() {
            cache.save(&compiled.root_dir);
        }
    }

    Ok(CheckOutput {
        outcomes,
        walk_errors,
        fix_guidance: compiled.fix_guidance,
    })
}

fn check_group(
    paths: &[PathBuf],
    compiled: &CompiledConfig,
    cwd_abs: &Path,
    file_cache: &Mutex<cache::Cache>,
) -> Vec<FileOutcome> {
    paths
        .par_iter()
        .map(|path| check_file(path, compiled, cwd_abs, file_cache))
        .collect()
}

fn check_file(
    path: &Path,
    compiled: &CompiledConfig,
    cwd_abs: &Path,
    file_cache: &Mutex<cache::Cache>,
) -> FileOutcome {
    let identity = PathIdentity::new(path, cwd_abs, &compiled.root_dir);
    let config_source = compiled.origin.clone();

    let make_outcome = |kind| FileOutcome {
        path: identity.absolute.clone(),
        display_path: identity.display.clone(),
        match_key: identity.match_key.clone(),
        config_source: config_source.clone(),
        kind,
    };

    let kind = match decide(compiled, &identity.match_key) {
        Decision::SkipNoLimit => OutcomeKind::NoLimit,
        Decision::Check { limit, matched_by } => {
            check_file_lines(path, &identity.cache_key, limit, matched_by, file_cache)
        }
    };

    make_outcome(kind)
}

fn check_file_lines(
    path: &Path,
    cache_key: &str,
    limit: usize,
    matched_by: loq_core::MatchBy,
    file_cache: &Mutex<cache::Cache>,
) -> OutcomeKind {
    // Get file mtime for cache lookup
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    // Try cache first (using relative path as key for consistency across directories)
    if let Some(outcome) = try_cached_outcome(cache_key, mtime, file_cache, limit, &matched_by) {
        return outcome;
    }

    // Cache miss - read file
    match count::inspect_file(path) {
        Ok(count::FileInspection::Binary) => {
            cache_result(file_cache, cache_key, mtime, cache::CachedResult::Binary);
            OutcomeKind::Binary
        }
        Ok(count::FileInspection::Text { lines }) => {
            cache_result(
                file_cache,
                cache_key,
                mtime,
                cache::CachedResult::Text(lines),
            );
            outcome_for_lines(lines, limit, matched_by)
        }
        // Missing/Unreadable can't be cached (no mtime available)
        Err(count::CountError::Missing) => OutcomeKind::Missing,
        Err(count::CountError::Unreadable(error)) => OutcomeKind::Unreadable {
            error: error.to_string(),
        },
    }
}

fn try_cached_outcome(
    cache_key: &str,
    mtime: Option<std::time::SystemTime>,
    file_cache: &Mutex<cache::Cache>,
    limit: usize,
    matched_by: &loq_core::MatchBy,
) -> Option<OutcomeKind> {
    if let Some(mt) = mtime {
        if let Ok(cache) = file_cache.lock() {
            if let Some(result) = cache.get(cache_key, mt) {
                return Some(cached_result_to_outcome(result, limit, matched_by.clone()));
            }
        }
    }
    None
}

fn cache_result(
    file_cache: &Mutex<cache::Cache>,
    cache_key: &str,
    mtime: Option<std::time::SystemTime>,
    result: cache::CachedResult,
) {
    if let Some(mt) = mtime {
        if let Ok(mut cache) = file_cache.lock() {
            cache.insert(cache_key.to_string(), mt, result);
        }
    }
}

fn cached_result_to_outcome(
    result: cache::CachedResult,
    limit: usize,
    matched_by: loq_core::MatchBy,
) -> OutcomeKind {
    match result {
        cache::CachedResult::Text(lines) => outcome_for_lines(lines, limit, matched_by),
        cache::CachedResult::Binary => OutcomeKind::Binary,
    }
}

const fn outcome_for_lines(
    lines: usize,
    limit: usize,
    matched_by: loq_core::MatchBy,
) -> OutcomeKind {
    if lines > limit {
        OutcomeKind::Violation {
            limit,
            actual: lines,
            matched_by,
        }
    } else {
        OutcomeKind::Pass {
            limit,
            actual: lines,
            matched_by,
        }
    }
}

#[cfg(test)]
mod tests;
