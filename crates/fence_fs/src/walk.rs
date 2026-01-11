use std::path::PathBuf;
use std::sync::mpsc;

use ignore::WalkBuilder;

use crate::FsError;

pub struct WalkOptions {
    pub respect_gitignore: bool,
}

pub fn expand_paths(paths: &[PathBuf], options: &WalkOptions) -> Result<Vec<PathBuf>, FsError> {
    let mut files = Vec::new();

    for path in paths {
        if path.exists() {
            if path.is_dir() {
                let dir_files = walk_directory(path, options)?;
                files.extend(dir_files);
            } else {
                files.push(path.to_path_buf());
            }
        } else {
            files.push(path.to_path_buf());
        }
    }

    Ok(files)
}

fn walk_directory(path: &PathBuf, options: &WalkOptions) -> Result<Vec<PathBuf>, FsError> {
    let (tx, rx) = mpsc::channel();

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
        let tx = tx.clone();
        Box::new(move |entry| {
            if let Ok(e) = entry {
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let _ = tx.send(e.into_path());
                }
            }
            ignore::WalkState::Continue
        })
    });

    drop(tx);
    Ok(rx.into_iter().collect())
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
        let paths = expand_paths(&[root.to_path_buf()], &options).unwrap();
        assert_eq!(paths.len(), 2);
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
        let paths = expand_paths(&[file.clone(), missing.clone()], &options).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|path| path.ends_with("a.txt")));
        assert!(paths.iter().any(|path| path.ends_with("missing.txt")));
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
        let paths = expand_paths(&[root.join("sub")], &options).unwrap();
        // Should have .gitignore and included.txt (ignored.txt is excluded)
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|path| path.ends_with("included.txt")));
        assert!(!paths.iter().any(|path| path.ends_with("ignored.txt")));
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
        let paths = expand_paths(&[root.join("sub")], &options).unwrap();
        // Should have all 3: .gitignore, ignored.txt, included.txt
        assert_eq!(paths.len(), 3);
        assert!(paths.iter().any(|path| path.ends_with("ignored.txt")));
    }
}
