use std::fs;
use std::path::Path;

use helm::git::commit;
use helm::git::diff::{self, DiffSource, LineOrigin};
use helm::git::stage;
use helm::git::status;

fn commit_file(repo: &git2::Repository, name: &str, content: &str, message: &str) -> git2::Oid {
    commit_bytes(repo, name, content.as_bytes(), message)
}

fn commit_bytes(repo: &git2::Repository, name: &str, content: &[u8], message: &str) -> git2::Oid {
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
fn stage_moves_untracked_file_to_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();

    stage::stage(&repo, "a.txt").unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    assert!(!st.unstaged.iter().any(|f| f.path == "a.txt"));
}

#[test]
fn stage_moves_modified_file_to_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    fs::write(tmp.path().join("a.txt"), "v2\n").unwrap();

    stage::stage(&repo, "a.txt").unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    assert!(!st.unstaged.iter().any(|f| f.path == "a.txt"));
}

#[test]
fn stage_records_a_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    fs::remove_file(tmp.path().join("a.txt")).unwrap();

    stage::stage(&repo, "a.txt").unwrap();

    let st = status::load(tmp.path()).unwrap();
    let staged = st
        .staged
        .iter()
        .find(|f| f.path == "a.txt")
        .expect("deletion should be staged");
    assert_eq!(staged.kind, status::ChangeKind::Deleted);
    assert!(!st.unstaged.iter().any(|f| f.path == "a.txt"));
}

#[test]
fn unstage_moves_staged_change_back_to_unstaged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");
    fs::write(tmp.path().join("a.txt"), "v2\n").unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    stage::unstage(&repo, "a.txt").unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));
    assert!(!st.staged.iter().any(|f| f.path == "a.txt"));
}

#[test]
fn stage_all_hides_and_skips_a_worktree_nested_in_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    let repo = git2::Repository::init(&root).unwrap();
    commit_file(&repo, "a.txt", "v1\n", "init");

    // A worktree placed INSIDE the repo root is an embedded repo (a directory
    // with a `.git` file): libgit2 reports it as a single untracked entry.
    repo.worktree("nested", &root.join("nested"), None).unwrap();
    fs::write(root.join("b.txt"), "new\n").unwrap();

    let before = status::load_repo(&repo).unwrap();
    assert!(
        !before.unstaged.iter().any(|f| f.path.starts_with("nested")),
        "the nested worktree must not show up in the unstaged list, got {:?}",
        before.unstaged
    );

    stage::stage_all(&repo).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(
        st.staged.iter().any(|f| f.path == "b.txt"),
        "stage all still stages the real change, got {:?}",
        st.staged
    );
    assert!(
        !st.staged.iter().any(|f| f.path.starts_with("nested")),
        "the nested worktree is never staged, got {:?}",
        st.staged
    );
}

/// A plain clone nested in the workdir is **not** a linked worktree, so
/// `nested_in_workdir` does not filter it out: libgit2 reports it as a single
/// untracked directory entry that `add_path` / `remove_file` refuse.
fn nested_clone(root: &Path) {
    let vendor = root.join("vendor");
    git2::Repository::init(&vendor).unwrap();
    fs::write(vendor.join("f.txt"), "x\n").unwrap();
}

#[test]
fn stage_all_reports_a_failure_without_dropping_the_other_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    let repo = git2::Repository::init(&root).unwrap();
    commit_file(&repo, "base.txt", "v1\n", "init");
    // Sorted around the failing entry: one before, one after.
    fs::write(root.join("a.txt"), "new\n").unwrap();
    nested_clone(&root);
    fs::write(root.join("z.txt"), "new\n").unwrap();

    stage::stage_all(&repo).unwrap_err();

    // Read from a fresh handle: what actually landed in the on-disk index, not
    // the mutating handle's cached copy.
    let st = status::load(&root).unwrap();
    assert!(
        st.staged.iter().any(|f| f.path == "a.txt"),
        "the entry staged before the failure must be written, got {:?}",
        st.staged
    );
    assert!(
        st.staged.iter().any(|f| f.path == "z.txt"),
        "the batch must not abort at the failing entry, got {:?}",
        st.staged
    );
    assert!(
        !st.staged.iter().any(|f| f.path.starts_with("vendor")),
        "the nested clone is never staged, got {:?}",
        st.staged
    );
}

#[test]
fn a_failed_stage_all_never_leaks_into_the_next_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    git2::Repository::init(&root).unwrap();
    // Worker-style long-lived handle: every mutation goes through the same
    // cached index (worker.rs).
    let repo = git2::Repository::open(&root).unwrap();
    commit_file(&repo, "base.txt", "v1\n", "init");
    fs::write(root.join("a.txt"), "new\n").unwrap();
    nested_clone(&root);

    stage::stage_all(&repo).unwrap_err();

    // What the on-disk index holds — what `git status` in a terminal pane shows.
    let staged: Vec<String> = status::load(&root)
        .unwrap()
        .staged
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let oid = commit::commit(&repo, "batch").unwrap();
    let tree = repo.find_commit(oid).unwrap().tree().unwrap();
    assert_eq!(
        tree.get_path(Path::new("a.txt")).is_ok(),
        staged.iter().any(|p| p == "a.txt"),
        "the commit must carry exactly what is staged on disk, got {staged:?}"
    );
    assert!(tree.get_path(Path::new("vendor")).is_err());
}

