//! Directory walking and file expansion.
//!
//! Expands paths (files and directories) into a list of files to check.

use std::path::PathBuf;
use std::sync::mpsc;

use ignore::WalkBuilder;
use thiserror::Error;

/// Error encountered while walking a directory.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct WalkError(pub String);

/// Result of expanding paths.
pub struct WalkResult {
    /// All discovered file paths.
    pub paths: Vec<PathBuf>,
    /// Errors encountered during walking.
    pub errors: Vec<WalkError>,
}

/// Options for directory walking.
pub struct WalkOptions {
    /// Whether to respect `.gitignore` files during walking.
    pub respect_gitignore: bool,
}

/// Expands paths into a flat list of files.
///
/// Directories are walked recursively. Non-existent paths are included
/// (to be reported as missing later). Uses parallel walking for performance.
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
                files.push(path.clone());
            }
        } else {
            files.push(path.clone());
        }
    }

    WalkResult {
        paths: files,
        errors,
    }
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

    WalkResult {
        paths: path_rx.into_iter().collect(),
        errors: error_rx.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn expands_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("a.txt"), "a").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.txt"), "b").unwrap();

        let options = WalkOptions {
            respect_gitignore: false,
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

        let options = WalkOptions {
            respect_gitignore: false,
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

        let options = WalkOptions {
            respect_gitignore: true,
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

        let options = WalkOptions {
            respect_gitignore: false,
        };
        let result = expand_paths(&[root.join("sub")], &options);
        // Should have all 3: .gitignore, ignored.txt, included.txt
        assert_eq!(result.paths.len(), 3);
        assert!(result
            .paths
            .iter()
            .any(|path| path.ends_with("ignored.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_file_not_followed_by_default() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("real.txt"), "content").unwrap();
        symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

        let options = WalkOptions {
            respect_gitignore: false,
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

        let options = WalkOptions {
            respect_gitignore: false,
        };
        // This should complete without hanging (ignore crate doesn't follow dir symlinks)
        let result = expand_paths(&[root.to_path_buf()], &options);

        // Should find the file but not loop infinitely
        assert!(result.paths.iter().any(|p| p.ends_with("file.txt")));
        // The symlink itself is not a file, so it won't appear in paths
    }
}
