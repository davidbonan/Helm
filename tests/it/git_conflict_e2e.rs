use std::fs;
use std::path::Path;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::cli;
use helm::git::conflict::{
    read_conflict, read_conflicts, resolve_file, resolve_file_side, ConflictFile, ConflictKind,
    Region,
};
use helm::git::sync::{self, SyncError, SyncOutcome};
use helm::theme::Palette;
use helm::ui::conflict_view::{conflict_view, ConflictEditorState, ResolveRequest};

fn set_test_config(repo: &git2::Repository) {
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    cfg.set_bool("commit.gpgsign", false).unwrap();
    // A global `core.autocrlf` would rewrite the fixtures' terminators on commit.
    cfg.set_bool("core.autocrlf", false).unwrap();
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
    diverged_text(
        "alpha\nbravo\ncharlie\n",
        "alpha\nbravo-feature\ncharlie\n",
        "alpha\nbravo-main\ncharlie\n",
    )
}

/// `diverged` with explicit byte-for-byte contents, so a test can pick the file's
/// line terminator and trailing newline.
fn diverged_text(base: &str, feature: &str, main_side: &str) -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "base.txt", base, "c1");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_file(&repo, tmp.path(), "base.txt", feature, "c-feature");
    checkout(&repo, &main);
    commit_file(&repo, tmp.path(), "base.txt", main_side, "c-main");
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

#[test]
fn taking_a_side_sees_a_resolution_made_by_another_handle() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    // Worker-style long-lived handle: its in-memory index snapshot is loaded
    // while base.txt still carries its merge stages.
    let worker = git2::Repository::open(tmp.path()).unwrap();
    assert!(worker.index().unwrap().has_conflicts());

    // Terminal pane: the file is resolved by hand and staged through a separate
    // handle, so the on-disk index holds no conflict for it any more.
    let in_a_pane = "alpha\nresolved-in-a-pane\ncharlie\n";
    fs::write(tmp.path().join("base.txt"), in_a_pane).unwrap();
    let external = git2::Repository::open(tmp.path()).unwrap();
    let mut index = external.index().unwrap();
    index.add_path(Path::new("base.txt")).unwrap();
    index.write().unwrap();

    let err = resolve_file_side(&worker, "base.txt", true).unwrap_err();
    assert!(
        err.message().contains("no conflict"),
        "a side cannot be taken on a path the index no longer reports as conflicted: {err}"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        in_a_pane,
        "taking a side off a stale index clobbered the pane's resolution"
    );
    assert!(!index_has_conflicts(&worker));
}

/// Drives the real conflict editor headless: ticks the A (ours) take box of the
/// single conflict region and clicks Save, returning the content it asks to write.
fn save_taking_ours(file: ConflictFile) -> String {
    struct Page {
        state: ConflictEditorState,
        resolve: Option<ResolveRequest>,
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 720.0))
        .build_ui_state(
            |ui, page: &mut Page| {
                let action = conflict_view(ui, &Palette::dark(), &mut page.state, false);
                if let Some(resolve) = action.resolve {
                    page.resolve = Some(resolve);
                }
            },
            Page {
                state: ConflictEditorState::new(vec![file]),
                resolve: None,
            },
        );
    harness.run();
    // Pane A's take boxes render first — index 0 is the first conflict on ours.
    harness
        .get_all_by(|node| format!("{:?}", node.role()) == "CheckBox")
        .next()
        .expect("take checkbox present")
        .click();
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();

    match harness.state().resolve.clone() {
        Some(ResolveRequest::Compose { content, .. }) => content,
        other => panic!("expected a Compose resolve, got {other:?}"),
    }
}

#[test]
fn resolving_a_crlf_file_through_the_editor_keeps_crlf() {
    let (tmp, _main) = diverged_text(
        "alpha\r\nbravo\r\ncharlie\r\n",
        "alpha\r\nbravo-feature\r\ncharlie\r\n",
        "alpha\r\nbravo-main\r\ncharlie\r\n",
    );
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();
    assert!(cf.eol.crlf, "the CRLF terminator is detected from the blob");
    assert!(cf.eol.final_newline);

    let content = save_taking_ours(cf);
    resolve_file(&repo, "base.txt", Some(&content)).unwrap();

    assert_eq!(
        fs::read(tmp.path().join("base.txt")).unwrap(),
        b"alpha\r\nbravo-main\r\ncharlie\r\n"
    );
    assert!(!index_has_conflicts(&repo));
}

