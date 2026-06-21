use std::fs;
use std::path::Path;

use helm::git::cli;
use helm::git::conflict::{read_conflict, read_conflicts, resolve_file, ConflictKind, Region};
use helm::git::sync::{self, SyncError, SyncOutcome};

fn set_test_config(repo: &git2::Repository) {
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    cfg.set_bool("commit.gpgsign", false).unwrap();
}

fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, message: &str) {
    fs::write(dir.join(name), content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    commit(repo, &mut index, message);
}

fn commit_remove(repo: &git2::Repository, dir: &Path, name: &str, message: &str) {
    fs::remove_file(dir.join(name)).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new(name)).unwrap();
    index.write().unwrap();
    commit(repo, &mut index, message);
}

fn commit(repo: &git2::Repository, index: &mut git2::Index, message: &str) {
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = repo.signature().unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap();
}

fn checkout(repo: &git2::Repository, branch: &str) {
    repo.set_head(&format!("refs/heads/{branch}")).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
}

/// `main` and `feature` diverge on the `bravo` line of `base.txt` over a shared
/// `alpha\nbravo\ncharlie` ancestor. Leaves the repo on `main`. Returns
/// `(tmp, main_branch_name)`.
fn diverged() -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(
        &repo,
        tmp.path(),
        "base.txt",
        "alpha\nbravo\ncharlie\n",
        "c1",
    );
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_file(
        &repo,
        tmp.path(),
        "base.txt",
        "alpha\nbravo-feature\ncharlie\n",
        "c-feature",
    );
    checkout(&repo, &main);
    commit_file(
        &repo,
        tmp.path(),
        "base.txt",
        "alpha\nbravo-main\ncharlie\n",
        "c-main",
    );
    (tmp, main)
}

#[test]
fn both_modified_under_merge_reports_regions_and_ours_theirs_labels() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Merge);

    let cf = read_conflict(&repo, "base.txt").unwrap();
    assert_eq!(cf.kind, ConflictKind::BothModified);
    assert!(cf.has_base);
    assert_eq!(cf.ours_label, "Current · ours");
    assert_eq!(cf.theirs_label, "Incoming · theirs");
    assert_eq!(
        cf.regions,
        vec![
            Region::Stable(vec!["alpha".to_string()]),
            Region::Conflict {
                ours: vec!["bravo-main".to_string()],
                theirs: vec!["bravo-feature".to_string()],
                base: vec!["bravo".to_string()],
            },
            Region::Stable(vec!["charlie".to_string()]),
        ]
    );
}

#[test]
fn both_modified_under_rebase_inverts_the_labels() {
    let (tmp, main) = diverged();
    let repo = git2::Repository::open(tmp.path()).unwrap();
    checkout(&repo, "feature");
    let rebased = cli::run(tmp.path(), &["rebase", &main]).unwrap();
    assert!(!rebased.success(), "rebase should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert!(
        matches!(
            repo.state(),
            git2::RepositoryState::Rebase
                | git2::RepositoryState::RebaseMerge
                | git2::RepositoryState::RebaseInteractive
        ),
        "unexpected state {:?}",
        repo.state()
    );

    let cf = read_conflict(&repo, "base.txt").unwrap();
    assert_eq!(cf.kind, ConflictKind::BothModified);
    // Stage 2 is the rebase target (onto = main), stage 3 the replayed commit.
    assert_eq!(cf.ours_label, "Current · onto");
    assert_eq!(cf.theirs_label, "Incoming · your commit");
    assert_eq!(
        cf.regions,
        vec![
            Region::Stable(vec!["alpha".to_string()]),
            Region::Conflict {
                ours: vec!["bravo-main".to_string()],
                theirs: vec!["bravo-feature".to_string()],
                base: vec!["bravo".to_string()],
            },
            Region::Stable(vec!["charlie".to_string()]),
        ]
    );
}