#[test]
fn stage_a_renamed_file_stages_both_sides() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    let before = status::load_repo(&repo).unwrap();
    assert!(
        before
            .unstaged
            .iter()
            .any(|f| f.path == "new.txt" && f.kind == status::ChangeKind::Renamed),
        "precondition: the move is detected as a rename, got {:?}",
        before.unstaged
    );

    stage::stage(&repo, "new.txt").unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(
        st.unstaged.is_empty(),
        "the old path's deletion is staged with the new path, got {:?}",
        st.unstaged
    );
    assert!(
        st.staged
            .iter()
            .any(|f| f.path == "new.txt" && f.kind == status::ChangeKind::Renamed),
        "got {:?}",
        st.staged
    );
}

#[test]
fn unstage_a_renamed_file_unstages_both_sides() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();
    let before = status::load_repo(&repo).unwrap();
    assert!(
        before
            .staged
            .iter()
            .any(|f| f.path == "new.txt" && f.kind == status::ChangeKind::Renamed),
        "precondition: the staged move is detected as a rename, got {:?}",
        before.staged
    );

    stage::unstage(&repo, "new.txt").unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(
        st.staged.is_empty(),
        "the old path's deletion leaves the index with the new path, got {:?}",
        st.staged
    );
    assert!(
        st.unstaged
            .iter()
            .any(|f| f.path == "new.txt" && f.kind == status::ChangeKind::Renamed),
        "got {:?}",
        st.unstaged
    );
}

#[test]
fn stage_all_covers_every_change_including_renames() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");
    commit_file(&repo, "mod.txt", "v1\n", "add mod");
    commit_file(&repo, "gone.txt", "bye\n", "add gone");
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    fs::write(tmp.path().join("mod.txt"), "v2\n").unwrap();
    fs::remove_file(tmp.path().join("gone.txt")).unwrap();
    fs::write(tmp.path().join("fresh.txt"), "hello\n").unwrap();

    stage::stage_all(&repo).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.unstaged.is_empty(), "got {:?}", st.unstaged);
    let kinds: Vec<(&str, status::ChangeKind)> = st
        .staged
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    assert!(kinds.contains(&("new.txt", status::ChangeKind::Renamed)));
    assert!(kinds.contains(&("mod.txt", status::ChangeKind::Modified)));
    assert!(kinds.contains(&("gone.txt", status::ChangeKind::Deleted)));
    assert!(kinds.contains(&("fresh.txt", status::ChangeKind::Added)));
}

#[test]
fn unstage_all_resets_every_change_including_renames() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");
    commit_file(&repo, "mod.txt", "v1\n", "add mod");
    fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();
    fs::write(tmp.path().join("mod.txt"), "v2\n").unwrap();
    stage::stage_all(&repo).unwrap();
    assert!(status::load_repo(&repo).unwrap().unstaged.is_empty());

    stage::unstage_all(&repo).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.staged.is_empty(), "got {:?}", st.staged);
    let kinds: Vec<(&str, status::ChangeKind)> = st
        .unstaged
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    assert!(kinds.contains(&("new.txt", status::ChangeKind::Renamed)));
    assert!(kinds.contains(&("mod.txt", status::ChangeKind::Modified)));
}

#[test]
fn stage_all_leaves_a_conflict_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "c.txt", "conflicted\n", "init");
    let mut index = repo.index().unwrap();
    let blob = repo.blob(b"base\n").unwrap();
    for stage_no in 1..=3u16 {
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
            flags: stage_no << 12,
            flags_extended: 0,
            path: b"c.txt".to_vec(),
        };
        index.add(&entry).unwrap();
    }
    index.write().unwrap();

    stage::stage_all(&repo).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(
        st.unstaged
            .iter()
            .any(|f| f.path == "c.txt" && f.kind == status::ChangeKind::Conflicted),
        "a conflict is read-only and survives stage-all (resolution happens in the terminal)"
    );
}

fn line_contents(
    repo: &git2::Repository,
    path: &str,
    source: DiffSource,
    origin: LineOrigin,
) -> Vec<String> {
    diff::file_diff(repo, path, source)
        .unwrap()
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.origin == origin)
        .map(|l| l.content.trim_end().to_string())
        .collect()
}

#[test]
fn stage_the_only_hunk_moves_file_to_staged_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "keep\nold\ntail\n", "init");
    fs::write(tmp.path().join("a.txt"), "keep\nnew\ntail\n").unwrap();

    let before = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(before.hunks.len(), 1);

    stage::stage_hunk(&repo, "a.txt", 0).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    assert!(!st.unstaged.iter().any(|f| f.path == "a.txt"));
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["new"]
    );
    assert!(
        diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
            .unwrap()
            .hunks
            .is_empty(),
        "the last unstaged hunk disappeared from the Unstaged diff"
    );
}

