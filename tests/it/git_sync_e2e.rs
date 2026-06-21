use std::fs;
use std::path::{Path, PathBuf};

use helm::git::cli;
use helm::git::rebase::{self, RebaseAction, RebaseStep};
use helm::git::sync::{self, PullMode, SyncError, SyncOutcome};

struct Fixture {
    _tmp: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
    bare: PathBuf,
    branch: String,
}

// Local `file://` remote (bare) + clone A (upstream) + clone B (test subject).
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("remote.git");
    git2::Repository::init_bare(&bare).unwrap();

    let a = tmp.path().join("a");
    let repo_a = git2::Repository::init(&a).unwrap();
    set_test_config(&repo_a);
    commit_file(&repo_a, &a, "base.txt", "base\n", "c1");
    let branch = repo_a.head().unwrap().shorthand().unwrap().to_string();
    let url = format!("file://{}", bare.display());
    repo_a.remote("origin", &url).unwrap();
    let pushed = cli::run(&a, &["push", "-u", "origin", &branch]).unwrap();
    assert!(pushed.success(), "seed push: {}", pushed.stderr);

    let cloned = cli::run(tmp.path(), &["clone", &url, "b"]).unwrap();
    assert!(cloned.success(), "clone: {}", cloned.stderr);
    let b = tmp.path().join("b");
    set_test_config(&git2::Repository::open(&b).unwrap());

    Fixture {
        _tmp: tmp,
        a,
        b,
        bare,
        branch,
    }
}

fn set_test_config(repo: &git2::Repository) {
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    // Commits created by the `git` subprocess (merge/rebase) must not depend on
    // a possible global gpgsign config on the machine.
    cfg.set_bool("commit.gpgsign", false).unwrap();
}

