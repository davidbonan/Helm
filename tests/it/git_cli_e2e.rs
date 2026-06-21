use std::path::Path;

use helm::git::cli::{self, CliError};

#[test]
fn git_version_reports_success() {
    let tmp = tempfile::tempdir().unwrap();

    let out = cli::run(tmp.path(), &["version"]).unwrap();

    assert!(out.success());
    assert!(out.stdout.starts_with("git version"));
    assert!(out.stderr.is_empty());
}

#[test]
fn failing_command_captures_stderr_and_exit_code() {
    let tmp = tempfile::tempdir().unwrap();

    let out = cli::run(tmp.path(), &["status"]).unwrap();

    assert!(!out.success());
    assert_eq!(out.code, Some(128));
    assert!(out.stderr.contains("not a git repository"));
}

#[test]
fn missing_binary_is_a_clear_error() {
    let tmp = tempfile::tempdir().unwrap();

    let err =
        cli::run_program(Path::new("/nonexistent/helm-git"), tmp.path(), &["version"]).unwrap_err();

    assert!(matches!(err, CliError::NotFound));
}

#[test]
fn command_runs_in_the_given_workdir() {
    let repo_dir = tempfile::tempdir().unwrap();
    git2::Repository::init(repo_dir.path()).unwrap();
    let plain_dir = tempfile::tempdir().unwrap();

    let inside = cli::run(repo_dir.path(), &["rev-parse", "--is-inside-work-tree"]).unwrap();
    let outside = cli::run(plain_dir.path(), &["rev-parse", "--is-inside-work-tree"]).unwrap();

    assert_eq!(inside.stdout.trim(), "true");
    assert!(!outside.success());
}

#[test]
fn subprocess_is_non_interactive() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut cfg = repo.config().unwrap();
    // printenv exits with an error if the variable is absent: a real signal.
    cfg.set_str("alias.prompt-probe", "!printenv GIT_TERMINAL_PROMPT")
        .unwrap();
    // read would block on an interactive stdin; immediate EOF ⇒ fallback branch.
    cfg.set_str("alias.stdin-probe", "!read -r line || echo stdin-closed")
        .unwrap();

    let prompt = cli::run(tmp.path(), &["prompt-probe"]).unwrap();
    assert!(prompt.success());
    assert_eq!(prompt.stdout.trim(), "0");

    let stdin = cli::run(tmp.path(), &["stdin-probe"]).unwrap();
    assert!(stdin.success());
    assert_eq!(stdin.stdout.trim(), "stdin-closed");
}

#[test]
fn subprocess_locale_is_pinned_to_c() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("alias.locale-probe", "!printenv LC_ALL")
        .unwrap();

    let out = cli::run(tmp.path(), &["locale-probe"]).unwrap();

    assert!(out.success());
    assert_eq!(out.stdout.trim(), "C");
}