#[test]
fn resolving_a_file_without_a_final_newline_does_not_add_one() {
    let (tmp, _main) = diverged_text(
        "alpha\nbravo\ncharlie",
        "alpha\nbravo-feature\ncharlie",
        "alpha\nbravo-main\ncharlie",
    );
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();
    assert!(!cf.eol.crlf);
    assert!(!cf.eol.final_newline, "the blob has no trailing newline");

    let content = save_taking_ours(cf);
    resolve_file(&repo, "base.txt", Some(&content)).unwrap();

    assert_eq!(
        fs::read(tmp.path().join("base.txt")).unwrap(),
        b"alpha\nbravo-main\ncharlie"
    );
}

#[test]
fn an_untouched_working_tree_file_reports_no_disk_divergence() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();

    // git writes the working tree in `merge` style (no base section, branch names
    // on the markers) while the reconstruction is diff3 — not a divergence.
    assert_eq!(cf.disk_divergence, None);
}

#[test]
fn a_hand_edited_working_tree_file_reports_its_content() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    fs::write(tmp.path().join("base.txt"), "alpha\nbravo-mine\ncharlie\n").unwrap();

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();

    assert_eq!(
        cf.disk_divergence.as_deref(),
        Some("alpha\nbravo-mine\ncharlie\n")
    );
}

#[test]
fn an_untouched_crlf_working_tree_file_is_not_a_divergence() {
    let (tmp, _main) = diverged_text(
        "alpha\r\nbravo\r\ncharlie\r\n",
        "alpha\r\nbravo-feature\r\ncharlie\r\n",
        "alpha\r\nbravo-main\r\ncharlie\r\n",
    );
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();
    assert!(cf.eol.crlf);

    // The reconstruction is LF; the terminator alone must not read as a hand edit.
    assert_eq!(cf.disk_divergence, None);
}

#[test]
fn the_diverging_content_reaches_the_editor_and_save_writes_it_back() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let mine = "alpha\nbravo-mine\ncharlie\n";
    fs::write(tmp.path().join("base.txt"), mine).unwrap();

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();

    let content = save_loading_my_version(cf);
    assert_eq!(content, mine);

    resolve_file(&repo, "base.txt", Some(&content)).unwrap();
    assert_eq!(
        fs::read(tmp.path().join("base.txt")).unwrap(),
        mine.as_bytes()
    );
    assert!(!index_has_conflicts(&repo));
}

/// Drives the real editor headless over a file flagged as diverging: takes the
/// notice's *Load my version* and saves — no region pick, the whole-file override
/// alone unlocks Save (conflicts.md §5).
fn save_loading_my_version(file: ConflictFile) -> String {
    struct Page {
        state: ConflictEditorState,
        resolve: Option<ResolveRequest>,
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 720.0))
        .build_ui_state(
            |ui, page: &mut Page| {
                let action = conflict_view(ui, &Palette::dark(), &mut page.state, false);
                if let Some(resolve) = action.resolve {
                    page.resolve = Some(resolve);
                }
            },
            Page {
                state: ConflictEditorState::new(vec![file]),
                resolve: None,
            },
        );
    harness.run();
    harness.get_by_label("Load my version").click();
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();

    match harness.state().resolve.clone() {
        Some(ResolveRequest::Compose { content, .. }) => content,
        other => panic!("expected a Compose resolve, got {other:?}"),
    }
}

/// Commits `name` as a symlink pointing at `target`.
fn commit_symlink(repo: &git2::Repository, dir: &Path, name: &str, target: &str, message: &str) {
    let full = dir.join(name);
    if fs::symlink_metadata(&full).is_ok() {
        fs::remove_file(&full).unwrap();
    }
    std::os::unix::fs::symlink(target, &full).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    commit(repo, &mut index, message);
}

fn index_entry(repo: &git2::Repository, path: &str) -> git2::IndexEntry {
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    index.get_path(Path::new(path), 0).unwrap()
}

