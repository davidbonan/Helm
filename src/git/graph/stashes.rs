use super::{commit_row, GraphCommit};

/// Inserts each stash (reflog `refs/stash`, most recent first) as a graph row,
/// just **above its base commit** (D-2026-06-03-graph-stash-rows).
///
/// Contract: the stash row is a **display-only construct** — `refs/stash` is
/// never pushed to the walk, the row is synthesized here after it. Only the
/// 1st parent (the base commit) is kept, as a synthetic display link the lane
/// assignment then dashes (`LaneCache::rows`): the stash's index/untracked
/// parent commits are not in the walk, and keeping them would open lanes
/// awaiting a commit that never arrives. Base beyond the loaded page ⇒ stash
/// omitted (it appears after **Load more**).
pub(super) fn insert_stashes(
    repo: &git2::Repository,
    commits: &mut Vec<GraphCommit>,
) -> Result<(), git2::Error> {
    let log = match repo.reflog("refs/stash") {
        Ok(log) => log,
        Err(err) if err.code() == git2::ErrorCode::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in log.iter() {
        let commit = repo.find_commit(entry.id_new())?;
        let Some(base) = commit.parent_ids().next() else {
            continue;
        };
        let Some(position) = commits.iter().position(|c| c.oid == base) else {
            continue;
        };
        let mut row = commit_row(&commit)?;
        row.parents.truncate(1);
        row.stash = true;
        commits.insert(position, row);
    }
    Ok(())
}
