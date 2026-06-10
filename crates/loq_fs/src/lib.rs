//! Filesystem operations for loq.
//!
//! This crate handles file discovery, walking directories, counting lines,
//! and orchestrating checks across multiple files with parallel processing.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cache;
pub mod count;
pub mod discover;
mod inspection;
pub mod path_identity;
pub mod stdin;
pub mod walk;

pub use path_identity::PathIdentity;

use std::path::{Path, PathBuf};

use loq_core::config::{compile_config, CompiledConfig, ConfigOrigin, LoqConfig};
use loq_core::decide::{decide, Decision};
use loq_core::report::{FileOutcome, OutcomeKind};
use rayon::prelude::*;

use inspection::Inspector;
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

    // The cache lives at the config root.
    let config_hash = cache::hash_config(&compiled);
    let file_cache = if options.use_cache {
        cache::Cache::load(&compiled.root_dir, config_hash)
    } else {
        cache::Cache::empty()
    };
    let inspector = Inspector::new(file_cache);

    // Canonicalize once here rather than per file.
    let cwd_abs = options
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| options.cwd.clone());

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

    let outcomes = check_group(&file_list, &compiled, &cwd_abs, &inspector);

    if options.use_cache {
        if let Some(cache) = inspector.into_cache() {
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
    inspector: &Inspector,
) -> Vec<FileOutcome> {
    paths
        .par_iter()
        .map(|path| check_file(path, compiled, cwd_abs, inspector))
        .collect()
}

fn check_file(
    path: &Path,
    compiled: &CompiledConfig,
    cwd_abs: &Path,
    inspector: &Inspector,
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
            inspector.inspect(path, &identity.cache_key, limit, matched_by)
        }
    };

    make_outcome(kind)
}

#[cfg(test)]
mod tests;
