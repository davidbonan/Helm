//! Business e2e for `notify::notify_with` (specs/agents.md): a real subprocess
//! stands in for macOS `osascript`, capturing its argv so we can assert the
//! `display notification` script handed to it — no banner posted, no fake runner.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use helm::notify::{completion_message, notify_with, NotifyError};

/// Writes an executable shell script standing in for `osascript`: it dumps each
/// arg on its own line to `capture`, then exits with `code`.
fn fake_osascript(dir: &Path, capture: &Path, code: i32) -> PathBuf {
    let path = dir.join("osascript");
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

#[test]
fn notify_hands_osascript_the_display_notification_script() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let osascript = fake_osascript(tmp.path(), &capture, 0);

    let (title, body) = completion_message("claude", "helm", Some("main"));
    notify_with(&osascript, &title, &body).unwrap();

    let argv = std::fs::read_to_string(&capture).unwrap();
    let mut lines = argv.lines();
    assert_eq!(lines.next(), Some("-e"));
    assert_eq!(
        lines.next(),
        Some("display notification \"helm · main\" with title \"Claude finished\""),
    );
}

#[test]
fn a_non_zero_osascript_exit_is_a_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let osascript = fake_osascript(tmp.path(), &capture, 1);

    let result = notify_with(&osascript, "Claude finished", "helm");
    assert!(
        matches!(result, Err(NotifyError::Failed(_))),
        "expected Failed, got {result:?}"
    );
}

#[test]
fn a_missing_osascript_binary_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let result = notify_with(&tmp.path().join("no-osascript"), "t", "b");
    assert_eq!(result, Err(NotifyError::NotFound));
}
