use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::*;
use crate::cli::OutputFormat;

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("fail"))
    }
}

#[test]
fn collect_inputs_reports_stdin_error() {
    let err = collect_inputs(vec![], true, &mut FailingReader, Path::new("."), None).unwrap_err();
    assert!(err.to_string().contains("failed to read stdin"));
}

#[test]
fn collect_inputs_empty_defaults_to_cwd() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(vec![], false, &mut empty_stdin, Path::new("/repo"), None).unwrap();
    assert_eq!(result, vec![PathBuf::from(".")]);
}

#[test]
fn collect_inputs_stdin_only_no_default() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(vec![], true, &mut empty_stdin, Path::new("/repo"), None).unwrap();
    assert!(result.is_empty());
}

#[test]
fn collect_inputs_stdin_with_paths() {
    let mut stdin: &[u8] = b"file1.rs\nfile2.rs\n";
    let result = collect_inputs(vec![], true, &mut stdin, Path::new("/repo"), None).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], PathBuf::from("/repo/file1.rs"));
    assert_eq!(result[1], PathBuf::from("/repo/file2.rs"));
}

#[test]
fn collect_inputs_mixed_paths_and_stdin() {
    let mut stdin: &[u8] = b"from_stdin.rs\n";
    let result = collect_inputs(
        vec![PathBuf::from("explicit.rs")],
        true,
        &mut stdin,
        Path::new("/repo"),
        None,
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&PathBuf::from("explicit.rs")));
    assert!(result.contains(&PathBuf::from("/repo/from_stdin.rs")));
}

#[test]
fn collect_inputs_uses_git_paths_when_no_path_filters() {
    let mut empty_stdin: &[u8] = b"";
    let git_paths = vec![PathBuf::from("/repo/src/a.rs")];
    let result = collect_inputs(
        vec![],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(git_paths.clone()),
    )
    .unwrap();
    assert_eq!(result, git_paths);
}

#[test]
fn collect_inputs_intersects_git_paths_with_selected_paths() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(
        vec![PathBuf::from("src")],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(vec![
            PathBuf::from("/repo/src/a.rs"),
            PathBuf::from("/repo/lib/b.rs"),
        ]),
    )
    .unwrap();

    assert_eq!(result, vec![PathBuf::from("/repo/src/a.rs")]);
}

#[test]
fn collect_inputs_intersects_git_paths_with_absolute_selected_path() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(
        vec![PathBuf::from("/repo/src")],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(vec![
            PathBuf::from("/repo/src/a.rs"),
            PathBuf::from("/repo/lib/b.rs"),
        ]),
    )
    .unwrap();

    assert_eq!(result, vec![PathBuf::from("/repo/src/a.rs")]);
}

#[test]
fn collect_inputs_git_intersection_can_be_empty() {
    let mut empty_stdin: &[u8] = b"";
    let result = collect_inputs(
        vec![PathBuf::from("src")],
        false,
        &mut empty_stdin,
        Path::new("/repo"),
        Some(vec![PathBuf::from("/repo/lib/b.rs")]),
    )
    .unwrap();

    assert!(result.is_empty());
}

#[test]
fn git_error_message_for_not_repository() {
    let message = git_error_message(&JsonFilter::Staged, git::GitError::NotRepository);
    assert_eq!(message, "--staged requires a git repository");
}

#[test]
fn git_error_message_for_git_not_available() {
    let message = git_error_message(&JsonFilter::Staged, git::GitError::GitNotAvailable);
    assert_eq!(message, "--staged requires git, but git is not available");
}

#[test]
fn git_error_message_for_io_error() {
    let error = std::io::Error::other("boom");
    let message = git_error_message(
        &JsonFilter::Diff {
            git_ref: "main".into(),
        },
        git::GitError::Io(error),
    );
    assert_eq!(message, "git failed: boom");
}

#[test]
fn git_error_message_for_command_failure() {
    let message = git_error_message(
        &JsonFilter::Diff {
            git_ref: "main".into(),
        },
        git::GitError::CommandFailed {
            stderr: "bad revision".into(),
        },
    );

    assert_eq!(message, "git failed: bad revision");
}

#[test]
fn to_git_filter_maps_staged() {
    let filter = to_git_filter(&JsonFilter::Staged);
    assert_eq!(filter, git::GitFilter::Staged);
}

#[test]
fn to_git_filter_maps_diff_ref() {
    let filter = to_git_filter(&JsonFilter::Diff {
        git_ref: "main".to_string(),
    });
    assert_eq!(
        filter,
        git::GitFilter::Diff {
            git_ref: "main".to_string(),
        }
    );
}

#[test]
fn check_filter_prefers_staged() {
    let args = CheckArgs {
        paths: vec![],
        stdin: false,
        no_cache: false,
        staged: true,
        diff_ref: Some("main".to_string()),
        output_format: OutputFormat::Text,
    };

    assert_eq!(check_filter(&args), Some(JsonFilter::Staged));
}

#[test]
fn check_filter_uses_diff_ref() {
    let args = CheckArgs {
        paths: vec![],
        stdin: false,
        no_cache: false,
        staged: false,
        diff_ref: Some("main".to_string()),
        output_format: OutputFormat::Text,
    };

    assert_eq!(
        check_filter(&args),
        Some(JsonFilter::Diff {
            git_ref: "main".to_string(),
        })
    );
}

#[test]
fn plan_inputs_rejects_stdin_with_git_filter() {
    let args = CheckArgs {
        paths: vec![],
        stdin: true,
        no_cache: false,
        staged: true,
        diff_ref: None,
        output_format: OutputFormat::Text,
    };
    let mut stdin: &[u8] = b"a.rs\n";

    let result = plan_inputs(&args, &mut stdin, Path::new("/repo"));
    let err = match result {
        Ok(_) => panic!("expected stdin/filter conflict"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("cannot combine '-'"));
}

#[test]
fn normalize_components_handles_current_and_parent_segments() {
    let path = Path::new("./src/nested/../file.rs");
    let normalized = normalize_components(path);
    assert_eq!(normalized, PathBuf::from("src/file.rs"));
}
