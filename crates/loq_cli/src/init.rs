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

    let _ = std::io::Write::flush(stdout);
    ExitStatus::Success
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
    };

    let output =
        loq_fs::run_check(vec![cwd.to_path_buf()], options).context("baseline check failed")?;

    let mut exempt = Vec::new();
    for outcome in output.outcomes {
        if let loq_core::OutcomeKind::Violation {
            severity: loq_core::Severity::Error,
            ..
        } = outcome.kind
        {
            let mut path = outcome.display_path.replace('\\', "/");
            if path.starts_with("./") {
                path = path.trim_start_matches("./").to_string();
            }
            exempt.push(path);
        }
    }

    exempt.sort();
    exempt.dedup();

    Ok(default_config_text(&exempt))
}

fn default_config_text(exempt: &[String]) -> String {
    let mut out = String::new();
    let exclude = loq_core::LoqConfig::init_template().exclude;

    writeln!(out, "default_max_lines = 500").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "respect_gitignore = true").unwrap();
    writeln!(out).unwrap();

    write_toml_array(&mut out, "exclude", &exclude);
    write_toml_array(&mut out, "exempt", exempt);

    writeln!(
        out,
        "# Last match wins. Put general rules first and overrides later."
    )
    .unwrap();
    writeln!(out, "[[rules]]").unwrap();
    writeln!(out, "path = \"**/*.tsx\"").unwrap();
    writeln!(out, "max_lines = 300").unwrap();
    writeln!(out, "severity = \"warning\"").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "[[rules]]").unwrap();
    writeln!(out, "path = \"tests/**/*\"").unwrap();
    write!(out, "max_lines = 500").unwrap();
    out
}

fn write_toml_array(out: &mut String, name: &str, items: &[String]) {
    if items.is_empty() {
        writeln!(out, "{name} = []").unwrap();
    } else {
        writeln!(out, "{name} = [").unwrap();
        for item in items {
            writeln!(out, "  \"{item}\",").unwrap();
        }
        writeln!(out, "]").unwrap();
    }
    writeln!(out).unwrap();
}
