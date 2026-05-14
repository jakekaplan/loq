//! Cached file inspection.
//!
//! Owns file metadata lookup, cache policy, line inspection, and conversion to
//! check outcomes.

use std::path::Path;
use std::sync::Mutex;

use loq_core::{Limit, MatchBy, Metric, OutcomeKind};

use crate::cache::{Cache, CachedResult};
use crate::count::{self, FileInspection};

/// Inspects files with a shared cache.
pub(crate) struct Inspector {
    cache: Mutex<Cache>,
}

impl Inspector {
    /// Creates an inspector backed by `cache`.
    pub(crate) const fn new(cache: Cache) -> Self {
        Self {
            cache: Mutex::new(cache),
        }
    }

    /// Inspects a file and returns its check outcome for the given limit.
    pub(crate) fn inspect(
        &self,
        path: &Path,
        cache_key: &str,
        limit: Limit,
        matched_by: MatchBy,
    ) -> OutcomeKind {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

        if let Some(outcome) = self.cached_outcome(cache_key, mtime, limit, &matched_by) {
            return outcome;
        }

        match count::inspect_file(path) {
            Ok(FileInspection::Binary) => {
                self.cache_result(cache_key, mtime, CachedResult::Binary);
                OutcomeKind::Binary
            }
            Ok(FileInspection::Text { lines, bytes }) => {
                let actual = measurement_for_limit(lines, bytes, limit);
                self.cache_result(cache_key, mtime, CachedResult::Text(actual));
                outcome_for_measurement(actual, limit, matched_by)
            }
            Err(count::CountError::Missing) => OutcomeKind::Missing,
            Err(count::CountError::Unreadable(error)) => OutcomeKind::Unreadable {
                error: error.to_string(),
            },
        }
    }

    /// Consumes the inspector and returns the inner cache when no worker still holds it.
    pub(crate) fn into_cache(self) -> Option<Cache> {
        self.cache.into_inner().ok()
    }

    fn cached_outcome(
        &self,
        cache_key: &str,
        mtime: Option<std::time::SystemTime>,
        limit: Limit,
        matched_by: &MatchBy,
    ) -> Option<OutcomeKind> {
        let mt = mtime?;
        let cache = self.cache.lock().ok()?;
        let result = cache.get(cache_key, mt)?;
        Some(cached_result_to_outcome(result, limit, matched_by.clone()))
    }

    fn cache_result(
        &self,
        cache_key: &str,
        mtime: Option<std::time::SystemTime>,
        result: CachedResult,
    ) {
        let Some(mt) = mtime else {
            return;
        };
        let Ok(mut cache) = self.cache.lock() else {
            return;
        };
        cache.insert(cache_key.to_string(), mt, result);
    }
}

fn cached_result_to_outcome(
    result: CachedResult,
    limit: Limit,
    matched_by: MatchBy,
) -> OutcomeKind {
    match result {
        CachedResult::Text(actual) => outcome_for_measurement(actual, limit, matched_by),
        CachedResult::Binary => OutcomeKind::Binary,
    }
}

const fn measurement_for_limit(lines: usize, bytes: usize, limit: Limit) -> usize {
    match limit.metric {
        Metric::Lines => lines,
        Metric::Tokens => {
            if bytes % 4 == 0 {
                bytes / 4
            } else {
                bytes / 4 + 1
            }
        }
    }
}

const fn outcome_for_measurement(actual: usize, limit: Limit, matched_by: MatchBy) -> OutcomeKind {
    if actual > limit.max {
        OutcomeKind::Violation {
            limit,
            actual,
            matched_by,
        }
    } else {
        OutcomeKind::Pass {
            limit,
            actual,
            matched_by,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;
    use std::time::SystemTime;
    use tempfile::NamedTempFile;

    #[test]
    fn text_file_outcome_uses_limit() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "a\nb\n").unwrap();
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(file.path(), "a.rs", Limit::lines(1), MatchBy::Default);

        assert!(matches!(
            outcome,
            OutcomeKind::Violation {
                actual: 2,
                limit: Limit {
                    metric: Metric::Lines,
                    max: 1
                },
                ..
            }
        ));
    }

    #[test]
    fn token_file_outcome_uses_ceil_bytes_over_four() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "12345").unwrap();
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(file.path(), "a.md", Limit::tokens(1), MatchBy::Default);

        assert!(matches!(
            outcome,
            OutcomeKind::Violation {
                actual: 2,
                limit: Limit {
                    metric: Metric::Tokens,
                    max: 1
                },
                ..
            }
        ));
    }

    #[test]
    fn token_file_outcome_handles_even_four_byte_chunks() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "1234").unwrap();
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(file.path(), "a.md", Limit::tokens(1), MatchBy::Default);

        assert!(matches!(
            outcome,
            OutcomeKind::Pass {
                actual: 1,
                limit: Limit {
                    metric: Metric::Tokens,
                    max: 1
                },
                ..
            }
        ));
    }

    #[test]
    fn binary_file_outcome_is_cached() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"\0binary").unwrap();
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(file.path(), "bin.dat", Limit::lines(1), MatchBy::Default);

        assert!(matches!(outcome, OutcomeKind::Binary));
    }

    #[test]
    fn missing_file_is_not_cacheable() {
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(
            Path::new("missing.rs"),
            "missing.rs",
            Limit::lines(1),
            MatchBy::Default,
        );

        assert!(matches!(outcome, OutcomeKind::Missing));
    }

    #[test]
    fn cache_result_ignores_missing_mtime() {
        let inspector = Inspector::new(Cache::empty());

        inspector.cache_result("a.rs", None, CachedResult::Text(1));

        let cache = inspector.into_cache().unwrap();
        assert!(cache.get("a.rs", SystemTime::UNIX_EPOCH).is_none());
    }

    #[test]
    fn cache_result_ignores_poisoned_cache_lock() {
        let inspector = Inspector::new(Cache::empty());
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = inspector.cache.lock().unwrap();
            panic!("poison cache");
        }));

        assert!(result.is_err());
        inspector.cache_result("a.rs", Some(SystemTime::UNIX_EPOCH), CachedResult::Text(1));
        assert!(inspector.into_cache().is_none());
    }
}
