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
            default_max_lines: Some(400),
            respect_gitignore: true,
            exclude: Vec::new(),
            exempt: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn init_template() -> Self {
        Self {
            default_max_lines: Some(400),
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

pub fn parse_config(path: &Path, text: &str) -> Result<FenceConfig, ConfigError> {
    let deserializer = toml::Deserializer::new(text);
    let mut unknown = Vec::new();
    let parsed: FenceConfig = serde_ignored::deserialize(deserializer, |path| {
        if let Some(key) = extract_key(&path) {
            unknown.push(key);
        }
    })
    .map_err(|err| ConfigError::Toml {
        path: path.to_path_buf(),
        message: err.to_string(),
        line_col: err
            .span()
            .and_then(|span| line_col_from_offset(text, span.start)),
    })?;

    if let Some(key) = unknown.into_iter().next() {
        let line_col = find_key_location(text, &key);
        let suggestion = suggest_key(&key);
        return Err(ConfigError::UnknownKey {
            path: path.to_path_buf(),
            key,
            line_col,
            suggestion,
        });
    }

    Ok(parsed)
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

fn extract_key(path: &serde_ignored::Path) -> Option<String> {
    let path_str = path.to_string();
    let mut last = path_str.split('.').next_back().unwrap_or(&path_str);
    if let Some(pos) = last.find('[') {
        last = &last[..pos];
    }
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

fn find_key_location(text: &str, key: &str) -> Option<(usize, usize)> {
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(key) {
            if rest.trim_start().starts_with('=') {
                let leading = line.len().saturating_sub(trimmed.len());
                return Some((line_idx + 1, leading + 1));
            }
        }
    }
    None
}

fn suggest_key(key: &str) -> Option<String> {
    let candidates = [
        "default_max_lines",
        "respect_gitignore",
        "exclude",
        "exempt",
        "rules",
        "path",
        "max_lines",
        "severity",
    ];
    let mut best = None;
    let mut best_score = usize::MAX;
    for candidate in candidates {
        let score = strsim::levenshtein(key, candidate);
        if score < best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    if best_score <= 3 {
        best.map(|s| s.to_string())
    } else {
        None
    }
}

fn line_col_from_offset(text: &str, offset: usize) -> Option<(usize, usize)> {
    if offset > text.len() {
        return None;
    }
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in text.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    Some((line, col))
}

fn default_respect_gitignore() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unknown_key_detection() {
        let text = "default_max_lines = 400\nmax_line = 10\n";
        let err = parse_config(Path::new(".fence.toml"), text).unwrap_err();
        match err {
            ConfigError::UnknownKey {
                key, suggestion, ..
            } => {
                assert_eq!(key, "max_line");
                assert_eq!(suggestion, Some("max_lines".to_string()));
            }
            _ => panic!("expected unknown key"),
        }
    }

    #[test]
    fn rule_severity_defaults_to_error() {
        let text = "default_max_lines = 400\n[[rules]]\npath = \"**/*.rs\"\nmax_lines = 10\n";
        let config = parse_config(Path::new(".fence.toml"), text).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].severity, Severity::Error);
    }

    #[test]
    fn respect_gitignore_defaults_true() {
        let text = "default_max_lines = 400\n";
        let config = parse_config(Path::new(".fence.toml"), text).unwrap();
        assert!(config.respect_gitignore);
    }

    #[test]
    fn invalid_toml_reports_error() {
        let text = "default_max_lines =\n";
        let err = parse_config(Path::new(".fence.toml"), text).unwrap_err();
        match err {
            ConfigError::Toml { .. } => {}
            _ => panic!("expected toml error"),
        }
    }

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
    fn unknown_key_without_location() {
        let text = "rules = [{ path = \"src/*.rs\", max_lines = 10, max_line = 20 }]\n";
        let err = parse_config(Path::new(".fence.toml"), text).unwrap_err();
        match err {
            ConfigError::UnknownKey { line_col, .. } => {
                assert!(line_col.is_none());
            }
            _ => panic!("expected unknown key"),
        }
    }

    #[test]
    fn unknown_key_without_suggestion() {
        let text = "banana = 1\n";
        let err = parse_config(Path::new(".fence.toml"), text).unwrap_err();
        match err {
            ConfigError::UnknownKey { suggestion, .. } => {
                assert!(suggestion.is_none());
            }
            _ => panic!("expected unknown key"),
        }
    }

    #[test]
    fn display_errors_are_stable() {
        let text = "default_max_lines =\n";
        let err = parse_config(Path::new(".fence.toml"), text).unwrap_err();
        assert!(err.to_string().contains(".fence.toml"));

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
}
