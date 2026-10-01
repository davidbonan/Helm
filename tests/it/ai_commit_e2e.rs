//! Business E2E for AI commit-message generation: the full pipeline (real git
//! context → prompt → provider subprocess → parse) exercised with a **fake
//! binary** (shell script) — never a real AI CLI.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use helm::agents::{CommitMessageRequest, CommitMessageSettings};
use helm::ai::{self, AiError, AiRunner};

const SHELL: &str = "/bin/sh";

/// The request running `provider -p "$HELM_PROMPT"`, default prompt.
fn request_running(provider: &Path) -> CommitMessageRequest {
    CommitMessageSettings::default()
        .request_running(&format!("'{}' -p \"$HELM_PROMPT\"", provider.display()))
}

fn generate(provider: &Path, repo: &Path) -> Result<ai::CommitSuggestion, AiError> {
    ai::generate_in_shell(Path::new(SHELL), repo, &request_running(provider))
}

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

    let request = CommitMessageRequest {
        prompt: "Write in English.\n{changes}".to_owned(),
        ..request_running(&provider)
    };

    let suggestion = ai::generate_in_shell(Path::new(SHELL), repo.path(), &request).unwrap();

    assert_eq!(suggestion.subject, "Add main entry point");
    assert_eq!(suggestion.description, "Bootstrap the binary.");

    let prompt = fs::read_to_string(bin.path().join("prompt.txt")).unwrap();
    assert!(
        prompt.starts_with("Write in English.\nStaged files"),
        "the prompt of the preferences is the one sent:\n{prompt}"
    );
    assert!(
        prompt.contains("imperative subject"),
        "the reply format closes the prompt:\n{prompt}"
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
fn the_command_runs_as_typed_with_the_prompt_as_one_argument() {
    let repo = repo_with_staged_file();
    let bin = tempfile::tempdir().unwrap();
    let provider = argv_recording_provider(bin.path(), "Add main entry point");
    let request = CommitMessageSettings::default().request_running(&format!(
        "'{}' --model haiku -p \"$HELM_PROMPT\"",
        provider.display()
    ));

    ai::generate_in_shell(Path::new(SHELL), repo.path(), &request).unwrap();

    let argv = fs::read_to_string(bin.path().join("argv.txt")).unwrap();
    let args: Vec<&str> = argv.split('\0').filter(|s| !s.is_empty()).collect();
    assert_eq!(&args[..3], ["--model", "haiku", "-p"]);
    assert_eq!(
        args.len(),
        4,
        "the multi-line prompt stays a single argument"
    );
}

#[test]
fn what_the_shell_prints_at_startup_stays_out_of_the_message() {
    let repo = repo_with_staged_file();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "Add main entry point");
    let noisy_shell = bin.path().join("noisy-sh");
    fs::write(
        &noisy_shell,
        "#!/bin/sh\necho 'Welcome back'\nexec /bin/sh \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&noisy_shell, fs::Permissions::from_mode(0o755)).unwrap();

    let suggestion =
        ai::generate_in_shell(&noisy_shell, repo.path(), &request_running(&provider)).unwrap();

    assert_eq!(suggestion.subject, "Add main entry point");
    assert_eq!(suggestion.description, "");
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

    let err = generate(&provider, repo.path()).unwrap_err();

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

    let err = generate(&provider, tmp.path()).unwrap_err();

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

    let err = generate(&provider, repo.path()).unwrap_err();

    assert!(
        matches!(&err, AiError::Failed(detail) if detail.contains("quota exceeded")),
        "{err:?}"
    );
    assert!(err.message().contains("quota exceeded"));
}

#[test]
fn a_command_the_shell_cannot_find_is_a_clear_error() {
    let repo = repo_with_staged_file();

    let err = generate(Path::new("/nonexistent/helm-ai"), repo.path()).unwrap_err();

    assert!(
        matches!(&err, AiError::Failed(detail) if detail.contains("/nonexistent/helm-ai")),
        "{err:?}"
    );
}

#[test]
fn the_runner_is_busy_until_drained_and_accepts_the_next_request() {
    let repo = repo_with_staged_file();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "Add main entry point");
    let mut runner = AiRunner::new(repo.path(), || {});
    assert!(!runner.busy());

    assert!(runner.request_in_shell(PathBuf::from(SHELL), request_running(&provider)));
    assert!(runner.busy());
    assert!(
        !runner.request_in_shell(PathBuf::from(SHELL), request_running(&provider)),
        "a second request is ignored while one is in flight"
    );

    let reply = runner.recv().unwrap().unwrap();
    assert_eq!(reply.subject, "Add main entry point");
    assert!(!runner.busy());

    assert!(runner.request_in_shell(PathBuf::from(SHELL), request_running(&provider)));
    assert!(runner.recv().unwrap().is_ok());
}