fn commit_file(
    repo: &git2::Repository,
    dir: &Path,
    name: &str,
    content: &str,
    message: &str,
) -> git2::Oid {
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
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

fn commit_and_push_upstream(fx: &Fixture, name: &str, content: &str, message: &str) -> git2::Oid {
    let repo_a = git2::Repository::open(&fx.a).unwrap();
    let oid = commit_file(&repo_a, &fx.a, name, content, message);
    let pushed = cli::run(&fx.a, &["push"]).unwrap();
    assert!(pushed.success(), "upstream push: {}", pushed.stderr);
    oid
}

#[test]
fn fetch_all_brings_remote_refs_without_touching_head() {
    let fx = fixture();
    let c2 = commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");

    assert_eq!(sync::fetch_all(&fx.b), Ok(SyncOutcome::Updated));

    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let remote_ref = repo_b
        .find_reference(&format!("refs/remotes/origin/{}", fx.branch))
        .unwrap();
    assert_eq!(remote_ref.target().unwrap(), c2);
    assert_ne!(repo_b.head().unwrap().target().unwrap(), c2);

    assert_eq!(sync::fetch_all(&fx.b), Ok(SyncOutcome::UpToDate));
}

#[test]
fn pull_ff_advances_then_reports_up_to_date() {
    let fx = fixture();
    let c2 = commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");

    assert_eq!(sync::pull(&fx.b, PullMode::Ff), Ok(SyncOutcome::Updated));

    let repo_b = git2::Repository::open(&fx.b).unwrap();
    assert_eq!(repo_b.head().unwrap().target().unwrap(), c2);

    assert_eq!(sync::pull(&fx.b, PullMode::Ff), Ok(SyncOutcome::UpToDate));
}

// Pull is limited to the current branch (D-2026-06-04-pull-branche-courante):
// another branch advanced on the remote is neither fetched nor reflected.
#[test]
fn pull_fetches_only_the_current_branch() {
    let fx = fixture();
    let repo_a = git2::Repository::open(&fx.a).unwrap();
    let base = repo_a.head().unwrap().peel_to_commit().unwrap();
    repo_a.branch("other", &base, false).unwrap();
    let pushed = cli::run(&fx.a, &["push", "origin", "other"]).unwrap();
    assert!(pushed.success(), "push other: {}", pushed.stderr);
    let fetched = cli::run(&fx.b, &["fetch", "origin"]).unwrap();
    assert!(fetched.success(), "fetch B: {}", fetched.stderr);
    let other_seen = base.id();

    repo_a.set_head("refs/heads/other").unwrap();
    repo_a
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    commit_file(&repo_a, &fx.a, "other.txt", "o\n", "c-other");
    let pushed = cli::run(&fx.a, &["push", "origin", "other"]).unwrap();
    assert!(pushed.success(), "push other v2: {}", pushed.stderr);
    repo_a
        .set_head(&format!("refs/heads/{}", fx.branch))
        .unwrap();
    repo_a
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let c2 = commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");

    assert_eq!(sync::pull(&fx.b, PullMode::Ff), Ok(SyncOutcome::Updated));

    let repo_b = git2::Repository::open(&fx.b).unwrap();
    assert_eq!(repo_b.head().unwrap().target().unwrap(), c2);
    assert_eq!(
        repo_b
            .find_reference(&format!("refs/remotes/origin/{}", fx.branch))
            .unwrap()
            .target()
            .unwrap(),
        c2
    );
    assert_eq!(
        repo_b
            .find_reference("refs/remotes/origin/other")
            .unwrap()
            .target()
            .unwrap(),
        other_seen
    );
}

// Without an upstream, falls back to bare `git pull`: the standard git error
// surfaces, tree intact.
#[test]
fn pull_without_upstream_fails_with_git_message() {
    let fx = fixture();
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let head_commit = repo_b.head().unwrap().peel_to_commit().unwrap();
    repo_b.branch("feature", &head_commit, false).unwrap();
    repo_b.set_head("refs/heads/feature").unwrap();

    let result = sync::pull(&fx.b, PullMode::Ff);

    assert!(matches!(result, Err(SyncError::Other(_))), "{result:?}");
    assert_eq!(repo_b.state(), git2::RepositoryState::Clean);
    assert_eq!(repo_b.head().unwrap().target().unwrap(), head_commit.id());
}

// Merged & deleted on the remote (e.g. the Bitbucket UI): the upstream ref is
// gone, so the pull stays silent and prunes the stale tracking ref instead of
// surfacing git's "couldn't find remote ref" toast
// (D-2026-06-16-pull-remote-branch-gone).
#[test]
fn pull_on_a_remote_deleted_branch_is_silent_and_prunes_the_tracking_ref() {
    let fx = fixture();
    let checked = cli::run(&fx.b, &["checkout", "-b", "feat/gone"]).unwrap();
    assert!(checked.success(), "checkout: {}", checked.stderr);
    let pushed = cli::run(&fx.b, &["push", "-u", "origin", "feat/gone"]).unwrap();
    assert!(pushed.success(), "push -u: {}", pushed.stderr);

    // Branch disappears on the remote without touching B's tracking ref.
    let bare = git2::Repository::open_bare(&fx.bare).unwrap();
    bare.find_reference("refs/heads/feat/gone")
        .unwrap()
        .delete()
        .unwrap();

    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let head = repo_b.head().unwrap().target().unwrap();
    assert!(repo_b
        .find_reference("refs/remotes/origin/feat/gone")
        .is_ok());

    assert_eq!(
        sync::pull(&fx.b, PullMode::Ff),
        Err(SyncError::RemoteBranchGone)
    );

    assert!(repo_b
        .find_reference("refs/remotes/origin/feat/gone")
        .is_err());
    assert_eq!(repo_b.state(), git2::RepositoryState::Clean);
    assert_eq!(repo_b.head().unwrap().target().unwrap(), head);
}

#[test]
fn pull_ff_only_refuses_divergence_and_leaves_tree_intact() {
    let fx = fixture();
    commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let c3 = commit_file(&repo_b, &fx.b, "local.txt", "local\n", "c3");

    assert_eq!(
        sync::pull(&fx.b, PullMode::FfOnly),
        Err(SyncError::FfOnlyRefused)
    );

    assert_eq!(repo_b.state(), git2::RepositoryState::Clean);
    assert_eq!(repo_b.head().unwrap().target().unwrap(), c3);
    assert_eq!(fs::read_to_string(fx.b.join("base.txt")).unwrap(), "base\n");
}

#[test]
fn pull_rebase_replays_local_commits_on_remote_tip() {
    let fx = fixture();
    let c2 = commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    commit_file(&repo_b, &fx.b, "local.txt", "local\n", "c3");

    assert_eq!(
        sync::pull(&fx.b, PullMode::Rebase),
        Ok(SyncOutcome::Updated)
    );

    let head = repo_b.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "c3");
    assert_eq!(head.parent(0).unwrap().id(), c2);
    assert_eq!(repo_b.state(), git2::RepositoryState::Clean);
}

