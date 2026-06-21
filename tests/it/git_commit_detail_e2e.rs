use std::collections::HashMap;
use std::fs;
use std::path::Path;

use helm::git::commit_detail::{self, CommitFile};
use helm::git::status::ChangeKind;

/// Commit the current state of `files` (write each, stage it) on top of `parents`,
/// returning the new commit oid. `removals` are staged as deletions.
fn commit_files(
    repo: &git2::Repository,
    files: &[(&str, &str)],
    removals: &[&str],
    message: &str,
    parents: &[git2::Oid],
    update_head: bool,
) -> git2::Oid {
    let dir = repo.workdir().unwrap();
    let mut index = repo.index().unwrap();
    for (name, content) in files {
        fs::write(dir.join(name), content).unwrap();
        index.add_path(Path::new(name)).unwrap();
    }
    for name in removals {
        let _ = fs::remove_file(dir.join(name));
        index.remove_path(Path::new(name)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Alice", "alice@example.com").unwrap();
    let parent_commits: Vec<git2::Commit> = parents
        .iter()
        .map(|oid| repo.find_commit(*oid).unwrap())
        .collect();
    let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
    let target = if update_head { Some("HEAD") } else { None };
    repo.commit(target, &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

fn files_by_path(files: &[CommitFile]) -> HashMap<String, CommitFile> {
    files.iter().map(|f| (f.path.clone(), f.clone())).collect()
}

fn kind_of(map: &HashMap<String, CommitFile>, path: &str) -> Option<ChangeKind> {
    map.get(path).map(|f| f.kind)
}

fn stats_of(map: &HashMap<String, CommitFile>, path: &str) -> Option<(usize, usize)> {
    map.get(path).map(|f| (f.additions, f.deletions))
}

#[test]
fn commit_with_three_files_lists_each_with_its_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    // Base: keep.txt and gone.txt exist so the second commit can modify/delete them.
    let base = commit_files(
        &repo,
        &[("keep.txt", "v1\n"), ("gone.txt", "bye\n")],
        &[],
        "base",
        &[],
        true,
    );

    // Second commit: add new.txt, modify keep.txt, delete gone.txt → 3 files.
    let second = commit_files(
        &repo,
        &[("new.txt", "fresh\n"), ("keep.txt", "v2\n")],
        &["gone.txt"],
        "changes",
        &[base],
        true,
    );

    let detail = commit_detail::load(tmp.path(), second).unwrap();

    assert_eq!(detail.meta.oid, second);
    assert_eq!(detail.meta.summary, "changes");
    assert_eq!(detail.meta.body, "");
    assert_eq!(detail.meta.author, "Alice");
    assert_eq!(detail.meta.email, "alice@example.com");
    assert_eq!(detail.meta.committer, "Alice");
    assert_eq!(detail.meta.parents, vec![base]);
    assert_eq!(
        detail.meta.short_id,
        &second.to_string()[..detail.meta.short_id.len()]
    );

    let map = files_by_path(&detail.files);
    assert_eq!(detail.files.len(), 3, "got {:?}", detail.files);
    assert_eq!(kind_of(&map, "new.txt"), Some(ChangeKind::Added));
    assert_eq!(kind_of(&map, "keep.txt"), Some(ChangeKind::Modified));
    assert_eq!(kind_of(&map, "gone.txt"), Some(ChangeKind::Deleted));
    // Line stats per file: new.txt is one added line, keep.txt swaps v1 for v2,
    // gone.txt loses its single line.
    assert_eq!(stats_of(&map, "new.txt"), Some((1, 0)));
    assert_eq!(stats_of(&map, "keep.txt"), Some((1, 1)));
    assert_eq!(stats_of(&map, "gone.txt"), Some((0, 1)));
    assert_eq!(detail.total_line_stats(), (2, 2));
}

#[test]
fn message_splits_into_summary_and_trimmed_body() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let oid = commit_files(
        &repo,
        &[("a.txt", "a\n")],
        &[],
        "Fix the filter\n\nApproved-by: Florian\nSecond line\n",
        &[],
        true,
    );

    let detail = commit_detail::load(tmp.path(), oid).unwrap();

    assert_eq!(detail.meta.summary, "Fix the filter");
    assert_eq!(detail.meta.body, "Approved-by: Florian\nSecond line");
}

#[test]
fn author_timezone_offset_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let dir = repo.workdir().unwrap();
    fs::write(dir.join("a.txt"), "a\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    // 2021-01-01T14:32:00Z committed at UTC+2 ⇒ wall clock 16:32.
    let when = git2::Time::new(1_609_511_520, 120);
    let sig = git2::Signature::new("Alice", "alice@example.com", &when).unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "tz", &tree, &[])
        .unwrap();

    let detail = commit_detail::load(tmp.path(), oid).unwrap();

    assert_eq!(detail.meta.time, 1_609_511_520);
    assert_eq!(detail.meta.offset_minutes, 120);
}

#[test]
fn binary_file_keeps_zero_line_stats() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let dir = repo.workdir().unwrap();
    fs::write(dir.join("blob.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    fs::write(dir.join("text.txt"), "one\ntwo\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("blob.bin")).unwrap();
    index.add_path(Path::new("text.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Alice", "alice@example.com").unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "bin", &tree, &[])
        .unwrap();

    let detail = commit_detail::load(tmp.path(), oid).unwrap();

    let map = files_by_path(&detail.files);
    assert_eq!(kind_of(&map, "blob.bin"), Some(ChangeKind::Added));
    assert_eq!(
        stats_of(&map, "blob.bin"),
        Some((0, 0)),
        "binary stays at 0/0 like the status sidebar (M13-2)"
    );
    assert_eq!(stats_of(&map, "text.txt"), Some((2, 0)));
    assert_eq!(detail.total_line_stats(), (2, 0));
}

#[test]
fn root_commit_diffs_against_empty_tree_so_every_file_is_added() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let root = commit_files(
        &repo,
        &[("a.txt", "a\n"), ("b.txt", "b\n")],
        &[],
        "root",
        &[],
        true,
    );

    let detail = commit_detail::load(tmp.path(), root).unwrap();

    assert!(detail.meta.parents.is_empty());
    let map = files_by_path(&detail.files);
    assert_eq!(detail.files.len(), 2);
    assert_eq!(kind_of(&map, "a.txt"), Some(ChangeKind::Added));
    assert_eq!(kind_of(&map, "b.txt"), Some(ChangeKind::Added));
}

