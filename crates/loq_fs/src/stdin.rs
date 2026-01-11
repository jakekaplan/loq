//! Reading file paths from stdin.
//!
//! Parses newline-delimited file paths, resolving relative paths
//! against the current working directory.

use std::io::{Read, Result as IoResult};
use std::path::{Path, PathBuf};

/// Reads file paths from a reader (typically stdin).
///
/// Paths are separated by newlines. Relative paths are resolved against `cwd`.
/// Empty lines are skipped.
pub fn read_paths(reader: &mut dyn Read, cwd: &Path) -> IoResult<Vec<PathBuf>> {
    let mut input = String::new();
    reader.read_to_string(&mut input)?;
    let mut paths = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = PathBuf::from(trimmed);
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        paths.push(path);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_stdin_list() {
        let input = b"src/a.rs\n\n./b.rs\n";
        let cwd = Path::new("/repo");
        let mut reader: &[u8] = input;
        let paths = read_paths(&mut reader, cwd).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/repo/src/a.rs"));
        assert_eq!(paths[1], PathBuf::from("/repo/./b.rs"));
    }

    #[test]
    fn absolute_paths_preserved() {
        let input = b"/absolute/path.rs\nrelative.rs\n";
        let cwd = Path::new("/repo");
        let mut reader: &[u8] = input;
        let paths = read_paths(&mut reader, cwd).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/absolute/path.rs"));
        assert_eq!(paths[1], PathBuf::from("/repo/relative.rs"));
    }
}
