use std::fs;
use std::path::Path;

use helm::git::commit;
use helm::git::diff::{self, DiffSource, LineOrigin};
use helm::git::stage;
use helm::git::status;

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