#[test]
fn merge_commit_diffs_against_first_parent_only() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let base = commit_files(&repo, &[("base.txt", "0\n")], &[], "base", &[], true);
    // Mainline: adds main.txt.
    let main_tip = commit_files(&repo, &[("main.txt", "m\n")], &[], "main", &[base], true);
    // Feature off base: adds feat.txt (not on HEAD).
    let feature = commit_files(
        &repo,
        &[("feat.txt", "f\n")],
        &[],
        "feature",
        &[base],
        false,
    );

    // Merge with parents [main_tip, feature]; its tree adds feat.txt on top of the
    // first parent (main_tip), and introduces merged.txt.
    let merge = commit_files(
        &repo,
        &[("feat.txt", "f\n"), ("merged.txt", "x\n")],
        &[],
        "merge",
        &[main_tip, feature],
        true,
    );

    let detail = commit_detail::load(tmp.path(), merge).unwrap();

    assert_eq!(detail.meta.parents, vec![main_tip, feature]);
    let map = files_by_path(&detail.files);
    // vs first parent (main_tip): main.txt already present ⇒ not listed; feat.txt
    // and merged.txt are the additions the merge brings over its mainline.
    assert_eq!(kind_of(&map, "feat.txt"), Some(ChangeKind::Added));
    assert_eq!(kind_of(&map, "merged.txt"), Some(ChangeKind::Added));
    assert!(
        !map.contains_key("main.txt"),
        "main.txt is in the first parent already, got {:?}",
        detail.files
    );
    assert!(!map.contains_key("base.txt"));
}

#[test]
fn renamed_file_is_detected_as_renamed() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let content = "line one\nline two\nline three\nline four\n";
    let base = commit_files(&repo, &[("old.txt", content)], &[], "base", &[], true);
    let renamed = commit_files(
        &repo,
        &[("new.txt", content)],
        &["old.txt"],
        "rename",
        &[base],
        true,
    );

    let detail = commit_detail::load(tmp.path(), renamed).unwrap();

    let map = files_by_path(&detail.files);
    assert_eq!(detail.files.len(), 1, "got {:?}", detail.files);
    assert_eq!(kind_of(&map, "new.txt"), Some(ChangeKind::Renamed));
}

/// A stash saved with INCLUDE_UNTRACKED stores untracked files in its 3rd
/// parent commit, absent from the stash tree: the detail must list them too —
/// a stash holding only untracked files looked empty (D-2026-06-05).
#[test]
fn stash_detail_lists_untracked_files_from_the_third_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    commit_files(&repo, &[("a.txt", "base\n")], &[], "base", &[], true);
    fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();
    helm::git::stash::stash(&repo).unwrap();
    let oid = repo.reflog("refs/stash").unwrap().get(0).unwrap().id_new();

    let detail = commit_detail::load(tmp.path(), oid).unwrap();

    let map = files_by_path(&detail.files);
    assert_eq!(kind_of(&map, "a.txt"), Some(ChangeKind::Modified));
    assert_eq!(kind_of(&map, "new.txt"), Some(ChangeKind::Added));
    assert_eq!(stats_of(&map, "new.txt"), Some((2, 0)));
}

/// Untracked-only stash: without the 3rd-parent files the detail was empty.
#[test]
fn untracked_only_stash_detail_is_not_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    commit_files(&repo, &[("a.txt", "base\n")], &[], "base", &[], true);
    fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
    helm::git::stash::stash(&repo).unwrap();
    let oid = repo.reflog("refs/stash").unwrap().get(0).unwrap().id_new();

    let detail = commit_detail::load(tmp.path(), oid).unwrap();

    let map = files_by_path(&detail.files);
    assert_eq!(detail.files.len(), 1, "got {:?}", detail.files);
    assert_eq!(kind_of(&map, "new.txt"), Some(ChangeKind::Added));
}