#[test]
fn pull_conflict_reports_conflicts_and_leaves_repo_in_progress() {
    let fx = fixture();
    commit_and_push_upstream(&fx, "base.txt", "from-a\n", "c2");
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    commit_file(&repo_b, &fx.b, "base.txt", "from-b\n", "c3");

    assert_eq!(sync::pull(&fx.b, PullMode::Ff), Err(SyncError::Conflicts));

    // Left as is (git.md §10): merge — or rebase if the machine config forces
    // pull.rebase; in both cases an "in progress" state.
    assert_ne!(repo_b.state(), git2::RepositoryState::Clean);
}

// Local repo with two diverged branches: `feature` (checked out) and the
// initial branch ahead by one commit. Rebase is a local op — no remote required.
// Returns the initial branch's name (init.defaultBranch varies) and its tip.
fn rebase_fixture(conflicting: bool) -> (tempfile::TempDir, String, git2::Oid) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "base.txt", "base\n", "c1");
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    let onto = repo.head().unwrap().shorthand().unwrap().to_string();
    repo.branch("feature", &base, false).unwrap();
    let onto_tip = commit_file(&repo, tmp.path(), "base.txt", "from-main\n", "c2");
    repo.set_head("refs/heads/feature").unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let (name, content) = if conflicting {
        ("base.txt", "from-feature\n")
    } else {
        ("feat.txt", "f\n")
    };
    commit_file(&repo, tmp.path(), name, content, "c-feat");
    (tmp, onto, onto_tip)
}

#[test]
fn rebase_onto_replays_the_current_branch_then_reports_up_to_date() {
    let (tmp, onto, onto_tip) = rebase_fixture(false);

    assert_eq!(
        sync::rebase_onto(tmp.path(), &onto),
        Ok(SyncOutcome::Updated)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "c-feat");
    assert_eq!(head.parent(0).unwrap().id(), onto_tip);
    assert_eq!(repo.state(), git2::RepositoryState::Clean);

    assert_eq!(
        sync::rebase_onto(tmp.path(), &onto),
        Ok(SyncOutcome::UpToDate)
    );
}

