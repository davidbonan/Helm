use std::fs;
use std::path::Path;

use helm::git::status::{self, ChangeKind};
use helm::git::{discard, stage};

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
fn discard_restores_a_modified_tracked_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    fs::write(tmp.path().join("a.txt"), "v2\n").unwrap();

    discard::discard_file(&repo, "a.txt").unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "v1\n"
    );
    let st = status::load(tmp.path()).unwrap();
    assert!(!st.unstaged.iter().any(|f| f.path == "a.txt"));
}

#[test]
fn discard_deletes_an_untracked_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("b.txt"), "scratch").unwrap();

    discard::discard_file(&repo, "b.txt").unwrap();

    assert!(!tmp.path().join("b.txt").exists());
    let st = status::load(tmp.path()).unwrap();
    assert!(!st.unstaged.iter().any(|f| f.path == "b.txt"));
}

#[test]
fn discard_keeps_the_staged_part_of_a_partially_staged_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    fs::write(tmp.path().join("a.txt"), "v2\n").unwrap();
    stage::stage(&repo, "a.txt").unwrap();
    fs::write(tmp.path().join("a.txt"), "v3\n").unwrap();

    discard::discard_file(&repo, "a.txt").unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "v2\n",
        "the working tree is restored to the indexed version, not HEAD"
    );
    let st = status::load(tmp.path()).unwrap();
    assert!(
        !st.unstaged.iter().any(|f| f.path == "a.txt"),
        "the unstaged delta is gone"
    );
    assert!(
        st.staged.iter().any(|f| f.path == "a.txt"),
        "the staged part is preserved"
    );
}

#[test]
fn discard_all_reverts_every_unstaged_change() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    fs::write(tmp.path().join("a.txt"), "changed\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();

    discard::discard_all(&repo).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "v1\n"
    );
    assert!(!tmp.path().join("new.txt").exists());
    let st = status::load(tmp.path()).unwrap();
    assert!(st.unstaged.is_empty(), "no unstaged change remains");
}

#[test]
fn discard_restores_a_renamed_file_to_its_old_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    let before = status::load(tmp.path()).unwrap();
    assert!(
        before
            .unstaged
            .iter()
            .any(|f| f.path == "new.txt" && f.kind == ChangeKind::Renamed),
        "precondition: the move is detected as a rename, got {:?}",
        before.unstaged
    );

    discard::discard_file(&repo, "new.txt").unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("old.txt")).unwrap(),
        "line1\nline2\nline3\nline4\n",
        "the old path is restored from the index"
    );
    assert!(!tmp.path().join("new.txt").exists());
    let st = status::load(tmp.path()).unwrap();
    assert!(st.unstaged.is_empty(), "got {:?}", st.unstaged);
}

#[test]
fn discard_all_restores_a_renamed_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();

    discard::discard_all(&repo).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("old.txt")).unwrap(),
        "line1\nline2\nline3\nline4\n"
    );
    assert!(!tmp.path().join("new.txt").exists());
    let st = status::load(tmp.path()).unwrap();
    assert!(st.unstaged.is_empty(), "got {:?}", st.unstaged);
}

#[test]
fn discard_treats_paths_literally_not_as_globs() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a1.txt", "v1\n", "init a1");
    commit_file(&repo, "a[1].txt", "v1\n", "init bracket");
    fs::write(tmp.path().join("a1.txt"), "changed\n").unwrap();
    fs::write(tmp.path().join("a[1].txt"), "changed\n").unwrap();

    discard::discard_file(&repo, "a[1].txt").unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a[1].txt")).unwrap(),
        "v1\n",
        "the literal target is restored"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("a1.txt")).unwrap(),
        "changed\n",
        "a sibling matching the glob 'a[1].txt' must not be reverted"
    );
}

#[test]
fn discard_hunk_reverts_only_that_hunk() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    commit_file(&repo, "a.txt", base, "init");
    fs::write(
        tmp.path().join("a.txt"),
        "1\nA\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n",
    )
    .unwrap();

    stage::discard_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n",
        "the first hunk is reverted to the index, the second is left untouched"
    );
    let st = status::load(tmp.path()).unwrap();
    assert!(
        st.unstaged.iter().any(|f| f.path == "a.txt"),
        "the second hunk's change still shows as unstaged"
    );
}

#[test]
fn discard_hunk_reverts_to_the_index_not_head() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    commit_file(&repo, "a.txt", base, "init");
    fs::write(
        tmp.path().join("a.txt"),
        "1\nA\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n",
    )
    .unwrap();
    // Stage the first hunk (A); only the second (B) stays unstaged.
    stage::stage_hunk(&repo, "a.txt", 0).unwrap();

    stage::discard_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "1\nA\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n",
        "the unstaged hunk reverts to the index (A kept), not to HEAD"
    );
    let st = status::load(tmp.path()).unwrap();
    assert!(
        st.staged.iter().any(|f| f.path == "a.txt"),
        "the staged hunk is preserved"
    );
    assert!(
        !st.unstaged.iter().any(|f| f.path == "a.txt"),
        "no unstaged change remains"
    );
}

#[test]
fn discard_hunk_of_an_untracked_file_deletes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();

    stage::discard_hunk(&repo, "new.txt", 0).unwrap();

    assert!(
        !tmp.path().join("new.txt").exists(),
        "a whole-file addition has no index side to revert onto, so the discard \
         removes the file like the file-level Discard"
    );
    let st = status::load(tmp.path()).unwrap();
    assert!(!st.unstaged.iter().any(|f| f.path == "new.txt"));
}

#[test]
fn discard_all_leaves_a_conflict_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "c.txt", "conflicted\n", "init");
    // Mark c.txt as a conflict in the index (3 stages) without touching the workdir.
    let mut index = repo.index().unwrap();
    let blob = repo.blob(b"base\n").unwrap();
    for stage in 1..=3u16 {
        let entry = git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: blob,
            flags: stage << 12,
            flags_extended: 0,
            path: b"c.txt".to_vec(),
        };
        index.add(&entry).unwrap();
    }
    index.write().unwrap();
    assert!(status::load(tmp.path())
        .unwrap()
        .unstaged
        .iter()
        .any(|f| f.path == "c.txt" && f.kind == ChangeKind::Conflicted));

    discard::discard_all(&repo).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(
        st.unstaged
            .iter()
            .any(|f| f.path == "c.txt" && f.kind == ChangeKind::Conflicted),
        "a conflict is read-only and survives discard-all"
    );
}