#[test]
fn unstage_the_only_hunk_moves_file_to_unstaged_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "keep\nold\ntail\n", "init");
    fs::write(tmp.path().join("a.txt"), "keep\nnew\ntail\n").unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    let before = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(before.hunks.len(), 1);

    stage::unstage_hunk(&repo, "a.txt", 0).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));
    assert!(!st.staged.iter().any(|f| f.path == "a.txt"));
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["new"]
    );
    assert!(
        diff::file_diff(&repo, "a.txt", DiffSource::Staged)
            .unwrap()
            .hunks
            .is_empty(),
        "the last staged hunk disappeared from the Staged diff"
    );
}

#[test]
fn stage_the_only_untracked_hunk_moves_file_to_staged_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();

    let before = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(before.hunks.len(), 1);

    stage::stage_hunk(&repo, "new.txt", 0).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "new.txt"));
    assert!(!st.unstaged.iter().any(|f| f.path == "new.txt"));
}

#[test]
fn unstage_the_only_added_file_hunk_moves_file_to_unstaged_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();
    stage::stage(&repo, "new.txt").unwrap();

    let before = diff::file_diff(&repo, "new.txt", DiffSource::Staged).unwrap();
    assert_eq!(before.hunks.len(), 1);

    stage::unstage_hunk(&repo, "new.txt", 0).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.unstaged.iter().any(|f| f.path == "new.txt"));
    assert!(!st.staged.iter().any(|f| f.path == "new.txt"));
}

#[test]
fn stage_one_hunk_of_two_leaves_file_in_both_sections() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    commit_file(&repo, "a.txt", base, "init");
    let edited = "1\nA\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n";
    fs::write(tmp.path().join("a.txt"), edited).unwrap();

    let before = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(
        before.hunks.len(),
        2,
        "two changes 10 lines apart are two hunks"
    );

    stage::stage_hunk(&repo, "a.txt", 0).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(
        st.staged.iter().any(|f| f.path == "a.txt"),
        "first hunk staged"
    );
    assert!(
        st.unstaged.iter().any(|f| f.path == "a.txt"),
        "second hunk still unstaged"
    );

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["A"],
        "only the first hunk's addition is staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["B"],
        "only the second hunk's addition remains unstaged"
    );
}

#[test]
fn stage_last_hunk_after_earlier_insertion() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    commit_file(&repo, "a.txt", base, "init");
    let edited = "1\n2\nINSERTED\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n";
    fs::write(tmp.path().join("a.txt"), edited).unwrap();

    let before = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(before.hunks.len(), 2);

    stage::stage_hunk(&repo, "a.txt", 1).unwrap();

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["B"],
        "the last hunk is staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["INSERTED"],
        "the earlier insertion remains unstaged"
    );
}

#[test]
fn stage_single_line_of_a_hunk_stages_only_that_line() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "1\n2\n3\n", "init");
    fs::write(tmp.path().join("a.txt"), "1\nADDED\n2\n3\nTAIL\n").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(d.hunks.len(), 1);
    let added_idx = d.hunks[0]
        .lines
        .iter()
        .position(|l| l.origin == LineOrigin::Addition && l.content.trim_end() == "ADDED")
        .unwrap();

    stage::stage_lines(&repo, "a.txt", 0, &[added_idx]).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["ADDED"],
        "only the selected line is staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["TAIL"],
        "the unselected line stays unstaged"
    );
}

#[test]
fn stage_single_line_of_an_untracked_file_stages_only_that_line() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();

    let d = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(d.hunks.len(), 1);
    assert_eq!(d.hunks[0].lines.len(), 2);

    stage::stage_lines(&repo, "new.txt", 0, &[0]).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "new.txt"));
    assert!(st.unstaged.iter().any(|f| f.path == "new.txt"));
    assert_eq!(
        line_contents(&repo, "new.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["one"],
        "only the selected line is staged"
    );
    assert_eq!(
        line_contents(&repo, "new.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["two"],
        "the unselected line stays unstaged"
    );
}

#[test]
fn stage_an_untracked_file_line_by_line_stages_it_fully() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();

    stage::stage_lines(&repo, "new.txt", 0, &[0]).unwrap();

    // After the first stage, the unstaged diff is recomputed: the remaining line
    // forms its own single-line hunk.
    let d = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(d.hunks.len(), 1);
    let remaining = d.hunks[0]
        .lines
        .iter()
        .position(|l| l.origin == LineOrigin::Addition)
        .unwrap();
    stage::stage_lines(&repo, "new.txt", 0, &[remaining]).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "new.txt"));
    assert!(!st.unstaged.iter().any(|f| f.path == "new.txt"));
    assert_eq!(
        line_contents(&repo, "new.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["one", "two"]
    );
}