#[test]
fn rebase_conflict_reports_conflicts_and_leaves_rebase_in_progress() {
    let (tmp, onto, _) = rebase_fixture(true);

    assert_eq!(
        sync::rebase_onto(tmp.path(), &onto),
        Err(SyncError::Conflicts)
    );

    // Left as is (git.md §9): the Merge/Rebase in progress banner tells the
    // lasting state, resolution happens in the terminal.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_ne!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn rebase_onto_detached_head_fails_before_running_git() {
    let (tmp, onto, _) = rebase_fixture(false);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();

    assert_eq!(
        sync::rebase_onto(tmp.path(), &onto),
        Err(SyncError::Other("HEAD is detached".into()))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn rebase_onto_a_dash_leading_ref_is_refused_before_running_git() {
    // `update-ref` (or a hostile remote) can mint a ref the porcelain refuses:
    // it must never reach the CLI where it would parse as a flag.
    let (tmp, _, _) = rebase_fixture(false);
    let created = cli::run(tmp.path(), &["update-ref", "refs/heads/-foo", "HEAD"]).unwrap();
    assert!(created.success(), "update-ref: {}", created.stderr);

    assert_eq!(
        sync::rebase_onto(tmp.path(), "-foo"),
        Err(SyncError::Other("invalid ref name '-foo'".into()))
    );
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn rebase_onto_an_unknown_ref_surfaces_the_git_error() {
    let (tmp, _, _) = rebase_fixture(false);

    let result = sync::rebase_onto(tmp.path(), "ghost");

    assert!(matches!(result, Err(SyncError::Other(_))), "{result:?}");
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn merge_brings_the_named_branch_into_the_current_then_reports_up_to_date() {
    let (tmp, from, from_tip) = rebase_fixture(false);

    assert_eq!(sync::merge(tmp.path(), &from), Ok(SyncOutcome::Updated));

    // A true merge commit on the current branch: its own tip stays 1st
    // parent, the merged branch's tip lands as 2nd.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "feature");
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2);
    assert_eq!(head.parent(1).unwrap().id(), from_tip);
    assert_eq!(repo.state(), git2::RepositoryState::Clean);

    assert_eq!(sync::merge(tmp.path(), &from), Ok(SyncOutcome::UpToDate));
}

#[test]
fn merge_conflict_reports_conflicts_and_leaves_merge_in_progress() {
    let (tmp, from, _) = rebase_fixture(true);

    assert_eq!(sync::merge(tmp.path(), &from), Err(SyncError::Conflicts));

    // Left as is (git.md §9): the Merge/Rebase in progress banner tells the
    // lasting state, resolution happens in the terminal.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Merge);
}

#[test]
fn merge_on_detached_head_fails_before_running_git() {
    let (tmp, from, _) = rebase_fixture(false);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();

    assert_eq!(
        sync::merge(tmp.path(), &from),
        Err(SyncError::Other("HEAD is detached".into()))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn merge_of_a_dash_leading_ref_is_refused_before_running_git() {
    // Same argument-injection guard as the rebase flavors: a '-'-leading ref
    // must never reach the CLI where it would parse as a flag.
    let (tmp, _, _) = rebase_fixture(false);

    assert_eq!(
        sync::merge(tmp.path(), "-foo"),
        Err(SyncError::Other("invalid ref name '-foo'".into()))
    );
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

// `pick` rewrites base.txt on the original branch; `feature` branches before it.
// `conflicting` makes `feature` rewrite the same line so the replay collides.
fn cherry_fixture(conflicting: bool) -> (tempfile::TempDir, git2::Oid) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "base.txt", "base\n", "c1");
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature", &base, false).unwrap();
    let pick = commit_file(&repo, tmp.path(), "base.txt", "from-main\n", "pick-me");
    repo.set_head("refs/heads/feature").unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    // feature gets its own tip so the replay lands on a fresh parent. When
    // conflicting, that tip rewrites the very line `pick` touched.
    let (name, content) = if conflicting {
        ("base.txt", "from-feature\n")
    } else {
        ("feat.txt", "f\n")
    };
    commit_file(&repo, tmp.path(), name, content, "c-feat");
    (tmp, pick)
}

#[test]
fn cherry_pick_replays_the_commit_on_the_current_branch() {
    let (tmp, pick) = cherry_fixture(false);

    assert_eq!(
        sync::cherry_pick(tmp.path(), &pick.to_string()),
        Ok(SyncOutcome::Updated)
    );

    // A brand-new commit on feature carrying the picked change — not the original.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "feature");
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "pick-me");
    assert_ne!(head.id(), pick);
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        "from-main\n"
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn cherry_pick_conflict_reports_conflicts_and_leaves_it_in_progress() {
    let (tmp, pick) = cherry_fixture(true);

    assert_eq!(
        sync::cherry_pick(tmp.path(), &pick.to_string()),
        Err(SyncError::Conflicts)
    );

    // Left in progress (git.md §9): the banner's Abort follows this state.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::CherryPick);
    assert_eq!(sync::abort_op(tmp.path()), Ok(SyncOutcome::Updated));
    assert_eq!(
        git2::Repository::open(tmp.path()).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn cherry_pick_on_detached_head_fails_before_running_git() {
    let (tmp, pick) = cherry_fixture(false);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();

    assert_eq!(
        sync::cherry_pick(tmp.path(), &pick.to_string()),
        Err(SyncError::Other("HEAD is detached".into()))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn cherry_pick_of_a_dash_leading_commit_is_refused_before_running_git() {
    // Same argument-injection guard as the rebase flavors: a '-'-leading
    // commit-ish must never reach the CLI where it would parse as a flag.
    let (tmp, _) = cherry_fixture(false);

    assert_eq!(
        sync::cherry_pick(tmp.path(), "-foo"),
        Err(SyncError::Other("invalid commit '-foo'".into()))
    );
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

// A linear history; `target` (c2) can be reverted. `conflicting` stacks a later
// edit on the same line so the inverse no longer applies.
fn revert_fixture(conflicting: bool) -> (tempfile::TempDir, git2::Oid) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "base.txt", "base\n", "c1");
    let target = commit_file(&repo, tmp.path(), "base.txt", "v2\n", "c2");
    if conflicting {
        commit_file(&repo, tmp.path(), "base.txt", "v3\n", "c3");
    }
    (tmp, target)
}

#[test]
fn revert_commits_the_inverse_on_the_current_branch() {
    let (tmp, target) = revert_fixture(false);

    assert_eq!(
        sync::revert(tmp.path(), &target.to_string()),
        Ok(SyncOutcome::Updated)
    );

    // The inverse of c2 (back to "base"), committed with no editor.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert!(head.message().unwrap().starts_with("Revert"));
    assert_eq!(
        fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn revert_conflict_reports_conflicts_and_leaves_it_in_progress() {
    let (tmp, target) = revert_fixture(true);

    assert_eq!(
        sync::revert(tmp.path(), &target.to_string()),
        Err(SyncError::Conflicts)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Revert);
    assert_eq!(sync::abort_op(tmp.path()), Ok(SyncOutcome::Updated));
    assert_eq!(
        git2::Repository::open(tmp.path()).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn revert_on_detached_head_fails_before_running_git() {
    let (tmp, target) = revert_fixture(false);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();

    assert_eq!(
        sync::revert(tmp.path(), &target.to_string()),
        Err(SyncError::Other("HEAD is detached".into()))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

// Like `rebase_fixture(false)` but with TWO commits on `feature` (f1: feat1.txt,
// f2: feat2.txt) — enough to exercise squash/fixup/drop/reword plans.
fn interactive_fixture() -> (tempfile::TempDir, String, git2::Oid) {
    let (tmp, onto, onto_tip) = rebase_fixture(false);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    // rebase_fixture's feature commit is "c-feat" (feat.txt); rename the story
    // here: f1 = that commit's follow-up, f2 on top.
    commit_file(&repo, tmp.path(), "feat2.txt", "two\n", "f2");
    (tmp, onto, onto_tip)
}

// Steps derived from the real plan (oldest first), one action per commit.
fn plan(workdir: &Path, onto: &str, actions: &[RebaseAction]) -> Vec<RebaseStep> {
    let repo = git2::Repository::open(workdir).unwrap();
    let commits = rebase::rebase_commits(&repo, onto).unwrap();
    assert_eq!(commits.len(), actions.len(), "fixture/plan mismatch");
    commits
        .into_iter()
        .zip(actions.iter().cloned())
        .map(|(commit, action)| RebaseStep {
            oid: commit.oid,
            action,
        })
        .collect()
}

#[test]
fn interactive_rebase_replays_picks_onto_the_target() {
    let (tmp, onto, onto_tip) = interactive_fixture();
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick, RebaseAction::Pick]);

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Ok(SyncOutcome::Updated)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap().trim_end(), "f2");
    let first = head.parent(0).unwrap();
    assert_eq!(first.message().unwrap().trim_end(), "c-feat");
    assert_eq!(first.parent(0).unwrap().id(), onto_tip);
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn interactive_rebase_squash_combines_both_messages_into_one_commit() {
    let (tmp, onto, onto_tip) = interactive_fixture();
    let steps = plan(
        tmp.path(),
        &onto,
        &[RebaseAction::Pick, RebaseAction::Squash],
    );

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Ok(SyncOutcome::Updated)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent(0).unwrap().id(), onto_tip);
    // GIT_EDITOR=true keeps git's combined message: both subjects survive.
    let message = head.message().unwrap();
    assert!(message.contains("c-feat"), "{message}");
    assert!(message.contains("f2"), "{message}");
    // Both trees melded into the single commit.
    assert!(tmp.path().join("feat.txt").exists());
    assert!(tmp.path().join("feat2.txt").exists());
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn interactive_rebase_fixup_melds_and_discards_the_fixup_message() {
    let (tmp, onto, onto_tip) = interactive_fixture();
    let steps = plan(
        tmp.path(),
        &onto,
        &[RebaseAction::Pick, RebaseAction::Fixup],
    );

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Ok(SyncOutcome::Updated)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap().trim_end(), "c-feat");
    assert_eq!(head.parent(0).unwrap().id(), onto_tip);
    assert!(tmp.path().join("feat2.txt").exists());
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn interactive_rebase_drop_removes_the_commit_and_its_changes() {
    let (tmp, onto, onto_tip) = interactive_fixture();
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick, RebaseAction::Drop]);

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Ok(SyncOutcome::Updated)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap().trim_end(), "c-feat");
    assert_eq!(head.parent(0).unwrap().id(), onto_tip);
    assert!(!tmp.path().join("feat2.txt").exists());
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn interactive_rebase_reword_amends_the_message_without_an_editor() {
    let (tmp, onto, onto_tip) = interactive_fixture();
    // Quotes and a $var: the message goes through `--amend -F <file>` — only
    // the file path crosses the shell, never the content.
    let new_message = "c-feat reworded\n\nbody with 'quotes' and $vars";
    let steps = plan(
        tmp.path(),
        &onto,
        &[RebaseAction::Reword(new_message.into()), RebaseAction::Pick],
    );

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Ok(SyncOutcome::Updated)
    );

    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap().trim_end(), "f2");
    let reworded = head.parent(0).unwrap();
    assert_eq!(reworded.message().unwrap().trim_end(), new_message);
    assert_eq!(reworded.parent(0).unwrap().id(), onto_tip);
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert!(
        !tmp.path().join(".git/helm-rebase").exists(),
        "the injected plan is cleaned up after success"
    );
}

#[test]
fn a_conflict_stopped_plan_survives_a_terminal_continue_with_a_pending_reword() {
    let (tmp, onto, _) = rebase_fixture(true);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    commit_file(&repo, tmp.path(), "feat2.txt", "two\n", "f2");
    let steps = plan(
        tmp.path(),
        &onto,
        &[
            RebaseAction::Pick,
            RebaseAction::Reword("f2 reworded".into()),
        ],
    );

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Err(SyncError::Conflicts)
    );

    // Resolution in the terminal (git.md §9): the pending reword's `exec` step
    // still reads its message file — the plan must not die with helm's return
    // (a tempdir would have vanished here).
    fs::write(tmp.path().join("base.txt"), "resolved\n").unwrap();
    let added = cli::run(tmp.path(), &["add", "base.txt"]).unwrap();
    assert!(added.success(), "add: {}", added.stderr);
    let continued = cli::run_with_env(
        tmp.path(),
        &["rebase", "--continue"],
        &[("GIT_EDITOR", "true".to_string())],
    )
    .unwrap();
    assert!(continued.success(), "continue: {}", continued.stderr);

    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap().trim_end(), "f2 reworded");
    assert_eq!(
        head.parent(0).unwrap().message().unwrap().trim_end(),
        "c-feat"
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn interactive_rebase_refuses_when_the_checked_out_branch_changed() {
    let (tmp, onto, _) = interactive_fixture();
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick, RebaseAction::Pick]);
    // A same-tip backup branch checked out after planning: the OID sequence
    // still matches — only the branch identity tells the plans apart.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let tip = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("backup", &tip, false).unwrap();
    repo.set_head("refs/heads/backup").unwrap();

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Err(SyncError::Other(
            "the checked-out branch changed since the plan was prepared — \
             reopen Interactive rebase"
                .into()
        ))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "backup");
    assert_eq!(repo.head().unwrap().target().unwrap(), tip.id());
}

#[test]
fn rebase_commands_refuse_while_an_operation_is_in_progress() {
    // An op can start in the terminal at any time: both rebase flavors refuse
    // with the banner's wording instead of surfacing git's raw fatal.
    let (tmp, onto, _) = rebase_fixture(true);
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick]);
    let merged = cli::run(tmp.path(), &["merge", &onto]).unwrap();
    assert!(!merged.success(), "merge should conflict");

    assert_eq!(
        sync::rebase_onto(tmp.path(), &onto),
        Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into()
        ))
    );
    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into()
        ))
    );

    // Still resolvable: the guards ran nothing.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Merge);
}

