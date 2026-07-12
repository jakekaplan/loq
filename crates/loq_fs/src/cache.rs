//! File measurement caching.
//!
//! Caches measurements keyed by relative file path and mtime to skip I/O on unchanged files.
//! Cache is invalidated when config changes (detected via config hash).
//! Keys are paths relative to config root for consistency across working directories.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::SystemTime;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use loq_core::config::CompiledConfig;

const CACHE_VERSION: u32 = 2; // Bumped for CachedResult enum
const CACHE_FILE: &str = ".loq_cache";

/// On-disk cache format (for deserialization).
#[derive(Deserialize)]
struct CacheFile {
    version: u32,
    config_hash: u64,
    entries: FxHashMap<String, CacheEntry>,
}

/// Borrowed view for serialization (avoids cloning entries).
#[derive(Serialize)]
struct CacheFileRef<'a> {
    version: u32,
    config_hash: u64,
    entries: &'a FxHashMap<String, CacheEntry>,
}

/// Cached inspection result for a file.
///
/// Only cacheable results are included - missing/unreadable files can't be cached
/// because we need an mtime for cache invalidation, and `metadata()` fails for those.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachedResult {
    /// File is text with the measured value for the active metric.
    Text(usize),
    /// File is binary.
    Binary,
}

/// Single cache entry for a file.
#[derive(Serialize, Deserialize, Clone)]
struct CacheEntry {
    mtime_secs: u64,
    mtime_nanos: u32,
    result: CachedResult,
}

/// In-memory cache for file measurements.
pub struct Cache {
    entries: FxHashMap<String, CacheEntry>,
    config_hash: u64,
    has_unsaved_changes: bool,
}

