//! Edit commands discover the config upward instead of spawning a stray one.

use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

fn write_file(dir: &Path, path: &str, contents: &str) {
    let full = dir.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(full, contents).unwrap();
}

fn repeat_lines(count: usize) -> String {
    "line\n".repeat(count)
}

#[test]
fn baseline_from_subdir_edits_root_config() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("loq.toml"), "default_max_lines = 500\n").unwrap();
    write_file(root, "pkg/legacy.txt", &repeat_lines(501));

    cargo_bin_cmd!("loq")
        .current_dir(root.join("pkg"))
        .args(["baseline"])
        .assert()
        .success();

    assert!(
        !root.join("pkg/loq.toml").exists(),
        "baseline created a stray config in the subdirectory"
    );
    let config = std::fs::read_to_string(root.join("loq.toml")).unwrap();
    assert!(
        config.contains("\"pkg/legacy.txt\""),
        "config was: {config}"
    );
    assert!(config.contains("max_lines = 501"), "config was: {config}");
}

#[test]
fn relax_from_subdir_edits_root_config() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("loq.toml"), "default_max_lines = 500\n").unwrap();
    write_file(root, "pkg/legacy.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(root.join("pkg"))
        .args(["relax"])
        .assert()
        .success();

    assert!(
        !root.join("pkg/loq.toml").exists(),
        "relax created a stray config in the subdirectory"
    );
    let config = std::fs::read_to_string(root.join("loq.toml")).unwrap();
    assert!(
        config.contains("\"pkg/legacy.txt\""),
        "config was: {config}"
    );
    assert!(config.contains("max_lines = 600"), "config was: {config}");
}

#[test]
fn tighten_from_subdir_edits_root_config() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("loq.toml"),
        "default_max_lines = 500\n\n[[rules]]\npath = \"pkg/legacy.txt\"\nmax_lines = 900\n",
    )
    .unwrap();
    write_file(root, "pkg/legacy.txt", &repeat_lines(600));

    cargo_bin_cmd!("loq")
        .current_dir(root.join("pkg"))
        .args(["tighten"])
        .assert()
        .success();

    assert!(
        !root.join("pkg/loq.toml").exists(),
        "tighten created a stray config in the subdirectory"
    );
    let config = std::fs::read_to_string(root.join("loq.toml")).unwrap();
    assert!(config.contains("max_lines = 600"), "config was: {config}");
}
