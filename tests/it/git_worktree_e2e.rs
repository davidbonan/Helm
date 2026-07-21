use std::fs;
use std::path::Path;

use helm::git::worktree::{self, DeleteError, WorktreeSourceKind};

fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    fs::create_dir_all(dir).unwrap();
    let repo = git2::Repository::init(dir).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    repo
}

fn commit_file(repo: &git2::Repository, name: &str) -> git2::Oid {
    commit_text(repo, name, "x\n")
}

fn commit_text(repo: &git2::Repository, name: &str, content: &str) -> git2::Oid {
    let dir = repo.workdir().unwrap();
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
    repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &parent_refs)
        .unwrap()
}

fn create_branch(repo: &git2::Repository, name: &str, oid: git2::Oid) {
    let commit = repo.find_commit(oid).unwrap();
    repo.branch(name, &commit, false).unwrap();
}

fn source_names(root: &Path) -> Vec<String> {
    worktree::available_sources(root, None)
        .unwrap()
        .into_iter()
        .map(|source| source.name)
        .collect()
}

#[test]
fn default_worktree_base_sits_next_to_the_root_and_keeps_branch_folders() {
    let root = Path::new("/Users/dev/helm-studio");

    assert_eq!(
        worktree::default_base(root).unwrap(),
        Path::new("/Users/dev/helm-studio.worktrees")
    );
    assert_eq!(
        worktree::path_for_branch(root, "feat/toto", None).unwrap(),
        Path::new("/Users/dev/helm-studio.worktrees/feat/toto")
    );
}

#[test]
fn linked_worktree_resolves_to_its_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    let expected = fs::canonicalize(&root_dir).unwrap();
    assert_eq!(worktree::resolve_root(&wt_path).unwrap(), expected);
    assert_eq!(worktree::resolve_root(&root_dir).unwrap(), expected);
}

#[test]
fn list_enumerates_created_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let feature = tmp.path().join("feature-x");
    let fix = tmp.path().join("fix-bug");
    repo.worktree("feature-x", &feature, None).unwrap();
    repo.worktree("fix-bug", &fix, None).unwrap();

    let listing = worktree::list(&root_dir).unwrap();

    assert!(!listing.bare);
    assert_eq!(listing.worktrees.len(), 2);
    let by_name = |name: &str| {
        listing
            .worktrees
            .iter()
            .find(|w| w.name == name)
            .unwrap_or_else(|| panic!("worktree {name} should be listed"))
    };
    let feature_info = by_name("feature-x");
    assert_eq!(feature_info.path, fs::canonicalize(&feature).unwrap());
    assert!(!feature_info.locked);
    assert!(!feature_info.prunable);
    let fix_info = by_name("fix-bug");
    assert_eq!(fix_info.path, fs::canonicalize(&fix).unwrap());
}

#[test]
fn available_sources_hide_checked_out_branches_and_existing_destinations() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let oid = commit_file(&repo, "a.txt");
    create_branch(&repo, "feat/toto", oid);
    create_branch(&repo, "fix/one", oid);

    let root = fs::canonicalize(&root_dir).unwrap();
    fs::create_dir_all(worktree::path_for_branch(&root, "fix/one", None).unwrap()).unwrap();

    let before = source_names(&root_dir);
    assert!(before.contains(&"feat/toto".to_owned()));
    assert!(
        !before.contains(&"fix/one".to_owned()),
        "a branch whose destination already exists must not be selectable"
    );

    let created = worktree::create(&root_dir, "feat/toto", None, None).unwrap();

    assert_eq!(
        created.path,
        fs::canonicalize(worktree::path_for_branch(&root, "feat/toto", None).unwrap()).unwrap()
    );
    assert!(!source_names(&root_dir).contains(&"feat/toto".to_owned()));
}