impl Cache {
    /// Creates an empty cache (used when caching is disabled).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: FxHashMap::default(),
            config_hash: 0,
            has_unsaved_changes: false,
        }
    }

    /// Loads cache from disk. Returns empty cache on any error or config mismatch.
    #[must_use]
    pub fn load(root: &Path, config_hash: u64) -> Self {
        let path = root.join(CACHE_FILE);

        let Ok(contents) = fs::read_to_string(&path) else {
            return Self::empty_with_hash(config_hash);
        };

        let Ok(cache_file) = serde_json::from_str::<CacheFile>(&contents) else {
            return Self::empty_with_hash(config_hash);
        };

        // Invalidate if version or config changed
        if cache_file.version != CACHE_VERSION || cache_file.config_hash != config_hash {
            return Self::empty_with_hash(config_hash);
        }

        Self {
            entries: cache_file.entries,
            config_hash,
            has_unsaved_changes: false,
        }
    }

    fn empty_with_hash(config_hash: u64) -> Self {
        Self {
            entries: FxHashMap::default(),
            config_hash,
            has_unsaved_changes: false,
        }
    }

    /// Looks up cached result. Returns None if not cached or mtime doesn't match.
    #[must_use]
    pub fn get(&self, key: &str, mtime: SystemTime) -> Option<CachedResult> {
        let entry = self.entries.get(key)?;
        let (secs, nanos) = mtime_to_parts(mtime);

        if entry.mtime_secs == secs && entry.mtime_nanos == nanos {
            Some(entry.result)
        } else {
            None
        }
    }

    /// Stores inspection result in cache.
    pub fn insert(&mut self, key: String, mtime: SystemTime, result: CachedResult) {
        let (secs, nanos) = mtime_to_parts(mtime);
        self.entries.insert(
            key,
            CacheEntry {
                mtime_secs: secs,
                mtime_nanos: nanos,
                result,
            },
        );
        self.has_unsaved_changes = true;
    }

    /// Saves cache to disk. Silently ignores errors (caching is best-effort).
    pub fn save(&self, root: &Path) {
        if !self.has_unsaved_changes {
            return;
        }

        let cache_ref = CacheFileRef {
            version: CACHE_VERSION,
            config_hash: self.config_hash,
            entries: &self.entries,
        };

        let Ok(contents) = serde_json::to_string(&cache_ref) else {
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

    // Hash default limit.
    config.default_limit.hash(&mut hasher);

    // Hash rules (patterns and limits)
    for rule in config.rules() {
        rule.limit.hash(&mut hasher);
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
            default_limit: default_max.map(loq_core::Limit::lines),
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
    fn insert_and_get_text() {
        let mut cache = Cache::empty_with_hash(123);
        let mtime = SystemTime::now();

        cache.insert("src/main.rs".to_string(), mtime, CachedResult::Text(42));

        assert_eq!(
            cache.get("src/main.rs", mtime),
            Some(CachedResult::Text(42))
        );
    }

    #[test]
    fn insert_and_get_binary() {
        let mut cache = Cache::empty_with_hash(123);
        let mtime = SystemTime::now();

        cache.insert("image.png".to_string(), mtime, CachedResult::Binary);

        assert_eq!(cache.get("image.png", mtime), Some(CachedResult::Binary));
    }

    #[test]
    fn mtime_mismatch_returns_none() {
        let mut cache = Cache::empty_with_hash(123);
        let mtime1 = SystemTime::UNIX_EPOCH;
        let mtime2 = SystemTime::now();

        cache.insert("src/main.rs".to_string(), mtime1, CachedResult::Text(42));

        assert!(cache.get("src/main.rs", mtime2).is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let temp = TempDir::new().unwrap();
        let config_hash = 12345;

        // Create and populate cache with different result types
        let mut cache = Cache::empty_with_hash(config_hash);
        let mtime = SystemTime::UNIX_EPOCH;
        cache.insert("test.rs".to_string(), mtime, CachedResult::Text(100));
        cache.insert("binary.dat".to_string(), mtime, CachedResult::Binary);
        cache.save(temp.path());

        // Load cache
        let loaded = Cache::load(temp.path(), config_hash);
        assert_eq!(loaded.get("test.rs", mtime), Some(CachedResult::Text(100)));
        assert_eq!(loaded.get("binary.dat", mtime), Some(CachedResult::Binary));
    }

    #[test]
    fn config_change_invalidates_cache() {
        let temp = TempDir::new().unwrap();

        // Save with one config hash
        let mut cache = Cache::empty_with_hash(111);
        cache.insert(
            "test.rs".to_string(),
            SystemTime::UNIX_EPOCH,
            CachedResult::Text(100),
        );
        cache.save(temp.path());

        // Load with different config hash
        let loaded = Cache::load(temp.path(), 222);
        assert!(loaded.get("test.rs", SystemTime::UNIX_EPOCH).is_none());
    }

    #[test]
    fn old_cache_version_is_discarded_and_rebuilt_as_v2() {
        let temp = TempDir::new().unwrap();
        let config_hash = 12345u64;

        // Write a cache file with old version format (v1 used `lines: usize` instead of `result`)
        let old_cache = serde_json::json!({
            "version": 1,
            "config_hash": config_hash,
            "entries": {
                "test.rs": {
                    "mtime_secs": 0,
                    "mtime_nanos": 0,
                    "lines": 100
                }
            }
        });
        fs::write(temp.path().join(CACHE_FILE), old_cache.to_string()).unwrap();

        // Loading should discard the old cache and return empty
        let mut loaded = Cache::load(temp.path(), config_hash);
        assert!(
            loaded.get("test.rs", SystemTime::UNIX_EPOCH).is_none(),
            "old v1 cache should be discarded"
        );

        // Insert new entry and save
        let mtime = SystemTime::UNIX_EPOCH;
        loaded.insert("new.rs".to_string(), mtime, CachedResult::Text(50));
        loaded.save(temp.path());

        // Verify the saved cache is v2 format
        let contents = fs::read_to_string(temp.path().join(CACHE_FILE)).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(saved["version"], 2, "saved cache should be v2");
        assert!(
            saved["entries"]["new.rs"]["result"].is_object()
                || saved["entries"]["new.rs"]["result"].is_string(),
            "v2 cache should have 'result' field, not 'lines'"
        );

        // Verify it can be reloaded
        let reloaded = Cache::load(temp.path(), config_hash);
        assert_eq!(
            reloaded.get("new.rs", mtime),
            Some(CachedResult::Text(50)),
            "v2 cache should roundtrip correctly"
        );
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
    fn no_save_when_unchanged() {
        let temp = TempDir::new().unwrap();
        let cache = Cache::empty_with_hash(123);

        // Save without any inserts - nothing written because unchanged
        cache.save(temp.path());

        // Cache file should not exist
        assert!(!temp.path().join(CACHE_FILE).exists());
    }
}