#[test]
fn unstage_single_line_of_a_hunk_unstages_only_that_line() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "1\n2\n3\n", "init");
    fs::write(tmp.path().join("a.txt"), "1\nADDED\n2\n3\nTAIL\n").unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(d.hunks.len(), 1);
    let added_idx = d.hunks[0]
        .lines
        .iter()
        .position(|l| l.origin == LineOrigin::Addition && l.content.trim_end() == "ADDED")
        .unwrap();

    stage::unstage_lines(&repo, "a.txt", 0, &[added_idx]).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["TAIL"],
        "the unselected line remains staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["ADDED"],
        "only the selected line is unstaged"
    );
}

#[test]
fn unstage_a_new_staged_file_line_by_line_returns_it_to_untracked() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();
    stage::stage(&repo, "new.txt").unwrap();

    let first_change = |repo: &git2::Repository| {
        diff::file_diff(repo, "new.txt", DiffSource::Staged)
            .unwrap()
            .hunks[0]
            .lines
            .iter()
            .position(|l| l.origin != LineOrigin::Context)
            .unwrap()
    };
    stage::unstage_lines(&repo, "new.txt", 0, &[first_change(&repo)]).unwrap();
    stage::unstage_lines(&repo, "new.txt", 0, &[first_change(&repo)]).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(
        !st.staged.iter().any(|f| f.path == "new.txt"),
        "no empty blob must remain staged after the last line is unstaged"
    );
    assert!(st.unstaged.iter().any(|f| f.path == "new.txt"));
    assert_eq!(
        line_contents(&repo, "new.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["one", "two"]
    );
}

#[test]
fn unstage_the_added_line_of_a_modified_pair_unstages_only_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "keep\nold\ntail\n", "init");
    fs::write(tmp.path().join("a.txt"), "keep\nnew\ntail\n").unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    let added_idx = d.hunks[0]
        .lines
        .iter()
        .position(|l| l.origin == LineOrigin::Addition)
        .unwrap();

    stage::unstage_lines(&repo, "a.txt", 0, &[added_idx]).unwrap();

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Deletion),
        vec!["old"],
        "the deletion stays staged"
    );
    assert!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition).is_empty(),
        "the addition is no longer staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["new"],
        "the addition is back in unstaged"
    );
}

#[test]
fn unstage_single_line_of_a_staged_file_deletion_restores_it_to_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo\n", "init");
    fs::remove_file(tmp.path().join("a.txt")).unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(d.hunks.len(), 1);
    assert!(d.hunks[0]
        .lines
        .iter()
        .all(|l| l.origin == LineOrigin::Deletion));

    stage::unstage_lines(&repo, "a.txt", 0, &[0]).unwrap();

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Deletion),
        vec!["two"],
        "only the unselected deletion stays staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Deletion),
        vec!["one"],
        "the restored line is deleted in the worktree, so it shows unstaged"
    );
}

#[test]
fn unstage_one_hunk_of_two_leaves_other_hunk_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    commit_file(&repo, "a.txt", base, "init");
    let edited = "1\nA\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n";
    fs::write(tmp.path().join("a.txt"), edited).unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    let staged = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(staged.hunks.len(), 2);

    stage::unstage_hunk(&repo, "a.txt", 0).unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(
        st.staged.iter().any(|f| f.path == "a.txt"),
        "second hunk still staged"
    );
    assert!(
        st.unstaged.iter().any(|f| f.path == "a.txt"),
        "first hunk back in unstaged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["B"],
        "only the second hunk remains staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["A"],
        "first hunk is unstaged again"
    );
}

#[test]
fn unstage_last_hunk_after_earlier_insertion() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n";
    commit_file(&repo, "a.txt", base, "init");
    let edited = "1\n2\nINSERTED\n3\n4\n5\n6\n7\n8\n9\n10\n11\nB\n13\n";
    fs::write(tmp.path().join("a.txt"), edited).unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    let staged = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(staged.hunks.len(), 2);

    stage::unstage_hunk(&repo, "a.txt", 1).unwrap();

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["INSERTED"],
        "the earlier insertion remains staged"
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Unstaged, LineOrigin::Addition),
        vec!["B"],
        "the last hunk is back in unstaged"
    );
}

#[test]
fn stage_modified_line_stages_deletion_and_addition_together() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "keep\nold\ntail\n", "init");
    fs::write(tmp.path().join("a.txt"), "keep\nnew\ntail\n").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    let indices: Vec<usize> = d.hunks[0]
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.origin != LineOrigin::Context)
        .map(|(i, _)| i)
        .collect();

    stage::stage_lines(&repo, "a.txt", 0, &indices).unwrap();

    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Deletion),
        vec!["old"]
    );
    assert_eq!(
        line_contents(&repo, "a.txt", DiffSource::Staged, LineOrigin::Addition),
        vec!["new"]
    );
    assert!(
        diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
            .unwrap()
            .hunks
            .is_empty(),
        "nothing left unstaged once the whole change is staged"
    );
}

