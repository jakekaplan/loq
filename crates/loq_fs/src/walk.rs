//! Directory walking and file expansion.
//!
//! Expands paths (files and directories) into a list of files to check,
//! filtering out excluded files (gitignore, exclude patterns) at this layer.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ignore::gitignore::Gitignore;
use ignore::WalkBuilder;
use loq_core::PatternList;
use thiserror::Error;

/// Error encountered while walking a directory.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct WalkError(pub String);

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
    /// Pre-loaded gitignore matcher (for filtering explicit paths).
    pub gitignore: Option<&'a Gitignore>,
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
/// All exclusion filtering (gitignore + exclude patterns) happens here.
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
                // Explicit file path - filter through gitignore + exclude
                if !is_excluded(path, options) {
                    files.push(path.clone());
                }
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

/// Checks if a path should be excluded (gitignore or exclude pattern).
fn is_excluded(path: &Path, options: &WalkOptions) -> bool {
    // Check gitignore
    if let Some(gitignore) = options.gitignore {
        let relative =
            pathdiff::diff_paths(path, options.root_dir).unwrap_or_else(|| path.to_path_buf());
        let matched = gitignore.matched_path_or_any_parents(&relative, false);
        if matched.is_ignore() && !matched.is_whitelist() {
            return true;
        }
    }

    // Check exclude patterns
    let relative =
        pathdiff::diff_paths(path, options.root_dir).unwrap_or_else(|| path.to_path_buf());
    let relative_str = normalize_path(&relative);
    options.exclude.matches(&relative_str).is_some()
}

#[cfg(windows)]
fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(not(windows))]
fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
                    if e.file_type().is_some_and(|t| t.is_file()) {
                        let _ = path_tx.send(e.into_path());
                    }
                }
                Err(e) => {
                    let _ = error_tx.send(WalkError(e.to_string()));
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
            let relative = pathdiff::diff_paths(p, options.root_dir).unwrap_or_else(|| p.clone());
            let relative_str = normalize_path(&relative);
            options.exclude.matches(&relative_str).is_none()
        })
        .collect();

    WalkResult {
        paths,
        errors: error_rx.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::config::{compile_config, ConfigOrigin, LoqConfig};
    use tempfile::TempDir;

    fn empty_exclude() -> loq_core::PatternList {
        let config = LoqConfig {
            exclude: vec![],
            ..LoqConfig::default()
        };
        let compiled =
            compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap();
        compiled.exclude_patterns().clone()
    }

    fn exclude_pattern(pattern: &str) -> loq_core::PatternList {
        let config = LoqConfig {
            exclude: vec![pattern.to_string()],
            ..LoqConfig::default()
        };
        let compiled =
            compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap();
        compiled.exclude_patterns().clone()
    }

    #[test]
    fn expands_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("a.txt"), "a").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.txt"), "b").unwrap();

        let exclude = empty_exclude();
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[root.to_path_buf()], &options);
        assert_eq!(result.paths.len(), 2);
    }

    #[test]
    fn expands_file_and_missing() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let file = root.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let missing = root.join("missing.txt");

        let exclude = empty_exclude();
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[file, missing], &options);
        assert_eq!(result.paths.len(), 2);
        assert!(result.paths.iter().any(|path| path.ends_with("a.txt")));
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("missing.txt")));
    }

    #[test]
    fn respects_gitignore_when_enabled() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/.gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.join("sub/ignored.txt"), "ignored").unwrap();
        std::fs::write(root.join("sub/included.txt"), "included").unwrap();

        let exclude = empty_exclude();
        let options = WalkOptions {
            respect_gitignore: true,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[root.join("sub")], &options);
        // Should have .gitignore and included.txt (ignored.txt is excluded)
        assert_eq!(result.paths.len(), 2);
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("included.txt")));
        assert!(!result
            .paths
            .iter()
            .any(|path| path.ends_with("ignored.txt")));
    }

    #[test]
    fn includes_gitignored_when_disabled() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/.gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.join("sub/ignored.txt"), "ignored").unwrap();
        std::fs::write(root.join("sub/included.txt"), "included").unwrap();

        let exclude = empty_exclude();
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[root.join("sub")], &options);
        // Should have all 3: .gitignore, ignored.txt, included.txt
        assert_eq!(result.paths.len(), 3);
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("ignored.txt")));
    }

    #[test]
    fn exclude_pattern_filters_walked_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("keep.rs"), "keep").unwrap();
        std::fs::write(root.join("skip.txt"), "skip").unwrap();

        let exclude = exclude_pattern("**/*.txt");
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[root.to_path_buf()], &options);
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
        assert!(!result.paths.iter().any(|p| p.ends_with("skip.txt")));
    }

    #[test]
    fn exclude_pattern_filters_explicit_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let keep = root.join("keep.rs");
        let skip = root.join("skip.txt");
        std::fs::write(&keep, "keep").unwrap();
        std::fs::write(&skip, "skip").unwrap();

        let exclude = exclude_pattern("**/*.txt");
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[keep, skip], &options);
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_file_not_followed_by_default() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("real.txt"), "content").unwrap();
        symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

        let exclude = empty_exclude();
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        let result = expand_paths(&[root.to_path_buf()], &options);

        // Real file is included
        assert!(result.paths.iter().any(|p| p.ends_with("real.txt")));
        // Symlink is NOT followed by default (ignore crate behavior)
        assert!(!result.paths.iter().any(|p| p.ends_with("link.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_parent_dir_does_not_loop() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/file.txt"), "content").unwrap();
        // Create symlink pointing back to parent - could cause infinite loop
        symlink(root, root.join("sub/parent_link")).unwrap();

        let exclude = empty_exclude();
        let options = WalkOptions {
            respect_gitignore: false,
            gitignore: None,
            exclude: &exclude,
            root_dir: root,
        };
        // This should complete without hanging (ignore crate doesn't follow dir symlinks)
        let result = expand_paths(&[root.to_path_buf()], &options);

        // Should find the file but not loop infinitely
        assert!(result.paths.iter().any(|p| p.ends_with("file.txt")));
        // The symlink itself is not a file, so it won't appear in paths
    }
}
