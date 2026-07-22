//! CLI target resolution against real repositories (specs/cli.md §3).

use std::fs;
use std::path::Path;

use helm::app::activate_target;
use helm::cli::{resolve_target, TargetError};
use helm::workspace::Workspace;

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    fs::create_dir_all(dir).unwrap();
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_file(repo: &git2::Repository, name: &str) {
    let dir = repo.workdir().unwrap();
    fs::write(dir.join(name), "x\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = repo.signature().unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &[])
        .unwrap();
}

#[test]
fn a_repository_root_resolves_to_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    let repo = init_repo_with_identity(&root);
    commit_file(&repo, "a.txt");

    assert_eq!(
        resolve_target(&root),
        Ok(fs::canonicalize(&root).unwrap()),
        "the target is canonicalized (the tmp dir is a symlink on macOS)"
    );
}

#[test]
fn a_subdirectory_walks_up_to_its_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    let repo = init_repo_with_identity(&root);
    commit_file(&repo, "a.txt");
    let nested = root.join("src/ui");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(
        resolve_target(&nested),
        Ok(fs::canonicalize(&root).unwrap()),
        "`helm .` must work from anywhere inside a checkout"
    );
}

#[test]
fn a_linked_worktree_resolves_to_itself_not_to_its_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("main");
    let repo = init_repo_with_identity(&root);
    commit_file(&repo, "a.txt");
    let wt = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt, None).unwrap();

    assert_eq!(
        resolve_target(&wt),
        Ok(fs::canonicalize(&wt).unwrap()),
        "the CLI targets the row to activate; the app derives the group root"
    );
    let nested = wt.join("deep/dir");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(resolve_target(&nested), Ok(fs::canonicalize(&wt).unwrap()));
}

#[test]
fn a_bare_repository_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("project.git");
    git2::Repository::init_bare(&bare).unwrap();

    assert_eq!(resolve_target(&bare), Err(TargetError::Bare));
}

#[test]
fn a_plain_folder_and_a_missing_path_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let plain = tmp.path().join("documents");
    fs::create_dir_all(&plain).unwrap();

    assert_eq!(resolve_target(&plain), Err(TargetError::NotGit));
    assert_eq!(
        resolve_target(&tmp.path().join("nope")),
        Err(TargetError::Missing)
    );
}

/// `main` + one linked worktree, returning `(root, worktree)` as canonical paths.
fn group(tmp: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = tmp.join("main");
    let repo = init_repo_with_identity(&root);
    commit_file(&repo, "a.txt");
    let wt = tmp.join("feature-x");
    repo.worktree("feature-x", &wt, None).unwrap();
    (
        fs::canonicalize(&root).unwrap(),
        fs::canonicalize(&wt).unwrap(),
    )
}

#[test]
fn an_unknown_target_imports_its_whole_group_and_activates_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, wt) = group(tmp.path());
    let mut ws = Workspace::new();

    assert_eq!(activate_target(&mut ws, &wt), Ok(()));

    let paths: Vec<_> = ws.repos().map(|r| r.path.clone()).collect();
    assert_eq!(paths, vec![root, wt.clone()], "the full group is imported");
    assert_eq!(ws.active_repo().map(|r| r.path.clone()), Some(wt));
}

#[test]
fn opening_a_target_unhides_and_unfolds_its_project() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, wt) = group(tmp.path());
    let mut ws = Workspace::new();
    activate_target(&mut ws, &wt).unwrap();
    ws.set_user_hidden(0, true);
    ws.set_collapsed(0, true);
    ws.set_active(0);

    assert_eq!(activate_target(&mut ws, &wt), Ok(()));

    assert!(!ws.is_user_hidden(1), "the project is revealed");
    assert!(!ws.is_collapsed(0), "the group is unfolded");
    assert!(!ws.is_hidden(1), "the row is no longer folded away");
    assert_eq!(ws.active_repo().map(|r| r.path.clone()), Some(wt));
}

#[test]
fn a_worktree_created_outside_the_app_joins_the_known_group() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let mut ws = Workspace::new();
    activate_target(&mut ws, &root_dir).unwrap();
    assert_eq!(ws.len(), 1);

    let wt = tmp.path().join("later");
    repo.worktree("later", &wt, None).unwrap();
    let wt = fs::canonicalize(&wt).unwrap();

    assert_eq!(activate_target(&mut ws, &wt), Ok(()));
    assert_eq!(ws.len(), 2, "the new worktree joined the existing group");
    assert_eq!(ws.parent_root(1).map(Path::to_path_buf), {
        Some(fs::canonicalize(&root_dir).unwrap())
    });
    assert_eq!(ws.active_repo().map(|r| r.path.clone()), Some(wt));
}

#[test]
fn a_non_git_target_is_refused_without_touching_the_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, wt) = group(tmp.path());
    let mut ws = Workspace::new();
    activate_target(&mut ws, &wt).unwrap();
    let plain = tmp.path().join("documents");
    fs::create_dir_all(&plain).unwrap();

    assert!(activate_target(&mut ws, &plain).is_err());
    assert_eq!(ws.len(), 2, "the workspace is untouched");
    assert_eq!(ws.active_repo().map(|r| r.path.clone()), Some(wt));
}
