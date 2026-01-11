#![forbid(unsafe_code)]

pub mod count;
pub mod discover;
pub mod stdin;
pub mod walk;

use std::path::{Path, PathBuf};

use fence_core::config::{compile_config, CompiledConfig, ConfigOrigin, FenceConfig};
use fence_core::decide::{decide, Decision};
use fence_core::report::{FileOutcome, OutcomeKind};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rayon::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("{0}")]
    Config(#[from] fence_core::config::ConfigError),
    #[error("{0}")]
    Io(std::io::Error),
    #[error("{0}")]
    Gitignore(String),
}

pub struct CheckOptions {
    pub config_path: Option<PathBuf>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct VerboseInfo {
    pub display_path: String,
    pub config_source: ConfigOrigin,
    pub decision: Decision,
}

pub struct CheckOutput {
    pub outcomes: Vec<FileOutcome>,
    pub verbose: Vec<VerboseInfo>,
}

fn load_config_from_path(path: PathBuf, fallback_cwd: &Path) -> Result<CompiledConfig, FsError> {
    let config_path = path.canonicalize().unwrap_or(path);
    let root_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| fallback_cwd.to_path_buf());
    let text = std::fs::read_to_string(&config_path).map_err(FsError::Io)?;
    let config = fence_core::config::parse_config(&config_path, &text)?;
    let compiled = compile_config(
        ConfigOrigin::File(config_path.clone()),
        root_dir,
        config,
        Some(&config_path),
    )?;
    Ok(compiled)
}

pub fn run_check(paths: Vec<PathBuf>, options: CheckOptions) -> Result<CheckOutput, FsError> {
    let mut file_list = walk::expand_paths(&paths)?;
    file_list.sort();
    file_list.dedup();

    let root_gitignore = load_gitignore(&options.cwd)?;
    let mut verbose = Vec::new();
    let mut outcomes = Vec::new();

    if let Some(config_path) = options.config_path {
        let compiled = load_config_from_path(config_path, &options.cwd)?;
        let (group_outcomes, group_verbose) =
            check_group(&file_list, &compiled, &options.cwd, root_gitignore.as_ref());
        outcomes.extend(group_outcomes);
        verbose.extend(group_verbose);
        return Ok(CheckOutput { outcomes, verbose });
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
                let config = FenceConfig::built_in_defaults();
                compile_config(ConfigOrigin::BuiltIn, options.cwd.clone(), config, None)?
            }
        };

        let (group_outcomes, group_verbose) = check_group(
            &group_paths,
            &compiled,
            &options.cwd,
            root_gitignore.as_ref(),
        );
        outcomes.extend(group_outcomes);
        verbose.extend(group_verbose);
    }

    Ok(CheckOutput { outcomes, verbose })
}

fn check_group(
    paths: &[PathBuf],
    compiled: &fence_core::config::CompiledConfig,
    cwd: &Path,
    gitignore: Option<&Gitignore>,
) -> (Vec<FileOutcome>, Vec<VerboseInfo>) {
    let checked: Vec<(FileOutcome, Decision)> = paths
        .par_iter()
        .map(|path| check_file(path, compiled, cwd, gitignore))
        .collect();
    let mut outcomes = Vec::new();
    let mut verbose = Vec::new();
    for (outcome, decision) in checked {
        verbose.push(VerboseInfo {
            display_path: outcome.display_path.clone(),
            config_source: compiled.origin.clone(),
            decision,
        });
        outcomes.push(outcome);
    }
    (outcomes, verbose)
}

