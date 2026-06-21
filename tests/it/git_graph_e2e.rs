use std::fs;
use std::path::Path;

use helm::git::graph::{self, Graph, RefKind};
use helm::git::{stash, worktree};

/// `stash::save` signs via `repo.signature()`: a local identity is required.
fn set_identity(repo: &git2::Repository) {
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
}

fn commit_on(
    repo: &git2::Repository,
    name: &str,
    content: &str,
    message: &str,
    parents: &[git2::Oid],
    update_head: bool,
) -> git2::Oid {
    let dir = repo.workdir().unwrap();
    fs::write(dir.join(name), content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let parent_commits: Vec<git2::Commit> = parents
        .iter()
        .map(|oid| repo.find_commit(*oid).unwrap())
        .collect();
    let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
    let target = if update_head { Some("HEAD") } else { None };
    repo.commit(target, &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

fn summaries(graph: &Graph) -> Vec<String> {
    graph.commits.iter().map(|c| c.summary.clone()).collect()
}

#[test]
fn graph_refs_mark_branches_that_can_create_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_identity(&repo);
    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    let commit = repo.find_commit(c1).unwrap();
    repo.branch("feat/x", &commit, false).unwrap();

    let graph = graph::load(tmp.path(), 0).unwrap();
    let refs = &graph.commits[0].refs;
    assert!(refs
        .iter()
        .any(|r| r.name == "feat/x" && r.worktree_available));
    assert!(refs.iter().any(|r| r.is_head && !r.worktree_available));

    worktree::create(tmp.path(), "feat/x", None, None).unwrap();
    let graph = graph::load(tmp.path(), 0).unwrap();
    assert!(graph.commits[0]
        .refs
        .iter()
        .any(|r| r.name == "feat/x" && !r.worktree_available));
}

#[test]
fn walk_lists_commits_with_decorations() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    let c2 = commit_on(&repo, "a.txt", "2", "second", &[c1], true);
    repo.tag_lightweight("v1", &repo.find_object(c1, None).unwrap(), false)
        .unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();

    assert_eq!(
        summaries(&g),
        vec!["second".to_string(), "first".to_string()]
    );
    assert!(!g.has_more);

    let head = g.commits.iter().find(|c| c.oid == c2).unwrap();
    // The checked-out branch carries `is_head` — no more "HEAD" pseudo-ref.
    assert!(head.refs.iter().any(|r| {
        r.is_head && r.kind == RefKind::Local && (r.name == "master" || r.name == "main")
    }));
    assert!(!head.refs.iter().any(|r| r.name == "HEAD"));

    let tagged = g.commits.iter().find(|c| c.oid == c1).unwrap();
    assert!(tagged
        .refs
        .iter()
        .any(|r| r.name == "v1" && r.kind == RefKind::Tag && !r.is_head));
    assert_eq!(tagged.parents, Vec::<git2::Oid>::new());
    assert_eq!(g.commits[0].parents, vec![c1]);
    assert_eq!(
        g.commits[0].short_id,
        &c2.to_string()[..g.commits[0].short_id.len()]
    );
}

#[test]
fn walk_includes_all_local_branches_and_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let base = commit_on(&repo, "a.txt", "base", "base", &[], true);
    let feature = commit_on(&repo, "b.txt", "feat", "feature", &[base], false);
    repo.branch("feature", &repo.find_commit(feature).unwrap(), false)
        .unwrap();
    let main_tip = commit_on(&repo, "c.txt", "main", "main work", &[base], true);
    let merge = commit_on(
        &repo,
        "a.txt",
        "merged",
        "merge feature",
        &[main_tip, feature],
        true,
    );

    let g = graph::load(tmp.path(), 0).unwrap();

    let oids: Vec<git2::Oid> = g.commits.iter().map(|c| c.oid).collect();
    for expected in [base, feature, main_tip, merge] {
        assert!(oids.contains(&expected), "missing commit {expected}");
    }

    let merge_commit = g.commits.iter().find(|c| c.oid == merge).unwrap();
    assert_eq!(merge_commit.parents, vec![main_tip, feature]);

    let feature_commit = g.commits.iter().find(|c| c.oid == feature).unwrap();
    assert!(feature_commit
        .refs
        .iter()
        .any(|r| r.name == "feature" && r.kind == RefKind::Local));

    // Lanes derived from the same display order are coherent: a merge row opens
    // (at least) a second lane, and the assignment never exceeds the commit
    // count in width.
    let topo: Vec<(git2::Oid, Vec<git2::Oid>)> = g
        .commits
        .iter()
        .map(|c| (c.oid, c.parents.clone()))
        .collect();
    let rows = graph::assign_lanes(&topo);
    assert_eq!(rows.len(), g.commits.len());

    let merge_index = g.commits.iter().position(|c| c.oid == merge).unwrap();
    let merge_row = &rows[merge_index];
    let distinct_targets = merge_row
        .edges
        .iter()
        .map(|e| e.to_lane)
        .collect::<std::collections::HashSet<_>>();
    assert!(
        distinct_targets.len() >= 2,
        "merge should branch into at least two lanes, got {:?}",
        merge_row.edges
    );
}

