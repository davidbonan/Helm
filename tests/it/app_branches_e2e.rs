use std::fs;
use std::path::{Path, PathBuf};

use helm::app::{add_picked_folders, workspace_branches};
use helm::workspace::{Repo, Workspace};

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
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &parent_refs)
        .unwrap();
}

#[test]
fn branches_reflect_each_repo_head_and_refuse_non_git_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    let repo = init_repo_with_identity(&repo_dir);
    commit_file(&repo, "a.txt");
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feat/sidebar", &head, false).unwrap();
    repo.set_head("refs/heads/feat/sidebar").unwrap();
    let plain_dir = tmp.path().join("notes");
    fs::create_dir_all(&plain_dir).unwrap();

    let mut ws = Workspace::new();
    let outcome = add_picked_folders(&mut ws, vec![repo_dir, plain_dir.clone()]);

    assert_eq!(
        outcome.rejected,
        vec![plain_dir],
        "a non-git folder is refused"
    );
    assert_eq!(
        workspace_branches(&ws),
        vec![Some("feat/sidebar".to_owned())]
    );
}

#[test]
fn branches_of_a_worktree_group_follow_each_working_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let mut ws = Workspace::new();
    add_picked_folders(&mut ws, vec![root_dir]);

    let labels = workspace_branches(&ws);
    let head = repo.head().unwrap().shorthand().unwrap().to_owned();
    assert_eq!(labels, vec![Some(head), Some("feature-x".to_owned())]);
}

#[test]
fn a_bare_root_and_a_gone_path_have_no_branch() {
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
    add_picked_folders(&mut ws, vec![wt_path]);
    ws.add(Repo::new(PathBuf::from("/no/such/repo")));

    assert_eq!(
        workspace_branches(&ws),
        vec![None, Some("checkout".to_owned()), None],
        "bare root and unreadable path stay single-line"
    );
}