#[test]
fn remote_source_creates_a_tracking_local_branch_and_nested_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let oid = commit_file(&repo, "a.txt");
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feat/toto", oid, true, "remote")
        .unwrap();

    let sources = worktree::available_sources(&root_dir, None).unwrap();
    let remote = sources
        .iter()
        .find(|source| source.name == "origin/feat/toto")
        .expect("remote source should be selectable when the local branch is absent");
    assert_eq!(remote.kind, WorktreeSourceKind::Remote);
    assert_eq!(remote.local_branch, "feat/toto");

    let created = worktree::create(&root_dir, "origin/feat/toto", None, None).unwrap();
    let root = fs::canonicalize(&root_dir).unwrap();
    assert_eq!(
        created.path,
        fs::canonicalize(worktree::path_for_branch(&root, "feat/toto", None).unwrap()).unwrap()
    );
    assert!(created.path.join("a.txt").exists());
    let local = repo
        .find_branch("feat/toto", git2::BranchType::Local)
        .unwrap();
    assert_eq!(
        local.upstream().unwrap().name().unwrap(),
        Some("origin/feat/toto")
    );
}

#[test]
fn create_with_custom_name_places_the_worktree_under_that_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let oid = commit_file(&repo, "a.txt");
    create_branch(&repo, "feat/toto", oid);

    // Slashes nest folders, like branch names do.
    let created = worktree::create(&root_dir, "feat/toto", Some("team/dave"), None).unwrap();

    let root = fs::canonicalize(&root_dir).unwrap();
    assert_eq!(
        created.path,
        fs::canonicalize(worktree::path_for_branch(&root, "team/dave", None).unwrap()).unwrap()
    );
    assert!(created.path.join("a.txt").exists());
    let wt_repo = git2::Repository::open(&created.path).unwrap();
    assert_eq!(
        wt_repo.head().unwrap().shorthand().unwrap(),
        "feat/toto",
        "the custom folder still checks out the source branch"
    );
}

#[test]
fn create_honors_a_configured_worktree_base() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let oid = commit_file(&repo, "a.txt");
    create_branch(&repo, "feat/toto", oid);
    let base = tmp.path().join("trees");

    let created = worktree::create(&root_dir, "feat/toto", None, Some(&base)).unwrap();

    assert_eq!(
        created.path,
        fs::canonicalize(base.join("feat/toto")).unwrap(),
        "the worktree lands under the configured base, not <root>.worktrees"
    );
    assert!(created.path.join("a.txt").exists());
    assert!(
        !worktree::default_base(&fs::canonicalize(&root_dir).unwrap())
            .unwrap()
            .exists(),
        "the default base is untouched when an override is set"
    );
}

#[test]
fn create_rejects_a_custom_name_escaping_the_worktrees_base() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let oid = commit_file(&repo, "a.txt");
    create_branch(&repo, "feat/toto", oid);

    // `a//b` is not listed: `Path::components` normalizes it to `a/b`.
    for name in ["../escape", "/abs", ".", "a/../b"] {
        let err = worktree::create(&root_dir, "feat/toto", Some(name), None).unwrap_err();
        assert!(
            matches!(err, worktree::CreateError::Unavailable(_)),
            "“{name}” must be refused as a worktree folder"
        );
    }
    let root = fs::canonicalize(&root_dir).unwrap();
    assert!(
        !worktree::default_base(&root).unwrap().exists(),
        "no folder must be created for a refused name"
    );
}

#[test]
fn remote_source_is_hidden_when_the_local_branch_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let oid = commit_file(&repo, "a.txt");
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feat", oid, true, "remote")
        .unwrap();
    create_branch(&repo, "feat", oid);

    let names = source_names(&root_dir);

    assert!(names.contains(&"feat".to_owned()));
    assert!(
        !names.contains(&"origin/feat".to_owned()),
        "remote source would need to create a local branch that already exists"
    );
}

fn commit_on_top(repo: &git2::Repository, parent: git2::Oid, msg: &str) -> git2::Oid {
    let parent_commit = repo.find_commit(parent).unwrap();
    let tree = parent_commit.tree().unwrap();
    let sig = repo.signature().unwrap();
    repo.commit(None, &sig, &sig, msg, &tree, &[&parent_commit])
        .unwrap()
}