#[test]
fn limit_paginates_without_silent_truncation() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let mut parent: Vec<git2::Oid> = Vec::new();
    for i in 0..5 {
        let oid = commit_on(
            &repo,
            "a.txt",
            &format!("v{i}"),
            &format!("c{i}"),
            &parent,
            true,
        );
        parent = vec![oid];
    }

    let limited = graph::load(tmp.path(), 3).unwrap();
    assert_eq!(limited.commits.len(), 3);
    assert!(limited.has_more, "older commits remain beyond the limit");

    let full = graph::load(tmp.path(), 100).unwrap();
    assert_eq!(full.commits.len(), 5);
    assert!(!full.has_more);
}

#[test]
fn page_extends_until_the_head_commit_when_the_checked_out_branch_is_old() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let mut oids: Vec<git2::Oid> = Vec::new();
    let mut parent: Vec<git2::Oid> = Vec::new();
    for i in 0..5 {
        let oid = commit_on(
            &repo,
            "a.txt",
            &format!("v{i}"),
            &format!("c{i}"),
            &parent,
            true,
        );
        oids.push(oid);
        parent = vec![oid];
    }
    // "old" branch checked out on c1: its commit is beyond a page of 2.
    repo.branch("old", &repo.find_commit(oids[1]).unwrap(), false)
        .unwrap();
    repo.set_head("refs/heads/old").unwrap();

    let g = graph::load(tmp.path(), 2).unwrap();

    // The page extends past the limit up to and including the HEAD commit…
    assert_eq!(summaries(&g), vec!["c4", "c3", "c2", "c1"]);
    let head = g.commits.iter().find(|c| c.oid == oids[1]).unwrap();
    assert!(head.refs.iter().any(|r| r.is_head && r.name == "old"));
    // … and pagination stays explicit: c0 waits behind Load more.
    assert!(g.has_more);
}

#[test]
fn page_extension_down_to_the_root_exhausts_the_walk() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let mut root: Option<git2::Oid> = None;
    let mut parent: Vec<git2::Oid> = Vec::new();
    for i in 0..5 {
        let oid = commit_on(
            &repo,
            "a.txt",
            &format!("v{i}"),
            &format!("c{i}"),
            &parent,
            true,
        );
        root.get_or_insert(oid);
        parent = vec![oid];
    }
    repo.branch("root", &repo.find_commit(root.unwrap()).unwrap(), false)
        .unwrap();
    repo.set_head("refs/heads/root").unwrap();

    let g = graph::load(tmp.path(), 2).unwrap();

    // HEAD at the root: the extension drains the walk, no phantom Load more.
    assert_eq!(g.commits.len(), 5);
    assert!(!g.has_more);
}

