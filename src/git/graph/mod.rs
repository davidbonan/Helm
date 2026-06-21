use std::collections::{HashMap, HashSet};

mod lanes;
mod model;
mod stashes;

pub use lanes::{assign_lanes, assign_lanes_with_wip, LaneCache};
pub use model::{Edge, Graph, GraphCommit, GraphRef, GraphRow, RefKind, PAGE_SIZE};

pub fn load(repo_path: &std::path::Path, limit: usize) -> Result<Graph, git2::Error> {
    let repo = git2::Repository::open(repo_path)?;
    load_repo(&repo, limit)
}

/// The page **always contains the `HEAD` commit**: a checked-out branch beyond
/// `limit` (workspace on an old branch) ⇒ the walk continues down to it —
/// otherwise locating the current branch (auto-scroll, git.md §9) would find
/// nothing on open. The caller realigns its page size on the actual size
/// received (Load more resumes from it).
pub fn load_repo(repo: &git2::Repository, limit: usize) -> Result<Graph, git2::Error> {
    let limit = if limit == 0 { PAGE_SIZE } else { limit };
    let (decorations, head_oid) = decorations(repo)?;

    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    push_glob(&mut walk, "refs/heads/*")?;
    push_glob(&mut walk, "refs/remotes/*")?;
    push_glob(&mut walk, "refs/tags/*")?;
    // Detached HEAD outside any ref: pushed explicitly, otherwise the walk
    // would never reach it (and the extension would traverse all of history).
    if let Some(oid) = head_oid {
        walk.push(oid)?;
    }

    let mut commits = Vec::new();
    let mut has_more = false;
    let mut head_pending = head_oid.is_some();
    for oid in walk {
        let oid = oid?;
        if commits.len() >= limit && !head_pending {
            has_more = true;
            break;
        }
        if Some(oid) == head_oid {
            head_pending = false;
        }
        let commit = repo.find_commit(oid)?;
        let mut row = commit_row(&commit)?;
        row.refs = decorations.get(&oid).cloned().unwrap_or_default();
        commits.push(row);
    }
    stashes::insert_stashes(repo, &mut commits)?;

    Ok(Graph { commits, has_more })
}

fn commit_row(commit: &git2::Commit) -> Result<GraphCommit, git2::Error> {
    let short_id = commit.as_object().short_id()?;
    Ok(GraphCommit {
        oid: commit.id(),
        short_id: short_id.as_str().unwrap_or_default().to_string(),
        summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
        body: commit
            .body()
            .ok()
            .flatten()
            .unwrap_or("")
            .trim()
            .to_string(),
        author: commit.author().name().unwrap_or("").to_string(),
        time: commit.author().when().seconds(),
        parents: commit.parent_ids().collect(),
        refs: Vec::new(),
        stash: false,
    })
}