#[test]
fn interactive_rebase_conflict_leaves_progress_then_abort_restores_the_branch() {
    let (tmp, onto, _) = rebase_fixture(true);
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let original = repo.head().unwrap().target().unwrap();
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick]);

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Err(SyncError::Conflicts)
    );
    assert_ne!(repo.state(), git2::RepositoryState::Clean);

    assert_eq!(sync::abort_op(tmp.path()), Ok(SyncOutcome::Updated));

    let reopened = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(reopened.state(), git2::RepositoryState::Clean);
    assert_eq!(reopened.head().unwrap().target().unwrap(), original);
    assert_eq!(reopened.head().unwrap().shorthand().unwrap(), "feature");
    assert!(
        !tmp.path().join(".git/helm-rebase").exists(),
        "an aborted plan no longer needs its injected todo"
    );
}

#[test]
fn interactive_rebase_refuses_a_stale_plan_before_running_git() {
    let (tmp, onto, _) = interactive_fixture();
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick, RebaseAction::Pick]);
    // The branch moves after the plan was prepared: a commit the todo does not
    // know about would be silently dropped.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let new_tip = commit_file(&repo, tmp.path(), "feat3.txt", "three\n", "f3");

    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Err(SyncError::Other(
            "the branch changed since the plan was prepared — reopen Interactive rebase".into()
        ))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(repo.head().unwrap().target().unwrap(), new_tip);
}

