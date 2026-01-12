//! File line count caching.
//!
//! Caches line counts keyed by relative file path and mtime to skip I/O on unchanged files.
//! Cache is invalidated when config changes (detected via config hash).
//! Keys are paths relative to config root for consistency across working directories.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::SystemTime;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use loq_core::config::CompiledConfig;

const CACHE_VERSION: u32 = 1;
const CACHE_FILE: &str = ".loq_cache";

/// On-disk cache format.
#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    config_hash: u64,
    entries: FxHashMap<String, CacheEntry>,
}

/// Single cache entry for a file.
#[derive(Serialize, Deserialize, Clone)]
struct CacheEntry {
    mtime_secs: u64,
    mtime_nanos: u32,
    lines: usize,
}

/// In-memory cache for file line counts.
pub struct Cache {
    entries: FxHashMap<String, CacheEntry>,
    config_hash: u64,
    dirty: bool,
}

impl Cache {
    /// Creates an empty cache (used when caching is disabled).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: FxHashMap::default(),
            config_hash: 0,
            dirty: false,
        }
    }

    /// Loads cache from disk. Returns empty cache on any error or config mismatch.
    #[must_use]
    pub fn load(root: &Path, config_hash: u64) -> Self {
        let path = root.join(CACHE_FILE);

        let Ok(contents) = fs::read_to_string(&path) else {
            return Self::with_hash(config_hash);
        };

        let Ok(cache_file) = serde_json::from_str::<CacheFile>(&contents) else {
            return Self::with_hash(config_hash);
        };

        // Invalidate if version or config changed
        if cache_file.version != CACHE_VERSION || cache_file.config_hash != config_hash {
            return Self::with_hash(config_hash);
        }

        Self {
            entries: cache_file.entries,
            config_hash,
            dirty: false,
        }
    }

    fn with_hash(config_hash: u64) -> Self {
        Self {
            entries: FxHashMap::default(),
            config_hash,
            dirty: false,
        }
    }

    /// Looks up cached line count. Returns None if not cached or mtime doesn't match.
    #[must_use]
    pub fn get(&self, key: &str, mtime: SystemTime) -> Option<usize> {
        let entry = self.entries.get(key)?;
        let (secs, nanos) = mtime_to_parts(mtime);

        if entry.mtime_secs == secs && entry.mtime_nanos == nanos {
            Some(entry.lines)
        } else {
            None
        }
    }

    /// Stores line count in cache.
    pub fn insert(&mut self, key: String, mtime: SystemTime, lines: usize) {
        let (secs, nanos) = mtime_to_parts(mtime);
        self.entries.insert(
            key,
            CacheEntry {
                mtime_secs: secs,
                mtime_nanos: nanos,
                lines,
            },
        );
        self.dirty = true;
    }

    /// Saves cache to disk. Silently ignores errors.
    pub fn save(&self, root: &Path) {
        if !self.dirty {
            return;
        }

        let cache_file = CacheFile {
            version: CACHE_VERSION,
            config_hash: self.config_hash,
            entries: self.entries.clone(),
        };

        let Ok(contents) = serde_json::to_string(&cache_file) else {
            return;
        };

        let _ = fs::write(root.join(CACHE_FILE), contents);
    }
}

fn mtime_to_parts(mtime: SystemTime) -> (u64, u32) {
    match mtime.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => (duration.as_secs(), duration.subsec_nanos()),
        Err(_) => (0, 0),
    }
}

/// Computes a hash of the config for cache invalidation.
#[must_use]
pub fn hash_config(config: &CompiledConfig) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();

    // Hash default_max_lines
    config.default_max_lines.hash(&mut hasher);

    // Hash rules (patterns and limits)
    for rule in config.rules() {
        rule.max_lines.hash(&mut hasher);
        for pattern in &rule.patterns {
            pattern.hash(&mut hasher);
        }
    }

    // Hash exclude patterns
    for pattern in config.exclude_patterns().patterns() {
        pattern.hash(&mut hasher);
    }

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use loq_core::config::{compile_config, ConfigOrigin, LoqConfig};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_config(default_max: Option<usize>) -> CompiledConfig {
        let config = LoqConfig {
            default_max_lines: default_max,
            ..LoqConfig::default()
        };
        compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap()
    }

    #[test]
    fn empty_cache_returns_none() {
        let cache = Cache::empty();
        let mtime = SystemTime::now();
        assert!(cache.get("foo.rs", mtime).is_none());
    }

    #[test]
    fn insert_and_get() {
        let mut cache = Cache::with_hash(123);
        let mtime = SystemTime::now();

        cache.insert("src/main.rs".to_string(), mtime, 42);

        assert_eq!(cache.get("src/main.rs", mtime), Some(42));
    }

    #[test]
    fn mtime_mismatch_returns_none() {
        let mut cache = Cache::with_hash(123);
        let mtime1 = SystemTime::UNIX_EPOCH;
        let mtime2 = SystemTime::now();

        cache.insert("src/main.rs".to_string(), mtime1, 42);

        assert!(cache.get("src/main.rs", mtime2).is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let temp = TempDir::new().unwrap();
        let config_hash = 12345;

        // Create and populate cache
        let mut cache = Cache::with_hash(config_hash);
        let mtime = SystemTime::UNIX_EPOCH;
        cache.insert("test.rs".to_string(), mtime, 100);
        cache.save(temp.path());

        // Load cache
        let loaded = Cache::load(temp.path(), config_hash);
        assert_eq!(loaded.get("test.rs", mtime), Some(100));
    }

    #[test]
    fn config_change_invalidates_cache() {
        let temp = TempDir::new().unwrap();

        // Save with one config hash
        let mut cache = Cache::with_hash(111);
        cache.insert("test.rs".to_string(), SystemTime::UNIX_EPOCH, 100);
        cache.save(temp.path());

        // Load with different config hash
        let loaded = Cache::load(temp.path(), 222);
        assert!(loaded.get("test.rs", SystemTime::UNIX_EPOCH).is_none());
    }

    #[test]
    fn hash_config_changes_with_default() {
        let config1 = make_config(Some(500));
        let config2 = make_config(Some(600));
        let config3 = make_config(None);

        let hash1 = hash_config(&config1);
        let hash2 = hash_config(&config2);
        let hash3 = hash_config(&config3);

        assert_ne!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_ne!(hash2, hash3);
    }

    #[test]
    fn no_save_when_not_dirty() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::with_hash(123);

        // Save without any inserts
        cache.save(temp.path());

        // Cache file should not exist
        assert!(!temp.path().join(CACHE_FILE).exists());
    }
}