fn check_file(
    path: &Path,
    compiled: &fence_core::config::CompiledConfig,
    cwd: &Path,
    gitignore: Option<&Gitignore>,
) -> (FileOutcome, Decision) {
    let display_path = pathdiff::diff_paths(path, cwd)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    if compiled.respect_gitignore {
        if let Some(gitignore) = gitignore {
            if is_gitignored(gitignore, path, cwd) {
                let pattern = ".gitignore".to_string();
                return (
                    FileOutcome {
                        path: path.to_path_buf(),
                        display_path,
                        kind: OutcomeKind::Excluded {
                            pattern: pattern.clone(),
                        },
                    },
                    Decision::Excluded { pattern },
                );
            }
        }
    }

    let relative =
        pathdiff::diff_paths(path, &compiled.root_dir).unwrap_or_else(|| path.to_path_buf());
    let relative_str = normalize_path(&relative);

    let decision = decide(compiled, &relative_str);

    let outcome = match &decision {
        Decision::Excluded { pattern } => FileOutcome {
            path: path.to_path_buf(),
            display_path,
            kind: OutcomeKind::Excluded {
                pattern: pattern.clone(),
            },
        },
        Decision::Exempt { pattern } => FileOutcome {
            path: path.to_path_buf(),
            display_path,
            kind: OutcomeKind::Exempt {
                pattern: pattern.clone(),
            },
        },
        Decision::SkipNoLimit => FileOutcome {
            path: path.to_path_buf(),
            display_path,
            kind: OutcomeKind::NoLimit,
        },
        Decision::Check {
            limit,
            severity,
            matched_by,
        } => match count::inspect_file(path) {
            Ok(count::FileInspection::Binary) => FileOutcome {
                path: path.to_path_buf(),
                display_path,
                kind: OutcomeKind::Binary,
            },
            Ok(count::FileInspection::Text { lines }) => {
                if lines > *limit {
                    FileOutcome {
                        path: path.to_path_buf(),
                        display_path,
                        kind: OutcomeKind::Violation {
                            limit: *limit,
                            actual: lines,
                            severity: *severity,
                            matched_by: matched_by.clone(),
                        },
                    }
                } else {
                    FileOutcome {
                        path: path.to_path_buf(),
                        display_path,
                        kind: OutcomeKind::Pass {
                            limit: *limit,
                            actual: lines,
                            severity: *severity,
                            matched_by: matched_by.clone(),
                        },
                    }
                }
            }
            Err(count::CountError::Missing) => FileOutcome {
                path: path.to_path_buf(),
                display_path,
                kind: OutcomeKind::Missing,
            },
            Err(count::CountError::Unreadable(error)) => FileOutcome {
                path: path.to_path_buf(),
                display_path,
                kind: OutcomeKind::Unreadable {
                    error: error.to_string(),
                },
            },
        },
    };
    (outcome, decision)
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
            ".fence.toml",
            "default_max_lines = 1\nexclude = [\"**/*.txt\"]\n",
        );
        let file = write_file(&temp, "a.txt", "a\nb\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join(".fence.toml")),
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
            ".fence.toml",
            "default_max_lines = 1\nexempt = [\"a.txt\"]\n",
        );
        let file = write_file(&temp, "a.txt", "a\nb\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join(".fence.toml")),
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
        write_file(&temp, ".fence.toml", "exempt = []\n");
        let file = write_file(&temp, "a.txt", "a\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join(".fence.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(output.outcomes[0].kind, OutcomeKind::NoLimit));
    }

    #[test]
    fn missing_files_reported() {
        let temp = TempDir::new().unwrap();
        write_file(&temp, ".fence.toml", "default_max_lines = 1\nexempt = []\n");
        let missing = temp.path().join("missing.txt");

        let output = run_check(
            vec![missing],
            CheckOptions {
                config_path: Some(temp.path().join(".fence.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(output.outcomes[0].kind, OutcomeKind::Missing));
    }

    #[test]
    fn binary_and_unreadable_are_reported() {
        let temp = TempDir::new().unwrap();
        let config = fence_core::config::FenceConfig {
            default_max_lines: Some(1),
            respect_gitignore: true,
            exclude: vec![],
            exempt: vec![],
            rules: vec![],
        };
        let compiled = fence_core::config::compile_config(
            fence_core::config::ConfigOrigin::BuiltIn,
            temp.path().to_path_buf(),
            config,
            None,
        )
        .unwrap();

        let binary = temp.path().join("binary.txt");
        std::fs::write(&binary, b"\0binary").unwrap();
        let (binary_outcome, _) = check_file(&binary, &compiled, temp.path(), None);
        assert!(matches!(binary_outcome.kind, OutcomeKind::Binary));

        let (dir_outcome, _) = check_file(temp.path(), &compiled, temp.path(), None);
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
            ".fence.toml",
            "default_max_lines = 10\nrespect_gitignore = false\n",
        );
        let file = write_file(&temp, "ignored.txt", "a\n");

        let output = run_check(
            vec![file],
            CheckOptions {
                config_path: Some(temp.path().join(".fence.toml")),
                cwd: temp.path().to_path_buf(),
            },
        )
        .unwrap();

        assert!(matches!(output.outcomes[0].kind, OutcomeKind::Pass { .. }));
    }
}
