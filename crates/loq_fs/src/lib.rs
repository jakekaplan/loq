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
    let config_path = path.canonicalize().unwrap_or(path);
    let root_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| fallback_cwd.to_path_buf());
    let text = std::fs::read_to_string(&config_path).map_err(|error| FsError::ConfigRead {
        path: config_path.clone(),
        error,
    })?;
    let config = loq_core::parse_config(&config_path, &text)?;
    let compiled = compile_config(
        ConfigOrigin::File(config_path.clone()),
        root_dir,
        config,
        Some(&config_path),
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
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let display_path = pathdiff::diff_paths(&canonical_path, cwd)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let config_source = compiled.origin.clone();

    if compiled.respect_gitignore {
        if let Some(gitignore) = gitignore {
            if is_gitignored(gitignore, &canonical_path, cwd) {
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

    let relative = pathdiff::diff_paths(&canonical_path, &compiled.root_dir)
        .unwrap_or_else(|| path.to_path_buf());
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

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
    let matched = gitignore.matched_path_or_any_parents(&relative, path.is_dir());
    matched.is_ignore() && !matched.is_whitelist()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, path: &str, contents: &str) -> PathBuf {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, contents).unwrap();
        full
    }

    #[test]
    fn excluded_files_are_skipped() {
        let temp = TempDir::new().unwrap();
        write_file(
            &temp,
            "loq.toml",
            "default_max_lines = 1\nexclude = [\"**/*.txt\"]\n",
        );
        let file = write_file(&temp, "a.txt", "a\nb\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join("loq.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(
            output.outcomes[0].kind,
            OutcomeKind::Excluded { .. }
        ));
    }

    #[test]
    fn exempt_files_are_skipped() {
        let temp = TempDir::new().unwrap();
        write_file(
            &temp,
            "loq.toml",
            "default_max_lines = 1\nexempt = [\"a.txt\"]\n",
        );
        let file = write_file(&temp, "a.txt", "a\nb\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join("loq.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(
            output.outcomes[0].kind,
            OutcomeKind::Exempt { .. }
        ));
    }

    #[test]
    fn no_default_skips_files() {
        let temp = TempDir::new().unwrap();
        write_file(&temp, "loq.toml", "exempt = []\n");
        let file = write_file(&temp, "a.txt", "a\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join("loq.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(output.outcomes[0].kind, OutcomeKind::NoLimit));
    }

    #[test]
    fn missing_files_reported() {
        let temp = TempDir::new().unwrap();
        write_file(&temp, "loq.toml", "default_max_lines = 1\nexempt = []\n");
        let missing = temp.path().join("missing.txt");

        let output = run_check(
            vec![missing],
            CheckOptions {
                config_path: Some(temp.path().join("loq.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(output.outcomes[0].kind, OutcomeKind::Missing));
    }

    #[test]
    fn binary_and_unreadable_are_reported() {
        let temp = TempDir::new().unwrap();
        let config = loq_core::config::LoqConfig {
            default_max_lines: Some(1),
            respect_gitignore: true,
            exclude: vec![],
            exempt: vec![],
            rules: vec![],
        };
        let compiled = loq_core::config::compile_config(
            loq_core::config::ConfigOrigin::BuiltIn,
            temp.path().to_path_buf(),
            config,
            None,
        )
        .unwrap();

        let binary = temp.path().join("binary.txt");
        std::fs::write(&binary, b"\0binary").unwrap();
        let binary_outcome = check_file(&binary, &compiled, temp.path(), None);
        assert!(matches!(binary_outcome.kind, OutcomeKind::Binary));

        let dir_outcome = check_file(temp.path(), &compiled, temp.path(), None);
        assert!(matches!(dir_outcome.kind, OutcomeKind::Unreadable { .. }));
    }

    #[test]
    fn gitignore_is_respected_by_default() {
        let temp = TempDir::new().unwrap();
        write_file(&temp, ".gitignore", "ignored.txt\n");
        let file = write_file(&temp, "ignored.txt", "a\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: None,
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(
            output.outcomes[0].kind,
            OutcomeKind::Excluded { .. }
        ));
    }

    #[test]
    fn gitignore_can_be_disabled() {
        let temp = TempDir::new().unwrap();
        write_file(&temp, ".gitignore", "ignored.txt\n");
        write_file(
            &temp,
            "loq.toml",
            "default_max_lines = 10\nrespect_gitignore = false\n",
        );
        let file = write_file(&temp, "ignored.txt", "a\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join("loq.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(output.outcomes[0].kind, OutcomeKind::Pass { .. }));
    }
}
