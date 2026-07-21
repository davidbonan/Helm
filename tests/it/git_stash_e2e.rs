use std::fs;
use std::path::Path;

use helm::git::status::load_repo;
use helm::git::{stash, worker::GitCommand, worker::GitResult, worker::GitWorker};

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str) -> git2::Oid {
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
    repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &parent_refs)
        .unwrap()
}

#[test]
fn stash_shelves_tracked_and_untracked_then_tree_is_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();

    stash::stash(&repo).unwrap();

    assert_eq!(load_repo(&repo).unwrap().changed_file_count(), 0);
    assert!(!tmp.path().join("new.txt").exists());
    assert_eq!(stash::count(&repo).unwrap(), 1);
    let log = repo.reflog("refs/stash").unwrap();
    let message = log.get(0).unwrap().message().unwrap().unwrap().to_string();
    assert!(message.contains(stash::STASH_MESSAGE));
}

#[test]
fn pop_restores_the_shelved_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
    stash::stash(&repo).unwrap();

    stash::pop(&repo).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "edited\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(stash::count(&repo).unwrap(), 0);
}

#[test]
fn pop_conflict_keeps_the_stash_and_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "stashed\n").unwrap();
    stash::stash(&repo).unwrap();
    commit_file(&repo, tmp.path(), "a.txt", "conflicting\n");

    assert!(stash::pop(&repo).is_err());

    assert_eq!(stash::count(&repo).unwrap(), 1, "the stash is kept");
    // Left as is, like `git stash pop`: markers placed, resolution in the terminal.
    assert!(fs::read_to_string(tmp.path().join("a.txt"))
        .unwrap()
        .contains("<<<<<<<"));
}

#[test]
fn count_follows_saves_and_pops() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    assert_eq!(stash::count(&repo).unwrap(), 0);

    fs::write(tmp.path().join("a.txt"), "one\n").unwrap();
    stash::stash(&repo).unwrap();
    assert_eq!(stash::count(&repo).unwrap(), 1);

    fs::write(tmp.path().join("b.txt"), "two\n").unwrap();
    stash::stash(&repo).unwrap();
    assert_eq!(stash::count(&repo).unwrap(), 2);

    stash::pop(&repo).unwrap();
    assert_eq!(stash::count(&repo).unwrap(), 1);
}

/// Stash commit oid of `stash@{index}` (same source as the graph rows: the
/// `refs/stash` reflog, most recent first).
fn stash_oid(repo: &git2::Repository, index: usize) -> git2::Oid {
    repo.reflog("refs/stash")
        .unwrap()
        .get(index)
        .unwrap()
        .id_new()
}

#[test]
fn pop_at_targets_the_given_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("one.txt"), "one\n").unwrap();
    stash::stash(&repo).unwrap();
    fs::write(tmp.path().join("two.txt"), "two\n").unwrap();
    stash::stash(&repo).unwrap();
    let older = stash_oid(&repo, 1);

    stash::pop_at(&repo, older).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("one.txt")).unwrap(),
        "one\n"
    );
    assert!(
        !tmp.path().join("two.txt").exists(),
        "the newer stash stays shelved"
    );
    assert_eq!(stash::count(&repo).unwrap(), 1);
}

#[test]
fn drop_at_removes_only_the_targeted_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("one.txt"), "one\n").unwrap();
    stash::stash(&repo).unwrap();
    fs::write(tmp.path().join("two.txt"), "two\n").unwrap();
    stash::stash(&repo).unwrap();
    let older = stash_oid(&repo, 1);
    let newer = stash_oid(&repo, 0);

    stash::drop_at(&repo, older).unwrap();

    assert_eq!(stash::count(&repo).unwrap(), 1);
    assert_eq!(stash_oid(&repo, 0), newer, "the newer stash survives");
    assert!(
        !tmp.path().join("one.txt").exists(),
        "nothing lands in the worktree"
    );
}

#[test]
fn apply_at_restores_the_changes_and_keeps_the_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
    stash::stash(&repo).unwrap();
    let oid = stash_oid(&repo, 0);

    stash::apply_at(&repo, oid).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "edited\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(
        stash::count(&repo).unwrap(),
        1,
        "apply is the no-drop twin of pop — the stash stays listed"
    );
    assert_eq!(stash_oid(&repo, 0), oid, "the same stash is still there");
}