/// `link` points at a different tracked file on each side, so the merge leaves
/// `120000` stages. Taking a side must re-create the **link**: writing the chosen
/// blob through the link would land in the file it points at.
#[test]
fn taking_a_side_on_a_symlink_conflict_rewrites_the_link_not_its_target() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "target_a.txt", "A-PRISTINE\n", "targets");
    commit_file(
        &repo,
        tmp.path(),
        "target_b.txt",
        "B-PRISTINE\n",
        "targets b",
    );
    commit_file(
        &repo,
        tmp.path(),
        "target_base.txt",
        "BASE\n",
        "targets base",
    );
    commit_symlink(&repo, tmp.path(), "link", "target_base.txt", "c1");
    let main = repo.head().unwrap().shorthand().unwrap().to_string();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();

    checkout(&repo, "feature");
    commit_symlink(&repo, tmp.path(), "link", "target_b.txt", "c-feature");
    checkout(&repo, &main);
    commit_symlink(&repo, tmp.path(), "link", "target_a.txt", "c-main");

    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    resolve_file_side(&repo, "link", false).unwrap();

    let full = tmp.path().join("link");
    assert!(
        fs::symlink_metadata(&full).unwrap().is_symlink(),
        "the resolution replaced the link with a regular file"
    );
    assert_eq!(fs::read_link(&full).unwrap(), Path::new("target_b.txt"));
    assert_eq!(
        fs::read_to_string(tmp.path().join("target_a.txt")).unwrap(),
        "A-PRISTINE\n",
        "the resolution was written through the link, into the file it pointed at"
    );
    assert_eq!(index_entry(&repo, "link").mode, 0o120000);
    assert!(!index_has_conflicts(&repo));
}

/// A composed Save is a resolution too: it must see the same index the side-taking
/// path sees, or an editor left open over an aborted merge writes its buffer into
/// a clean tree.
#[test]
fn saving_a_composition_on_a_path_no_longer_conflicted_is_refused() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let editor = git2::Repository::open(tmp.path()).unwrap();
    assert!(editor.index().unwrap().has_conflicts());

    let aborted = cli::run(tmp.path(), &["merge", "--abort"]).unwrap();
    assert!(aborted.success(), "abort should succeed");
    let on_disk = fs::read_to_string(tmp.path().join("base.txt")).unwrap();

    let err = resolve_file(&editor, "base.txt", Some("alpha\nstale\ncharlie\n")).unwrap_err();
    assert!(
        err.message().contains("no conflict"),
        "a composition cannot be saved on a path the index no longer reports as conflicted: {err}"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        on_disk,
        "the stale composition was written over the aborted merge's working tree"
    );
    assert!(!index_has_conflicts(&editor));
}

/// `git checkout --ours/--theirs` restores that side's mode; taking a side must
/// record the same thing rather than whatever mode the merge left on disk.
#[test]
fn taking_a_side_records_that_sides_exec_bit() {
    use std::os::unix::fs::PermissionsExt;

    let (tmp, main) = diverged();
    let repo = git2::Repository::open(tmp.path()).unwrap();
    checkout(&repo, "feature");
    fs::set_permissions(
        tmp.path().join("base.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    commit_file(
        &repo,
        tmp.path(),
        "base.txt",
        "alpha\nbravo-feature\ncharlie\n",
        "c-feature-exec",
    );
    checkout(&repo, &main);

    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    // The merge left the working tree executable (theirs' mode), so staging what is
    // on disk would record `100755` for a side that never carried the bit.
    assert_eq!(
        fs::metadata(tmp.path().join("base.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0o111
    );

    resolve_file_side(&repo, "base.txt", true).unwrap();

    assert_eq!(
        index_entry(&repo, "base.txt").mode,
        0o100644,
        "taking ours recorded an exec bit ours never carried"
    );
}

/// A conflicted submodule cannot be read as a blob. It must be left out of the
/// rail instead of failing the whole read — the editor would otherwise refuse to
/// open for every other conflicted file of the repository.
#[test]
fn a_conflicted_gitlink_does_not_hide_the_other_conflicts() {
    let (tmp, _main) = diverged();
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let commit_oid = repo.head().unwrap().peel_to_commit().unwrap().id();
    let mut index = repo.index().unwrap();
    for stage in 1..=3u16 {
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o160000,
                uid: 0,
                gid: 0,
                file_size: 0,
                id: commit_oid,
                flags: stage << 12,
                flags_extended: 0,
                path: b"sub".to_vec(),
            })
            .unwrap();
    }
    index.write().unwrap();

    let files = read_conflicts(&repo).unwrap();
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        vec!["base.txt"],
        "the gitlink should be skipped, not fail the whole rail"
    );
}

/// Both sides added the same line next to their own change: git trims it out of
/// the conflict when it writes the working tree, the diff3 reconstruction keeps it
/// inside. Same file, no hand edit — no notice.
#[test]
fn sides_sharing_a_boundary_line_are_not_a_disk_divergence() {
    let (tmp, _main) = diverged_text(
        "alpha\ncharlie\n",
        "alpha\nshared\nbravo-feature\ncharlie\n",
        "alpha\nshared\nbravo-main\ncharlie\n",
    );
    let merged = cli::run(tmp.path(), &["merge", "feature"]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let cf = read_conflict(&repo, "base.txt").unwrap();

    assert_eq!(
        cf.disk_divergence, None,
        "an untouched working tree was reported as edited outside helm"
    );
}
