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
    let output = Command::new("git")
        .current_dir(dir.path())
        .args(args)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn init_git_repo(dir: &TempDir) {
    run_git(dir, &["init"]);
    run_git(dir, &["config", "user.name", "Loq Test"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
}
