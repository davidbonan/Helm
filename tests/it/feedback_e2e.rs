//! Business e2e for `feedback::open_with` (specs/feedback.md): a real
//! subprocess stands in for macOS `open`, capturing its argv to a file so we can
//! assert the GitHub `issues/new` URL it would hand to LaunchServices — no
//! browser launched, no fake runner.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use helm::feedback::{open_with, FeedbackError, FeedbackKind};

/// Writes an executable shell script standing in for `open`: it dumps each arg
/// on its own line to `capture`, then exits with `code`.
fn fake_open(dir: &Path, capture: &Path, code: i32) -> PathBuf {
    let path = dir.join("open");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexit {code}\n",
            capture.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn captured_url(capture: &Path) -> String {
    std::fs::read_to_string(capture).unwrap().trim().to_owned()
}

#[test]
fn open_hands_an_issue_url_with_the_title_body_and_label() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let open = fake_open(tmp.path(), &capture, 0);

    open_with(
        &open,
        "davidbonan/Helm",
        FeedbackKind::Bug,
        "the split focus is lost on resize",
        "helm 0.2.0 · macOS 15.5",
    )
    .unwrap();

    let url = captured_url(&capture);
    assert!(
        url.starts_with("https://github.com/davidbonan/Helm/issues/new?"),
        "{url}"
    );
    assert!(
        url.contains("title=the%20split%20focus%20is%20lost%20on%20resize"),
        "{url}"
    );
    assert!(
        url.contains("body=the%20split%20focus%20is%20lost%20on%20resize"),
        "{url}"
    );
    assert!(url.contains("labels=bug"), "{url}");
    assert!(url.contains("helm%200.2.0"), "{url}");
}

#[test]
fn the_label_reflects_the_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let open = fake_open(tmp.path(), &capture, 0);

    open_with(
        &open,
        "davidbonan/Helm",
        FeedbackKind::Suggestion,
        "a dark-mode toggle would be nice",
        "meta",
    )
    .unwrap();

    assert!(
        captured_url(&capture).contains("labels=enhancement"),
        "{}",
        captured_url(&capture)
    );
}

#[test]
fn a_non_zero_open_exit_is_a_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let open = fake_open(tmp.path(), &capture, 1);

    let result = open_with(&open, "davidbonan/Helm", FeedbackKind::Bug, "boom", "meta");
    assert!(
        matches!(result, Err(FeedbackError::OpenFailed(_))),
        "expected OpenFailed, got {result:?}"
    );
}

#[test]
fn a_missing_open_binary_is_a_dedicated_error() {
    let tmp = tempfile::tempdir().unwrap();
    let result = open_with(
        &tmp.path().join("no-open-here"),
        "davidbonan/Helm",
        FeedbackKind::Bug,
        "boom",
        "meta",
    );
    assert_eq!(result, Err(FeedbackError::OpenNotFound));
}
