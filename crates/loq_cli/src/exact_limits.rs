//! Managed exact-path limits in `loq.toml`.
//!
//! Owns the TOML rule shape for literal file paths, including glob escaping,
//! rule discovery, and safe updates/removal by rule index.

use std::collections::HashMap;
use std::path::Path;

use loq_fs::path_identity::normalize_key;
use toml_edit::{DocumentMut, Item, Table};

const GLOB_ESCAPED_LITERALS: [(&str, char); 6] = [
    ("[*]", '*'),
    ("[?]", '?'),
    ("[[]", '['),
    ("[]]", ']'),
    ("[{]", '{'),
    ("[}]", '}'),
];

/// One exact-path limit rule in `loq.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactLimit {
    /// Maximum allowed lines.
    pub max_lines: usize,
    rule_index: usize,
}

/// Exact-path limits collected from `loq.toml`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExactLimits {
    rules: HashMap<String, ExactLimit>,
}

impl ExactLimits {
    /// Collect existing exact-path rules.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn collect(doc: &DocumentMut) -> Self {
        let mut rules = HashMap::new();

        if let Some(rules_array) = doc.get("rules").and_then(Item::as_array_of_tables) {
            for (idx, rule) in rules_array.iter().enumerate() {
                if let Some(path_value) = rule.get("path") {
                    let paths = extract_paths(path_value);
                    if paths.len() == 1 && is_exact_path(&paths[0]) {
                        if let Some(max_lines) = rule.get("max_lines").and_then(Item::as_integer) {
                            let unescaped = unescape_glob(&paths[0]);
                            let normalized = normalize_key(&unescaped);
                            rules.insert(
                                normalized,
                                ExactLimit {
                                    max_lines: max_lines as usize,
                                    rule_index: idx,
                                },
                            );
                        }
                    }
                }
            }
        }

        Self { rules }
    }

    /// Returns true when an exact-path rule exists for `path`.
    pub fn contains_path(&self, path: &str) -> bool {
        self.rules.contains_key(path)
    }

    /// Returns the exact-path limit for `path`.
    pub fn get(&self, path: &str) -> Option<ExactLimit> {
        self.rules.get(path).copied()
    }

    /// Iterates over collected exact-path limits.
    pub fn iter(&self) -> impl Iterator<Item = (&str, ExactLimit)> {
        self.rules
            .iter()
            .map(|(path, limit)| (path.as_str(), *limit))
    }

    /// Iterates over limits inside a config-relative path scope.
    pub fn within<'a>(
        &'a self,
        scope: &'a str,
    ) -> impl Iterator<Item = (&'a str, ExactLimit)> + 'a {
        self.iter()
            .filter(move |(path, _)| scope.is_empty() || Path::new(path).starts_with(scope))
    }
}

/// Adds or updates an exact-path rule.
pub(crate) fn set_limit(doc: &mut DocumentMut, limits: &ExactLimits, path: &str, max_lines: usize) {
    if let Some(limit) = limits.get(path) {
        update_limit(doc, limit, max_lines);
    } else {
        add_limit(doc, path, max_lines);
    }
}

/// Updates `max_lines` for an existing exact-path rule.
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn update_limit(doc: &mut DocumentMut, limit: ExactLimit, new_max: usize) {
    if let Some(rule) = rule_mut(doc, limit.rule_index) {
        rule["max_lines"] = toml_edit::value(new_max as i64);
    }
}

/// Removes exact-path rules, preserving index correctness.
pub(crate) fn remove_limits(doc: &mut DocumentMut, limits: impl IntoIterator<Item = ExactLimit>) {
    let mut indices = limits
        .into_iter()
        .map(|limit| limit.rule_index)
        .collect::<Vec<_>>();
    indices.sort_unstable_by(|a, b| b.cmp(a));
    for idx in indices {
        remove_rule(doc, idx);
    }
}

/// Extract path strings from a path value (can be string or array).
pub(crate) fn extract_paths(value: &Item) -> Vec<String> {
    if let Some(s) = value.as_str() {
        vec![s.to_string()]
    } else if let Some(arr) = value.as_array() {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else {
        vec![]
    }
}

/// Check if a path is an exact path (no unescaped glob metacharacters).
///
/// Exact paths written by `add_limit` are escaped with `globset::escape`.
/// If unescape -> escape round-trips to the same string, path is exact.
pub(crate) fn is_exact_path(path: &str) -> bool {
    let unescaped = unescape_glob(path);
    escape_exact_path(&unescaped) == path
}

/// Unescape glob metacharacters escaped by `globset::escape`.
///
/// `globset::escape` uses single-character classes for literals:
/// `[*]`, `[?]`, `[[]`, `[]]`, `[{]`, `[}]`.
fn unescape_glob(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut i = 0;

    while i < path.len() {
        let rest = &path[i..];

        if let Some((sequence, ch)) = escaped_literal_at(rest) {
            out.push(ch);
            i += sequence.len();
            continue;
        }

        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        }
    }

    out
}

fn escaped_literal_at(s: &str) -> Option<(&'static str, char)> {
    for (sequence, ch) in GLOB_ESCAPED_LITERALS {
        if s.starts_with(sequence) {
            return Some((sequence, ch));
        }
    }
    None
}

fn escape_exact_path(path: &str) -> String {
    globset::escape(path)
}

