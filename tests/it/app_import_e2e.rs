use std::fs;
use std::path::{Path, PathBuf};

use helm::app::add_picked_folders;
use helm::workspace::{Repo, Workspace};

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    fs::create_dir_all(dir).unwrap();
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_file(repo: &git2::Repository, name: &str) -> git2::Oid {
    let dir = repo.workdir().unwrap();
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

fn paths_of(ws: &Workspace) -> Vec<PathBuf> {
    ws.repos().map(|r| r.path.clone()).collect()
}

#[test]
fn importing_a_worktree_adds_the_full_group_and_activates_the_chosen_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    let syncs = add_picked_folders(&mut ws, vec![wt_path.clone()]).syncs;

    assert!(syncs.is_empty(), "a first import creates a group, no sync");
    let root = fs::canonicalize(&root_dir).unwrap();
    let wt = fs::canonicalize(&wt_path).unwrap();
    assert_eq!(paths_of(&ws), vec![root.clone(), wt.clone()]);
    assert!(ws.is_group_root(0));
    assert_eq!(ws.parent_root(1), Some(root.as_path()));
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(wt),
        "the chosen path is the active row, not the root"
    );
}

#[test]
fn reimporting_completes_the_group_without_duplicates_and_remaps_indices() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let feature = tmp.path().join("feature-x");
    repo.worktree("feature-x", &feature, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![feature.clone()]);

    // A worktree created outside the app after the first import: the reimport
    // (via the root) completes the group by appending the newcomer after the
    // existing manual order, without duplicating the existing one.
    let aaa = tmp.path().join("aaa");
    repo.worktree("aaa", &aaa, None).unwrap();
    let syncs = add_picked_folders(&mut ws, vec![root_dir.clone()]).syncs;

    let root = fs::canonicalize(&root_dir).unwrap();
    assert_eq!(
        paths_of(&ws),
        vec![
            root.clone(),
            fs::canonicalize(&feature).unwrap(),
            fs::canonicalize(&aaa).unwrap(),
        ]
    );
    assert_eq!(
        syncs.len(),
        1,
        "a reimport reconciles the existing group via sync"
    );
    assert_eq!(
        syncs[0].mapping,
        vec![Some(0), Some(1)],
        "feature-x keeps its slot; aaa is appended after it"
    );
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(root),
        "the chosen path (the root) becomes active"
    );
}

#[test]
fn reimporting_the_same_worktree_changes_nothing_but_the_active_row() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir.clone()]);
    let before = paths_of(&ws);

    let syncs = add_picked_folders(&mut ws, vec![wt_path.clone()]).syncs;

    assert_eq!(paths_of(&ws), before, "no duplicate entry");
    assert_eq!(syncs.len(), 1);
    assert_eq!(
        syncs[0].mapping,
        vec![Some(0), Some(1)],
        "an unchanged group keeps every index in place"
    );
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(fs::canonicalize(&wt_path).unwrap())
    );
}

#[test]
fn importing_a_bare_root_worktree_groups_it_and_never_selects_the_bare_root() {
    let tmp = tempfile::tempdir().unwrap();
    let bare_dir = tmp.path().join("proj.git");
    let repo = git2::Repository::init_bare(&bare_dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    let wt_path = tmp.path().join("checkout");
    repo.worktree("checkout", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![wt_path.clone()]);

    let bare_root = fs::canonicalize(&bare_dir).unwrap();
    let wt = fs::canonicalize(&wt_path).unwrap();
    assert_eq!(paths_of(&ws), vec![bare_root, wt.clone()]);
    assert!(ws.repo(0).unwrap().bare, "the root is flagged bare");
    assert_eq!(ws.active_repo().map(|r| r.path.clone()), Some(wt.clone()));
    assert!(!ws.set_active(0), "a bare root refuses activation");

    // Import via the bare root itself: the chosen path is not selectable ⇒
    // activation stays on the worktree.
    let mut ws2 = Workspace::new();
    add_picked_folders(&mut ws2, vec![bare_dir.clone()]);
    assert_eq!(ws2.active_repo().map(|r| r.path.clone()), Some(wt));
}

#[test]
fn importing_a_submodule_keeps_it_standalone() {
    let tmp = tempfile::tempdir().unwrap();
    let child_dir = tmp.path().join("child");
    let child = init_repo_with_identity(&child_dir);
    commit_file(&child, "c.txt");
    let parent_dir = tmp.path().join("parent");
    let parent = init_repo_with_identity(&parent_dir);
    commit_file(&parent, "p.txt");
    let url = child_dir.to_str().unwrap();
    let mut sm = parent.submodule(url, Path::new("sub"), true).unwrap();
    sm.clone(None).unwrap();
    sm.add_finalize().unwrap();
    let sub_path = parent_dir.join("sub");

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![sub_path.clone()]);

    assert_eq!(paths_of(&ws), vec![fs::canonicalize(&sub_path).unwrap()]);
    assert_eq!(ws.parent_root(0), None, "a submodule is never grouped");
    assert!(!ws.is_group_root(0));
}

#[test]
fn importing_a_root_skips_its_prunable_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();
    fs::remove_dir_all(&wt_path).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir.clone()]);

    assert_eq!(
        paths_of(&ws),
        vec![fs::canonicalize(&root_dir).unwrap()],
        "a worktree whose directory is gone is treated as deleted"
    );
}

#[test]
fn importing_a_worktree_of_an_existing_flat_root_completes_it_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");

    // Legacy flat entry (migration M11-4): root alone, **raw** path (macOS
    // tempdir: /var/… ≠ canonical /private/var/…) — deduplication must go
    // through canonicalized comparison, not raw equality.
    let mut ws = Workspace::new();
    ws.add(Repo::new(root_dir.clone()));
    assert_ne!(fs::canonicalize(&root_dir).unwrap(), root_dir);

    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();
    add_picked_folders(&mut ws, vec![wt_path.clone()]);

    assert_eq!(
        paths_of(&ws),
        vec![root_dir.clone(), fs::canonicalize(&wt_path).unwrap()],
        "the legacy entry is completed in place, keeping its stored path"
    );
    assert_eq!(ws.parent_root(1), Some(root_dir.as_path()));
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(fs::canonicalize(&wt_path).unwrap())
    );
}
