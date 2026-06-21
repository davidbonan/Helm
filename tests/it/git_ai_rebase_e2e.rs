//! Business E2E for the AI rebase: the full pipeline (guards → prompt →
//! agentic provider subprocess → verified outcome) exercised with **fake
//! binaries** (shell scripts that really run `git rebase`) — never a real AI
//! CLI. Mirrors `ai_commit_e2e` for the provider seams and `git_sync_e2e` for
//! the repo fixtures.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use helm::git::ai_rebase::{self, AiRebaseError, AiRebaseOutcome, AiRebaseRequest, AiRebaseRunner};
use helm::git::cli;
use helm::git::rebase;
use helm::git::worker::{MutationLock, SyncCommand, SyncRunner};

fn set_test_config(repo: &git2::Repository) {
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    cfg.set_bool("commit.gpgsign", false).unwrap();
}

fn commit_file(
    repo: &git2::Repository,
    dir: &Path,
    name: &str,
    content: &str,
    message: &str,
) -> git2::Oid {
    fs::write(dir.join(name), content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = repo.signature().unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

// Local repo with two diverged branches: `feature` (checked out, one commit
// `c-feat`) and the initial branch ahead by one commit. Same shape as the
// plain-rebase fixture (git_sync_e2e).
fn rebase_fixture(conflicting: bool) -> (tempfile::TempDir, String, git2::Oid) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "base.txt", "base\n", "c1");
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    let onto = repo.head().unwrap().shorthand().unwrap().to_string();
    repo.branch("feature", &base, false).unwrap();
    let onto_tip = commit_file(&repo, tmp.path(), "base.txt", "from-main\n", "c2");
    repo.set_head("refs/heads/feature").unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let (name, content) = if conflicting {
        ("base.txt", "from-feature\n")
    } else {
        ("feat.txt", "f\n")
    };
    commit_file(&repo, tmp.path(), name, content, "c-feat");
    (tmp, onto, onto_tip)
}

/// The request the recap modal would hand to the runner: current branch +
/// the freshly derived plan oids.
fn request_for(workdir: &Path, onto: &str) -> AiRebaseRequest {
    let repo = git2::Repository::open(workdir).unwrap();
    let current = repo.head().unwrap().shorthand().unwrap().to_string();
    let expected = rebase::rebase_commits(&repo, onto)
        .unwrap()
        .iter()
        .map(|c| c.oid)
        .collect();
    AiRebaseRequest {
        current,
        onto: onto.to_string(),
        instructions: String::new(),
        expected,
    }
}

/// Fake agentic provider: captures the received prompt (`-p <prompt>`) into
/// `prompt.txt` then runs `body` in the repo (the subprocess cwd).
fn fake_provider(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-agent");
    let capture = dir.join("prompt.txt");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s' \"$2\" > '{}'\n{body}\n",
            capture.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_clean_rebase_reports_completed_and_replays_the_branch() {
    let (tmp, onto, onto_tip) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(
        bin.path(),
        &format!(
            "git rebase '{onto}' >/dev/null 2>&1\n\
             printf 'Rebased feature onto {onto}; no conflicts.'"
        ),
    );
    let mut request = request_for(tmp.path(), &onto);
    request.instructions = "Keep every commit as is.".to_string();

    let report = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Completed);
    assert_eq!(
        report.summary,
        format!("Rebased feature onto {onto}; no conflicts.")
    );

    // The outcome is verified on the repo, not believed from the report.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "c-feat");
    assert_eq!(head.parent(0).unwrap().id(), onto_tip);

    let prompt = fs::read_to_string(bin.path().join("prompt.txt")).unwrap();
    assert!(
        prompt.contains(&format!("rebase 'feature' onto '{onto}'")),
        "the task names both branches:\n{prompt}"
    );
    assert!(
        prompt.contains("NEVER push"),
        "the no-push contract:\n{prompt}"
    );
    assert!(prompt.contains("c-feat"), "the commit recap:\n{prompt}");
    assert!(
        prompt.contains("Additional instructions from the user:\nKeep every commit as is."),
        "the modal instructions reach the prompt:\n{prompt}"
    );
}