#[test]
fn apply_at_conflict_keeps_the_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "stashed\n").unwrap();
    stash::stash(&repo).unwrap();
    commit_file(&repo, tmp.path(), "a.txt", "conflicting\n");
    let oid = stash_oid(&repo, 0);

    assert!(stash::apply_at(&repo, oid).is_err());

    assert_eq!(
        stash::count(&repo).unwrap(),
        1,
        "the stash stays either way — apply never drops"
    );
}

#[test]
fn pop_at_conflict_keeps_the_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "stashed\n").unwrap();
    stash::stash(&repo).unwrap();
    commit_file(&repo, tmp.path(), "a.txt", "conflicting\n");
    let oid = stash_oid(&repo, 0);

    assert!(stash::pop_at(&repo, oid).is_err());

    assert_eq!(stash::count(&repo).unwrap(), 1, "the stash is kept");
}

#[test]
fn pop_at_and_drop_at_on_a_gone_stash_are_clear_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
    stash::stash(&repo).unwrap();
    let oid = stash_oid(&repo, 0);
    stash::pop(&repo).unwrap();

    assert!(stash::pop_at(&repo, oid).is_err());
    assert!(stash::drop_at(&repo, oid).is_err());
}

#[test]
fn stash_paths_shelves_both_states_of_only_the_listed_files() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base-a\n");
    commit_file(&repo, tmp.path(), "b.txt", "base-b\n");

    // a.txt carries a staged change plus a further unstaged edit on top.
    fs::write(tmp.path().join("a.txt"), "staged-a\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();
    fs::write(tmp.path().join("a.txt"), "worktree-a\n").unwrap();
    // b.txt is modified but not part of the selection.
    fs::write(tmp.path().join("b.txt"), "edited-b\n").unwrap();

    stash::stash_paths(&repo, &["a.txt".to_owned()]).unwrap();

    assert_eq!(stash::count(&repo).unwrap(), 1, "a single stash entry");
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "base-a\n",
        "both the staged and the unstaged change of a.txt are stashed"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("b.txt")).unwrap(),
        "edited-b\n",
        "the unlisted file keeps its change"
    );
}

#[test]
fn stash_paths_groups_the_whole_selection_into_one_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base-a\n");
    commit_file(&repo, tmp.path(), "b.txt", "base-b\n");
    commit_file(&repo, tmp.path(), "c.txt", "base-c\n");
    fs::write(tmp.path().join("a.txt"), "edited-a\n").unwrap();
    fs::write(tmp.path().join("b.txt"), "edited-b\n").unwrap();
    fs::write(tmp.path().join("c.txt"), "edited-c\n").unwrap();

    stash::stash_paths(&repo, &["a.txt".to_owned(), "b.txt".to_owned()]).unwrap();

    assert_eq!(
        stash::count(&repo).unwrap(),
        1,
        "the two files land in a single stash, never one entry each"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("c.txt")).unwrap(),
        "edited-c\n",
        "the unlisted file is untouched"
    );
}

#[test]
fn stash_paths_leaves_a_staged_bystander_out_of_the_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base-a\n");
    commit_file(&repo, tmp.path(), "b.txt", "base-b\n");

    // a.txt: the unstaged target. b.txt: a *staged* bystander preparing a commit.
    fs::write(tmp.path().join("a.txt"), "edited-a\n").unwrap();
    fs::write(tmp.path().join("b.txt"), "edited-b\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("b.txt")).unwrap();
    index.write().unwrap();

    stash::stash_paths(&repo, &["a.txt".to_owned()]).unwrap();

    let status = load_repo(&repo).unwrap();
    assert!(
        status.staged.iter().any(|f| f.path == "b.txt"),
        "the bystander's staged change survives the stash"
    );

    // The entry must touch a.txt alone — the index-leak baked every staged file in.
    let stash_oid = repo.reflog("refs/stash").unwrap().get(0).unwrap().id_new();
    let stash_tree = repo.find_commit(stash_oid).unwrap().tree().unwrap();
    let head_tree = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .tree()
        .unwrap();
    let diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&stash_tree), None)
        .unwrap();
    let mut changed: Vec<String> = diff
        .deltas()
        .filter_map(|delta| {
            delta
                .new_file()
                .path()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .collect();
    changed.sort();
    changed.dedup();
    assert_eq!(
        changed,
        vec!["a.txt".to_string()],
        "the stash holds only the selected file"
    );
}

const RENAMED_BODY: &str = "one\ntwo\nthree\nfour\n";

#[test]
fn stash_paths_takes_both_sides_of_an_unstaged_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "old.txt", RENAMED_BODY);
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();

    // The row the sidebar offers carries the new path alone — that is what Stash sends.
    let unstaged = load_repo(&repo).unwrap().unstaged;
    assert_eq!(
        unstaged.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        vec!["new.txt"],
        "the rename shows as a single row on its new path"
    );

    stash::stash_paths(&repo, &["new.txt".to_owned()]).unwrap();

    assert_eq!(stash::count(&repo).unwrap(), 1);
    assert_eq!(
        load_repo(&repo).unwrap().changed_file_count(),
        0,
        "neither side of the rename dangles in the tree"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("old.txt")).unwrap(),
        RENAMED_BODY,
        "the old path is back on disk, not lost with the stash"
    );
    assert!(!tmp.path().join("new.txt").exists());

    stash::pop(&repo).unwrap();

    assert!(!tmp.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
        RENAMED_BODY,
        "popping restores the whole move"
    );
}