#[test]
fn remote_source_refreshes_a_stale_local_homonym_behind_the_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let c1 = commit_file(&repo, "a.txt");
    let c2 = commit_file(&repo, "b.txt"); // default branch advances; c2 descends c1
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feat", c2, true, "remote")
        .unwrap();
    create_branch(&repo, "feat", c1); // strictly behind origin/feat, not checked out

    let names = source_names(&root_dir);
    assert!(
        names.contains(&"feat".to_owned()),
        "the stale local is still selectable as-is"
    );
    assert!(
        names.contains(&"origin/feat".to_owned()),
        "the remote homonym is selectable to refresh a strictly-behind local"
    );

    let created = worktree::create(&root_dir, "origin/feat", None, None).unwrap();

    let wt_repo = git2::Repository::open(&created.path).unwrap();
    assert_eq!(
        wt_repo.head().unwrap().peel_to_commit().unwrap().id(),
        c2,
        "the refreshed worktree checks out the remote tip, not the stale local"
    );
    let local = repo.find_branch("feat", git2::BranchType::Local).unwrap();
    assert_eq!(
        local.get().peel_to_commit().unwrap().id(),
        c2,
        "the local branch is recreated on the remote tip"
    );
    assert_eq!(
        local.upstream().unwrap().name().unwrap(),
        Some("origin/feat")
    );
}

#[test]
fn remote_source_is_hidden_when_the_local_homonym_has_unpushed_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let c1 = commit_file(&repo, "a.txt");
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/feat", c1, true, "remote")
        .unwrap();
    // Local `feat` carries a commit the remote lacks: deleting it would lose work.
    let ahead = commit_on_top(&repo, c1, "unpushed");
    create_branch(&repo, "feat", ahead);

    let names = source_names(&root_dir);

    assert!(names.contains(&"feat".to_owned()));
    assert!(
        !names.contains(&"origin/feat".to_owned()),
        "a local ahead of its remote must not be silently dropped"
    );
}

#[test]
fn remote_source_stays_hidden_when_the_local_homonym_is_checked_out() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let c1 = commit_file(&repo, "a.txt");
    // The checked-out branch (e.g. `main`) sits behind its remote homonym — yet
    // git refuses to delete a checked-out branch, so it stays unavailable.
    let head = repo.head().unwrap().shorthand().unwrap().to_owned();
    let c2 = commit_on_top(&repo, c1, "remote ahead");
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference(&format!("refs/remotes/origin/{head}"), c2, true, "remote")
        .unwrap();

    let names = source_names(&root_dir);

    assert!(
        !names.contains(&format!("origin/{head}")),
        "a checked-out branch cannot be replaced, even when behind its remote"
    );
}

#[test]
fn create_branch_starts_at_head_without_upstream_and_checks_it_out() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let head = commit_file(&repo, "a.txt");
    let base = repo.head().unwrap().shorthand().unwrap().to_owned();

    let created = worktree::create_branch(&root_dir, "feat/new", None, None).unwrap();

    let root = fs::canonicalize(&root_dir).unwrap();
    assert_eq!(
        created.path,
        fs::canonicalize(worktree::path_for_branch(&root, "feat/new", None).unwrap()).unwrap()
    );
    assert_eq!(created.source.local_branch, "feat/new");
    assert_eq!(
        created.source.name, base,
        "HELM_SOURCE_BRANCH carries the base, not the new branch"
    );

    let wt_repo = git2::Repository::open(&created.path).unwrap();
    assert_eq!(wt_repo.head().unwrap().shorthand().unwrap(), "feat/new");
    assert_eq!(
        wt_repo.head().unwrap().peel_to_commit().unwrap().id(),
        head,
        "the branch starts at the root HEAD commit"
    );
    let local = repo
        .find_branch("feat/new", git2::BranchType::Local)
        .unwrap();
    assert!(
        local.upstream().is_err(),
        "a fly-created branch has no upstream"
    );
}

#[test]
fn create_branch_rolls_back_the_branch_when_the_worktree_add_fails() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    // A read-only base: create_dir_all sees it already exists, but libgit2's mkdir
    // of the worktree leaf inside it fails — exercising the post-branch rollback.
    let base = tmp.path().join("locked");
    fs::create_dir_all(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o555)).unwrap();

    let err = worktree::create_branch(&root_dir, "feat", None, Some(&base)).unwrap_err();

    fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(err, worktree::CreateError::Git(_)),
        "expected a git error from the failed worktree add, got {err:?}"
    );
    assert!(
        repo.find_branch("feat", git2::BranchType::Local).is_err(),
        "the fly-created branch must be rolled back when the worktree add fails"
    );
}