#[test]
fn a_conflict_resolved_by_the_provider_reports_completed() {
    let (tmp, onto, _) = rebase_fixture(true);
    let bin = tempfile::tempdir().unwrap();
    // The "agent": rebase, resolve the conflict in favor of a merged content,
    // continue — the loop a real provider would run.
    let provider = fake_provider(
        bin.path(),
        &format!(
            "git rebase '{onto}' >/dev/null 2>&1 || {{\n\
             printf 'resolved\\n' > base.txt\n\
             git add base.txt\n\
             GIT_EDITOR=true git rebase --continue >/dev/null 2>&1\n\
             }}\n\
             printf 'One conflict in base.txt, resolved by merging both intents.'"
        ),
    );
    let request = request_for(tmp.path(), &onto);

    let report = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Completed);
    assert!(report.summary.contains("resolved by merging both intents"));
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        "resolved\n"
    );
}

#[test]
fn an_unresolved_conflict_reports_left_in_progress() {
    let (tmp, onto, _) = rebase_fixture(true);
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(
        bin.path(),
        &format!(
            "git rebase '{onto}' >/dev/null 2>&1 || true\n\
             printf 'Conflict in base.txt: could not resolve safely, left for review.'"
        ),
    );
    let request = request_for(tmp.path(), &onto);

    let report = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap();

    // The lasting state wins over the provider's exit code: banner + report.
    assert_eq!(report.outcome, AiRebaseOutcome::LeftInProgress);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_ne!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn a_provider_that_does_nothing_reports_unchanged() {
    let (tmp, onto, _) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "printf 'Nothing was done.'");
    let request = request_for(tmp.path(), &onto);

    let report = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Unchanged);
}

#[test]
fn an_empty_reply_still_reports_the_verified_outcome() {
    let (tmp, onto, _) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), &format!("git rebase '{onto}' >/dev/null 2>&1"));
    let request = request_for(tmp.path(), &onto);

    let report = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Completed);
    assert_eq!(report.summary, "The provider returned no report.");
}

#[test]
fn a_failing_provider_surfaces_its_stderr() {
    let (tmp, onto, _) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "echo 'quota exceeded' >&2\nexit 1");
    let request = request_for(tmp.path(), &onto);

    let err = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap_err();

    assert_eq!(err, AiRebaseError::Failed("quota exceeded".to_string()));
    assert!(err.message().contains("AI rebase failed"));
}

#[test]
fn a_missing_provider_binary_is_a_clear_error() {
    let (tmp, onto, _) = rebase_fixture(false);
    let request = request_for(tmp.path(), &onto);

    let err = ai_rebase::run_with(Path::new("/nonexistent/helm-agent"), tmp.path(), &request)
        .unwrap_err();

    assert_eq!(
        err,
        AiRebaseError::NotFound("/nonexistent/helm-agent".to_string())
    );
    assert!(err.message().contains("Preferences"));
}

#[test]
fn a_dirty_tree_is_refused_before_invoking_the_provider() {
    let (tmp, onto, _) = rebase_fixture(false);
    fs::write(tmp.path().join("feat.txt"), "dirty\n").unwrap();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "printf 'never called'");
    let request = request_for(tmp.path(), &onto);

    let err = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap_err();

    assert!(err.message().contains("uncommitted changes"), "got {err:?}");
    assert!(
        !bin.path().join("prompt.txt").exists(),
        "the provider must never see the user's WIP"
    );
}

#[test]
fn an_untracked_file_alone_does_not_block_the_rebase() {
    let (tmp, onto, _) = rebase_fixture(false);
    fs::write(tmp.path().join("notes.txt"), "scratch\n").unwrap();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(
        bin.path(),
        &format!("git rebase '{onto}' >/dev/null 2>&1\nprintf 'Rebased.'"),
    );
    let request = request_for(tmp.path(), &onto);

    let report = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Completed);
}

