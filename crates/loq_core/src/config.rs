//! Configuration types and compilation.
//!
//! Defines the structure of `loq.toml` files and compiles glob patterns
//! into efficient matchers.

use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;
use thiserror::Error;

/// Violation severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Causes non-zero exit code.
    #[default]
    Error,
    /// Reported but does not fail the check.
    Warning,
}

/// A path-specific line limit rule.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// Glob pattern to match files (e.g., `**/*.rs`).
    pub path: String,
    /// Maximum allowed lines for matched files.
    pub max_lines: usize,
    /// Severity when limit is exceeded (default: error).
    #[serde(default)]
    pub severity: Severity,
}

/// Parsed `loq.toml` configuration (before compilation).
#[derive(Debug, Clone, Deserialize)]
pub struct LoqConfig {
    /// Default line limit for files not matching any rule.
    pub default_max_lines: Option<usize>,
    /// Whether to skip files matched by `.gitignore`.
    #[serde(default = "default_respect_gitignore")]
    pub respect_gitignore: bool,
    /// Glob patterns for files to completely skip (not counted).
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Glob patterns for files to exempt (counted but not checked).
    #[serde(default)]
    pub exempt: Vec<String>,
    /// Path-specific rules (last match wins).
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for LoqConfig {
    fn default() -> Self {
        Self {
            default_max_lines: Some(500),
            respect_gitignore: true,
            exclude: Vec::new(),
            exempt: Vec::new(),
            rules: Vec::new(),
        }
    }
}

impl LoqConfig {
    /// Returns the built-in defaults used when no config file is found.
    #[must_use]
    pub fn built_in_defaults() -> Self {
        Self::default()
    }

    /// Returns a template config for `loq init`.
    #[must_use]
    pub fn init_template() -> Self {
        Self {
            rules: vec![
                Rule {
                    path: "**/*.tsx".to_string(),
                    max_lines: 300,
                    severity: Severity::Warning,
                },
                Rule {
                    path: "tests/**/*".to_string(),
                    max_lines: 500,
                    severity: Severity::Error,
                },
            ],
            ..Self::default()
        }
    }
}

/// Where a configuration came from.
#[derive(Debug, Clone)]
pub enum ConfigOrigin {
    /// Using built-in defaults (no config file found).
    BuiltIn,
    /// Loaded from a specific file path.
    File(PathBuf),
}

/// Configuration with compiled glob matchers, ready for use.
#[derive(Debug, Clone)]
pub struct CompiledConfig {
    /// Where this config came from.
    pub origin: ConfigOrigin,
    /// Root directory for relative path matching.
    pub root_dir: PathBuf,
    /// Default line limit for files not matching any rule.
    pub default_max_lines: Option<usize>,
    /// Whether to respect `.gitignore` patterns.
    pub respect_gitignore: bool,
    exclude: PatternList,
    exempt: PatternList,
    rules: Vec<CompiledRule>,
}

impl CompiledConfig {
    /// Returns the exclude pattern list.
    #[must_use]
    pub const fn exclude_patterns(&self) -> &PatternList {
        &self.exclude
    }

    /// Returns the exempt pattern list.
    #[must_use]
    pub const fn exempt_patterns(&self) -> &PatternList {
        &self.exempt
    }

    /// Returns the compiled rules.
    #[must_use]
    pub fn rules(&self) -> &[CompiledRule] {
        &self.rules
    }
}

/// A rule with a compiled glob matcher.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    /// Original glob pattern string.
    pub pattern: String,
    /// Maximum allowed lines.
    pub max_lines: usize,
    /// Severity when limit exceeded.
    pub severity: Severity,
    matcher: GlobMatcher,
}

impl CompiledRule {
    /// Tests if the given path matches this rule's pattern.
    #[must_use]
    pub fn is_match(&self, path: &str) -> bool {
        self.matcher.is_match(path)
    }
}

/// A list of compiled glob patterns for matching paths.
#[derive(Debug, Clone)]
pub struct PatternList {
    patterns: Vec<PatternMatcher>,
}

impl PatternList {
    /// Creates a new pattern list from compiled matchers.
    pub(crate) const fn new(patterns: Vec<PatternMatcher>) -> Self {
        Self { patterns }
    }

    /// Returns the first matching pattern, or `None` if no match.
    #[must_use]
    pub fn matches(&self, path: &str) -> Option<&str> {
        for pattern in &self.patterns {
            if pattern.matcher.is_match(path) {
                return Some(pattern.pattern.as_str());
            }
        }
        None
    }
}

/// A single compiled glob pattern.
#[derive(Debug, Clone)]
pub(crate) struct PatternMatcher {
    pattern: String,
    matcher: GlobMatcher,
}

/// Errors that can occur when parsing or compiling configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// TOML syntax error.
    #[error("{}", format_toml_error(path, line_col, message))]
    Toml {
        /// Path to the config file.
        path: PathBuf,
        /// Error message from the TOML parser.
        message: String,
        /// Location in the file (line, column).
        line_col: Option<(usize, usize)>,
    },
    /// Unknown key in the config file.
    #[error("{}", format_unknown_key_error(path, key, line_col, suggestion))]
    UnknownKey {
        /// Path to the config file.
        path: PathBuf,
        /// The unrecognized key.
        key: String,
        /// Location in the file (line, column).
        line_col: Option<(usize, usize)>,
        /// Suggested correction if one is close enough.
        suggestion: Option<String>,
    },
    /// Invalid glob pattern.
    #[error("{} - invalid glob '{}': {}", path.display(), pattern, message)]
    Glob {
        /// Path to the config file.
        path: PathBuf,
        /// The invalid pattern.
        pattern: String,
        /// Error message from the glob parser.
        message: String,
    },
}

