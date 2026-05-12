//! Path identity for checked files.
//!
//! Centralizes the path forms used by loq: absolute filesystem path,
//! cwd-relative display path, config-root-relative match key, and cache key.

use std::path::{Path, PathBuf};

/// The path forms loq needs for one checked file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathIdentity {
    /// Absolute path to the file, or cwd joined with the input path for missing files.
    pub absolute: PathBuf,
    /// Path relative to the working directory for output.
    pub display: String,
    /// Path relative to the config root for rule matching and managed exact-path rules.
    pub match_key: String,
    /// Cache key for file inspection results.
    pub cache_key: String,
}

impl PathIdentity {
    /// Builds path identity from an input path, working directory, and config root.
    ///
    /// `cwd` and `root` should be absolute and canonicalized once by the caller.
    #[must_use]
    pub fn new(path: &Path, cwd: &Path, root: &Path) -> Self {
        let absolute = path.canonicalize().unwrap_or_else(|_| cwd.join(path));
        let display = display_key(&absolute, cwd);
        let match_key = relative_key(&absolute, root);
        let cache_key = match_key.clone();

        Self {
            absolute,
            display,
            match_key,
            cache_key,
        }
    }
}

/// Normalizes a path key used in config-owned paths.
#[must_use]
pub fn normalize_key(path: &str) -> String {
    #[cfg(windows)]
    {
        let path = path.replace('\\', "/");
        path.strip_prefix("./").unwrap_or(&path).to_string()
    }
    #[cfg(not(windows))]
    {
        path.strip_prefix("./").unwrap_or(path).to_string()
    }
}

fn display_key(path: &Path, cwd: &Path) -> String {
    let relative = pathdiff::diff_paths(path, cwd).unwrap_or_else(|| path.to_path_buf());
    normalize_path(&relative)
}

/// Computes a path relative to root, normalized to forward slashes.
///
/// Falls back to the original path if it cannot be made relative.
#[must_use]
fn relative_key(path: &Path, root: &Path) -> String {
    let relative = {
        #[cfg(windows)]
        {
            relative_path_windows(path, root).unwrap_or_else(|| path.to_path_buf())
        }
        #[cfg(not(windows))]
        {
            pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf())
        }
    };
    normalize_path(&relative)
}

#[cfg(windows)]
fn relative_path_windows(path: &Path, root: &Path) -> Option<PathBuf> {
    if let (Ok(path), Ok(root)) = (path.canonicalize(), root.canonicalize()) {
        let path = strip_verbatim_prefix(&path);
        let root = strip_verbatim_prefix(&root);
        if let Ok(relative) = path.strip_prefix(&root) {
            return Some(relative.to_path_buf());
        }
        if let Some(relative) = pathdiff::diff_paths(&path, &root) {
            return Some(relative);
        }
    }

    let stripped_path = strip_verbatim_prefix(path);
    let stripped_root = strip_verbatim_prefix(root);
    if let Ok(relative) = stripped_path.strip_prefix(&stripped_root) {
        return Some(relative.to_path_buf());
    }

    pathdiff::diff_paths(&stripped_path, &stripped_root)
}

/// Strip Windows verbatim prefixes (\\?\ / \\?\UNC\) for consistent diffing.
#[cfg(windows)]
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        let mut out = String::from(r"\\");
        out.push_str(rest);
        PathBuf::from(out)
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn normalize_path(path: &Path) -> String {
    normalize_key(path.to_string_lossy().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn identity_uses_cwd_for_display_and_root_for_match_key() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let cwd = root.join("sub");
        std::fs::create_dir(&cwd).unwrap();
        let file = root.join("src/app.rs");
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let identity = PathIdentity::new(&file, &cwd, &root);

        assert_eq!(identity.display, "../src/app.rs");
        assert_eq!(identity.match_key, "src/app.rs");
        assert_eq!(identity.cache_key, identity.match_key);
        assert_eq!(identity.absolute, file.canonicalize().unwrap());
    }

    #[test]
    fn missing_path_falls_back_to_cwd_join() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let missing = Path::new("missing.rs");

        let identity = PathIdentity::new(missing, &root, &root);

        assert_eq!(identity.absolute, root.join("missing.rs"));
        assert_eq!(identity.display, "missing.rs");
        assert_eq!(identity.match_key, "missing.rs");
    }

    #[test]
    fn normalize_key_strips_leading_dot_slash() {
        assert_eq!(normalize_key("./foo/bar"), "foo/bar");
        assert_eq!(normalize_key("./.git/logs/HEAD"), ".git/logs/HEAD");
        assert_eq!(normalize_key("foo/bar"), "foo/bar");
        assert_eq!(normalize_key("."), ".");
    }

    #[cfg(windows)]
    #[test]
    fn relative_key_handles_verbatim_root() {
        let path = Path::new(r"C:\repo\project\generated\big.txt");
        let root = Path::new(r"\\?\C:\repo\project");
        let relative = relative_key(path, root);
        assert_eq!(relative, "generated/big.txt");
    }
}
