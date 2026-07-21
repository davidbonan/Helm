use std::fs;
use std::path::Path;

use helm::git::status::{self, ChangeKind};

fn commit_file(repo: &git2::Repository, name: &str, content: &str, message: &str) -> git2::Oid {
    let dir = repo.workdir().unwrap();
    fs::write(dir.join(name), content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

#[test]
fn untracked_file_shows_as_unstaged() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();

    let st = status::load(tmp.path()).unwrap();

    assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));
    assert!(st.staged.is_empty());
}

#[test]
fn added_file_shows_as_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();

    let st = status::load(tmp.path()).unwrap();

    assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    assert!(st.unstaged.is_empty());
}

#[test]
fn staged_rename_is_classified_as_renamed() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(
        &repo,
        "old.txt",
        "stable content for rename detection\n",
        "init",
    );

    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();

    let st = status::load(tmp.path()).unwrap();

    let renamed = st
        .staged
        .iter()
        .find(|f| f.kind == ChangeKind::Renamed)
        .expect("a rename should be detected in the staged section");
    assert_eq!(renamed.path, "new.txt");
    assert!(!st.staged.iter().any(|f| f.path == "old.txt"));
}

#[test]
fn conflicted_file_is_marked_and_listed_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = commit_file(&repo, "f.txt", "base\n", "base");
    let base_commit = repo.find_commit(base).unwrap();
    let ours_branch = repo.head().unwrap().name().unwrap().to_string();

    let theirs = repo.branch("theirs", &base_commit, false).unwrap();
    repo.set_head(theirs.get().name().unwrap()).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let theirs_commit = commit_file(&repo, "f.txt", "theirs\n", "theirs");

    repo.set_head(&ours_branch).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    commit_file(&repo, "f.txt", "ours\n", "ours");

    let annotated = repo
        .find_annotated_commit(repo.find_commit(theirs_commit).unwrap().id())
        .unwrap();
    repo.merge(&[&annotated], None, None).unwrap();

    let st = status::load(tmp.path()).unwrap();

    let conflict = st
        .unstaged
        .iter()
        .find(|f| f.path == "f.txt")
        .expect("conflicted file should be listed");
    assert_eq!(conflict.kind, ChangeKind::Conflicted);
    assert!(!st.staged.iter().any(|f| f.path == "f.txt"));
}

#[test]
fn merge_in_progress_is_detected_then_cleared_after_resolution() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = commit_file(&repo, "f.txt", "base\n", "base");
    let base_commit = repo.find_commit(base).unwrap();
    let ours_branch = repo.head().unwrap().name().unwrap().to_string();

    let theirs = repo.branch("theirs", &base_commit, false).unwrap();
    repo.set_head(theirs.get().name().unwrap()).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let theirs_commit = commit_file(&repo, "f.txt", "theirs\n", "theirs");

    repo.set_head(&ours_branch).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let ours_commit = commit_file(&repo, "f.txt", "ours\n", "ours");

    assert!(
        !status::op_in_progress(&repo),
        "clean repo before the merge"
    );

    let annotated = repo
        .find_annotated_commit(repo.find_commit(theirs_commit).unwrap().id())
        .unwrap();
    repo.merge(&[&annotated], None, None).unwrap();

    assert!(
        status::op_in_progress(&repo),
        "a conflicting merge leaves Repository::state() != Clean"
    );

    fs::write(tmp.path().join("f.txt"), "resolved\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("f.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let ours = repo.find_commit(ours_commit).unwrap();
    let theirs = repo.find_commit(theirs_commit).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "merge", &tree, &[&ours, &theirs])
        .unwrap();
    repo.cleanup_state().unwrap();

    assert!(
        !status::op_in_progress(&repo),
        "resolution + commit brings the state back to Clean"
    );
}

#[test]
fn line_stats_cover_untracked_modified_and_staged_deltas() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "tracked.txt", "one\ntwo\nthree\n", "init");

    // Untracked: +N (whole content), −0.
    fs::write(tmp.path().join("new.txt"), "a\nb\nc\nd\n").unwrap();
    // Modified unstaged: 1 line replaced ⇒ +1/−1, plus 1 addition ⇒ +2/−1.
    fs::write(tmp.path().join("tracked.txt"), "one\nTWO\nthree\nfour\n").unwrap();
    // Staged: file added to the index ⇒ +N in the staged section.
    fs::write(tmp.path().join("staged.txt"), "x\ny\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("staged.txt")).unwrap();
    index.write().unwrap();

    let st = status::load(tmp.path()).unwrap();

    let untracked = st.unstaged.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(untracked.kind, ChangeKind::Untracked);
    assert_eq!(
        (untracked.additions, untracked.deletions),
        (4, 0),
        "untracked content counts as additions"
    );

    let modified = st
        .unstaged
        .iter()
        .find(|f| f.path == "tracked.txt")
        .unwrap();
    assert_eq!(
        (modified.additions, modified.deletions),
        (2, 1),
        "worktree delta counts replaced and added lines"
    );

    let staged = st.staged.iter().find(|f| f.path == "staged.txt").unwrap();
    assert_eq!(
        (staged.additions, staged.deletions),
        (2, 0),
        "staged delta is measured against HEAD"
    );

    assert_eq!(
        st.total_line_stats(),
        (8, 1),
        "summary totals add up both sections"
    );
}