#[test]
fn stash_paths_takes_both_sides_of_a_staged_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "old.txt", RENAMED_BODY);
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();

    let staged = load_repo(&repo).unwrap().staged;
    assert_eq!(
        staged.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        vec!["new.txt"]
    );

    stash::stash_paths(&repo, &["new.txt".to_owned()]).unwrap();

    assert_eq!(stash::count(&repo).unwrap(), 1);
    assert_eq!(
        load_repo(&repo).unwrap().changed_file_count(),
        0,
        "no staged deletion of the old path is left behind"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("old.txt")).unwrap(),
        RENAMED_BODY
    );
    assert!(!tmp.path().join("new.txt").exists());

    stash::pop(&repo).unwrap();

    assert!(!tmp.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
        RENAMED_BODY
    );
}

#[test]
fn stash_paths_with_no_paths_is_a_clear_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();

    assert!(stash::stash_paths(&repo, &[]).is_err());
    assert_eq!(stash::count(&repo).unwrap(), 0);
}

#[test]
fn worker_stash_files_shelves_only_the_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base-a\n");
    commit_file(&repo, tmp.path(), "b.txt", "base-b\n");
    fs::write(tmp.path().join("a.txt"), "edited-a\n").unwrap();
    fs::write(tmp.path().join("b.txt"), "edited-b\n").unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::StashFiles(vec!["a.txt".to_owned()]));
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.stash_count, 1);
            assert!(snap.status.unstaged.iter().any(|f| f.path == "b.txt"));
            assert!(!snap.status.unstaged.iter().any(|f| f.path == "a.txt"));
        }
        other => panic!("expected snapshot after stash, got {other:?}"),
    }
}

#[test]
fn stash_on_clean_tree_is_a_clear_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");

    assert!(stash::stash(&repo).is_err());
    assert_eq!(stash::count(&repo).unwrap(), 0);
}

#[test]
fn worker_stash_commands_respond_with_a_counted_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Stash);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.stash_count, 1);
            assert!(snap.status.unstaged.is_empty());
        }
        other => panic!("expected snapshot after stash, got {other:?}"),
    }

    worker.send(GitCommand::StashPop);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.stash_count, 0);
            assert!(snap.status.unstaged.iter().any(|f| f.path == "a.txt"));
        }
        other => panic!("expected snapshot after pop, got {other:?}"),
    }
}

#[test]
fn worker_targeted_stash_commands_resolve_the_oid() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("one.txt"), "one\n").unwrap();
    stash::stash(&repo).unwrap();
    fs::write(tmp.path().join("two.txt"), "two\n").unwrap();
    stash::stash(&repo).unwrap();
    let older = stash_oid(&repo, 1);
    let newer = stash_oid(&repo, 0);

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::StashDropAt(older));
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => assert_eq!(snap.stash_count, 1),
        other => panic!("expected snapshot after drop, got {other:?}"),
    }

    worker.send(GitCommand::StashPopAt(newer));
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.stash_count, 0);
            assert!(snap.status.unstaged.iter().any(|f| f.path == "two.txt"));
        }
        other => panic!("expected snapshot after pop, got {other:?}"),
    }
}

#[test]
fn worker_apply_responds_with_the_restored_diff_and_keeps_the_stash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo_with_identity(tmp.path());
    commit_file(&repo, tmp.path(), "a.txt", "base\n");
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
    stash::stash(&repo).unwrap();
    let oid = stash_oid(&repo, 0);

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::StashApplyAt(oid));
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert_eq!(snap.stash_count, 1, "the stash is kept");
            assert!(snap.status.unstaged.iter().any(|f| f.path == "a.txt"));
        }
        other => panic!("expected snapshot after apply, got {other:?}"),
    }
}
