use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub path: String,
    pub max_lines: usize,
    #[serde(default)]
    pub severity: Severity,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FenceConfig {
    pub default_max_lines: Option<usize>,
    #[serde(default = "default_respect_gitignore")]
    pub respect_gitignore: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub exempt: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl FenceConfig {
    pub fn built_in_defaults() -> Self {
        Self {
            default_max_lines: Some(500),
            respect_gitignore: true,
            exclude: Vec::new(),
            exempt: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn init_template() -> Self {
        Self {
            default_max_lines: Some(500),
            respect_gitignore: true,
            exclude: Vec::new(),
            exempt: Vec::new(),
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
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConfigOrigin {
    BuiltIn,
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct CompiledConfig {
    pub origin: ConfigOrigin,
    pub root_dir: PathBuf,
    pub default_max_lines: Option<usize>,
    pub respect_gitignore: bool,
    exclude: PatternList,
    exempt: PatternList,
    rules: Vec<CompiledRule>,
}

impl CompiledConfig {
    pub fn exclude_patterns(&self) -> &PatternList {
        &self.exclude
    }

    pub fn exempt_patterns(&self) -> &PatternList {
        &self.exempt
    }

    pub fn rules(&self) -> &[CompiledRule] {
        &self.rules
    }
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub pattern: String,
    pub max_lines: usize,
    pub severity: Severity,
    matcher: GlobMatcher,
}

impl CompiledRule {
    pub fn is_match(&self, path: &str) -> bool {
        self.matcher.is_match(path)
    }
}

#[derive(Debug, Clone)]
pub struct PatternList {
    patterns: Vec<PatternMatcher>,
}

impl PatternList {
    pub fn new(patterns: Vec<PatternMatcher>) -> Self {
        Self { patterns }
    }

    pub fn matches(&self, path: &str) -> Option<&str> {
        for pattern in &self.patterns {
            if pattern.matcher.is_match(path) {
                return Some(pattern.pattern.as_str());
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct PatternMatcher {
    pattern: String,
    matcher: GlobMatcher,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{}", format_toml_error(path, line_col, message))]
    Toml {
        path: PathBuf,
        message: String,
        line_col: Option<(usize, usize)>,
    },
    #[error("{}", format_unknown_key_error(path, key, line_col, suggestion))]
    UnknownKey {
        path: PathBuf,
        key: String,
        line_col: Option<(usize, usize)>,
        suggestion: Option<String>,
    },
    #[error("{} - invalid glob '{}': {}", path.display(), pattern, message)]
    Glob {
        path: PathBuf,
        pattern: String,
        message: String,
    },
}

fn format_toml_error(path: &Path, line_col: &Option<(usize, usize)>, message: &str) -> String {
    if let Some((line, col)) = line_col {
        format!("{}:{}:{} - {}", path.display(), line, col, message)
    } else {
        format!("{} - {}", path.display(), message)
    }
}

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

pub fn compile_config(
    origin: ConfigOrigin,
    root_dir: PathBuf,
    config: FenceConfig,
    source_path: Option<&Path>,
) -> Result<CompiledConfig, ConfigError> {
    let path_for_errors = source_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("<built-in defaults>"));

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

fn default_respect_gitignore() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn invalid_glob_reports_error() {
        let config = FenceConfig {
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
        let config = FenceConfig {
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
