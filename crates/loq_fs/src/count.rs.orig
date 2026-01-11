//! Line counting with binary detection.
//!
//! Efficiently counts lines in files using buffered reads and SIMD-accelerated
//! newline detection. Detects binary files by checking for null bytes.

use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::Path;

use memchr::{memchr, memchr_iter};
use thiserror::Error;

/// Result of inspecting a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileInspection {
    /// File appears to be binary (contains null bytes).
    Binary,
    /// File is text with the given line count.
    Text {
        /// Number of lines (wc -l style: newline-terminated).
        lines: usize,
    },
}

/// Errors that can occur when counting lines.
#[derive(Debug, Error)]
pub enum CountError {
    /// File does not exist.
    #[error("file not found")]
    Missing,
    /// File could not be read.
    #[error("failed to read file: {0}")]
    Unreadable(#[from] std::io::Error),
}

/// Inspects a file to determine if it's binary or count its lines.
///
/// Uses buffered reading for efficiency and checks for null bytes
/// in the first chunk to detect binary files.
pub fn inspect_file(path: &Path) -> Result<FileInspection, CountError> {
    let mut file = File::open(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => CountError::Missing,
        _ => CountError::Unreadable(err),
    })?;

    let mut buf = [0u8; 8192];
    let mut read = read_chunk(&mut file, &mut buf).map_err(CountError::Unreadable)?;
    if read == 0 {
        return Ok(FileInspection::Text { lines: 0 });
    }

    if memchr(0, &buf[..read]).is_some() {
        return Ok(FileInspection::Binary);
    }

    let mut newlines = memchr_iter(b'\n', &buf[..read]).count();
    let mut last_byte = buf[read - 1];

    loop {
        read = read_chunk(&mut file, &mut buf).map_err(CountError::Unreadable)?;
        if read == 0 {
            break;
        }
        newlines += memchr_iter(b'\n', &buf[..read]).count();
        last_byte = buf[read - 1];
    }

    let mut lines = newlines;
    if last_byte != b'\n' {
        lines += 1;
    }

    Ok(FileInspection::Text { lines })
}

fn read_chunk(file: &mut File, buf: &mut [u8]) -> IoResult<usize> {
    file.read(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn write_temp(contents: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents).unwrap();
        file
    }

    use std::io::Write;

    #[test]
    fn count_empty_file() {
        let file = write_temp(b"");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 0 });
    }

    #[test]
    fn count_trailing_newline() {
        let file = write_temp(b"a\n");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 1 });
    }

    #[test]
    fn count_no_trailing_newline() {
        let file = write_temp(b"a");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 1 });
    }

    #[test]
    fn count_multiple_lines() {
        let file = write_temp(b"a\nb\n");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 2 });
    }

    #[test]
    fn count_multiple_lines_no_trailing_newline() {
        let file = write_temp(b"a\nb");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 2 });
    }

    #[test]
    fn binary_detection_first_chunk() {
        let file = write_temp(b"\0binary");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Binary);
    }

    #[test]
    fn missing_file_returns_missing() {
        let path = std::path::Path::new("does-not-exist.txt");
        let err = inspect_file(path).unwrap_err();
        assert!(matches!(err, CountError::Missing));
    }

    #[test]
    fn unreadable_path_returns_unreadable() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = inspect_file(dir.path()).unwrap_err();
        assert!(matches!(err, CountError::Unreadable(_)));
    }

    #[test]
    fn count_large_file_multiple_chunks() {
        // Buffer size is 8192 bytes, so create file larger than that
        let mut content = Vec::new();
        for i in 0..1000 {
            content.extend_from_slice(format!("line number {i}\n").as_bytes());
        }
        let file = write_temp(&content);
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 1000 });
    }

    #[test]
    fn count_large_file_no_trailing_newline() {
        // Test multi-chunk reading where last byte isn't newline
        let mut content = Vec::new();
        for i in 0..999 {
            content.extend_from_slice(format!("line number {i}\n").as_bytes());
        }
        content.extend_from_slice(b"final line without newline");
        let file = write_temp(&content);
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 1000 });
    }

    #[test]
    fn binary_detection_only_checks_first_chunk() {
        // Current behavior: null bytes are only detected in the first 8KB chunk.
        // A file with null bytes AFTER the first chunk is treated as text.
        // This is a deliberate performance trade-off.
        let mut content = vec![b'a'; 8192]; // Fill first chunk with 'a'
        content.push(0); // Null byte in second chunk
        content.push(b'\n');
        let file = write_temp(&content);
        let result = inspect_file(file.path()).unwrap();
        // This returns Text, not Binary - the null byte in chunk 2 is not detected
        assert_eq!(result, FileInspection::Text { lines: 1 });
    }

    #[test]
    fn crlf_line_endings_counted_by_lf() {
        // Windows-style CRLF (\r\n) - we count \n only, so this is 3 lines
        let file = write_temp(b"line1\r\nline2\r\nline3\r\n");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 3 });
    }

    #[test]
    fn mixed_line_endings() {
        // Mix of \n and \r\n - we only count \n
        let file = write_temp(b"unix\nwindows\r\nmore unix\n");
        let result = inspect_file(file.path()).unwrap();
        assert_eq!(result, FileInspection::Text { lines: 3 });
    }

    #[test]
    fn cr_only_not_counted_as_line() {
        // Old Mac style \r only - NOT counted as line endings
        let file = write_temp(b"line1\rline2\rline3\r");
        let result = inspect_file(file.path()).unwrap();
        // No \n chars, but file doesn't end in \n, so we count 1 line
        assert_eq!(result, FileInspection::Text { lines: 1 });
    }
}
