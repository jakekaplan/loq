use std::path::PathBuf;

use walkdir::WalkDir;

use crate::FsError;

pub fn expand_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, FsError> {
    let mut files = Vec::new();
    for path in paths {
        if path.exists() {
            if path.is_dir() {
                for entry in WalkDir::new(path) {
                    let entry = entry.map_err(|err| FsError::Io(std::io::Error::other(err)))?;
                    if entry.file_type().is_file() {
                        files.push(entry.path().to_path_buf());
                    }
                }
            } else {
                files.push(path.to_path_buf());
            }
        } else {
            files.push(path.to_path_buf());
        }
    }

    for file in &mut files {
        if let Ok(canonical) = file.canonicalize() {
            *file = canonical;
        }
    }

    Ok(files)
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

        let paths = expand_paths(&[root.to_path_buf()]).unwrap();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn expands_file_and_missing() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let file = root.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let missing = root.join("missing.txt");

        let paths = expand_paths(&[file.clone(), missing.clone()]).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|path| path.ends_with("a.txt")));
        assert!(paths.iter().any(|path| path.ends_with("missing.txt")));
    }
}
