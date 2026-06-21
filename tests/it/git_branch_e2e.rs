use std::fs;
use std::path::Path;

use helm::git::branch::{self, Branch};
use helm::git::worker::{GitCommand, GitResult, GitWorker};

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_file(repo: &git2::Repository, dir: &Path, name: &str) -> git2::Oid {
    fs::write(dir.join(name), "x\n").unwrap();
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
    repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &parent_refs)
        .unwrap()
}

fn head_branch(repo: &git2::Repository) -> String {
    repo.head().unwrap().shorthand().unwrap().to_string()
}

#[test]
fn unborn_head_reports_target_branch_name() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    assert!(repo.head().is_err());

    let target = repo.find_reference("HEAD").unwrap();
    let expected = target
        .symbolic_target()
        .unwrap()
        .unwrap()
        .strip_prefix("refs/heads/")
        .unwrap()
        .to_string();

    assert_eq!(branch::current(&repo).unwrap(), Branch::Unborn(expected));
}

#[test]
fn symbolic_head_reports_branch_name() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");

    let name = repo.head().unwrap().shorthand().unwrap().to_string();
    assert!(!repo.head_detached().unwrap());

    assert_eq!(branch::current(&repo).unwrap(), Branch::Named(name));
}

#[test]
fn detached_head_reports_short_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");

    repo.set_head_detached(oid).unwrap();
    assert!(repo.head_detached().unwrap());

    let expected_short = repo
        .find_object(oid, None)
        .unwrap()
        .short_id()
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let got = branch::current(&repo).unwrap();
    assert_eq!(got, Branch::Detached(expected_short.clone()));
    assert!(
        oid.to_string().starts_with(&expected_short),
        "short hash {expected_short} should prefix full oid {oid}"
    );
}

#[test]
fn checkout_switches_head_to_local_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("feature", &repo.find_commit(oid).unwrap(), false)
        .unwrap();

    branch::checkout(&repo, "feature").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("feature".into())
    );
}

#[test]
fn checkout_dirty_tree_stashes_changes_first() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("feature", &repo.find_commit(oid).unwrap(), false)
        .unwrap();
    // A modified tracked file + an untracked one: both must go into the stash.
    fs::write(tmp.path().join("a.txt"), "modified\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();

    branch::checkout(&repo, "feature").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("feature".into())
    );
    // The tree is clean: the changes are in the stash, not lost.
    assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "x\n");
    assert!(!tmp.path().join("new.txt").exists());
    let mut stashes = Vec::new();
    let mut reopened = git2::Repository::open(tmp.path()).unwrap();
    reopened
        .stash_foreach(|_, message, _| {
            stashes.push(message.to_string());
            true
        })
        .unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(
        stashes[0].contains("auto-stash before checkout feature"),
        "stash message: {}",
        stashes[0]
    );
}

#[test]
fn checkout_clean_tree_creates_no_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("feature", &repo.find_commit(oid).unwrap(), false)
        .unwrap();

    branch::checkout(&repo, "feature").unwrap();

    let mut count = 0;
    let mut reopened = git2::Repository::open(tmp.path()).unwrap();
    reopened
        .stash_foreach(|_, _, _| {
            count += 1;
            true
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn checkout_remote_ref_creates_tracking_local_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    // Remote-tracking ref without a local counterpart. The remote must be
    // configured: `set_upstream` resolves `origin` via the fetch refspecs.
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feature", oid, false, "remote")
        .unwrap();

    branch::checkout(&repo, "origin/feature").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("feature".into())
    );
    let local = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap();
    assert_eq!(local.get().target(), Some(oid));
    let upstream = local.upstream().unwrap();
    assert_eq!(upstream.name().unwrap(), Some("origin/feature"));
}

#[test]
fn checkout_remote_with_local_on_same_commit_checks_out_local() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    // Local `feature` and `origin/feature` on the same commit: checkout of the
    // local one as is — neither fast-forward nor detached HEAD.
    repo.branch("feature", &repo.find_commit(c1).unwrap(), false)
        .unwrap();
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feature", c1, false, "remote")
        .unwrap();

    branch::checkout(&repo, "origin/feature").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("feature".into())
    );
}

#[test]
fn checkout_remote_with_stale_local_behind_fast_forwards_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    let c2 = commit_file(&repo, tmp.path(), "b.txt");
    // Local `feature` on c1, `origin/feature` on c2 (descendant): the local one
    // is simply behind — fast-forward to the target commit, then checkout.
    repo.branch("feature", &repo.find_commit(c1).unwrap(), false)
        .unwrap();
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feature", c2, false, "remote")
        .unwrap();

    branch::checkout(&repo, "origin/feature").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("feature".into())
    );
    let local = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap();
    assert_eq!(local.get().target(), Some(c2));
    assert_eq!(repo.head().unwrap().target(), Some(c2));
}

