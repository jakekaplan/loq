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

    // Step 3: Canonicalize cwd once (instead of per-file)
    let cwd_abs = options
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| options.cwd.clone());

    // Step 4: Walk paths, filtering through gitignore + exclude patterns
    let walk_options = walk::WalkOptions {
        respect_gitignore: compiled.respect_gitignore,
        exclude: compiled.exclude_patterns(),
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
        file_cache.save(&compiled.root_dir);
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
    file_cache: &cache::Cache,
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
    file_cache: &cache::Cache,
) -> FileOutcome {
    // Canonicalize to get absolute path for consistent matching.
    // Falls back to joining with cwd for non-existent files.
    let abs_path = path.canonicalize().unwrap_or_else(|_| cwd_abs.join(path));

    let display_path = normalize_path(
        &pathdiff::diff_paths(&abs_path, cwd_abs).unwrap_or_else(|| abs_path.clone()),
    );
    let config_source = compiled.origin.clone();

    let make_outcome = |kind| FileOutcome {
        path: abs_path.clone(),
        display_path: display_path.clone(),
        config_source: config_source.clone(),
        kind,
    };

    let relative_str = relative_path_str(&abs_path, &compiled.root_dir);

    let kind = match decide(compiled, &relative_str) {
        Decision::SkipNoLimit => OutcomeKind::NoLimit,
        Decision::Check { limit, matched_by } => {
            check_file_lines(path, &relative_str, limit, matched_by, file_cache)
        }
    };

    make_outcome(kind)
}

fn check_file_lines(
    path: &Path,
    cache_key: &str,
    limit: usize,
    matched_by: loq_core::MatchBy,
    file_cache: &cache::Cache,
) -> OutcomeKind {
    // Get file mtime for cache lookup
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    // Try cache first (using relative path as key for consistency across directories)
    if let Some(mt) = mtime {
        if let Some(result) = file_cache.get(cache_key, mt) {
            return cached_result_to_outcome(result, limit, matched_by);
        }
    }

    // Cache miss - read file
    match count::inspect_file(path) {
        Ok(count::FileInspection::Binary) => {
            if let Some(mt) = mtime {
                file_cache.insert(cache_key.to_string(), mt, cache::CachedResult::Binary);
            }
            OutcomeKind::Binary
        }
        Ok(count::FileInspection::Text { lines }) => {
            if let Some(mt) = mtime {
                file_cache.insert(cache_key.to_string(), mt, cache::CachedResult::Text(lines));
            }
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
        // Missing/Unreadable can't be cached (no mtime available)
        Err(count::CountError::Missing) => OutcomeKind::Missing,
        Err(count::CountError::Unreadable(error)) => OutcomeKind::Unreadable {
            error: error.to_string(),
        },
    }
}

fn cached_result_to_outcome(
    result: cache::CachedResult,
    limit: usize,
    matched_by: loq_core::MatchBy,
) -> OutcomeKind {
    match result {
        cache::CachedResult::Text(lines) => {
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
        cache::CachedResult::Binary => OutcomeKind::Binary,
    }
}

/// Computes a path relative to root, normalized to forward slashes.
///
/// Falls back to the original path if it cannot be made relative.
pub(crate) fn relative_path_str(path: &Path, root: &Path) -> String {
    let relative = {
        #[cfg(windows)]
        {
            relative_path_windows(path, root).unwrap_or_else(|| path.to_path_buf())
        }
        #[cfg(not(windows))]
        {
            pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf())
        }
    };
    normalize_path(&relative)
}

#[cfg(windows)]
fn relative_path_windows(path: &Path, root: &Path) -> Option<PathBuf> {
    if let (Ok(path), Ok(root)) = (path.canonicalize(), root.canonicalize()) {
        let path = strip_verbatim_prefix(&path);
        let root = strip_verbatim_prefix(&root);
        if let Ok(relative) = path.strip_prefix(&root) {
            return Some(relative.to_path_buf());
        }
        if let Some(relative) = pathdiff::diff_paths(&path, &root) {
            return Some(relative);
        }
    }

    let stripped_path = strip_verbatim_prefix(path);
    let stripped_root = strip_verbatim_prefix(root);
    if let Ok(relative) = stripped_path.strip_prefix(&stripped_root) {
        return Some(relative.to_path_buf());
    }

    pathdiff::diff_paths(&stripped_path, &stripped_root)
}

/// Strip Windows verbatim prefixes (\\?\ / \\?\UNC\) for consistent diffing.
#[cfg(windows)]
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        let mut out = String::from(r"\\");
        out.push_str(rest);
        PathBuf::from(out)
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// Normalizes a path for pattern matching.
///
/// - Converts backslashes to forward slashes (Windows)
/// - Strips leading `./` prefix (walker returns `./foo` when started from `.`)
#[cfg(windows)]
fn normalize_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

/// Normalizes a path for pattern matching.
///
/// Strips leading `./` prefix (walker returns `./foo` when started from `.`).
#[cfg(not(windows))]
fn normalize_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

#[cfg(test)]
mod tests;
