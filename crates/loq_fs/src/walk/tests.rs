use super::*;
use loq_core::config::{compile_config, LoqConfig};
use tempfile::TempDir;

#[cfg(unix)]
struct PermissionGuard {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl Drop for PermissionGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;

        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode));
    }
}

fn empty_exclude() -> loq_core::PatternList {
    let config = LoqConfig {
        exclude: vec![],
        ..LoqConfig::default()
    };
    let compiled = compile_config(PathBuf::from("."), config, None).unwrap();
    compiled.exclude_patterns().clone()
}

fn exclude_pattern(pattern: &str) -> loq_core::PatternList {
    let config = LoqConfig {
        exclude: vec![pattern.to_string()],
        ..LoqConfig::default()
    };
    let compiled = compile_config(PathBuf::from("."), config, None).unwrap();
    compiled.exclude_patterns().clone()
}

#[test]
fn expands_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("a.txt"), "a").unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/b.txt"), "b").unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);
    assert_eq!(result.paths.len(), 2);
}

#[test]
fn expands_file_and_missing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("a.txt");
    std::fs::write(&file, "a").unwrap();
    let missing = root.join("missing.txt");

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(&[file, missing], &options);
    assert_eq!(result.paths.len(), 2);
    assert!(result.paths.iter().any(|path| path.ends_with("a.txt")));
    assert!(result
        .paths
        .iter()
        .any(|path| path.ends_with("missing.txt")));
}

#[test]
fn respects_gitignore_when_enabled() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/.gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(root.join("sub/ignored.txt"), "ignored").unwrap();
    std::fs::write(root.join("sub/included.txt"), "included").unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: true,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(&[root.join("sub")], &options);
    // Should have .gitignore and included.txt (ignored.txt is excluded)
    assert_eq!(result.paths.len(), 2);
    assert!(result
        .paths
        .iter()
        .any(|path| path.ends_with("included.txt")));
    assert!(!result
        .paths
        .iter()
        .any(|path| path.ends_with("ignored.txt")));
}

#[test]
fn includes_gitignored_when_disabled() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/.gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(root.join("sub/ignored.txt"), "ignored").unwrap();
    std::fs::write(root.join("sub/included.txt"), "included").unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(&[root.join("sub")], &options);
    // Should have all 3: .gitignore, ignored.txt, included.txt
    assert_eq!(result.paths.len(), 3);
    assert!(result
        .paths
        .iter()
        .any(|path| path.ends_with("ignored.txt")));
}

#[test]
fn exclude_pattern_filters_walked_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("keep.rs"), "keep").unwrap();
    std::fs::write(root.join("skip.txt"), "skip").unwrap();

    let exclude = exclude_pattern("**/*.txt");
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);
    assert_eq!(result.paths.len(), 1);
    assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
    assert!(!result.paths.iter().any(|p| p.ends_with("skip.txt")));
}

#[test]
fn exclude_pattern_filters_explicit_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let keep = root.join("keep.rs");
    let skip = root.join("skip.txt");
    std::fs::write(&keep, "keep").unwrap();
    std::fs::write(&skip, "skip").unwrap();

    let exclude = exclude_pattern("**/*.txt");
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(&[keep, skip], &options);
    assert_eq!(result.paths.len(), 1);
    assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
}

#[cfg(unix)]
#[test]
fn walk_errors_are_reported_for_unreadable_dirs() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let blocked = root.join("blocked");
    std::fs::create_dir(&blocked).unwrap();

    let original_mode = std::fs::metadata(&blocked).unwrap().permissions().mode();
    let _guard = PermissionGuard {
        path: blocked.clone(),
        mode: original_mode,
    };

    let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&blocked, perms).unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);

    assert!(
        !result.errors.is_empty(),
        "expected walk errors for unreadable directory"
    );
}

#[test]
fn exclude_dotdir_pattern_without_leading_globstar() {
    // Regression test: `.git/**` should exclude .git directory contents
    // Previously failed when walker returned paths with "./" prefix
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".git/logs")).unwrap();
    std::fs::write(root.join(".git/logs/HEAD"), "ref").unwrap();
    std::fs::write(root.join("keep.rs"), "keep").unwrap();

    let exclude = exclude_pattern(".git/**");
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);
    assert_eq!(result.paths.len(), 1, "got: {:?}", result.paths);
    assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
    assert!(!result
        .paths
        .iter()
        .any(|p| p.to_string_lossy().contains(".git")));
}

#[cfg(unix)]
#[test]
fn symlink_to_file_not_followed_by_default() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("real.txt"), "content").unwrap();
    symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);

    // Real file is included
    assert!(result.paths.iter().any(|p| p.ends_with("real.txt")));
    // Symlink is NOT followed by default (ignore crate behavior)
    assert!(!result.paths.iter().any(|p| p.ends_with("link.txt")));
}

#[cfg(unix)]
#[test]
fn symlink_to_parent_dir_does_not_loop() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/file.txt"), "content").unwrap();
    // Create symlink pointing back to parent - could cause infinite loop
    symlink(&root, root.join("sub/parent_link")).unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    // This should complete without hanging (ignore crate doesn't follow dir symlinks)
    let result = expand_paths(std::slice::from_ref(&root), &options);

    // Should find the file but not loop infinitely
    assert!(result.paths.iter().any(|p| p.ends_with("file.txt")));
    // The symlink itself is not a file, so it won't appear in paths
}

#[test]
fn hardcoded_excludes_filter_loq_cache_dir() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("keep.rs"), "keep").unwrap();
    std::fs::create_dir(root.join(".loq_cache")).unwrap();
    std::fs::write(root.join(".loq_cache/cached.txt"), "cached").unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);
    assert_eq!(result.paths.len(), 1);
    assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
    assert!(!result
        .paths
        .iter()
        .any(|p| p.to_string_lossy().contains(".loq_cache")));
}

#[test]
fn hardcoded_excludes_filter_loq_toml() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("keep.rs"), "keep").unwrap();
    std::fs::write(root.join("loq.toml"), "[config]").unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    let result = expand_paths(std::slice::from_ref(&root), &options);
    assert_eq!(result.paths.len(), 1);
    assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
    assert!(!result.paths.iter().any(|p| p.ends_with("loq.toml")));
}

#[test]
fn hardcoded_excludes_filter_explicit_loq_toml() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let keep = root.join("keep.rs");
    let loq_toml = root.join("loq.toml");
    std::fs::write(&keep, "keep").unwrap();
    std::fs::write(&loq_toml, "[config]").unwrap();

    let exclude = empty_exclude();
    let options = WalkOptions {
        respect_gitignore: false,
        exclude: &exclude,
        cwd: &root,
        root_dir: &root,
    };
    // Pass loq.toml explicitly - should still be filtered
    let result = expand_paths(&[keep, loq_toml], &options);
    assert_eq!(result.paths.len(), 1);
    assert!(result.paths.iter().any(|p| p.ends_with("keep.rs")));
}