#[test]
fn unstage_on_unborn_head_removes_from_index() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();
    stage::stage(&repo, "a.txt").unwrap();
    assert!(status::load(tmp.path())
        .unwrap()
        .staged
        .iter()
        .any(|f| f.path == "a.txt"));

    stage::unstage(&repo, "a.txt").unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));
    assert!(!st.staged.iter().any(|f| f.path == "a.txt"));
}

#[test]
fn stage_sees_index_changes_made_by_another_handle() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base", "init");
    // b.txt tracked + modified: staging it goes straight to add_path/write,
    // with no status pass that would refresh the index as a side effect.
    commit_file(&repo, "b.txt", "b", "add b");
    fs::write(tmp.path().join("a.txt"), "a").unwrap();
    fs::write(tmp.path().join("b.txt"), "b v2").unwrap();

    // Worker-style long-lived handle: force-load its in-memory index snapshot.
    let worker = git2::Repository::open(tmp.path()).unwrap();
    worker.index().unwrap();

    // Terminal-style external staging (a separate handle, like `git add` in a pane).
    let external = git2::Repository::open(tmp.path()).unwrap();
    let mut index = external.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();

    // Stage b.txt through the long-lived handle: a.txt's entry must survive.
    stage::stage(&worker, "b.txt").unwrap();

    let st = status::load(tmp.path()).unwrap();
    assert!(
        st.staged.iter().any(|f| f.path == "a.txt"),
        "stage through a stale handle clobbered the externally staged a.txt"
    );
    assert!(st.staged.iter().any(|f| f.path == "b.txt"));
}

/// Bytes of `path` as recorded in the on-disk index (read through a fresh handle).
fn staged_bytes(dir: &Path, path: &str) -> Vec<u8> {
    let repo = git2::Repository::open(dir).unwrap();
    let index = repo.index().unwrap();
    let entry = index
        .get_path(Path::new(path), 0)
        .expect("path must be staged");
    let blob = repo.find_blob(entry.id).unwrap();
    blob.content().to_vec()
}

/// The three "no final newline" shapes: the `\ No newline at end of file` marker
/// is a marker, not a diff line — staging the tail hunk must stage the working
/// tree bytes verbatim.
fn stage_tail_hunk_is_byte_identical(head: &str, worktree: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", head, "init");
    fs::write(tmp.path().join("a.txt"), worktree).unwrap();

    stage::stage_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(
        staged_bytes(tmp.path(), "a.txt"),
        worktree.as_bytes(),
        "staged blob must match the working tree byte for byte"
    );
}

#[test]
fn stage_hunk_adds_a_missing_final_newline() {
    stage_tail_hunk_is_byte_identical("one\ntwo", "one\ntwo\n");
}

#[test]
fn stage_hunk_removes_a_final_newline() {
    stage_tail_hunk_is_byte_identical("one\ntwo\n", "one\ntwo");
}

#[test]
fn stage_hunk_edits_a_tail_line_that_never_had_a_final_newline() {
    stage_tail_hunk_is_byte_identical("one\ntwo", "one\nTWO");
}

#[test]
fn stage_lines_on_a_tail_without_a_final_newline_is_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo", "init");
    fs::write(tmp.path().join("a.txt"), "one\nTWO").unwrap();

    let hunk = &diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
        .unwrap()
        .hunks[0];
    let changed: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.origin != LineOrigin::Context)
        .map(|(idx, _)| idx)
        .collect();

    stage::stage_lines(&repo, "a.txt", 0, &changed).unwrap();

    assert_eq!(staged_bytes(tmp.path(), "a.txt"), b"one\nTWO");
}

#[test]
fn a_missing_final_newline_marker_is_not_a_diff_line() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo", "init");
    fs::write(tmp.path().join("a.txt"), "one\nTWO").unwrap();

    let hunk = &diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
        .unwrap()
        .hunks[0];

    assert_eq!(
        hunk.lines
            .iter()
            .filter(|l| l.origin == LineOrigin::Addition)
            .count(),
        1,
        "one added line, no phantom marker row"
    );
    assert_eq!(
        hunk.lines
            .iter()
            .filter(|l| l.origin == LineOrigin::Deletion)
            .count(),
        1
    );
    assert!(
        !hunk
            .lines
            .iter()
            .any(|l| l.content.contains("No newline at end of file")),
        "the marker must not be materialized as a line"
    );
}

/// Latin-1 payload: `é` `è` `û` `ç` `É` `Ç` as single high bytes. No NUL, so
/// libgit2 does not flag the delta binary and the granular staging pills reach it.
const LATIN1_HEAD: &[u8] = b"one\nqu\xe9bec\ntwo\nfran\xe7ais\n";
const LATIN1_WORKTREE: &[u8] = b"one\nQU\xc9BEC\ntwo\nFRAN\xc7AIS\n";

