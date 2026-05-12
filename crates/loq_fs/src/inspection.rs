//! Cached file inspection.
//!
//! Owns file metadata lookup, cache policy, line inspection, and conversion to
//! check outcomes.

use std::path::Path;
use std::sync::Mutex;

use loq_core::{MatchBy, OutcomeKind};

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
        limit: usize,
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
            Ok(FileInspection::Text { lines }) => {
                self.cache_result(cache_key, mtime, CachedResult::Text(lines));
                outcome_for_lines(lines, limit, matched_by)
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
        limit: usize,
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
    limit: usize,
    matched_by: MatchBy,
) -> OutcomeKind {
    match result {
        CachedResult::Text(lines) => outcome_for_lines(lines, limit, matched_by),
        CachedResult::Binary => OutcomeKind::Binary,
    }
}

const fn outcome_for_lines(lines: usize, limit: usize, matched_by: MatchBy) -> OutcomeKind {
    if lines > limit {
        OutcomeKind::Violation {
            limit,
            actual: lines,
            matched_by,
        }
    } else {
        OutcomeKind::Pass {
            limit,
            actual: lines,
            matched_by,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn text_file_outcome_uses_limit() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "a\nb\n").unwrap();
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(file.path(), "a.rs", 1, MatchBy::Default);

        match outcome {
            OutcomeKind::Violation { actual, limit, .. } => {
                assert_eq!(actual, 2);
                assert_eq!(limit, 1);
            }
            other => panic!("expected violation, got {other:?}"),
        }
    }

    #[test]
    fn binary_file_outcome_is_cached() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"\0binary").unwrap();
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(file.path(), "bin.dat", 1, MatchBy::Default);

        assert!(matches!(outcome, OutcomeKind::Binary));
    }

    #[test]
    fn missing_file_is_not_cacheable() {
        let inspector = Inspector::new(Cache::empty());

        let outcome = inspector.inspect(Path::new("missing.rs"), "missing.rs", 1, MatchBy::Default);

        assert!(matches!(outcome, OutcomeKind::Missing));
    }
}
