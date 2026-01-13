//! Init command implementation.

use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use termcolor::WriteColor;

use crate::cli::InitArgs;
use crate::output::print_error;
use crate::ExitStatus;

pub fn run_init<W1: WriteColor, W2: WriteColor>(
    _args: &InitArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join("loq.toml");
    if path.exists() {
        return print_error(stderr, "loq.toml already exists");
    }

    let content = default_config_text();

    if let Err(err) = std::fs::write(&path, content) {
        return print_error(stderr, &format!("failed to write loq.toml: {err}"));
    }

    // Add .loq_cache to .gitignore if not already present
    add_to_gitignore(&cwd);

    let _ = std::io::Write::flush(stdout);
    ExitStatus::Success
}

/// Adds `.loq_cache` to `.gitignore` if the file exists and doesn't already contain it.
fn add_to_gitignore(cwd: &Path) {
    let gitignore_path = cwd.join(".gitignore");

    // Only modify existing .gitignore files
    if !gitignore_path.is_file() {
        return;
    }

    let Ok(contents) = std::fs::read_to_string(&gitignore_path) else {
        return;
    };

    // Check if already ignored
    if contents.lines().any(|line| line.trim() == ".loq_cache") {
        return;
    }

    // Append .loq_cache
    let new_contents = if contents.ends_with('\n') || contents.is_empty() {
        format!("{contents}.loq_cache\n")
    } else {
        format!("{contents}\n.loq_cache\n")
    };

    let _ = std::fs::write(&gitignore_path, new_contents);
}

fn default_config_text() -> String {
    let mut out = String::new();

    writeln!(out, "default_max_lines = 500").unwrap();
    writeln!(out, "respect_gitignore = true").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "# Paths, files, or glob patterns to exclude").unwrap();
    writeln!(out, "exclude = []").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "# Text to display when violations occur (useful when piping to LLMs)"
    )
    .unwrap();
    writeln!(out, "# fix_guidance = \"\"").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "# Rules override defaults for specific paths. Last match wins."
    )
    .unwrap();
    writeln!(out, "# [[rules]]").unwrap();
    writeln!(out, "# path = \"**/*.ext\"").unwrap();
    writeln!(out, "# max_lines = 300").unwrap();
    writeln!(out, "#").unwrap();
    writeln!(out, "# [[rules]]").unwrap();
    writeln!(out, "# path = \"some/path/**/*\"").unwrap();
    write!(out, "# max_lines = 1000").unwrap();

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_has_expected_values() {
        let text = default_config_text();
        assert!(text.contains("default_max_lines = 500"));
        assert!(text.contains("respect_gitignore = true"));
        assert!(text.contains("exclude = []"));
        assert!(text.contains("# [[rules]]"));
        assert!(text.contains("# max_lines = 1000"));
    }

    #[test]
    fn add_to_gitignore_creates_entry() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(".gitignore"), "node_modules\n").unwrap();

        add_to_gitignore(temp.path());

        let contents = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert!(contents.contains(".loq_cache"));
        assert!(contents.ends_with(".loq_cache\n"));
    }

    #[test]
    fn add_to_gitignore_skips_if_already_present() {
        let temp = TempDir::new().unwrap();
        let original = "node_modules\n.loq_cache\ntarget\n";
        std::fs::write(temp.path().join(".gitignore"), original).unwrap();

        add_to_gitignore(temp.path());

        let contents = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert_eq!(contents, original);
    }

    #[test]
    fn add_to_gitignore_handles_no_trailing_newline() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(".gitignore"), "node_modules").unwrap();

        add_to_gitignore(temp.path());

        let contents = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert_eq!(contents, "node_modules\n.loq_cache\n");
    }

    #[test]
    fn add_to_gitignore_skips_if_no_gitignore() {
        let temp = TempDir::new().unwrap();
        // No .gitignore file

        add_to_gitignore(temp.path());

        assert!(!temp.path().join(".gitignore").exists());
    }

    #[test]
    fn add_to_gitignore_handles_empty_file() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(".gitignore"), "").unwrap();

        add_to_gitignore(temp.path());

        let contents = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert_eq!(contents, ".loq_cache\n");
    }
}