#[test]
fn checkout_remote_with_diverged_local_detaches_on_remote_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    let c2 = commit_file(&repo, tmp.path(), "b.txt");
    // Diverged local `feature`: its own commit c3 starts from c1, while
    // `origin/feature` points at c2 — the local one stays put, HEAD detaches on
    // the remote's commit.
    repo.branch("feature", &repo.find_commit(c1).unwrap(), false)
        .unwrap();
    branch::checkout(&repo, "feature").unwrap();
    let c3 = commit_file(&repo, tmp.path(), "c.txt");
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feature", c2, false, "remote")
        .unwrap();

    branch::checkout(&repo, "origin/feature").unwrap();

    assert!(repo.head_detached().unwrap());
    assert_eq!(repo.head().unwrap().target(), Some(c2));
    let local = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap();
    assert_eq!(
        local.get().target(),
        Some(c3),
        "the diverged local branch does not move"
    );
}

#[test]
fn checkout_unknown_branch_errs_and_leaves_head() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let before = branch::current(&repo).unwrap();

    assert!(branch::checkout(&repo, "missing").is_err());
    assert_eq!(branch::current(&repo).unwrap(), before);
}

#[test]
fn failed_checkout_restores_the_auto_stashed_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    fs::write(tmp.path().join("a.txt"), "modified\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();

    assert!(branch::checkout(&repo, "missing").is_err());

    // The changes came back to the working tree — not silently left in a stash.
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "modified\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
        "untracked\n"
    );
    let mut count = 0;
    let mut reopened = git2::Repository::open(tmp.path()).unwrap();
    reopened
        .stash_foreach(|_, _, _| {
            count += 1;
            true
        })
        .unwrap();
    assert_eq!(count, 0, "the auto-stash was popped back");
}

#[test]
fn delete_local_removes_a_non_checked_out_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("feature", &repo.find_commit(oid).unwrap(), false)
        .unwrap();

    branch::delete_local(&repo, "feature").unwrap();

    assert!(repo
        .find_branch("feature", git2::BranchType::Local)
        .is_err());
    // Only the ref disappears: the commit stays reachable.
    assert!(repo.find_commit(oid).is_ok());
}

#[test]
fn delete_local_refuses_the_checked_out_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let current = repo.head().unwrap().shorthand().unwrap().to_string();

    assert!(branch::delete_local(&repo, &current).is_err());

    assert!(repo.find_branch(&current, git2::BranchType::Local).is_ok());
}

#[test]
fn delete_local_unknown_branch_errs() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");

    assert!(branch::delete_local(&repo, "ghost").is_err());
}

#[test]
fn create_and_checkout_creates_on_head_and_switches() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let head = commit_file(&repo, tmp.path(), "b.txt");

    branch::create_and_checkout(&repo, "feature/login").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("feature/login".into())
    );
    let created = repo
        .find_branch("feature/login", git2::BranchType::Local)
        .unwrap();
    assert_eq!(created.get().target(), Some(head), "created on HEAD");
}

#[test]
fn create_duplicate_branch_errs_without_touching_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("taken", &repo.find_commit(c1).unwrap(), false)
        .unwrap();
    let c2 = commit_file(&repo, tmp.path(), "b.txt");
    let before = branch::current(&repo).unwrap();

    assert!(branch::create_and_checkout(&repo, "taken").is_err());

    assert_eq!(branch::current(&repo).unwrap(), before);
    let taken = repo.find_branch("taken", git2::BranchType::Local).unwrap();
    assert_eq!(
        taken.get().target(),
        Some(c1),
        "the existing branch does not move"
    );
    assert_ne!(c1, c2);
}

#[test]
fn create_with_invalid_name_errs_and_creates_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let before = repo.branches(None).unwrap().count();

    assert!(branch::create_and_checkout(&repo, "with space").is_err());
    assert!(branch::create_and_checkout(&repo, "a..b").is_err());

    assert_eq!(repo.branches(None).unwrap().count(), before);
}

#[test]
fn create_on_unborn_head_is_a_clean_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());

    assert!(branch::create_and_checkout(&repo, "feature").is_err());
    assert_eq!(repo.branches(None).unwrap().count(), 0);
}

