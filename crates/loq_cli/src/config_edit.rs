//! Shared helpers for editing `loq.toml` with `toml_edit`.

use std::path::Path;

use anyhow::{Context, Result};
use loq_core::config::{DEFAULT_MAX_LINES, DEFAULT_RESPECT_GITIGNORE};
use toml_edit::{DocumentMut, Item};

use crate::init::add_to_gitignore;

/// Create a default document for initializing `loq.toml`.
pub(crate) fn default_document() -> DocumentMut {
    let mut doc = DocumentMut::new();
    doc["default_max_lines"] = toml_edit::value(default_max_lines_i64());
    doc["respect_gitignore"] = toml_edit::value(DEFAULT_RESPECT_GITIGNORE);
    doc["exclude"] = Item::Value(toml_edit::Value::Array(toml_edit::Array::default()));
    doc
}

fn default_max_lines_i64() -> i64 {
    i64::try_from(DEFAULT_MAX_LINES).unwrap_or(i64::MAX)
}

pub(crate) fn load_doc_or_default(config_path: &Path) -> Result<(DocumentMut, bool)> {
    if config_path.exists() {
        let config_text = std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let doc = config_text
            .parse()
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        Ok((doc, true))
    } else {
        Ok((default_document(), false))
    }
}

pub(crate) fn write_doc(config_path: &Path, doc: &DocumentMut) -> Result<()> {
    std::fs::write(config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(())
}

pub(crate) fn persist_doc(
    cwd: &Path,
    config_path: &Path,
    doc: &DocumentMut,
    config_existed: bool,
) -> Result<()> {
    write_doc(config_path, doc)?;
    if !config_existed {
        add_to_gitignore(cwd);
    }
    Ok(())
}

pub(crate) fn threshold_from_doc(doc: &DocumentMut, explicit: Option<usize>) -> usize {
    explicit.unwrap_or_else(|| {
        doc.get("default_max_lines")
            .and_then(Item::as_integer)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(DEFAULT_MAX_LINES)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_document_has_expected_defaults() {
        let doc = default_document();
        assert_eq!(
            doc.get("default_max_lines").and_then(Item::as_integer),
            Some(default_max_lines_i64())
        );
        assert_eq!(
            doc.get("respect_gitignore").and_then(Item::as_bool),
            Some(DEFAULT_RESPECT_GITIGNORE)
        );
        let exclude = doc.get("exclude").and_then(Item::as_array);
        assert!(exclude.is_some());
        assert_eq!(exclude.unwrap().len(), 0);
    }
}
