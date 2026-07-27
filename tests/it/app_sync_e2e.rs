use std::fs;
use std::path::{Path, PathBuf};

use helm::app::{add_picked_folders, sync_workspace_groups};
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
fn sync_appends_a_worktree_created_out_of_app_after_the_manual_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let m_feature = tmp.path().join("m-feature");
    repo.worktree("m-feature", &m_feature, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir.clone()]);

    let a_feature = tmp.path().join("a-feature");
    repo.worktree("a-feature", &a_feature, None).unwrap();
    let outcome = sync_workspace_groups(&mut ws);

    assert!(outcome.changed);
    assert_eq!(
        paths_of(&ws),
        vec![
            fs::canonicalize(&root_dir).unwrap(),
            fs::canonicalize(&m_feature).unwrap(),
            fs::canonicalize(&a_feature).unwrap(),
        ],
        "the manual order survives; the discovered worktree is appended (not alpha-inserted)"
    );
    assert_eq!(
        outcome.syncs.last().unwrap().mapping,
        vec![Some(0), Some(1)],
        "the existing child keeps its slot; the discovered one lands at the end"
    );
}

#[test]
fn sync_purges_a_worktree_deleted_out_of_app_and_active_falls_back_to_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![wt_path.clone()]);
    assert_eq!(ws.active(), Some(1), "the imported worktree is active");

    fs::remove_dir_all(&wt_path).unwrap();
    let outcome = sync_workspace_groups(&mut ws);

    assert!(outcome.changed);
    let root = fs::canonicalize(&root_dir).unwrap();
    assert_eq!(paths_of(&ws), vec![root.clone()]);
    assert_eq!(
        outcome.syncs.last().unwrap().mapping,
        vec![Some(0), None],
        "the gone worktree maps to None so its PTYs get killed"
    );
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(root),
        "the active selection falls back to the root"
    );
}

#[test]
fn sync_with_nothing_changed_reports_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir.clone()]);

    let outcome = sync_workspace_groups(&mut ws);

    assert!(
        !outcome.changed,
        "no disk change, prefs must not be rewritten"
    );
    assert_eq!(paths_of(&ws).len(), 2);
}

#[test]
fn sync_regroups_a_migrated_flat_entry_into_its_group() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    // Migration M11-4: a legacy worktree path becomes a root-only project ⇒ a
    // flat entry at the raw path, which the startup sync must fold back.
    let mut ws = Workspace::new();
    ws.add(Repo::new(wt_path.clone()));
    assert_eq!(ws.active(), Some(0));

    let outcome = sync_workspace_groups(&mut ws);

    assert!(outcome.changed);
    let root = fs::canonicalize(&root_dir).unwrap();
    let wt = fs::canonicalize(&wt_path).unwrap();
    assert_eq!(paths_of(&ws), vec![root.clone(), wt.clone()]);
    assert_eq!(ws.parent_root(1), Some(root.as_path()));
    assert_eq!(
        outcome.syncs.first().unwrap().mapping,
        vec![None],
        "the stray flat entry is dropped in favor of the group"
    );
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(wt),
        "the active selection follows the stray into its group entry"
    );
}

#[test]
fn sync_reconciles_the_bare_flag_of_a_restored_root() {
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

    // Restoration from prefs (M11-4): the bare flag is not persisted.
    let mut ws = Workspace::new();
    ws.add(Repo::new(fs::canonicalize(&bare_dir).unwrap()));

    sync_workspace_groups(&mut ws);

    assert!(ws.repo(0).unwrap().bare, "the bare flag is rediscovered");
    assert_eq!(
        paths_of(&ws),
        vec![
            fs::canonicalize(&bare_dir).unwrap(),
            fs::canonicalize(&wt_path).unwrap(),
        ],
        "its worktrees are discovered too"
    );
}

#[test]
fn sync_leaves_an_unreadable_root_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir.clone()]);
    fs::remove_dir_all(&root_dir).unwrap();

    let outcome = sync_workspace_groups(&mut ws);

    assert!(
        !outcome.changed,
        "a gone root is the startup purge's job, not sync's"
    );
    assert_eq!(
        paths_of(&ws),
        vec![fs::canonicalize(tmp.path()).unwrap().join("main")]
    );
}

#[test]
fn a_renamed_worktree_followed_in_place_survives_the_next_sync() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![wt_path]);
    assert_eq!(ws.active(), Some(1), "the imported worktree is active");

    let before = ws.repo(1).unwrap().path.clone();
    let moved = helm::git::worktree::rename(&root_dir, &before, "feature-y").unwrap();
    assert!(ws.set_repo_path(1, moved.clone()));
    let outcome = sync_workspace_groups(&mut ws);

    assert!(
        !outcome.changed,
        "the entry already followed the move: nothing to purge nor discover"
    );
    assert_eq!(
        paths_of(&ws),
        vec![fs::canonicalize(&root_dir).unwrap(), moved.clone()]
    );
    assert_eq!(
        outcome.syncs.last().unwrap().mapping,
        vec![Some(0), Some(1)],
        "the renamed worktree keeps its slot — its PTYs are not killed"
    );
    assert_eq!(
        ws.active_repo().map(|r| r.path.clone()),
        Some(moved),
        "the selection stays on the renamed worktree"
    );
    assert_eq!(ws.repo(1).unwrap().name, "feature-y");
}