/// `DiffLine::content` is `from_utf8_lossy`'d for display: staging must re-derive
/// the diff's raw bytes, otherwise every high byte lands in the index as U+FFFD.
#[test]
fn stage_hunk_on_a_non_utf8_file_is_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_bytes(&repo, "a.txt", LATIN1_HEAD, "init");
    fs::write(tmp.path().join("a.txt"), LATIN1_WORKTREE).unwrap();

    stage::stage_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(
        staged_bytes(tmp.path(), "a.txt"),
        LATIN1_WORKTREE,
        "staged blob must match the working tree byte for byte"
    );
}

/// The silent shape: a high byte inside an **added** line only. Every context line
/// is ASCII, so `apply` happily reports `Ok` and the corruption reaches the index.
#[test]
fn stage_hunk_adding_a_non_utf8_line_is_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_bytes(&repo, "a.txt", b"one\ntwo\n", "init");
    let worktree: &[u8] = b"one\ncaf\xe9\ntwo\n";
    fs::write(tmp.path().join("a.txt"), worktree).unwrap();

    stage::stage_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(staged_bytes(tmp.path(), "a.txt"), worktree);
}

/// A line selection stages the chosen lines verbatim **and** leaves the unchosen
/// deletion — re-emitted as a context line — with its own bytes intact.
#[test]
fn stage_lines_on_a_non_utf8_file_is_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_bytes(&repo, "a.txt", LATIN1_HEAD, "init");
    fs::write(tmp.path().join("a.txt"), LATIN1_WORKTREE).unwrap();

    let hunk = &diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
        .unwrap()
        .hunks[0];
    let origins: Vec<LineOrigin> = hunk.lines.iter().map(|l| l.origin).collect();
    assert_eq!(
        origins,
        vec![
            LineOrigin::Context,
            LineOrigin::Deletion,
            LineOrigin::Addition,
            LineOrigin::Context,
            LineOrigin::Deletion,
            LineOrigin::Addition,
        ]
    );

    stage::stage_lines(&repo, "a.txt", 0, &[1, 2]).unwrap();

    assert_eq!(
        staged_bytes(tmp.path(), "a.txt"),
        b"one\nQU\xc9BEC\ntwo\nfran\xe7ais\n",
        "only the first change is staged, both sides byte for byte"
    );
}

/// Non-UTF-8 **and** no final newline: the `*_EOFNL` markers must be filtered out
/// of the raw bytes exactly as `file_diff` filters them out of `hunk.lines`, or the
/// selection indices the UI hands over address the wrong lines.
#[test]
fn stage_lines_on_a_non_utf8_tail_without_a_final_newline_is_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_bytes(&repo, "a.txt", b"one\ncaf\xe9", "init");
    fs::write(tmp.path().join("a.txt"), b"one\nCAF\xc9").unwrap();

    let hunk = &diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
        .unwrap()
        .hunks[0];
    let changed: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.origin != LineOrigin::Context)
        .map(|(idx, _)| idx)
        .collect();
    assert_eq!(changed, vec![1, 2]);

    stage::stage_lines(&repo, "a.txt", 0, &changed).unwrap();

    assert_eq!(staged_bytes(tmp.path(), "a.txt"), b"one\nCAF\xc9");
}

#[test]
fn stage_hunk_of_an_unstaged_rename_moves_the_file_with_that_hunk_only() {
    // The rename's new path is not in the index: the filtered patch has to move
    // the old one (rename headers) instead of declaring a new file, otherwise
    // its context lines have no preimage to apply to.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(
        &repo,
        "old.txt",
        "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20\n",
        "init",
    );
    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(
        tmp.path().join("new.txt"),
        "l1\nFIRST\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nSECOND\nl17\nl18\nl19\nl20\n",
    )
    .unwrap();
    let d = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(d.hunks.len(), 2, "precondition: two separate edits");

    stage::stage_hunk(&repo, "new.txt", 0).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert_eq!(
        st.staged
            .iter()
            .map(|f| (f.path.as_str(), f.kind, f.additions, f.deletions))
            .collect::<Vec<_>>(),
        vec![("new.txt", status::ChangeKind::Renamed, 1, 1)],
        "the rename lands staged carrying the first hunk only"
    );
    assert_eq!(
        st.unstaged
            .iter()
            .map(|f| (f.path.as_str(), f.kind, f.additions, f.deletions))
            .collect::<Vec<_>>(),
        vec![("new.txt", status::ChangeKind::Modified, 1, 1)],
        "the second hunk stays unstaged on the new path"
    );
}

#[test]
fn unstage_hunk_of_a_staged_rename_keeps_the_move_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "l1\nl2\nl3\nl4\n", "init");
    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(tmp.path().join("new.txt"), "l1\nCHANGED\nl3\nl4\n").unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();

    stage::unstage_hunk(&repo, "new.txt", 0).unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert_eq!(
        st.staged
            .iter()
            .map(|f| (f.path.as_str(), f.kind, f.additions, f.deletions))
            .collect::<Vec<_>>(),
        vec![("new.txt", status::ChangeKind::Renamed, 0, 0)],
        "the move itself stays staged, stripped of the edit"
    );
    assert_eq!(
        st.unstaged
            .iter()
            .map(|f| (f.path.as_str(), f.kind, f.additions, f.deletions))
            .collect::<Vec<_>>(),
        vec![("new.txt", status::ChangeKind::Modified, 1, 1)],
    );
}