#[test]
fn added_by_both_under_merge_has_no_base() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "seed.txt", "seed\n", "c1");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_file(
        &repo,
        tmp.path(),
        "new.txt",
        "feature-content\n",
        "c-feature",
    );
    checkout(&repo, &main);
    commit_file(&repo, tmp.path(), "new.txt", "main-content\n", "c-main");

    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "new.txt").unwrap();
    assert_eq!(cf.kind, ConflictKind::AddedByBoth);
    assert!(!cf.has_base);
    assert_eq!(
        cf.regions,
        vec![Region::Conflict {
            ours: vec!["main-content".to_string()],
            theirs: vec!["feature-content".to_string()],
            base: vec![],
        }]
    );
}

#[test]
fn delete_modify_under_merge_is_deleted_by_them() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "keep.txt", "keep\n", "c0");
    commit_file(&repo, tmp.path(), "doomed.txt", "content\n", "c1");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_remove(&repo, tmp.path(), "doomed.txt", "c-delete");
    checkout(&repo, &main);
    commit_file(&repo, tmp.path(), "doomed.txt", "modified\n", "c-modify");

    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Merge);
    let cf = read_conflict(&repo, "doomed.txt").unwrap();
    assert_eq!(cf.kind, ConflictKind::DeletedByThem);
    assert!(cf.has_base);
    assert!(cf.regions.is_empty());
}

#[test]
fn delete_modify_under_rebase_is_deleted_by_us() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "keep.txt", "keep\n", "c0");
    commit_file(&repo, tmp.path(), "doomed.txt", "content\n", "c1");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_file(&repo, tmp.path(), "doomed.txt", "modified\n", "c-modify");
    checkout(&repo, &main);
    commit_remove(&repo, tmp.path(), "doomed.txt", "c-delete");

    checkout(&repo, "feature");
    let rebased = cli::run(tmp.path(), &["rebase", &main]).unwrap();
    assert!(!rebased.success(), "rebase should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    // Stage 2 (onto = main) deleted the file, stage 3 (the replayed commit) kept it.
    let cf = read_conflict(&repo, "doomed.txt").unwrap();
    assert_eq!(cf.kind, ConflictKind::DeletedByUs);
    assert!(cf.has_base);
    assert!(cf.regions.is_empty());
}

fn index_has_conflicts(repo: &git2::Repository) -> bool {
    let mut index = repo.index().unwrap();
    index.read(false).unwrap();
    index.has_conflicts()
}

/// `main` modifies `doomed.txt`, `feature` deletes it; merging `feature` leaves a
/// delete/modify conflict with the repo mid-merge on `main`.
fn delete_modify_merge() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "keep.txt", "keep\n", "c0");
    commit_file(&repo, tmp.path(), "doomed.txt", "content\n", "c1");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_remove(&repo, tmp.path(), "doomed.txt", "c-delete");
    checkout(&repo, &main);
    commit_file(&repo, tmp.path(), "doomed.txt", "modified\n", "c-modify");

    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "delete/modify should conflict");
    tmp
}

#[test]
fn read_conflicts_lists_every_conflicting_file_then_clears_when_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "a.txt", "a0\n", "c0-a");
    commit_file(&repo, tmp.path(), "b.txt", "b0\n", "c0-b");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_file(&repo, tmp.path(), "a.txt", "a-feature\n", "c-f-a");
    commit_file(&repo, tmp.path(), "b.txt", "b-feature\n", "c-f-b");
    checkout(&repo, &main);
    commit_file(&repo, tmp.path(), "a.txt", "a-main\n", "c-m-a");
    commit_file(&repo, tmp.path(), "b.txt", "b-main\n", "c-m-b");

    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict on both files");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let mut conflicts = read_conflicts(&repo).unwrap();
    conflicts.sort_by(|x, y| x.path.cmp(&y.path));
    let paths: Vec<&str> = conflicts.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["a.txt", "b.txt"],
        "the rail lists every conflicting file"
    );

    // Resolving the whole rail empties the list (the editor closes) and lets the
    // banner's Continue finalize the merge (conflicts.md §2-3).
    resolve_file(&repo, "a.txt", Some("a-main\n")).unwrap();
    resolve_file(&repo, "b.txt", Some("b-main\n")).unwrap();
    assert!(
        read_conflicts(&repo).unwrap().is_empty(),
        "no conflict remains once every file is resolved"
    );

    assert_eq!(sync::continue_op(tmp.path()), Ok(SyncOutcome::Updated));
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "the merge commit has two parents");
}