#[allow(clippy::ref_option)]
fn format_toml_error(path: &Path, line_col: &Option<(usize, usize)>, message: &str) -> String {
    if let Some((line, col)) = line_col {
        format!("{}:{}:{} - {}", path.display(), line, col, message)
    } else {
        format!("{} - {}", path.display(), message)
    }
}

#[allow(clippy::ref_option)]
fn format_unknown_key_error(
    path: &Path,
    key: &str,
    line_col: &Option<(usize, usize)>,
    suggestion: &Option<String>,
) -> String {
    let base = format_toml_error(path, line_col, &format!("unknown key '{key}'"));
    if let Some(suggestion) = suggestion {
        format!("{base}\n       did you mean '{suggestion}'?")
    } else {
        base
    }
}

/// Compiles a parsed configuration into efficient matchers.
///
/// Takes a `LoqConfig` and compiles all glob patterns into matchers.
/// The `root_dir` is used for relative path matching during checks.
pub fn compile_config(
    origin: ConfigOrigin,
    root_dir: PathBuf,
    config: LoqConfig,
    source_path: Option<&Path>,
) -> Result<CompiledConfig, ConfigError> {
    let path_for_errors =
        source_path.map_or_else(|| PathBuf::from("<built-in defaults>"), Path::to_path_buf);

    let exclude = compile_patterns(&config.exclude, &path_for_errors)?;
    let exempt = compile_patterns(&config.exempt, &path_for_errors)?;
    let mut rules = Vec::new();
    for rule in config.rules {
        let matcher = compile_glob(&rule.path, &path_for_errors)?;
        rules.push(CompiledRule {
            pattern: rule.path,
            max_lines: rule.max_lines,
            severity: rule.severity,
            matcher,
        });
    }

    Ok(CompiledConfig {
        origin,
        root_dir,
        default_max_lines: config.default_max_lines,
        respect_gitignore: config.respect_gitignore,
        exclude,
        exempt,
        rules,
    })
}

fn compile_patterns(patterns: &[String], source_path: &Path) -> Result<PatternList, ConfigError> {
    let mut compiled = Vec::new();
    for pattern in patterns {
        let matcher = compile_glob(pattern, source_path)?;
        compiled.push(PatternMatcher {
            pattern: pattern.clone(),
            matcher,
        });
    }
    Ok(PatternList::new(compiled))
}

fn compile_glob(pattern: &str, source_path: &Path) -> Result<GlobMatcher, ConfigError> {
    #[cfg(windows)]
    let builder = {
        let mut builder = GlobBuilder::new(pattern);
        builder.case_insensitive(true);
        builder
    };
    #[cfg(not(windows))]
    let builder = GlobBuilder::new(pattern);
    let glob = builder.build().map_err(|err| ConfigError::Glob {
        path: source_path.to_path_buf(),
        pattern: pattern.to_string(),
        message: err.to_string(),
    })?;
    Ok(glob.compile_matcher())
}

const fn default_respect_gitignore() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_config_has_expected_values() {
        let config = LoqConfig::default();
        assert_eq!(config.default_max_lines, Some(500));
        assert!(config.respect_gitignore);
        assert!(config.exclude.is_empty());
        assert!(config.exempt.is_empty());
        assert!(config.rules.is_empty());
    }

    #[test]
    fn built_in_defaults_matches_default() {
        let default = LoqConfig::default();
        let built_in = LoqConfig::built_in_defaults();
        assert_eq!(default.default_max_lines, built_in.default_max_lines);
        assert_eq!(default.respect_gitignore, built_in.respect_gitignore);
    }

    #[test]
    fn init_template_has_rules() {
        let template = LoqConfig::init_template();
        assert_eq!(template.default_max_lines, Some(500));
        assert_eq!(template.rules.len(), 2);
        assert_eq!(template.rules[0].path, "**/*.tsx");
        assert_eq!(template.rules[1].path, "tests/**/*");
    }

    #[test]
    fn invalid_glob_reports_error() {
        let config = LoqConfig {
            default_max_lines: Some(1),
            respect_gitignore: true,
            exclude: vec![],
            exempt: vec![],
            rules: vec![Rule {
                path: "[[".to_string(),
                max_lines: 1,
                severity: Severity::Error,
            }],
        };
        let err =
            compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();
        match err {
            ConfigError::Glob { .. } => {}
            _ => panic!("expected glob error"),
        }
    }

    #[test]
    fn glob_error_display_is_stable() {
        let config = LoqConfig {
            default_max_lines: Some(1),
            respect_gitignore: true,
            exclude: vec!["[[".to_string()],
            exempt: vec![],
            rules: vec![],
        };
        let err =
            compile_config(ConfigOrigin::BuiltIn, PathBuf::from("."), config, None).unwrap_err();
        assert!(err.to_string().contains("invalid glob"));
    }

    #[test]
    fn pattern_list_no_match_returns_none() {
        let patterns = vec![PatternMatcher {
            pattern: "*.rs".to_string(),
            matcher: globset::GlobBuilder::new("*.rs")
                .literal_separator(true)
                .build()
                .unwrap()
                .compile_matcher(),
        }];
        let list = PatternList::new(patterns);
        assert!(list.matches("foo.txt").is_none());
    }

    #[test]
    fn format_toml_error_without_location() {
        let msg = format_toml_error(Path::new("test.toml"), &None, "parse error");
        assert_eq!(msg, "test.toml - parse error");
    }

    #[test]
    fn format_unknown_key_error_without_suggestion() {
        let msg = format_unknown_key_error(Path::new("test.toml"), "xyz", &Some((1, 1)), &None);
        assert!(msg.contains("unknown key 'xyz'"));
        assert!(!msg.contains("did you mean"));
    }
}