#[test]
fn line_stats_split_a_partially_staged_file_per_section() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "f.txt", "one\ntwo\n", "init");

    // Stage a first revision (+1), then modify the tree again (+1).
    fs::write(tmp.path().join("f.txt"), "one\ntwo\nthree\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("f.txt")).unwrap();
    index.write().unwrap();
    fs::write(tmp.path().join("f.txt"), "one\ntwo\nthree\nfour\n").unwrap();

    let st = status::load(tmp.path()).unwrap();

    let staged = st.staged.iter().find(|f| f.path == "f.txt").unwrap();
    assert_eq!((staged.additions, staged.deletions), (1, 0));
    let unstaged = st.unstaged.iter().find(|f| f.path == "f.txt").unwrap();
    assert_eq!((unstaged.additions, unstaged.deletions), (1, 0));
}

#[test]
fn binary_file_keeps_zero_line_stats() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("blob.bin"), [0u8, 159, 146, 150, 0, 7]).unwrap();

    let st = status::load(tmp.path()).unwrap();

    let bin = st.unstaged.iter().find(|f| f.path == "blob.bin").unwrap();
    assert_eq!(
        (bin.additions, bin.deletions),
        (0, 0),
        "a binary delta shows no line stats"
    );
}

#[test]
fn deleted_file_counts_its_lines_as_deletions() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "gone.txt", "a\nb\nc\n", "init");
    fs::remove_file(tmp.path().join("gone.txt")).unwrap();

    let st = status::load(tmp.path()).unwrap();

    let deleted = st.unstaged.iter().find(|f| f.path == "gone.txt").unwrap();
    assert_eq!(deleted.kind, ChangeKind::Deleted);
    assert_eq!((deleted.additions, deleted.deletions), (0, 3));
}

#[test]
fn unstaged_rename_counts_only_the_edit_not_the_whole_file() {
    // The row is paired as a rename (statuses sets FIND_FOR_UNTRACKED): its
    // stats must be the rename's own delta, not `+<whole file> −0` (git.md §8).
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");

    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(tmp.path().join("new.txt"), "line1\nCHANGED\nline3\nline4\n").unwrap();

    let st = status::load(tmp.path()).unwrap();

    let renamed = st.unstaged.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(renamed.kind, ChangeKind::Renamed);
    assert_eq!((renamed.additions, renamed.deletions), (1, 1));
}

#[test]
fn staged_rename_counts_only_the_edit_not_the_whole_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");

    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(tmp.path().join("new.txt"), "line1\nCHANGED\nline3\nline4\n").unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();

    let st = status::load(tmp.path()).unwrap();

    let renamed = st.staged.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(renamed.kind, ChangeKind::Renamed);
    assert_eq!((renamed.additions, renamed.deletions), (1, 1));
}

#[test]
fn ignored_file_is_absent_from_status() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join(".gitignore"), "ignored.log\n").unwrap();
    fs::write(tmp.path().join("ignored.log"), "noise").unwrap();

    let st = status::load(tmp.path()).unwrap();

    assert!(!st.unstaged.iter().any(|f| f.path == "ignored.log"));
    assert!(!st.staged.iter().any(|f| f.path == "ignored.log"));
    assert!(st.unstaged.iter().any(|f| f.path == ".gitignore"));
}

#[test]
fn is_dirty_ignores_a_worktree_nested_in_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    let repo = git2::Repository::init(&root).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    // Relative worktree base (worktrees.md §6): the linked worktree lands inside
    // the workdir, where libgit2 reports it as one untracked directory entry —
    // the panel hides it, so the checkout auto-stash must not see it either.
    repo.worktree("nested", &root.join("nested"), None).unwrap();

    assert!(status::load_repo(&repo).unwrap().unstaged.is_empty());
    assert!(!status::is_dirty(&repo).unwrap());

    fs::write(root.join("b.txt"), "new\n").unwrap();
    assert!(
        status::is_dirty(&repo).unwrap(),
        "a real change still counts"
    );
}
