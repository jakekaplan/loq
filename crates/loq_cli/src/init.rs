//! Init command implementation.

use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, Result};
use loq_fs::CheckOptions;
use tempfile::NamedTempFile;
use termcolor::WriteColor;

use crate::cli::InitArgs;
use crate::output::print_error;
use crate::ExitStatus;

/// A file that violates the default limit, captured for baseline.
struct BaselineEntry {
    path: String,
    lines: usize,
}

pub fn run_init<W1: WriteColor, W2: WriteColor>(
    args: &InitArgs,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitStatus {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join("loq.toml");
    if path.exists() {
        return print_error(stderr, "loq.toml already exists");
    }

    let content = if args.baseline {
        match baseline_config(&cwd) {
            Ok(content) => content,
            Err(err) => return print_error(stderr, &format!("{err:#}")),
        }
    } else {
        default_config_text(&[])
    };

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

fn baseline_config(cwd: &Path) -> Result<String> {
    let template = default_config_text(&[]);
    let mut temp_file =
        NamedTempFile::new_in(cwd).context("failed to create baseline temp file")?;
    std::io::Write::write_all(&mut temp_file, template.as_bytes())
        .context("failed to write baseline config")?;

    let options = CheckOptions {
        config_path: Some(temp_file.path().to_path_buf()),
        cwd: cwd.to_path_buf(),
        use_cache: false,
    };

    let output =
        loq_fs::run_check(vec![cwd.to_path_buf()], options).context("baseline check failed")?;

    let mut entries = Vec::new();
    for outcome in output.outcomes {
        if let loq_core::OutcomeKind::Violation {
            severity: loq_core::Severity::Error,
            actual,
            ..
        } = outcome.kind
        {
            let mut path = outcome.display_path.replace('\\', "/");
            if path.starts_with("./") {
                path = path.trim_start_matches("./").to_string();
            }
            entries.push(BaselineEntry {
                path,
                lines: actual,
            });
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);

    Ok(default_config_text(&entries))
}

fn default_config_text(baseline: &[BaselineEntry]) -> String {
    let mut out = String::new();

    writeln!(out, "default_max_lines = 500").unwrap();
    writeln!(out, "respect_gitignore = true").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "# Paths, files, or glob patterns to exclude").unwrap();
    writeln!(out, "exclude = []").unwrap();

    if baseline.is_empty() {
        // Show commented-out example rules
        writeln!(out).unwrap();
        writeln!(
            out,
            "# Rules override defaults for specific paths. Last match wins."
        )
        .unwrap();
        writeln!(out, "# [[rules]]").unwrap();
        writeln!(out, "# path = \"**/*.ext\"").unwrap();
        writeln!(out, "# severity = \"warning\"").unwrap();
        writeln!(out, "#").unwrap();
        writeln!(out, "# [[rules]]").unwrap();
        writeln!(out, "# path = \"some/path/**/*\"").unwrap();
        write!(out, "# max_lines = 1000").unwrap();
    } else {
        // Baseline rules: one per file, locked at current line count
        writeln!(out).unwrap();
        writeln!(
            out,
            "# Baseline: files locked at current size (any growth is an error)"
        )
        .unwrap();
        for entry in baseline {
            writeln!(out).unwrap();
            writeln!(out, "[[rules]]").unwrap();
            writeln!(out, "path = \"{}\"", entry.path).unwrap();
            write!(out, "max_lines = {}", entry.lines).unwrap();
        }
    }

    out
}