/// Push a ref glob, tolerating an absent namespace (e.g. no remotes or tags) so
/// the walk still proceeds with whatever refs exist locally.
fn push_glob(walk: &mut git2::Revwalk, glob: &str) -> Result<(), git2::Error> {
    match walk.push_glob(glob) {
        Ok(()) => Ok(()),
        Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Also returns the oid of the `HEAD` commit (resolved once for both the
/// `is_head` marking **and** the page extension in `load_repo`).
type Decorations = (HashMap<git2::Oid, Vec<GraphRef>>, Option<git2::Oid>);

fn decorations(repo: &git2::Repository) -> Result<Decorations, git2::Error> {
    let mut map: HashMap<git2::Oid, Vec<GraphRef>> = HashMap::new();
    // Repo-wide names for `counterpart`: local branches, and the full remote
    // name per branch behind any `<remote>/` — first remote wins, alphabetical
    // ref order (`origin/HEAD` excluded: a remote symref, not a deletable
    // branch).
    let mut locals: HashSet<String> = HashSet::new();
    let mut remote_names: HashMap<String, String> = HashMap::new();
    // Enumerates which branches are worktree-eligible to decorate the graph; the
    // per-project base only affects destination paths, not this membership.
    let worktree_sources: HashSet<String> =
        crate::git::worktree::available_sources_repo(repo, None)
            .unwrap_or_default()
            .into_iter()
            .map(|source| source.name)
            .collect();

    for reference in repo.references()? {
        let reference = reference?;
        let kind = if reference.is_branch() {
            RefKind::Local
        } else if reference.is_remote() {
            RefKind::Remote
        } else if reference.is_tag() {
            RefKind::Tag
        } else {
            continue;
        };
        let name = match reference.shorthand() {
            Ok(name) => name.to_string(),
            Err(_) => continue,
        };
        match kind {
            RefKind::Local => {
                locals.insert(name.clone());
            }
            RefKind::Remote => {
                if let Some((_, branch)) = name.split_once('/') {
                    if branch != "HEAD" {
                        remote_names
                            .entry(branch.to_string())
                            .or_insert_with(|| name.clone());
                    }
                }
            }
            RefKind::Tag => {}
        }
        // Peel to a commit so annotated tags (which point at a tag object)
        // decorate the commit they reference.
        if let Ok(commit) = reference.peel_to_commit() {
            let worktree_available = worktree_sources.contains(&name);
            map.entry(commit.id()).or_default().push(GraphRef {
                name,
                kind,
                is_head: false,
                also_remote: false,
                counterpart: None,
                worktree_available,
            });
        }
    }

    let mut head_branch: Option<String> = None;
    let mut head_oid = None;
    if let Ok(head) = repo.head() {
        if let Ok(commit) = head.peel_to_commit() {
            head_oid = Some(commit.id());
            let refs = map.entry(commit.id()).or_default();
            head_branch = head
                .is_branch()
                .then(|| head.shorthand().ok().map(str::to_string))
                .flatten();
            match head_branch.as_deref() {
                Some(name) => {
                    if let Some(local) = refs
                        .iter_mut()
                        .find(|r| r.kind == RefKind::Local && r.name == name)
                    {
                        local.is_head = true;
                    }
                }
                // Detached HEAD: no branch to mark ⇒ dedicated entry.
                None => refs.push(GraphRef {
                    name: "HEAD".to_string(),
                    kind: RefKind::Local,
                    is_head: true,
                    also_remote: false,
                    counterpart: None,
                    worktree_available: false,
                }),
            }
        }
    }

    for refs in map.values_mut() {
        annotate_counterparts(refs, &locals, &remote_names, head_branch.as_deref());
        merge_local_remote(refs);
        sort_refs(refs);
    }

    Ok((map, head_oid))
}

/// Fills `counterpart`: the **name** of the same-named branch on the other side
/// (local ⇄ remote) when it exists, even on a different commit — the chip menu
/// then names the deletions on both sides (git.md §9). A **checked-out** local
/// homonym does not count for a remote ref: git refuses to delete it, the entry
/// would be dead.
fn annotate_counterparts(
    refs: &mut [GraphRef],
    locals: &HashSet<String>,
    remote_names: &HashMap<String, String>,
    head_branch: Option<&str>,
) {
    for r in refs.iter_mut() {
        r.counterpart = match r.kind {
            RefKind::Local => remote_names.get(&r.name).cloned(),
            RefKind::Remote => r.name.split_once('/').and_then(|(_, branch)| {
                (locals.contains(branch) && head_branch != Some(branch)).then(|| branch.to_string())
            }),
            RefKind::Tag => None,
        };
    }
}

/// Merges `x` + `<remote>/x` pointing at the **same commit**: the local entry
/// gets `also_remote`, the remote entry disappears. A remote without a local
/// counterpart stays as-is.
fn merge_local_remote(refs: &mut Vec<GraphRef>) {
    let locals: Vec<String> = refs
        .iter()
        .filter(|r| r.kind == RefKind::Local)
        .map(|r| r.name.clone())
        .collect();
    let mut merged: Vec<String> = Vec::new();
    refs.retain(|r| {
        let branch = match (r.kind, r.name.split_once('/')) {
            (RefKind::Remote, Some((_, branch))) => branch,
            _ => return true,
        };
        if locals.iter().any(|l| l == branch) {
            merged.push(branch.to_string());
            false
        } else {
            true
        }
    });
    for r in refs.iter_mut() {
        if r.kind == RefKind::Local && merged.iter().any(|m| m == &r.name) {
            r.also_remote = true;
        }
    }
}

/// Stable sort of a commit's decorations: checked-out first, then locals,
/// remotes, tags — and by name within an equal kind (deterministic rendering).
fn sort_refs(refs: &mut [GraphRef]) {
    refs.sort_by(|a, b| {
        (!a.is_head, a.kind, a.name.as_str()).cmp(&(!b.is_head, b.kind, b.name.as_str()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_ref(name: &str, kind: RefKind) -> GraphRef {
        GraphRef {
            name: name.to_string(),
            kind,
            is_head: false,
            also_remote: false,
            counterpart: None,
            worktree_available: false,
        }
    }

    #[test]
    fn annotate_counterparts_names_the_other_side() {
        // `feat` (local) and `origin/feat` (remote, different commit): each
        // carries the other's **name** — the menu names both deletions.
        let mut refs = vec![
            graph_ref("feat", RefKind::Local),
            graph_ref("origin/feat", RefKind::Remote),
            graph_ref("origin/other", RefKind::Remote),
            graph_ref("v1", RefKind::Tag),
        ];
        let locals = HashSet::from(["feat".to_string()]);
        let remotes = HashMap::from([("feat".to_string(), "origin/feat".to_string())]);
        annotate_counterparts(&mut refs, &locals, &remotes, Some("main"));

        assert_eq!(refs[0].counterpart.as_deref(), Some("origin/feat"));
        assert_eq!(refs[1].counterpart.as_deref(), Some("feat"));
        assert_eq!(refs[2].counterpart, None, "remote with no same-named local");
        assert_eq!(refs[3].counterpart, None, "never on a tag");
    }

    #[test]
    fn annotate_counterparts_skips_the_checked_out_local() {
        // `origin/main` when `main` is checked-out: git refuses to delete the
        // current branch, the entry would be dead.
        let mut refs = vec![graph_ref("origin/main", RefKind::Remote)];
        let locals = HashSet::from(["main".to_string()]);
        annotate_counterparts(&mut refs, &locals, &HashMap::new(), Some("main"));

        assert_eq!(refs[0].counterpart, None);
    }

    #[test]
    fn merge_local_remote_folds_matching_remote_into_local() {
        let mut refs = vec![
            graph_ref("origin/feature", RefKind::Remote),
            graph_ref("feature", RefKind::Local),
        ];
        merge_local_remote(&mut refs);

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "feature");
        assert_eq!(refs[0].kind, RefKind::Local);
        assert!(refs[0].also_remote);
    }

    #[test]
    fn merge_local_remote_keeps_unmatched_remote() {
        let mut refs = vec![
            graph_ref("origin/other", RefKind::Remote),
            graph_ref("feature", RefKind::Local),
        ];
        merge_local_remote(&mut refs);

        assert_eq!(refs.len(), 2);
        assert!(!refs.iter().any(|r| r.also_remote));
    }

    #[test]
    fn sort_refs_orders_head_then_kind_then_name() {
        let mut refs = vec![
            graph_ref("v1", RefKind::Tag),
            graph_ref("origin/zeta", RefKind::Remote),
            graph_ref("beta", RefKind::Local),
            GraphRef {
                is_head: true,
                ..graph_ref("main", RefKind::Local)
            },
        ];
        sort_refs(&mut refs);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["main", "beta", "origin/zeta", "v1"]);
    }
}
