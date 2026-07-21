use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use helm::git::branch::Branch;
use helm::git::sync::{self, SyncOutcome};
use helm::git::worker::{
    drain_sync_refresh, GitCommand, GitResult, GitWorker, SyncCommand, SyncReply, SyncRunner,
};
use helm::git::{cli, tag};

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
fn create_lightweight_tags_the_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");

    tag::create_lightweight(&repo, "v1.0", oid).unwrap();

    let reference = repo.find_reference("refs/tags/v1.0").unwrap();
    assert_eq!(
        reference.target(),
        Some(oid),
        "a lightweight tag targets the commit directly (no tag object)"
    );
}

#[test]
fn create_lightweight_rejects_duplicate_and_invalid_names() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");

    tag::create_lightweight(&repo, "taken", oid).unwrap();
    assert!(
        tag::create_lightweight(&repo, "taken", oid).is_err(),
        "duplicate tag"
    );
    assert!(
        tag::create_lightweight(&repo, "with space", oid).is_err(),
        "invalid name"
    );

    assert_eq!(
        repo.tag_names(None).unwrap().len(),
        1,
        "only the first (valid, unique) tag is created"
    );
}

#[test]
fn tag_and_branch_sharing_a_name_do_not_collide() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    repo.branch("shared", &repo.find_commit(oid).unwrap(), false)
        .unwrap();

    tag::create_lightweight(&repo, "shared", oid).unwrap();

    assert!(
        repo.find_branch("shared", git2::BranchType::Local).is_ok(),
        "the branch is untouched"
    );
    assert_eq!(
        repo.find_reference("refs/tags/shared").unwrap().target(),
        Some(oid),
        "the tag lives under refs/tags, independent of refs/heads"
    );
}

#[test]
fn worker_create_tag_at_tags_without_switching() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    let on = head_branch(&repo);

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CreateTagAt {
        name: "release".into(),
        at: oid,
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
    assert_eq!(
        repo.find_reference("refs/tags/release").unwrap().target(),
        Some(oid)
    );
}

#[test]
fn checkout_detached_lands_on_the_tag_commit_and_auto_stashes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    commit_file(&repo, tmp.path(), "b.txt");
    tag::create_lightweight(&repo, "v1.0", c1).unwrap();
    fs::write(tmp.path().join("a.txt"), "dirty\n").unwrap();

    tag::checkout_detached(&repo, "v1.0").unwrap();

    assert!(repo.head_detached().unwrap(), "HEAD detached on the tag");
    assert_eq!(repo.head().unwrap().target(), Some(c1), "checked out v1.0");
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "x\n",
        "the dirty edit was set aside, not carried onto the tag"
    );
    let mut stashes = 0;
    repo.stash_foreach(|_, _, _| {
        stashes += 1;
        true
    })
    .unwrap();
    assert_eq!(stashes, 1, "the working-tree change was auto-stashed");
}

#[test]
fn worker_checkout_tag_detaches_head() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let c1 = commit_file(&repo, tmp.path(), "a.txt");
    commit_file(&repo, tmp.path(), "b.txt");
    tag::create_lightweight(&repo, "v1.0", c1).unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CheckoutTag("v1.0".into()));

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => assert!(
            matches!(snap.branch, Branch::Detached(_)),
            "expected a detached HEAD, got {:?}",
            snap.branch
        ),
        other => panic!("expected a detached snapshot, got {other:?}"),
    }
    assert_eq!(repo.head().unwrap().target(), Some(c1));
}

#[test]
fn delete_removes_the_local_tag_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    tag::create_lightweight(&repo, "v1.0", oid).unwrap();

    tag::delete(&repo, "v1.0").unwrap();

    assert!(
        repo.find_reference("refs/tags/v1.0").is_err(),
        "the tag ref is gone"
    );
    assert!(tag::delete(&repo, "ghost").is_err(), "missing tag ⇒ Err");
}

fn origin_fixture() -> (tempfile::TempDir, git2::Repository, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("remote.git");
    git2::Repository::init_bare(&bare).unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    tag::create_lightweight(&repo, "v1.0", oid).unwrap();
    let url = format!("file://{}", bare.display());
    repo.remote("origin", &url).unwrap();
    (tmp, repo, bare)
}

#[test]
fn push_tag_publishes_it_to_origin() {
    let (tmp, _repo, bare) = origin_fixture();

    assert_eq!(
        sync::push_tag(tmp.path(), "v1.0").unwrap(),
        SyncOutcome::Updated
    );

    assert!(
        git2::Repository::open(&bare)
            .unwrap()
            .find_reference("refs/tags/v1.0")
            .is_ok(),
        "origin now carries the tag"
    );
}