fn commit_symlink(repo: &git2::Repository, name: &str, target: &str, message: &str) {
    let dir = repo.workdir().unwrap();
    std::os::unix::fs::symlink(target, dir.join(name)).unwrap();
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
        .unwrap();
}

fn repoint_symlink(dir: &Path, name: &str, target: &str) {
    fs::remove_file(dir.join(name)).unwrap();
    std::os::unix::fs::symlink(target, dir.join(name)).unwrap();
}

#[test]
fn stage_a_symlink_repointed_at_a_missing_target_stages_the_modification() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "here.txt", "v1\n", "init");
    commit_symlink(&repo, "link", "here.txt", "add link");
    repoint_symlink(tmp.path(), "link", "not-yet-there.txt");

    stage::stage(&repo, "link").unwrap();

    let st = status::load_repo(&repo).unwrap();
    assert!(st.unstaged.is_empty(), "got {:?}", st.unstaged);
    assert_eq!(
        st.staged
            .iter()
            .map(|f| (f.path.as_str(), f.kind))
            .collect::<Vec<_>>(),
        vec![("link", status::ChangeKind::Modified)],
        "the dangling link is staged as its new target, not as a deletion",
    );
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let index = repo.index().unwrap();
    let entry = index.get_path(Path::new("link"), 0).unwrap();
    assert_eq!(
        repo.find_blob(entry.id).unwrap().content(),
        b"not-yet-there.txt",
    );
    assert_ne!(
        entry.id,
        head.tree()
            .unwrap()
            .get_path(Path::new("link"))
            .unwrap()
            .id()
    );
}

#[test]
fn stage_all_agrees_with_stage_on_a_symlink_repointed_at_a_missing_target() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "here.txt", "v1\n", "init");
    commit_symlink(&repo, "link", "here.txt", "add link");
    repoint_symlink(tmp.path(), "link", "not-yet-there.txt");

    stage::stage_all(&repo).unwrap();
    let batch = status::load_repo(&repo).unwrap();

    stage::unstage(&repo, "link").unwrap();
    stage::stage(&repo, "link").unwrap();
    let per_file = status::load_repo(&repo).unwrap();

    assert_eq!(batch.staged, per_file.staged);
    assert_eq!(batch.unstaged, per_file.unstaged);
}

fn index_mode(repo: &git2::Repository, path: &str) -> u32 {
    repo.index()
        .unwrap()
        .get_path(Path::new(path), 0)
        .unwrap()
        .mode
}

fn write_executable(dir: &Path, name: &str, content: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn partial_stage_of_an_untracked_executable_file_keeps_the_exec_bit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    write_executable(tmp.path(), "run.sh", "one\ntwo\n");

    stage::stage_lines(&repo, "run.sh", 0, &[0]).unwrap();

    assert_eq!(
        index_mode(&repo, "run.sh"),
        0o100755,
        "the partial patch must declare the working tree's exec bit"
    );
    assert_eq!(
        line_contents(&repo, "run.sh", DiffSource::Staged, LineOrigin::Addition),
        vec!["one"],
    );
}

#[test]
fn partial_stage_of_an_untracked_plain_file_stays_non_executable() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    fs::write(tmp.path().join("plain.txt"), "one\ntwo\n").unwrap();

    stage::stage_lines(&repo, "plain.txt", 0, &[0]).unwrap();

    assert_eq!(index_mode(&repo, "plain.txt"), 0o100644);
}

#[test]
fn partial_stage_agrees_with_stage_when_core_filemode_is_off() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.filemode", false)
        .unwrap();
    commit_file(&repo, "base.txt", "base\n", "init");
    write_executable(tmp.path(), "run.sh", "one\ntwo\n");
    write_executable(tmp.path(), "whole.sh", "one\ntwo\n");

    stage::stage_lines(&repo, "run.sh", 0, &[0]).unwrap();
    stage::stage(&repo, "whole.sh").unwrap();

    assert_eq!(
        index_mode(&repo, "run.sh"),
        index_mode(&repo, "whole.sh"),
        "a repo that cannot carry the exec bit must not gain one from a partial stage"
    );
    assert_eq!(index_mode(&repo, "run.sh"), 0o100644);
}

#[test]
fn whole_file_stage_of_an_untracked_executable_keeps_the_exec_bit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    write_executable(tmp.path(), "run.sh", "one\ntwo\n");

    stage::stage(&repo, "run.sh").unwrap();

    assert_eq!(index_mode(&repo, "run.sh"), 0o100755);
}