#[test]
fn interactive_rebase_guards_refuse_bad_input_before_running_git() {
    let (tmp, onto, _) = interactive_fixture();
    let steps = plan(tmp.path(), &onto, &[RebaseAction::Pick, RebaseAction::Pick]);

    // Argument-injection guard, checked first.
    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", "-foo", &steps),
        Err(SyncError::Other("invalid ref name '-foo'".into()))
    );
    // Empty plan.
    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &[]),
        Err(SyncError::Other("nothing to rebase".into()))
    );
    // Invalid plan (meld with nothing below) re-checked at the execution gate.
    let melding = plan(
        tmp.path(),
        &onto,
        &[RebaseAction::Squash, RebaseAction::Pick],
    );
    let result = sync::interactive_rebase(tmp.path(), "feature", &onto, &melding);
    assert!(matches!(result, Err(SyncError::Other(_))), "{result:?}");
    // Detached HEAD.
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();
    assert_eq!(
        sync::interactive_rebase(tmp.path(), "feature", &onto, &steps),
        Err(SyncError::Other("HEAD is detached".into()))
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(repo.head().unwrap().target().unwrap(), head);
}

#[test]
fn abort_op_with_a_clean_repo_fails_without_running_git() {
    let (tmp, _, _) = rebase_fixture(false);

    assert_eq!(
        sync::abort_op(tmp.path()),
        Err(SyncError::Other("no operation in progress".into()))
    );
}

