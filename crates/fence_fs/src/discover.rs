use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::FsError;

pub struct ConfigDiscovery {
    cache: HashMap<PathBuf, Option<PathBuf>>,
}

impl ConfigDiscovery {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn find_in_dir(&mut self, dir: &Path) -> Result<Option<PathBuf>, FsError> {
        if let Some(cached) = self.cache.get(dir) {
            return Ok(cached.clone());
        }

        let candidate = dir.join(".fence.toml");
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
        std::fs::write(root.join(".fence.toml"), "default_max_lines = 10").unwrap();
        std::fs::write(sub.join(".fence.toml"), "default_max_lines = 20").unwrap();

        let file = sub.join("file.txt");
        std::fs::write(&file, "hello").unwrap();

        let mut discovery = ConfigDiscovery::new();
        let found = find_config(&file, &mut discovery).unwrap();
        assert_eq!(found.unwrap(), sub.join(".fence.toml"));
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
}
