//! Business E2E for AI commit-message generation: the full pipeline (real git
//! context → prompt → provider subprocess → parse) exercised with a **fake
//! binary** (shell script) — never a real AI CLI.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use helm::ai::{self, AiError, AiRunner};

/// Fake provider: captures the received prompt (`-p <prompt>`) into `prompt.txt`
/// then replies `reply` on stdout.
fn fake_provider(dir: &Path, reply: &str) -> PathBuf {
    let path = dir.join("fake-ai");
    let capture = dir.join("prompt.txt");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s' \"$2\" > '{}'\nprintf '%s' '{reply}'\n",
            capture.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Fake provider: records its full argv (NUL-separated, since the prompt arg is
/// itself multi-line) into `argv.txt` then replies `reply` on stdout.
fn argv_recording_provider(dir: &Path, reply: &str) -> PathBuf {
    let path = dir.join("fake-ai-argv");
    let capture = dir.join("argv.txt");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done > '{}'\nprintf '%s' '{reply}'\n",
            capture.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn failing_provider(dir: &Path) -> PathBuf {
    let path = dir.join("fake-ai-fail");
    fs::write(&path, "#!/bin/sh\necho 'quota exceeded' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Real repo with a staged file (`git2`, like the other business E2Es).
fn repo_with_staged_file() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("main.rs")).unwrap();
    index.write().unwrap();
    tmp
}

#[test]
fn generation_feeds_only_the_staged_diff_and_parses_the_reply() {
    let repo = repo_with_staged_file();
    // Noise outside the index: untracked + worktree modification after staging
    // — neither must reach the prompt.
    fs::write(repo.path().join("notes.txt"), "scratch notes\n").unwrap();
    fs::write(repo.path().join("main.rs"), "fn main() { dirty() }\n").unwrap();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "Add main entry point\n\nBootstrap the binary.");

    let suggestion = ai::generate_with(&provider, &[], repo.path(), "Write in English.").unwrap();

    assert_eq!(suggestion.subject, "Add main entry point");
    assert_eq!(suggestion.description, "Bootstrap the binary.");

    let prompt = fs::read_to_string(bin.path().join("prompt.txt")).unwrap();
    assert!(
        prompt.contains("Additional instructions:\nWrite in English."),
        "the preference instructions reach the prompt:\n{prompt}"
    );
    assert!(
        prompt.contains("Staged diff") && prompt.contains("+fn main() {}"),
        "the staged diff reaches the prompt:\n{prompt}"
    );
    assert!(
        prompt.contains("Staged files") && prompt.contains("main.rs"),
        "the staged file list reaches the prompt:\n{prompt}"
    );
    assert!(
        !prompt.contains("notes.txt") && !prompt.contains("dirty()"),
        "untracked and worktree changes must stay out of the prompt:\n{prompt}"
    );
}

#[test]
fn the_model_flags_precede_the_prompt_in_the_invocation() {
    let repo = repo_with_staged_file();
    let bin = tempfile::tempdir().unwrap();
    let provider = argv_recording_provider(bin.path(), "Add main entry point");

    ai::generate_with(&provider, &["--model", "haiku"], repo.path(), "").unwrap();

    let argv = fs::read_to_string(bin.path().join("argv.txt")).unwrap();
    let args: Vec<&str> = argv.split('\0').filter(|s| !s.is_empty()).collect();
    assert_eq!(
        &args[..3],
        ["--model", "haiku", "-p"],
        "model flags precede `-p <prompt>`"
    );
    assert_eq!(args.len(), 4, "the prompt follows as a single argument");
}

#[test]
fn unstaged_changes_alone_refuse_to_generate() {
    let repo = repo_with_staged_file();
    // Commit then an unstaged modification: nothing left in the index.
    {
        let repo = git2::Repository::open(repo.path()).unwrap();
        let sig = git2::Signature::now("t", "t@t").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    fs::write(repo.path().join("main.rs"), "fn main() { run() }\n").unwrap();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "never called");

    let err = ai::generate_with(&provider, &[], repo.path(), "").unwrap_err();

    assert_eq!(err, AiError::NoChanges);
    assert!(
        !bin.path().join("prompt.txt").exists(),
        "the prompt only covers staged changes — the provider must not be invoked"
    );
}

#[test]
fn a_clean_tree_refuses_to_generate() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "never called");

    let err = ai::generate_with(&provider, &[], tmp.path(), "").unwrap_err();

    assert_eq!(err, AiError::NoChanges);
    assert!(
        !bin.path().join("prompt.txt").exists(),
        "the provider must not be invoked on a clean tree"
    );
}

#[test]
fn a_failing_provider_surfaces_its_stderr() {
    let repo = repo_with_staged_file();
    let bin = tempfile::tempdir().unwrap();
    let provider = failing_provider(bin.path());

    let err = ai::generate_with(&provider, &[], repo.path(), "").unwrap_err();

    assert_eq!(err, AiError::Failed("quota exceeded".to_owned()));
    assert!(err.message().contains("quota exceeded"));
}

#[test]
fn a_missing_provider_binary_is_a_clear_error() {
    let repo = repo_with_staged_file();

    let err =
        ai::generate_with(Path::new("/nonexistent/helm-ai"), &[], repo.path(), "").unwrap_err();

    assert_eq!(err, AiError::NotFound("/nonexistent/helm-ai".to_owned()));
    assert!(err.message().contains("Preferences"));
}

#[test]
fn the_runner_is_busy_until_drained_and_accepts_the_next_request() {
    let repo = repo_with_staged_file();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "Add main entry point");
    let mut runner = AiRunner::new(repo.path(), || {});
    assert!(!runner.busy());

    assert!(runner.request_program(provider.clone(), &[], String::new()));
    assert!(runner.busy());
    assert!(
        !runner.request_program(provider.clone(), &[], String::new()),
        "a second request is ignored while one is in flight"
    );

    let reply = runner.recv().unwrap().unwrap();
    assert_eq!(reply.subject, "Add main entry point");
    assert!(!runner.busy());

    assert!(runner.request_program(provider, &[], String::new()));
    assert!(runner.recv().unwrap().is_ok());
}