#[test]
fn push_tag_is_unambiguous_next_to_a_same_named_branch() {
    let (tmp, repo, bare) = origin_fixture();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("v1.0", &head, false).unwrap();

    assert_eq!(
        sync::push_tag(tmp.path(), "v1.0").unwrap(),
        SyncOutcome::Updated,
        "a bare 'v1.0' refspec would be refused: matches more than one"
    );

    let remote = git2::Repository::open(&bare).unwrap();
    assert!(
        remote.find_reference("refs/tags/v1.0").is_ok(),
        "origin now carries the tag"
    );
    assert!(
        remote.find_reference("refs/heads/v1.0").is_err(),
        "the same-named branch is never pushed"
    );
}

#[test]
fn delete_remote_tag_removes_it_from_origin_only() {
    let (tmp, repo, bare) = origin_fixture();
    assert!(cli::run(tmp.path(), &["push", "origin", "v1.0"])
        .unwrap()
        .success());

    assert_eq!(
        sync::delete_remote_tag(tmp.path(), "v1.0").unwrap(),
        SyncOutcome::Updated
    );

    assert!(
        git2::Repository::open(&bare)
            .unwrap()
            .find_reference("refs/tags/v1.0")
            .is_err(),
        "the remote tag is gone"
    );
    assert!(
        repo.find_reference("refs/tags/v1.0").is_ok(),
        "the local tag is untouched (local deletion runs on the worker)"
    );
}

fn drain_until_reply(runner: &mut SyncRunner, worker: &GitWorker) -> Vec<SyncReply> {
    for _ in 0..400 {
        let replies = drain_sync_refresh(runner, worker, false, 10);
        if !replies.is_empty() {
            return replies;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("sync op never completed");
}

#[test]
fn combined_tag_delete_runs_local_after_remote_success() {
    let (tmp, repo, bare) = origin_fixture();
    assert!(cli::run(tmp.path(), &["push", "origin", "v1.0"])
        .unwrap()
        .success());

    let worker = GitWorker::spawn(tmp.path(), || {});
    let mut runner = SyncRunner::new(tmp.path(), || {});
    assert!(runner.request(SyncCommand::DeleteRemoteThenLocalTag("v1.0".into())));

    let replies = drain_until_reply(&mut runner, &worker);
    assert_eq!(replies.len(), 1);
    assert!(replies[0].result.is_ok(), "{:?}", replies[0].result);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(_),
                source,
            },
        )) => assert_eq!(source, GitCommand::DeleteTag("v1.0".to_owned())),
        other => panic!("expected the local tag delete after remote success, got {other:?}"),
    }
    match worker.recv() {
        Some((_, GitResult::Status { result: Ok(_), .. })) => {}
        other => panic!("expected the follow-up status refresh, got {other:?}"),
    }
    assert!(
        repo.find_reference("refs/tags/v1.0").is_err(),
        "the local tag is gone"
    );
    assert!(
        git2::Repository::open(&bare)
            .unwrap()
            .find_reference("refs/tags/v1.0")
            .is_err(),
        "the remote tag is gone"
    );
}

#[test]
fn combined_tag_delete_keeps_local_when_remote_fails() {
    // A genuine network failure — not a tag merely absent on origin: the delete
    // refspec is fully qualified (`refs/tags/v1.0`), so git treats a missing
    // remote tag as an idempotent success. Only an unreachable remote fails the
    // push, and that is what must keep the local tag.
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    let oid = commit_file(&repo, tmp.path(), "a.txt");
    tag::create_lightweight(&repo, "v1.0", oid).unwrap();
    repo.remote("origin", "file:///helm/no-such-repo.git")
        .unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    let mut runner = SyncRunner::new(tmp.path(), || {});
    assert!(runner.request(SyncCommand::DeleteRemoteThenLocalTag("v1.0".into())));

    let replies = drain_until_reply(&mut runner, &worker);
    assert!(
        replies[0].result.is_err(),
        "an unreachable remote fails the push"
    );
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(_),
                source,
            },
        )) => assert_eq!(
            source,
            GitCommand::Status,
            "only the status refresh follows"
        ),
        other => panic!("expected just the status refresh, got {other:?}"),
    }
    assert!(
        repo.find_reference("refs/tags/v1.0").is_ok(),
        "no silent half: the local tag stays until the remote delete lands"
    );
}