#[test]
fn create_at_creates_a_branch_without_moving_head() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let target = commit_file(&repo, tmp.path(), "a.txt");
    let head = commit_file(&repo, tmp.path(), "b.txt");
    let before = branch::current(&repo).unwrap();

    branch::create_at(
        &repo,
        "feature/login",
        &format!("refs/heads/{}", head_branch(&repo)),
    )
    .unwrap();

    // HEAD untouched; the new branch sits at the source ref's commit.
    assert_eq!(branch::current(&repo).unwrap(), before);
    let created = repo
        .find_branch("feature/login", git2::BranchType::Local)
        .unwrap();
    assert_eq!(created.get().target(), Some(head));
    assert!(created.upstream().is_err(), "no upstream is configured");
    assert_ne!(target, head);
}

#[test]
fn create_at_from_a_remote_ref_resolves_the_remote_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    repo.reference("refs/remotes/origin/feature", oid, false, "remote")
        .unwrap();

    branch::create_at(&repo, "feature", "refs/remotes/origin/feature").unwrap();

    let created = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap();
    assert_eq!(created.get().target(), Some(oid));
    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named(head_branch(&repo)),
        "HEAD stays on its branch — no checkout"
    );
}

#[test]
fn create_at_from_a_tag_resolves_the_tagged_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    let target = repo.find_object(oid, None).unwrap();
    repo.tag_lightweight("v1.0", &target, false).unwrap();

    branch::create_at(&repo, "from-tag", "refs/tags/v1.0").unwrap();

    let created = repo
        .find_branch("from-tag", git2::BranchType::Local)
        .unwrap();
    assert_eq!(created.get().target(), Some(oid));
}

#[test]
fn create_at_rejects_duplicate_and_invalid_names() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let source = format!("refs/heads/{}", head_branch(&repo));
    let before = repo.branches(None).unwrap().count();

    branch::create_at(&repo, "taken", &source).unwrap();
    assert!(
        branch::create_at(&repo, "taken", &source).is_err(),
        "duplicate name"
    );
    assert!(
        branch::create_at(&repo, "with space", &source).is_err(),
        "invalid name"
    );

    assert_eq!(
        repo.branches(None).unwrap().count(),
        before + 1,
        "only the first (valid, unique) branch is created"
    );
}

#[test]
fn worker_create_branch_at_creates_without_switching() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let on = head_branch(&repo);

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CreateBranchAt {
        name: "side".into(),
        at: format!("refs/heads/{on}"),
    });

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.branch, Branch::Named(on), "HEAD did not move");
        }
        other => panic!("expected a snapshot on the original branch, got {other:?}"),
    }
    assert!(repo.find_branch("side", git2::BranchType::Local).is_ok());
}

#[test]
fn worker_create_branch_responds_with_switched_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CreateBranch("feature".into()));

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.branch, Branch::Named("feature".into()));
        }
        other => panic!("expected snapshot on the new branch, got {other:?}"),
    }
}

fn commit_content(repo: &git2::Repository, dir: &Path, name: &str, content: &str) -> git2::Oid {
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
    repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &parent_refs)
        .unwrap()
}

fn staged_paths(status: &helm::git::status::RepoStatus) -> Vec<&str> {
    status.staged.iter().map(|f| f.path.as_str()).collect()
}

fn unstaged_paths(status: &helm::git::status::RepoStatus) -> Vec<&str> {
    status.unstaged.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn reset_soft_moves_head_and_leaves_the_diff_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_content(&repo, tmp.path(), "f.txt", "v1\n");
    commit_content(&repo, tmp.path(), "f.txt", "v2\n");

    branch::reset(&repo, c1, git2::ResetType::Soft).unwrap();

    assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().id(), c1);
    let status = helm::git::status::load_repo(&repo).unwrap();
    assert_eq!(
        staged_paths(&status),
        ["f.txt"],
        "index keeps the newer tree"
    );
    assert!(unstaged_paths(&status).is_empty());
    assert_eq!(
        fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        "v2\n",
        "working tree untouched"
    );
}

#[test]
fn reset_mixed_moves_head_and_index_leaves_the_diff_unstaged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_content(&repo, tmp.path(), "f.txt", "v1\n");
    commit_content(&repo, tmp.path(), "f.txt", "v2\n");

    branch::reset(&repo, c1, git2::ResetType::Mixed).unwrap();

    assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().id(), c1);
    let status = helm::git::status::load_repo(&repo).unwrap();
    assert!(
        staged_paths(&status).is_empty(),
        "index reset to the target"
    );
    assert_eq!(unstaged_paths(&status), ["f.txt"]);
    assert_eq!(
        fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        "v2\n",
        "working tree untouched"
    );
}