#[allow(clippy::cast_possible_wrap)]
fn add_limit(doc: &mut DocumentMut, path: &str, max_lines: usize) {
    if doc.get("rules").is_none() {
        doc["rules"] = Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(|item| item.as_array_of_tables_mut())
    {
        let mut rule = Table::new();
        rule["path"] = toml_edit::value(escape_exact_path(path));
        rule["max_lines"] = toml_edit::value(max_lines as i64);
        rules.push(rule);
    }
}

fn remove_rule(doc: &mut DocumentMut, idx: usize) {
    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(|item| item.as_array_of_tables_mut())
    {
        rules.remove(idx);
    }
}

fn rule_mut(doc: &mut DocumentMut, idx: usize) -> Option<&mut Table> {
    doc.get_mut("rules")
        .and_then(|item| item.as_array_of_tables_mut())
        .and_then(|rules| rules.get_mut(idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::{Array, Formatted, Value};

    #[test]
    fn is_exact_path_detects_globs() {
        assert!(is_exact_path("src/main.rs"));
        assert!(is_exact_path("foo/bar/baz.txt"));
        assert!(!is_exact_path("**/*.rs"));
        assert!(!is_exact_path("src/*.rs"));
        assert!(!is_exact_path("src/[ab].rs"));
        assert!(!is_exact_path("src/{a,b}.rs"));
        assert!(!is_exact_path("src/?.rs"));
    }

    #[test]
    fn is_exact_path_handles_escaped_metacharacters() {
        assert!(is_exact_path("routes/[[]id[]]/page.svelte"));
        assert!(is_exact_path("src/literal-[*]-[?]-[{]-[}]"));
        assert!(!is_exact_path("routes/[[]id[]]/*.svelte"));
        assert!(!is_exact_path("[[]a-z].rs"));
    }

    #[test]
    fn unescape_glob_reverses_escape() {
        for (escaped, unescaped) in GLOB_ESCAPED_LITERALS {
            let input = format!("foo{escaped}bar");
            let output = format!("foo{unescaped}bar");
            assert_eq!(unescape_glob(&input), output);
        }
    }

    #[test]
    fn unescape_glob_handles_adjacent_escaped_sequences() {
        assert_eq!(unescape_glob("[[][*][?][{][}][]]"), "[*?{}]");
    }

    #[test]
    fn extract_paths_from_string() {
        let item = Item::Value(Value::String(Formatted::new("src/main.rs".into())));
        assert_eq!(extract_paths(&item), vec!["src/main.rs"]);
    }

    #[test]
    fn extract_paths_from_array() {
        let mut arr = Array::new();
        arr.push("a.rs");
        arr.push("b.rs");
        let item = Item::Value(Value::Array(arr));
        assert_eq!(extract_paths(&item), vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn collect_filters_non_exact_rules() {
        let doc: DocumentMut = r#"
[[rules]]
path = "src/a.rs"
max_lines = 10

[[rules]]
path = ["src/b.rs", "src/c.rs"]
max_lines = 20

[[rules]]
path = "**/*.rs"
max_lines = 30
"#
        .parse()
        .unwrap();

        let limits = ExactLimits::collect(&doc);
        assert_eq!(limits.rules.len(), 1);
        assert_eq!(limits.get("src/a.rs").unwrap().max_lines, 10);
    }

    #[test]
    fn set_update_remove_flow() {
        let mut doc = DocumentMut::new();
        let limits = ExactLimits::collect(&doc);

        set_limit(&mut doc, &limits, "src/a.rs", 10);
        let limits = ExactLimits::collect(&doc);
        set_limit(&mut doc, &limits, "src/b.rs", 12);

        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules
                .get(0)
                .unwrap()
                .get("max_lines")
                .and_then(Item::as_integer),
            Some(10)
        );

        let limits = ExactLimits::collect(&doc);
        let limit = limits.get("src/a.rs").unwrap();
        update_limit(&mut doc, limit, 15);
        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        assert_eq!(
            rules
                .get(0)
                .unwrap()
                .get("max_lines")
                .and_then(Item::as_integer),
            Some(15)
        );

        let limits = ExactLimits::collect(&doc);
        remove_limits(&mut doc, [limits.get("src/b.rs").unwrap()]);
        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules.get(0).unwrap().get("path").and_then(Item::as_str),
            Some("src/a.rs")
        );
    }

    #[test]
    fn collect_normalizes_dot_slash() {
        let doc: DocumentMut = r#"
[[rules]]
path = "./src/a.rs"
max_lines = 10
"#
        .parse()
        .unwrap();

        let limits = ExactLimits::collect(&doc);
        assert_eq!(limits.get("src/a.rs").unwrap().max_lines, 10);
    }

    #[test]
    fn set_escapes_glob_metacharacters() {
        let mut doc = DocumentMut::new();
        let limits = ExactLimits::collect(&doc);

        set_limit(&mut doc, &limits, "routes/[id]/page.svelte", 100);

        let rules = doc.get("rules").and_then(Item::as_array_of_tables).unwrap();
        let first = rules.get(0).unwrap();
        assert_eq!(
            first.get("path").and_then(Item::as_str),
            Some("routes/[[]id[]]/page.svelte")
        );
    }

    #[test]
    fn collect_unescapes_escaped_paths() {
        let doc: DocumentMut = r#"
[[rules]]
path = "routes/[[]id[]]/page.svelte"
max_lines = 100
"#
        .parse()
        .unwrap();

        let limits = ExactLimits::collect(&doc);
        assert_eq!(
            limits.get("routes/[id]/page.svelte").unwrap().max_lines,
            100
        );
    }
}