#[test]
fn create_branch_refuses_existing_checked_out_and_remote_homonyms() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let head = commit_file(&repo, "a.txt");
    create_branch(&repo, "Feat", head); // existing local, different case
    let checked_out = repo.head().unwrap().shorthand().unwrap().to_owned();
    repo.remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    repo.reference("refs/remotes/origin/bar", head, true, "remote")
        .unwrap();

    let upper = checked_out.to_uppercase();
    for taken in [
        "feat",
        "Feat",
        checked_out.as_str(),
        upper.as_str(),
        "bar",
        "BAR",
    ] {
        let err = worktree::create_branch(&root_dir, taken, None, None).unwrap_err();
        assert!(
            matches!(err, worktree::CreateError::Unavailable(_)),
            "“{taken}” must be refused as an existing branch, got {err:?}"
        );
    }
    assert!(
        worktree::list(&root_dir).unwrap().worktrees.is_empty(),
        "no worktree is created for a refused name"
    );
}

#[test]
fn create_branch_from_detached_head_labels_the_short_id() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    let head = commit_file(&repo, "a.txt");
    repo.set_head_detached(head).unwrap();
    let short = repo
        .find_object(head, None)
        .unwrap()
        .short_id()
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();

    let created = worktree::create_branch(&root_dir, "feat/new", None, None).unwrap();

    assert_eq!(
        created.source.name, short,
        "a detached base is labelled by its short commit id"
    );
    assert_eq!(created.source.local_branch, "feat/new");
    let wt_repo = git2::Repository::open(&created.path).unwrap();
    assert_eq!(wt_repo.head().unwrap().peel_to_commit().unwrap().id(), head);
    assert!(repo
        .find_branch("feat/new", git2::BranchType::Local)
        .is_ok());
}

#[test]
fn locked_worktree_is_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt = repo
        .worktree("feature-x", &tmp.path().join("feature-x"), None)
        .unwrap();
    wt.lock(Some("in use")).unwrap();

    let listing = worktree::list(&root_dir).unwrap();

    assert_eq!(listing.worktrees.len(), 1);
    assert!(listing.worktrees[0].locked);
    assert!(!listing.worktrees[0].prunable);
}

#[test]
fn worktree_with_deleted_directory_is_prunable() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();
    fs::remove_dir_all(&wt_path).unwrap();

    let listing = worktree::list(&root_dir).unwrap();

    assert_eq!(listing.worktrees.len(), 1);
    assert!(listing.worktrees[0].prunable);
    // libgit2 records the resolved path at creation: compare via the parent
    // (always present), since the deleted directory can no longer be canonicalized.
    let expected = fs::canonicalize(tmp.path()).unwrap().join("feature-x");
    assert_eq!(listing.worktrees[0].path, expected);
}

#[test]
fn bare_root_is_detected_and_its_worktree_resolves_to_bare_dir() {
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

    let listing = worktree::list(&bare_dir).unwrap();

    assert!(listing.bare);
    assert_eq!(listing.worktrees.len(), 1);
    assert_eq!(listing.worktrees[0].name, "checkout");
    assert_eq!(
        worktree::resolve_root(&wt_path).unwrap(),
        fs::canonicalize(&bare_dir).unwrap()
    );
}

#[test]
fn delete_clean_worktree_removes_dir_and_metadata_branch_survives() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();

    worktree::delete(&root_dir, "feature-x", false).unwrap();

    assert!(!wt_path.exists(), "worktree directory should be deleted");
    assert!(worktree::list(&root_dir).unwrap().worktrees.is_empty());
    assert!(
        repo.find_branch("feature-x", git2::BranchType::Local)
            .is_ok(),
        "the branch must survive the worktree deletion"
    );
}

#[test]
fn delete_dirty_worktree_errs_with_count_unless_forced() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();
    fs::write(wt_path.join("wip-1.txt"), "x\n").unwrap();
    fs::write(wt_path.join("wip-2.txt"), "x\n").unwrap();

    let err = worktree::delete(&root_dir, "feature-x", false).unwrap_err();
    assert!(
        matches!(err, DeleteError::Dirty(2)),
        "expected Dirty(2), got {err:?}"
    );
    assert!(wt_path.exists());

    worktree::delete(&root_dir, "feature-x", true).unwrap();
    assert!(!wt_path.exists());
    assert!(worktree::list(&root_dir).unwrap().worktrees.is_empty());
}

