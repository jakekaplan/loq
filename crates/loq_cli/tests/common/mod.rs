use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

pub fn write_file(dir: &TempDir, path: &str, contents: &str) {
    let full = dir.path().join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(full, contents).unwrap();
}

pub fn run_git(dir: &TempDir, args: &[&str]) {
    run_git_in_dir(dir.path(), args);
}

pub fn run_git_in_dir(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(dead_code)]
pub fn git_head(dir: &Path) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git rev-parse HEAD failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub fn init_git_repo(dir: &TempDir) {
    run_git(dir, &["init"]);
    run_git(dir, &["config", "user.name", "Loq Test"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
}