#[test]
fn merge_resolve_then_continue_creates_the_merge_commit() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    resolve_file(&repo, "base.txt", Some("alpha\nbravo-main\ncharlie\n")).unwrap();
    assert!(
        !index_has_conflicts(&repo),
        "resolution clears the merge stages"
    );

    assert_eq!(sync::continue_op(tmp.path()), Ok(SyncOutcome::Updated));

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "the merge commit has two parents");
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        "alpha\nbravo-main\ncharlie\n"
    );
}

#[test]
fn rebase_continue_loops_through_each_conflicting_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "base.txt", "L1\nL2\nL3\n", "c0");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_file(&repo, tmp.path(), "base.txt", "L1\nF1\nL3\n", "c-f1");
    commit_file(&repo, tmp.path(), "base.txt", "L1\nF2\nL3\n", "c-f2");
    checkout(&repo, &main);
    commit_file(&repo, tmp.path(), "base.txt", "L1\nMAIN\nL3\n", "c-main");

    checkout(&repo, "feature");
    let rebased = cli::run(tmp.path(), &["rebase", &main]).unwrap();
    assert!(!rebased.success(), "the first replayed commit conflicts");

    // Resolving the first conflict to the onto side forces the second commit to
    // conflict too — the banner re-populates instead of finishing (conflicts.md §2).
    let repo = git2::Repository::open(tmp.path()).unwrap();
    resolve_file(&repo, "base.txt", Some("L1\nMAIN\nL3\n")).unwrap();
    assert_eq!(sync::continue_op(tmp.path()), Err(SyncError::Conflicts));

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert!(
        matches!(
            repo.state(),
            git2::RepositoryState::Rebase
                | git2::RepositoryState::RebaseMerge
                | git2::RepositoryState::RebaseInteractive
        ),
        "still rebasing on the next commit, state {:?}",
        repo.state()
    );

    resolve_file(&repo, "base.txt", Some("L1\nF2\nL3\n")).unwrap();
    assert_eq!(sync::continue_op(tmp.path()), Ok(SyncOutcome::Updated));

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        "L1\nF2\nL3\n"
    );
}

#[test]
fn delete_modify_resolved_by_keep_then_continue_keeps_the_file() {
    let tmp = delete_modify_merge();

    let repo = git2::Repository::open(tmp.path()).unwrap();
    resolve_file(&repo, "doomed.txt", Some("modified\n")).unwrap();
    assert!(!index_has_conflicts(&repo));

    assert_eq!(sync::continue_op(tmp.path()), Ok(SyncOutcome::Updated));

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(
        fs::read_to_string(tmp.path().join("doomed.txt")).unwrap(),
        "modified\n"
    );
}

#[test]
fn delete_modify_resolved_by_delete_then_continue_removes_the_file() {
    let tmp = delete_modify_merge();

    let repo = git2::Repository::open(tmp.path()).unwrap();
    resolve_file(&repo, "doomed.txt", None).unwrap();
    assert!(!index_has_conflicts(&repo));
    let mut index = repo.index().unwrap();
    index.read(false).unwrap();
    assert!(
        index.get_path(Path::new("doomed.txt"), 0).is_none(),
        "the delete resolution drops the index entry"
    );

    assert_eq!(sync::continue_op(tmp.path()), Ok(SyncOutcome::Updated));

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert!(
        !tmp.path().join("doomed.txt").exists(),
        "the file is removed from the working tree"
    );
}