#[test]
fn delete_clean_worktree_holding_ignored_files_errs_with_count_unless_forced() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_text(&repo, ".gitignore", ".env\nbuild/\n");
    let with_ignored = tmp.path().join("feature-x");
    repo.worktree("feature-x", &with_ignored, None).unwrap();
    fs::write(with_ignored.join(".env"), "TOKEN=1\n").unwrap();
    fs::create_dir(with_ignored.join("build")).unwrap();
    fs::write(with_ignored.join("build/out.o"), "x\n").unwrap();
    let clean = tmp.path().join("feature-y");
    repo.worktree("feature-y", &clean, None).unwrap();

    // `build/` counts once: the ignored directory is not recursed.
    let err = worktree::delete(&root_dir, "feature-x", false).unwrap_err();
    assert!(
        matches!(err, DeleteError::Ignored(2)),
        "expected Ignored(2), got {err:?}"
    );
    assert!(
        with_ignored.exists(),
        "nothing is deleted before confirmation"
    );

    worktree::delete(&root_dir, "feature-x", true).unwrap();
    assert!(!with_ignored.exists());

    // No ignored file ⇒ still an immediate deletion, no confirmation.
    worktree::delete(&root_dir, "feature-y", false).unwrap();
    assert!(!clean.exists());
    assert!(worktree::list(&root_dir).unwrap().worktrees.is_empty());
}

#[test]
fn delete_by_path_resolves_the_libgit2_name_from_the_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    // libgit2 name ≠ directory name: resolution by path must find it.
    let wt_path = tmp.path().join("feature-dir");
    repo.worktree("feature-name", &wt_path, None).unwrap();

    worktree::delete_by_path(&root_dir, &wt_path, false).unwrap();

    assert!(!wt_path.exists(), "worktree directory should be deleted");
    assert!(worktree::list(&root_dir).unwrap().worktrees.is_empty());
}

#[test]
fn delete_runner_reports_dirty_off_thread_then_force_deletes() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    repo.worktree("feature-x", &wt_path, None).unwrap();
    fs::write(wt_path.join("wip.txt"), "x\n").unwrap();

    let mut runner = worktree::DeleteRunner::new(|| {});
    let request = worktree::DeleteRequest {
        root: root_dir.clone(),
        path: wt_path.clone(),
        label: "feature-x".to_owned(),
        force: false,
    };
    assert!(runner.request(request.clone()));
    let reply = runner.recv().unwrap();
    assert!(
        matches!(reply.result, Err(DeleteError::Dirty(1))),
        "expected Dirty(1), got {:?}",
        reply.result
    );
    assert!(wt_path.exists(), "a dirty worktree is not deleted unforced");
    assert!(!runner.busy());

    assert!(runner.request(worktree::DeleteRequest {
        force: true,
        ..request
    }));
    let reply = runner.recv().unwrap();
    assert!(
        reply.result.is_ok(),
        "forced delete should succeed, got {:?}",
        reply.result
    );
    assert!(!wt_path.exists());
    assert!(worktree::list(&root_dir).unwrap().worktrees.is_empty());
}

#[test]
fn delete_locked_worktree_is_refused_with_reason_even_forced() {
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = tmp.path().join("main");
    let repo = init_repo_with_identity(&root_dir);
    commit_file(&repo, "a.txt");
    let wt_path = tmp.path().join("feature-x");
    let wt = repo.worktree("feature-x", &wt_path, None).unwrap();
    wt.lock(Some("in use")).unwrap();

    let err = worktree::delete(&root_dir, "feature-x", false).unwrap_err();
    assert!(
        matches!(&err, DeleteError::Locked(Some(reason)) if reason == "in use"),
        "expected Locked(Some(\"in use\")), got {err:?}"
    );

    let forced = worktree::delete(&root_dir, "feature-x", true).unwrap_err();
    assert!(matches!(forced, DeleteError::Locked(_)));
    assert!(wt_path.exists(), "a locked worktree must not be deleted");
}

#[test]
fn submodule_stays_standalone() {
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
    assert!(
        sub_path.join(".git").is_file(),
        "submodule .git should be a gitlink file, like a linked worktree's"
    );
    assert_eq!(
        worktree::resolve_root(&sub_path).unwrap(),
        fs::canonicalize(&sub_path).unwrap()
    );
    assert!(worktree::list(&sub_path).unwrap().worktrees.is_empty());
}