#[test]
fn detached_head_outside_any_ref_is_still_walked() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    // Commit outside any ref (neither branch nor tag), then a detached HEAD on it.
    let dangling = commit_on(&repo, "b.txt", "2", "floating", &[c1], false);
    repo.set_head_detached(dangling).unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();

    let head = g
        .commits
        .iter()
        .find(|c| c.oid == dangling)
        .expect("the detached HEAD commit enters the walk");
    assert!(head.refs.iter().any(|r| r.name == "HEAD" && r.is_head));
}

#[test]
fn commit_body_is_loaded_and_trimmed() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let with_body = commit_on(
        &repo,
        "a.txt",
        "1",
        "subject line\n\nApproved-by: Ada\nSecond body line\n",
        &[],
        true,
    );
    let plain = commit_on(&repo, "a.txt", "2", "subject only", &[with_body], true);

    let g = graph::load(tmp.path(), 0).unwrap();

    let detailed = g.commits.iter().find(|c| c.oid == with_body).unwrap();
    assert_eq!(detailed.summary, "subject line");
    assert_eq!(detailed.body, "Approved-by: Ada\nSecond body line");

    let bare = g.commits.iter().find(|c| c.oid == plain).unwrap();
    assert!(bare.body.is_empty());
}

#[test]
fn matching_remote_branch_merges_into_local_decoration() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    let branch = repo.head().unwrap().shorthand().unwrap().to_string();
    repo.reference(
        &format!("refs/remotes/origin/{branch}"),
        c1,
        false,
        "remote tracking",
    )
    .unwrap();
    repo.reference("refs/remotes/origin/other", c1, false, "remote only")
        .unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();
    let head = g.commits.iter().find(|c| c.oid == c1).unwrap();

    // `master` + `origin/master` (same commit) ⇒ a single local entry with `also_remote`.
    let local = head
        .refs
        .iter()
        .find(|r| r.name == branch && r.kind == RefKind::Local)
        .unwrap();
    assert!(local.also_remote);
    assert!(!head
        .refs
        .iter()
        .any(|r| r.name == format!("origin/{branch}")));
    // A remote without a local counterpart stays a Remote entry.
    assert!(head
        .refs
        .iter()
        .any(|r| { r.name == "origin/other" && r.kind == RefKind::Remote && !r.also_remote }));
}

#[test]
fn diverged_local_and_remote_homonyms_carry_counterpart() {
    // `feat` (local, c2) and `origin/feat` (remote, c1) diverge: no merge, but
    // each chip knows the other side exists (`counterpart`) — the menu then
    // offers both deletions (git.md §9).
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    let c2 = commit_on(&repo, "a.txt", "2", "second", &[c1], true);
    repo.branch("feat", &repo.find_commit(c2).unwrap(), false)
        .unwrap();
    repo.reference("refs/remotes/origin/feat", c1, false, "remote tracking")
        .unwrap();
    // The checked-out branch also has a remote homonym elsewhere: git would
    // refuse its local deletion ⇒ the remote chip does not offer it.
    let head_branch = repo.head().unwrap().shorthand().unwrap().to_string();
    repo.reference(
        &format!("refs/remotes/origin/{head_branch}"),
        c1,
        false,
        "remote tracking",
    )
    .unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();
    let top = g.commits.iter().find(|c| c.oid == c2).unwrap();
    let base = g.commits.iter().find(|c| c.oid == c1).unwrap();

    let local = top.refs.iter().find(|r| r.name == "feat").unwrap();
    assert_eq!(
        local.counterpart.as_deref(),
        Some("origin/feat"),
        "the local branch carries the full remote name"
    );
    assert!(!local.also_remote, "diverged ⇒ no merge");
    let remote = base.refs.iter().find(|r| r.name == "origin/feat").unwrap();
    assert_eq!(
        remote.counterpart.as_deref(),
        Some("feat"),
        "the remote carries the local name"
    );

    let head_remote = base
        .refs
        .iter()
        .find(|r| r.name == format!("origin/{head_branch}"))
        .unwrap();
    assert_eq!(
        head_remote.counterpart, None,
        "the checked-out local homonym does not count"
    );
    let head_local = top.refs.iter().find(|r| r.is_head).unwrap();
    assert_eq!(
        head_local.counterpart.as_deref(),
        Some(format!("origin/{head_branch}").as_str()),
        "the checked-out local branch keeps its remote entry"
    );
}