#[test]
fn reset_hard_moves_head_index_and_working_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_content(&repo, tmp.path(), "f.txt", "v1\n");
    commit_content(&repo, tmp.path(), "f.txt", "v2\n");

    branch::reset(&repo, c1, git2::ResetType::Hard).unwrap();

    assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().id(), c1);
    let status = helm::git::status::load_repo(&repo).unwrap();
    assert!(
        staged_paths(&status).is_empty() && unstaged_paths(&status).is_empty(),
        "everything matches the target — clean tree"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        "v1\n",
        "working tree reverted to the target"
    );
}

#[test]
fn reset_hard_leaves_untracked_files_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_content(&repo, tmp.path(), "f.txt", "v1\n");
    commit_content(&repo, tmp.path(), "f.txt", "v2\n");
    fs::write(tmp.path().join("scratch.txt"), "keep\n").unwrap();

    branch::reset(&repo, c1, git2::ResetType::Hard).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("scratch.txt")).unwrap(),
        "keep\n",
        "untracked file survives a hard reset (git semantics)"
    );
}

#[test]
fn worker_reset_mixed_responds_with_the_unstaged_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_content(&repo, tmp.path(), "f.txt", "v1\n");
    commit_content(&repo, tmp.path(), "f.txt", "v2\n");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Reset {
        target: c1,
        mode: git2::ResetType::Mixed,
    });

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(
                snap.status
                    .unstaged
                    .iter()
                    .map(|f| f.path.as_str())
                    .collect::<Vec<_>>(),
                ["f.txt"],
                "the moved-past change shows up unstaged in the refreshed snapshot"
            );
        }
        other => panic!("expected a refreshed snapshot, got {other:?}"),
    }
    assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().id(), c1);
}

#[test]
fn rename_moves_a_non_current_branch_and_drops_the_old_name() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    let on = head_branch(&repo);
    repo.branch("feature", &repo.find_commit(c1).unwrap(), false)
        .unwrap();

    branch::rename(&repo, "feature", "feat").unwrap();

    assert!(repo.find_branch("feat", git2::BranchType::Local).is_ok());
    assert!(repo
        .find_branch("feature", git2::BranchType::Local)
        .is_err());
    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named(on),
        "renaming a side branch leaves HEAD where it was"
    );
}

#[test]
fn rename_current_branch_moves_head_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let on = head_branch(&repo);

    branch::rename(&repo, &on, "trunk").unwrap();

    assert_eq!(
        branch::current(&repo).unwrap(),
        Branch::Named("trunk".into()),
        "the symbolic HEAD follows the renamed current branch"
    );
    assert!(repo.find_branch(&on, git2::BranchType::Local).is_err());
}

#[test]
fn rename_keeps_the_upstream_configuration() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("feature", &repo.find_commit(c1).unwrap(), false)
        .unwrap();
    {
        let mut cfg = repo.config().unwrap();
        cfg.set_str("branch.feature.remote", "origin").unwrap();
        cfg.set_str("branch.feature.merge", "refs/heads/feature")
            .unwrap();
    }

    branch::rename(&repo, "feature", "feat").unwrap();

    let cfg = repo.config().unwrap();
    assert_eq!(
        cfg.get_string("branch.feat.remote").unwrap(),
        "origin",
        "the branch's config section moves with it"
    );
    assert!(cfg.get_string("branch.feature.remote").is_err());
}

#[test]
fn rename_refuses_an_existing_name_and_leaves_both_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    let commit = repo.find_commit(c1).unwrap();
    repo.branch("feature", &commit, false).unwrap();
    repo.branch("taken", &commit, false).unwrap();

    assert!(
        branch::rename(&repo, "feature", "taken").is_err(),
        "force is never used — a duplicate name is refused"
    );
    assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());
    assert!(repo.find_branch("taken", git2::BranchType::Local).is_ok());
}

#[test]
fn rename_refuses_an_invalid_name() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("feature", &repo.find_commit(c1).unwrap(), false)
        .unwrap();

    assert!(branch::rename(&repo, "feature", "bad name").is_err());
    assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());
}

#[test]
fn worker_rename_responds_with_the_new_current_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt");
    let on = head_branch(&repo);

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::RenameBranch {
        from: on,
        to: "trunk".into(),
    });

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.branch, Branch::Named("trunk".into()));
        }
        other => panic!("expected a refreshed snapshot, got {other:?}"),
    }
}
