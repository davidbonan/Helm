/// Kind of a typed decoration (M10-4) — drives the chip glyph in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

/// Typed decoration pointing at a commit. The checked-out branch carries
/// `is_head` (no synthetic "HEAD" entry — except a dedicated one when HEAD is
/// detached); a local branch matching `<remote>/<name>` on the same commit is
/// merged into one entry with `also_remote`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRef {
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
    pub also_remote: bool,
    /// Name of the same-named branch **on the other side** when it exists, even
    /// on a different commit: local ref ⇒ full remote name (`origin/<name>`),
    /// remote ref ⇒ local name — the chip menu then names and offers the
    /// deletions on both sides (git.md §9). For a remote ref, a checked-out
    /// local homonym does not count: git refuses to delete it.
    pub counterpart: Option<String>,
    /// Whether this ref can create a linked worktree right now. Computed on the
    /// git worker with the same source filter as the creation modal; the UI only
    /// hides/shows the menu entry and execution revalidates before writing.
    pub worktree_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub oid: git2::Oid,
    pub short_id: String,
    pub summary: String,
    /// Message body (after the summary line), shown dimmed following the
    /// summary (M10-6). Empty if the message fits on a single line.
    pub body: String,
    pub author: String,
    /// Author time in seconds since the Unix epoch (UTC).
    pub time: i64,
    pub parents: Vec<git2::Oid>,
    /// Decorations pointing at this commit: branches and tags, typed and sorted
    /// (checked-out first, then locals, remotes, tags).
    pub refs: Vec<GraphRef>,
    /// Stash row (reflog `refs/stash`, D-2026-06-03-graph-stash-rows): inserted
    /// just above its base commit, dashed archive node.
    pub stash: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Graph {
    pub commits: Vec<GraphCommit>,
    /// `true` when the walk stopped at the limit and older commits remain
    /// (pagination — never a silent truncation).
    pub has_more: bool,
}

impl Graph {
    /// Effective walk page size: commits excluding stash rows (inserted
    /// afterwards, never counted in `limit`). The caller realigns its
    /// pagination on this when the walk extended all the way to HEAD.
    pub fn page_len(&self) -> usize {
        self.commits.iter().filter(|c| !c.stash).count()
    }
}

/// One edge segment leaving a row downward, from a lane on this row to a lane on
/// the next row, so the renderer can draw lane continuity and merges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from_lane: usize,
    pub to_lane: usize,
    /// Segment of the WIP → HEAD link (dirty tree): drawn **dashed**. `true`
    /// while the lane is exclusive to the link; only a merge link (2nd+ parent
    /// = HEAD) can still join it, the lane then carries a real branch and
    /// becomes solid.
    pub dashed: bool,
    /// Link to a 2nd+ parent (merge): the renderer bends it at the merge row
    /// (horizontal out of the node, then down the joined lane) — a first-parent
    /// transition bends at the other end, into its parent.
    pub merge: bool,
}

/// Lane assignment for a single commit row: the lane its node sits on plus the
/// edges descending toward the next row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub lane: usize,
    pub edges: Vec<Edge>,
}

/// Page size for the commit walk. A `limit` of `0` means "first page"; the UI
/// grows it by this step on **Load more** (pagination, no silent truncation —
/// git.md §9, M9-8).
pub const PAGE_SIZE: usize = 200;
