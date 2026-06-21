use std::fs;
use std::path::Path;

use helm::git::commit;

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn stage(repo: &git2::Repository, name: &str) {
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
}

#[test]
fn initial_commit_on_unborn_head_creates_root_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    fs::write(tmp.path().join("a.txt"), "hello\n").unwrap();
    stage(&repo, "a.txt");
    assert!(repo.head().is_err());

    let oid = commit::commit(&repo, "init").unwrap();

    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.id(), oid);
    assert_eq!(head.message().unwrap(), "init");
    assert_eq!(head.parent_count(), 0);
    assert!(head.tree().unwrap().get_path(Path::new("a.txt")).is_ok());
}

#[test]
fn commit_on_existing_head_adds_child_with_index_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    fs::write(tmp.path().join("a.txt"), "v1\n").unwrap();
    stage(&repo, "a.txt");
    let first = commit::commit(&repo, "first").unwrap();

    fs::write(tmp.path().join("b.txt"), "v2\n").unwrap();
    stage(&repo, "b.txt");
    let second = commit::commit(&repo, "second").unwrap();

    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.id(), second);
    assert_eq!(head.parent_count(), 1);
    assert_eq!(head.parent(0).unwrap().id(), first);
    assert!(head.tree().unwrap().get_path(Path::new("b.txt")).is_ok());
}

#[test]
fn commit_without_signature_returns_actionable_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let empty = git2::Config::new().unwrap();
    repo.set_config(&empty).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello\n").unwrap();
    stage(&repo, "a.txt");

    let err = commit::commit(&repo, "init").unwrap_err();

    assert!(
        err.message().contains("user.name") && err.message().contains("user.email"),
        "unexpected error message: {}",
        err.message()
    );
    assert!(repo.head().is_err());
}

#[test]
fn commit_refuses_when_nothing_is_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    fs::write(tmp.path().join("a.txt"), "v1\n").unwrap();
    stage(&repo, "a.txt");
    let head = commit::commit(&repo, "first").unwrap();

    let err = commit::commit(&repo, "empty").unwrap_err();

    assert!(
        err.message().contains("nothing staged"),
        "unexpected error message: {}",
        err.message()
    );
    assert_eq!(repo.head().unwrap().target(), Some(head));
}

#[test]
fn commit_refuses_during_merge_or_rebase_state() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    fs::write(tmp.path().join("a.txt"), "v1\n").unwrap();
    stage(&repo, "a.txt");
    let head = commit::commit(&repo, "first").unwrap();
    fs::write(tmp.path().join("b.txt"), "v2\n").unwrap();
    stage(&repo, "b.txt");
    fs::write(repo.path().join("MERGE_HEAD"), head.to_string()).unwrap();
    assert_ne!(repo.state(), git2::RepositoryState::Clean);

    let err = commit::commit(&repo, "merge").unwrap_err();

    assert!(
        err.message().contains("resolve from the terminal"),
        "unexpected error message: {}",
        err.message()
    );
    assert_eq!(repo.head().unwrap().target(), Some(head));
}