#[test]
fn abort_op_aborts_a_merge_started_in_the_terminal() {
    // The banner also covers ops started outside helm: the abort flavor
    // follows the repo state (here `merge --abort`).
    let (tmp, onto, _) = rebase_fixture(true);
    let original = git2::Repository::open(tmp.path())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    let merged = cli::run(tmp.path(), &["merge", &onto]).unwrap();
    assert!(!merged.success(), "merge should conflict");
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Merge);

    assert_eq!(sync::abort_op(tmp.path()), Ok(SyncOutcome::Updated));

    let reopened = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(reopened.state(), git2::RepositoryState::Clean);
    assert_eq!(reopened.head().unwrap().target().unwrap(), original);
}

#[test]
fn push_updates_the_remote_branch() {
    let fx = fixture();
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let c2 = commit_file(&repo_b, &fx.b, "new.txt", "n\n", "c2");

    assert_eq!(sync::push(&fx.b), Ok(SyncOutcome::Updated));

    let bare = git2::Repository::open(&fx.bare).unwrap();
    let remote_branch = bare
        .find_reference(&format!("refs/heads/{}", fx.branch))
        .unwrap();
    assert_eq!(remote_branch.target().unwrap(), c2);
}

#[test]
fn push_without_upstream_creates_it_on_origin() {
    let fx = fixture();
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let head_commit = repo_b.head().unwrap().peel_to_commit().unwrap();
    repo_b.branch("feature", &head_commit, false).unwrap();
    repo_b.set_head("refs/heads/feature").unwrap();
    let c = commit_file(&repo_b, &fx.b, "feat.txt", "f\n", "feat");

    assert_eq!(sync::push(&fx.b), Ok(SyncOutcome::Updated));

    let reopened = git2::Repository::open(&fx.b).unwrap();
    let branch = reopened
        .find_branch("feature", git2::BranchType::Local)
        .unwrap();
    assert!(branch.upstream().is_ok());
    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert_eq!(
        bare.find_reference("refs/heads/feature")
            .unwrap()
            .target()
            .unwrap(),
        c
    );
}

#[test]
fn push_non_fast_forward_is_rejected_without_force() {
    let fx = fixture();
    let c2 = commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    commit_file(&repo_b, &fx.b, "local.txt", "local\n", "c3");

    assert_eq!(sync::push(&fx.b), Err(SyncError::NonFastForward));

    // The remote did not move: never a force.
    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert_eq!(
        bare.find_reference(&format!("refs/heads/{}", fx.branch))
            .unwrap()
            .target()
            .unwrap(),
        c2
    );
}

#[test]
fn force_push_with_lease_overwrites_after_an_amend() {
    let fx = fixture();
    // Rewrite the published commit: the plain push is now non-fast-forward, but
    // the lease holds (B's remote-tracking ref still matches the remote tip).
    let amended = cli::run(&fx.b, &["commit", "--amend", "-m", "c1 amended"]).unwrap();
    assert!(amended.success(), "amend: {}", amended.stderr);
    let new_head = git2::Repository::open(&fx.b)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();

    assert_eq!(sync::push(&fx.b), Err(SyncError::NonFastForward));
    assert_eq!(sync::force_push(&fx.b), Ok(SyncOutcome::Updated));

    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert_eq!(
        bare.find_reference(&format!("refs/heads/{}", fx.branch))
            .unwrap()
            .target()
            .unwrap(),
        new_head
    );
}