#[test]
fn detached_head_keeps_a_dedicated_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    repo.set_head_detached(c1).unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();
    let head = g.commits.iter().find(|c| c.oid == c1).unwrap();

    assert!(head.refs.iter().any(|r| r.name == "HEAD" && r.is_head));
    // The branch is no longer checked out: no `is_head` on it.
    assert!(!head
        .refs
        .iter()
        .any(|r| r.is_head && r.kind == RefKind::Local && r.name != "HEAD"));
}

#[test]
fn unborn_head_yields_empty_graph() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();

    assert!(g.commits.is_empty());
    assert!(!g.has_more);
}

#[test]
fn stashes_appear_directly_above_their_base_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_identity(&repo);

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    let c2 = commit_on(&repo, "a.txt", "2", "second", &[c1], true);
    fs::write(tmp.path().join("a.txt"), "dirty").unwrap();
    stash::save(&repo, "older stash").unwrap();
    fs::write(tmp.path().join("a.txt"), "dirty again").unwrap();
    stash::save(&repo, "newer stash").unwrap();

    let g = graph::load(tmp.path(), 0).unwrap();

    // git2 prefixes the message: "On <branch>: <message>".
    let position = |suffix: &str| {
        g.commits
            .iter()
            .position(|c| c.summary.ends_with(suffix))
            .unwrap_or_else(|| panic!("stash \"{suffix}\" missing from the graph"))
    };
    let newer = position("newer stash");
    let older = position("older stash");
    let base = g.commits.iter().position(|c| c.oid == c2).unwrap();
    assert_eq!(newer + 1, older, "the most recent stash comes first");
    assert_eq!(older + 1, base, "the stashes sit just above their base");

    let row = &g.commits[newer];
    assert!(row.stash);
    assert_eq!(
        row.parents,
        vec![c2],
        "only the 1st parent (base) is kept — the index/untracked commits are not rows"
    );
    assert!(row.refs.is_empty(), "no chip for a stash");
    assert!(!g.commits[base].stash);
}

#[test]
fn popped_stash_leaves_the_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_identity(&repo);

    commit_on(&repo, "a.txt", "1", "first", &[], true);
    fs::write(tmp.path().join("a.txt"), "dirty").unwrap();
    stash::stash(&repo).unwrap();
    assert!(graph::load(tmp.path(), 0)
        .unwrap()
        .commits
        .iter()
        .any(|c| c.stash));

    stash::pop(&repo).unwrap();

    assert!(!graph::load(tmp.path(), 0)
        .unwrap()
        .commits
        .iter()
        .any(|c| c.stash));
}

#[test]
fn stash_with_base_beyond_the_page_is_omitted() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    set_identity(&repo);

    let c1 = commit_on(&repo, "a.txt", "1", "first", &[], true);
    fs::write(tmp.path().join("a.txt"), "dirty").unwrap();
    stash::save(&repo, "shelved").unwrap();
    let mut parent = c1;
    for i in 0..3 {
        parent = commit_on(
            &repo,
            "a.txt",
            &format!("v{i}"),
            &format!("c{i}"),
            &[parent],
            true,
        );
    }

    // Page of 3: the base (c1) is beyond it ⇒ stash omitted, no dangling lane.
    let limited = graph::load(tmp.path(), 3).unwrap();
    assert!(limited.has_more);
    assert!(!limited.commits.iter().any(|c| c.stash));

    // Full page (Load more) ⇒ the stash appears.
    assert!(graph::load(tmp.path(), 100)
        .unwrap()
        .commits
        .iter()
        .any(|c| c.stash));
}