#[test]
fn guards_refuse_bad_input_before_invoking_the_provider() {
    let (tmp, onto, _) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "printf 'never called'");

    // Stale recap: the branch grew a commit after the modal opened.
    let mut stale = request_for(tmp.path(), &onto);
    stale.expected.clear();
    let err = ai_rebase::run_with(&provider, tmp.path(), &stale).unwrap_err();
    assert!(err.message().contains("reopen AI rebase"), "got {err:?}");

    // Checked-out branch changed since the recap was prepared.
    let mut switched = request_for(tmp.path(), &onto);
    switched.current = "ghost".to_string();
    let err = ai_rebase::run_with(&provider, tmp.path(), &switched).unwrap_err();
    assert!(
        err.message().contains("checked-out branch changed"),
        "got {err:?}"
    );

    // A `-`-leading target must never reach a CLI as a flag.
    let mut dash = request_for(tmp.path(), &onto);
    dash.onto = "-foo".to_string();
    let err = ai_rebase::run_with(&provider, tmp.path(), &dash).unwrap_err();
    assert!(err.message().contains("invalid ref name"), "got {err:?}");

    // Nothing to replay: the target already contains the branch.
    let nothing = AiRebaseRequest {
        current: "feature".to_string(),
        onto: "feature".to_string(),
        instructions: String::new(),
        expected: Vec::new(),
    };
    let err = ai_rebase::run_with(&provider, tmp.path(), &nothing).unwrap_err();
    assert!(err.message().contains("nothing to rebase"), "got {err:?}");

    assert!(
        !bin.path().join("prompt.txt").exists(),
        "every guard fires before the provider"
    );
}

#[test]
fn an_operation_in_progress_refuses_before_invoking_the_provider() {
    let (tmp, onto, _) = rebase_fixture(true);
    let request = request_for(tmp.path(), &onto);
    // A rebase stopped on a conflict from the terminal.
    let out = cli::run(tmp.path(), &["rebase", &onto]).unwrap();
    assert!(!out.success(), "the fixture rebase must conflict");
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "printf 'never called'");

    let err = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap_err();

    assert!(err.message().contains("already in progress"), "got {err:?}");
    assert!(!bin.path().join("prompt.txt").exists());
}

#[test]
fn a_detached_head_refuses_before_invoking_the_provider() {
    let (tmp, onto, _) = rebase_fixture(false);
    let request = request_for(tmp.path(), &onto);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();
    let bin = tempfile::tempdir().unwrap();
    let provider = fake_provider(bin.path(), "printf 'never called'");

    let err = ai_rebase::run_with(&provider, tmp.path(), &request).unwrap_err();

    assert!(err.message().contains("HEAD is detached"), "got {err:?}");
    assert!(!bin.path().join("prompt.txt").exists());
}

/// Waits for a marker file the fake provider touches — the run is then
/// observably past that point.
fn wait_for(path: &Path) {
    for _ in 0..400 {
        if path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("marker {} never appeared", path.display());
}

#[test]
fn cancel_kills_the_provider_aborts_the_rebase_and_restores_the_branch() {
    let (tmp, onto, _) = rebase_fixture(true);
    let head_before = git2::Repository::open(tmp.path())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    let bin = tempfile::tempdir().unwrap();
    let started = bin.path().join("started");
    // The provider conflicts, leaves the rebase in progress, then stalls —
    // the user cancels instead of waiting out the timeout.
    let provider = fake_provider(
        bin.path(),
        &format!(
            "git rebase '{onto}' >/dev/null 2>&1 || true\n\
             touch '{}'\nsleep 30",
            started.display()
        ),
    );
    let request = request_for(tmp.path(), &onto);

    let lock = MutationLock::new();
    let mut runner = AiRebaseRunner::new(tmp.path(), lock.clone(), || {});
    assert!(runner.request_program(provider, request));
    wait_for(&started);
    assert!(runner.elapsed().is_some(), "the chip timer runs");
    assert!(!runner.cancelling());

    runner.cancel();
    assert!(runner.cancelling(), "Cancel turns inert until the reply");
    let report = runner.recv().unwrap().unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Unchanged);
    assert!(
        report.summary.contains("aborted") && report.summary.contains("restored"),
        "got: {}",
        report.summary
    );
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
    assert!(!runner.busy() && !runner.cancelling());
    let mut sync = SyncRunner::new_with_lock(tmp.path(), lock, || {});
    assert!(
        sync.request(SyncCommand::FetchAll),
        "the lock is released after a cancel"
    );
    let _ = sync.recv();
}