#[test]
fn force_push_with_lease_is_refused_when_the_remote_moved() {
    let fx = fixture();
    // The remote advances, but B never fetches: its remote-tracking ref is stale,
    // so the lease must make git refuse rather than overwrite c2.
    let c2 = commit_and_push_upstream(&fx, "base.txt", "v2\n", "c2");
    let amended = cli::run(&fx.b, &["commit", "--amend", "-m", "c1 amended"]).unwrap();
    assert!(amended.success(), "amend: {}", amended.stderr);

    assert_eq!(sync::force_push(&fx.b), Err(SyncError::StaleInfo));

    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert_eq!(
        bare.find_reference(&format!("refs/heads/{}", fx.branch))
            .unwrap()
            .target()
            .unwrap(),
        c2,
        "the lease held: the remote tip was never overwritten"
    );
}

#[test]
fn force_push_without_an_upstream_is_refused() {
    let fx = fixture();
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let head_commit = repo_b.head().unwrap().peel_to_commit().unwrap();
    repo_b.branch("feature", &head_commit, false).unwrap();
    repo_b.set_head("refs/heads/feature").unwrap();
    commit_file(&repo_b, &fx.b, "feat.txt", "f\n", "feat");

    assert_eq!(sync::force_push(&fx.b), Err(SyncError::NoUpstream));

    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert!(
        bare.find_reference("refs/heads/feature").is_err(),
        "nothing published without an upstream"
    );
}

#[test]
fn delete_remote_branch_removes_the_branch_on_the_remote() {
    let fx = fixture();
    // `feature` branch pushed to origin from A, fetched into B: the remote chip
    // `origin/feature` exists on the B side.
    let repo_a = git2::Repository::open(&fx.a).unwrap();
    let base = repo_a.head().unwrap().peel_to_commit().unwrap();
    repo_a.branch("feature", &base, false).unwrap();
    let pushed = cli::run(&fx.a, &["push", "origin", "feature"]).unwrap();
    assert!(pushed.success(), "push feature: {}", pushed.stderr);
    let fetched = cli::run(&fx.b, &["fetch", "origin"]).unwrap();
    assert!(fetched.success(), "fetch B: {}", fetched.stderr);

    assert_eq!(
        sync::delete_remote_branch(&fx.b, "origin/feature"),
        Ok(SyncOutcome::Updated)
    );

    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert!(bare.find_reference("refs/heads/feature").is_err());
    // The local remote-tracking ref disappears too: the graph follows.
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    assert!(repo_b
        .find_reference("refs/remotes/origin/feature")
        .is_err());
}

#[test]
fn delete_remote_branch_resolves_a_local_name_to_its_remote_homonym() {
    let fx = fixture();
    // Merged local chip (`also_remote`): local `feature` + `origin/feature` at
    // the same commit — the remote deletion receives the **local** name.
    let repo_b = git2::Repository::open(&fx.b).unwrap();
    let base = repo_b.head().unwrap().peel_to_commit().unwrap();
    repo_b.branch("feature", &base, false).unwrap();
    let pushed = cli::run(&fx.b, &["push", "origin", "feature"]).unwrap();
    assert!(pushed.success(), "push feature: {}", pushed.stderr);

    assert_eq!(
        sync::delete_remote_branch(&fx.b, "feature"),
        Ok(SyncOutcome::Updated)
    );

    let bare = git2::Repository::open(&fx.bare).unwrap();
    assert!(bare.find_reference("refs/heads/feature").is_err());
    // The local branch itself does not move.
    let reopened = git2::Repository::open(&fx.b).unwrap();
    assert!(reopened
        .find_branch("feature", git2::BranchType::Local)
        .is_ok());
}

#[test]
fn delete_remote_branch_unknown_name_errs_before_any_network() {
    let fx = fixture();

    let result = sync::delete_remote_branch(&fx.b, "ghost");

    assert!(matches!(result, Err(SyncError::Other(_))), "{result:?}");
}

#[test]
fn operations_without_remote_report_no_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_test_config(&repo);
    commit_file(&repo, tmp.path(), "a.txt", "a\n", "c1");

    assert_eq!(sync::fetch_all(tmp.path()), Err(SyncError::NoRemote));
    assert_eq!(
        sync::pull(tmp.path(), PullMode::Ff),
        Err(SyncError::NoRemote)
    );
    assert_eq!(sync::push(tmp.path()), Err(SyncError::NoRemote));
    assert_eq!(
        sync::delete_remote_branch(tmp.path(), "origin/feature"),
        Err(SyncError::NoRemote)
    );
}
