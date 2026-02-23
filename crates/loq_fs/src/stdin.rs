//! Reading file paths from stdin.
//!
//! Parses newline-delimited file paths, resolving relative paths
//! against the current working directory.
//!
//! Note: Paths must be valid UTF-8. Non-UTF-8 paths will cause an error.
//! This is a practical limitation - most real-world paths are UTF-8.

use std::io::{BufRead, BufReader, Read, Result as IoResult};
use std::path::{Path, PathBuf};

/// Reads file paths from a reader (typically stdin).
///
/// Paths are separated by newlines. Relative paths are resolved against `cwd`.
/// Empty lines are skipped. Leading/trailing spaces are preserved so filenames
/// containing spaces are handled correctly. Uses streaming line-by-line reading
/// to avoid loading the entire input into memory at once.
///
/// # Errors
///
/// Returns an error if reading fails or if the input contains invalid UTF-8.
pub fn read_paths(reader: &mut dyn Read, cwd: &Path) -> IoResult<Vec<PathBuf>> {
    let buf_reader = BufReader::new(reader);
    let mut paths = Vec::new();

    for line_result in buf_reader.lines() {
        let line = line_result?;
        if line.is_empty() {
            continue;
        }

        let path = PathBuf::from(line);
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

    #[test]
    fn preserves_leading_and_trailing_spaces() {
        let input = b" spaced.txt \n";
        let cwd = Path::new("/repo");
        let mut reader: &[u8] = input;
        let paths = read_paths(&mut reader, cwd).unwrap();
        assert_eq!(paths, vec![PathBuf::from("/repo/ spaced.txt ")]);
    }
}