#[test]
fn cancel_before_the_provider_touches_the_repo_reports_unchanged() {
    let (tmp, onto, _) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    let started = bin.path().join("started");
    let provider = fake_provider(
        bin.path(),
        &format!("touch '{}'\nsleep 30", started.display()),
    );
    let request = request_for(tmp.path(), &onto);

    let mut runner = AiRebaseRunner::new(tmp.path(), MutationLock::new(), || {});
    assert!(runner.request_program(provider, request));
    wait_for(&started);
    runner.cancel();
    let report = runner.recv().unwrap().unwrap();

    assert_eq!(report.outcome, AiRebaseOutcome::Unchanged);
    assert!(
        report.summary.contains("had not started"),
        "got: {}",
        report.summary
    );
}

#[test]
fn dropping_the_runner_kills_the_provider_and_restores_the_repo() {
    let (tmp, onto, _) = rebase_fixture(false);
    let head_before = git2::Repository::open(tmp.path())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    let bin = tempfile::tempdir().unwrap();
    let started = bin.path().join("started");
    let finished = bin.path().join("finished");
    let provider = fake_provider(
        bin.path(),
        &format!(
            "touch '{started}'\nsleep 2\ngit rebase '{onto}' >/dev/null 2>&1\ntouch '{finished}'",
            started = started.display(),
            finished = finished.display()
        ),
    );
    let request = request_for(tmp.path(), &onto);

    let mut runner = AiRebaseRunner::new(tmp.path(), MutationLock::new(), || {});
    assert!(runner.request_program(provider, request));
    wait_for(&started);
    // Repo switch / quit: the drop cancels and joins — once it returns, the
    // provider is dead and the repo settled.
    drop(runner);

    assert!(
        !finished.exists(),
        "the provider must not outlive the session"
    );
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
}

#[test]
fn the_runner_holds_the_mutation_lock_for_the_whole_run() {
    let (tmp, onto, _) = rebase_fixture(false);
    let bin = tempfile::tempdir().unwrap();
    // `sleep` keeps the run observable: busy + lock held while it runs.
    let provider = fake_provider(
        bin.path(),
        &format!("sleep 1\ngit rebase '{onto}' >/dev/null 2>&1\nprintf 'Rebased.'"),
    );
    let request = request_for(tmp.path(), &onto);

    let lock = MutationLock::new();
    let mut runner = AiRebaseRunner::new(tmp.path(), lock.clone(), || {});
    let mut sync = SyncRunner::new_with_lock(tmp.path(), lock, || {});
    assert!(!runner.busy());

    assert!(runner.request_program(provider.clone(), request.clone()));
    assert!(runner.busy());
    assert!(
        !runner.request_program(provider, request),
        "a second request is ignored while one is in flight"
    );
    assert!(
        !sync.request(SyncCommand::FetchAll),
        "sync ops are excluded while the provider rewrites history"
    );

    let report = runner.recv().unwrap().unwrap();
    assert_eq!(report.outcome, AiRebaseOutcome::Completed);
    assert!(!runner.busy());
    assert!(
        sync.request(SyncCommand::FetchAll),
        "the lock is released once the run is drained"
    );
    let _ = sync.recv();
}