/// `git apply --cached` of a single hunk leaves the entry's mode alone; a filtered
/// patch that says nothing about the mode makes libgit2 re-record the default blob
/// mode, so a tracked script loses its exec bit one hunk at a time.
#[test]
fn staging_a_hunk_of_a_tracked_executable_keeps_the_exec_bit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let body: String = (1..=12).map(|i| format!("line{i}\n")).collect();
    write_executable(tmp.path(), "run.sh", &body);
    commit_file(&repo, "run.sh", &body, "init");
    assert_eq!(index_mode(&repo, "run.sh"), 0o100755);

    let edited = body
        .replace("line1\n", "LINE1\n")
        .replace("line12\n", "LINE12\n");
    fs::write(tmp.path().join("run.sh"), &edited).unwrap();
    assert_eq!(
        diff::file_diff(&repo, "run.sh", DiffSource::Unstaged)
            .unwrap()
            .hunks
            .len(),
        2,
        "the fixture must have two separate hunks"
    );

    stage::stage_hunk(&repo, "run.sh", 0).unwrap();

    assert_eq!(
        index_mode(&repo, "run.sh"),
        0o100755,
        "staging a hunk dropped the exec bit the entry already had"
    );
}

#[test]
fn unstaging_a_hunk_of_a_tracked_executable_keeps_the_exec_bit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let body: String = (1..=12).map(|i| format!("line{i}\n")).collect();
    write_executable(tmp.path(), "run.sh", &body);
    commit_file(&repo, "run.sh", &body, "init");

    let edited = body
        .replace("line1\n", "LINE1\n")
        .replace("line12\n", "LINE12\n");
    fs::write(tmp.path().join("run.sh"), &edited).unwrap();
    stage::stage(&repo, "run.sh").unwrap();
    assert_eq!(index_mode(&repo, "run.sh"), 0o100755);

    stage::unstage_hunk(&repo, "run.sh", 0).unwrap();

    assert_eq!(
        index_mode(&repo, "run.sh"),
        0o100755,
        "unstaging a hunk dropped the exec bit the entry already had"
    );
}

/// The diff of an untracked symlink shows what the link points **at**, so it can
/// carry several lines and the panel can offer a line selection on it. Staging
/// part of it must still record a link, not a regular blob holding the target's
/// content — the parity `stage` already keeps (git.md §2).
#[test]
fn partially_staging_an_untracked_symlink_stages_the_link() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "target.txt", "alpha\nbravo\ncharlie\n", "init");
    std::os::unix::fs::symlink("target.txt", tmp.path().join("link")).unwrap();

    stage::stage_lines(&repo, "link", 0, &[0]).unwrap();

    assert_eq!(index_mode(&repo, "link"), 0o120000);
    let entry = {
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        index.get_path(Path::new("link"), 0).unwrap()
    };
    assert_eq!(
        repo.find_blob(entry.id).unwrap().content(),
        b"target.txt",
        "the link was staged as a file holding its target's content"
    );
}

/// `git add` normalises an untracked CRLF file under `text=auto`; so does the
/// whole-file `stage`. Partial staging must record the same bytes rather than what
/// the file happens to hold on disk.
#[test]
fn partially_staging_an_untracked_file_goes_through_the_repos_filters() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, ".gitattributes", "* text=auto\n", "attrs");
    fs::write(tmp.path().join("crlf.txt"), "one\r\ntwo\r\nthree\r\n").unwrap();

    stage::stage_lines(&repo, "crlf.txt", 0, &[0, 1]).unwrap();

    let entry = {
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        index.get_path(Path::new("crlf.txt"), 0).unwrap()
    };
    assert_eq!(
        repo.find_blob(entry.id).unwrap().content(),
        b"one\ntwo\n",
        "the selection was staged with the CRLF `git add` normalises away"
    );
}

/// Reverse-applying a hunk whose old side ends without a newline: the marker has
/// to close the body, not sit between the two sides.
#[test]
fn discarding_a_hunk_that_adds_the_final_newline_restores_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo\nthree", "init");
    fs::write(tmp.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();

    stage::discard_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(
        fs::read(tmp.path().join("a.txt")).unwrap(),
        b"one\ntwo\nthree"
    );
}

#[test]
fn discarding_a_hunk_that_appends_after_an_unterminated_line_restores_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo\nthree", "init");
    fs::write(tmp.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();

    stage::discard_hunk(&repo, "a.txt", 0).unwrap();

    assert_eq!(
        fs::read(tmp.path().join("a.txt")).unwrap(),
        b"one\ntwo\nthree"
    );
}

#[test]
fn unstaging_a_hunk_of_a_file_without_a_final_newline_restores_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo\nthree", "init");
    fs::write(tmp.path().join("a.txt"), "one\ntwo\nTHREE").unwrap();
    stage::stage(&repo, "a.txt").unwrap();

    stage::unstage_hunk(&repo, "a.txt", 0).unwrap();

    let entry = {
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        index.get_path(Path::new("a.txt"), 0).unwrap()
    };
    assert_eq!(
        repo.find_blob(entry.id).unwrap().content(),
        b"one\ntwo\nthree"
    );
}
