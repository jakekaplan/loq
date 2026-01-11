//! Configuration file discovery.
//!
//! Finds `loq.toml` files by walking up the directory tree.
//! Results are cached for performance when checking many files.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::FsError;

/// Cached config file discovery.
///
/// Caches the results of searching for `loq.toml` files to avoid
/// repeated filesystem lookups when checking many files in the same tree.
pub struct ConfigDiscovery {
    cache: FxHashMap<PathBuf, Option<PathBuf>>,
}

impl ConfigDiscovery {
    /// Creates a new discovery instance with an empty cache.
    pub fn new() -> Self {
        Self {
            cache: FxHashMap::default(),
        }
    }

    /// Finds a config file in or above the given directory.
    ///
    /// Searches upward from `dir` looking for `loq.toml`.
    /// Results are cached for subsequent lookups.
    pub fn find_in_dir(&mut self, dir: &Path) -> Result<Option<PathBuf>, FsError> {
        if let Some(cached) = self.cache.get(dir) {
            return Ok(cached.clone());
        }

        let candidate = dir.join("loq.toml");
        if candidate.is_file() {
            let value = Some(candidate);
            self.cache.insert(dir.to_path_buf(), value.clone());
            return Ok(value);
        }

        let result = match dir.parent() {
            Some(parent) => self.find_in_dir(parent)?,
            None => None,
        };
        self.cache.insert(dir.to_path_buf(), result.clone());
        Ok(result)
    }
}

impl Default for ConfigDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Finds the config file applicable to a given file path.
///
/// Looks for `loq.toml` starting from the file's parent directory.
pub fn find_config(
    path: &Path,
    discovery: &mut ConfigDiscovery,
) -> Result<Option<PathBuf>, FsError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    discovery.find_in_dir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn finds_nearest_config() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.join("loq.toml"), "default_max_lines = 10").unwrap();
        std::fs::write(sub.join("loq.toml"), "default_max_lines = 20").unwrap();

        let file = sub.join("file.txt");
        std::fs::write(&file, "hello").unwrap();

        let mut discovery = ConfigDiscovery::new();
        let found = find_config(&file, &mut discovery).unwrap();
        assert_eq!(found.unwrap(), sub.join("loq.toml"));
    }

    #[test]
    fn no_config_returns_none() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "hello").unwrap();

        let mut discovery = ConfigDiscovery::new();
        let found = find_config(&file, &mut discovery).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn cache_hit_returns_cached_value() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("loq.toml"), "default_max_lines = 10").unwrap();

        let file1 = root.join("file1.txt");
        let file2 = root.join("file2.txt");
        std::fs::write(&file1, "hello").unwrap();
        std::fs::write(&file2, "world").unwrap();

        let mut discovery = ConfigDiscovery::new();
        // First call populates cache
        let found1 = find_config(&file1, &mut discovery).unwrap();
        assert!(found1.is_some());
        // Second call should hit cache
        let found2 = find_config(&file2, &mut discovery).unwrap();
        assert_eq!(found1, found2);
    }

    #[test]
    fn default_impl_works() {
        let mut discovery = ConfigDiscovery::default();
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "hello").unwrap();
        let found = find_config(&file, &mut discovery).unwrap();
        assert!(found.is_none());
    }
}
