//! Domain model for the Pull Requests cockpit (pull-requests.md §4): the
//! source-agnostic shape the GitHub / Bitbucket parsers map onto, plus the two
//! pure helpers the cockpit relies on — role classification ("me"-relative) and
//! dedupe by `(forge, repo, number)`. No I/O here: the runner (PR4) owns the
//! shell-out, the parsers (PR2/PR3) own the JSON.

use crate::git::forge::Forge;

/// Which cloud forge produced a PR. The display string lives in `repo_label`;
/// this is the glyph/source discriminator the list groups and chips key on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForgeKind {
    GitHub,
    Bitbucket,
}

/// "me"-relative role of a PR in the cockpit (pull-requests.md §1). Exclusive:
/// a PR I authored is `Mine` even if I were also a reviewer (no self-review).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrRole {
    Mine,
    ToReview,
}

impl PrRole {
    /// Classify a PR against the cached identity: author wins over reviewer,
    /// `None` when the PR concerns neither role (it is not listed).
    pub fn classify(is_author: bool, is_requested_reviewer: bool) -> Option<PrRole> {
        if is_author {
            Some(PrRole::Mine)
        } else if is_requested_reviewer {
            Some(PrRole::ToReview)
        } else {
            None
        }
    }
}

/// Lifecycle state of an open PR. Merged/closed are out of scope (§1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Draft,
}

/// Aggregate CI status, also the status of a single `CheckRun`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Checks {
    Passing,
    Failing,
    Pending,
    #[default]
    None,
}

/// Aggregate review decision, also a single reviewer's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Review {
    Approved,
    ChangesRequested,
    Pending,
    #[default]
    None,
}

/// A requested reviewer and where they stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    pub name: String,
    pub state: Review,
}

/// One open PR concerning the user, source-agnostic (pull-requests.md §4).
/// `forge_kind` / `repo_label` come from the `Forge` that produced the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub title: String,
    pub role: PrRole,
    pub state: PrState,
    pub author: String,
    pub source_branch: String,
    pub dest_branch: String,
    pub url: String,
    pub updated_at: String,
    pub checks: Checks,
    pub review: Review,
    pub reviewers: Vec<Reviewer>,
    /// Labels on the PR. **GitHub only** — Bitbucket Cloud has no PR-label concept,
    /// so it always maps to an empty vector (pull-requests.md §10).
    pub labels: Vec<String>,
    /// Lines added / removed by the PR, for the list row's ± stats. **GitHub only**:
    /// `gh pr list` returns them as scalars, while Bitbucket would need one extra
    /// `diffstat` request per PR — too costly for a list refresh, so it maps to
    /// `None` and the row omits the column (pull-requests.md §5).
    pub diffstat: Option<(u32, u32)>,
    /// Number of comments on the PR, for the list row. **Bitbucket only**: its list
    /// payload carries `comment_count` for free, whereas `gh pr list --json comments`
    /// would pull every comment *body* of every PR. `None` ⇒ the row omits it (§5).
    pub comment_count: Option<u32>,
}

/// A single comment in a PR's thread. Conversation comments leave `path` and both
/// line anchors empty; **inline** review comments anchor to a diff row, carrying
/// the side they were left on (`old_lineno` for the deleted side, `new_lineno` for
/// the added/context side) so the overlay can place them on the right row.
/// `parent_id` links a reply to the comment it answers (pull-requests.md §11).
/// `context` is the few lines of code the comment was left on (GitHub's `diff_hunk`),
/// shown as a snippet in the center's inline-comments section (pull-requests.md §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrComment {
    pub author: String,
    pub body: String,
    pub path: Option<String>,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub id: Option<u64>,
    pub parent_id: Option<u64>,
    pub context: Option<String>,
    /// ISO-8601 timestamp the comment was posted, rendered as a relative age in the
    /// card (pull-requests.md §11). Empty when the forge payload omits it.
    pub created_at: String,
    /// Whether the review thread this comment belongs to is resolved on the forge
    /// (GitHub `reviewThread.isResolved`; Bitbucket inline `resolution`). Carried on
    /// every comment of the thread; the UI reads the root's value.
    pub resolved: bool,
    /// GitHub review-thread node id (`PRRT_…`) — the handle `resolveReviewThread`
    /// needs. `None` on Bitbucket (resolved by comment id) and conversation comments.
    pub thread_id: Option<String>,
}

/// A single CI check run shown in the detail panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    pub status: Checks,
}

/// One commit in a PR's history (pull-requests.md §5): `sha` is the full hash, `short`
/// the abbreviated form, `subject` the first message line, `author` the display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrCommit {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub author: String,
}

/// Lazily-fetched detail for the selected PR (pull-requests.md §5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrDetail {
    pub body: String,
    pub comments: Vec<PrComment>,
    pub check_runs: Vec<CheckRun>,
    /// Commits in the PR, oldest first (per-commit diff: T5).
    pub commits: Vec<PrCommit>,
    /// ISO-8601 creation timestamp (GitHub `createdAt` / Bitbucket `created_on`),
    /// rendered as a relative "Created … ago" in the detail (pull-requests.md §11).
    pub created_at: String,
}

/// The verdict a submitted review carries (pull-requests.md §11). `Comment` posts
/// the line notes without an approval state; the other two set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewVerdict {
    #[default]
    Comment,
    Approve,
    RequestChanges,
}

/// One drafted line comment ready to post: file path, new-side line, and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftComment {
    pub path: String,
    pub line: u32,
    pub body: String,
}

/// Flatten the draft store into postable line comments — only notes anchored to a
/// line with non-blank text. Files stay in path order, comments in stored order.
pub fn draft_comments(store: &crate::review::FileComments) -> Vec<DraftComment> {
    let mut out = Vec::new();
    for (path, comments) in store {
        for c in comments {
            if let Some(line) = c.line_ref() {
                if !c.note.trim().is_empty() {
                    out.push(DraftComment {
                        path: path.clone(),
                        line,
                        body: c.note.clone(),
                    });
                }
            }
        }
    }
    out
}

/// Group a PR's **inline** comments into the diff overlay's `ForgeThreads`,
/// keyed by file path then anchor line and kept in source (chronological) order.
/// Conversation comments (no `path`/`line`) are excluded — they stay in the rail.
pub fn forge_threads(comments: &[PrComment]) -> crate::review::ForgeThreads {
    let mut threads = crate::review::ForgeThreads::new();
    for c in comments {
        let Some(path) = c.path.as_deref() else {
            continue;
        };
        if c.old_lineno.is_none() && c.new_lineno.is_none() {
            continue;
        }
        threads
            .entry(path.to_owned())
            .or_default()
            .entry((c.old_lineno, c.new_lineno))
            .or_default()
            .push(crate::review::ThreadComment {
                author: c.author.clone(),
                body: c.body.clone(),
                id: c.id,
                created_at: c.created_at.clone(),
                context: c.context.clone(),
                resolved: c.resolved,
                thread_id: c.thread_id.clone(),
            });
    }
    threads
}

/// One line of a code-context snippet shown above a comment (pull-requests.md §5):
/// its old/new line numbers and whether it was added, deleted or unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetLine {
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub kind: SnippetKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetKind {
    Added,
    Deleted,
    Context,
}

/// Parse a unified-diff hunk (GitHub's `diff_hunk`) into the last `max_lines`
/// rows ending at the commented line, each tagged with its line numbers and
/// add/delete/context kind. The `@@ -a,b +c,d @@` header seeds the counters;
/// the snippet keeps the tail so the commented row sits at the bottom.
pub fn hunk_snippet(hunk: &str, max_lines: usize) -> Vec<SnippetLine> {
    let mut out: Vec<SnippetLine> = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in hunk.lines() {
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some((o, n)) = parse_hunk_header(rest) {
                old_no = o;
                new_no = n;
            }
            continue;
        }
        let first = line.chars().next();
        let (kind, old, new) = match first {
            Some('+') => {
                let l = new_no;
                new_no += 1;
                (SnippetKind::Added, None, Some(l))
            }
            Some('-') => {
                let l = old_no;
                old_no += 1;
                (SnippetKind::Deleted, Some(l), None)
            }
            // "\ No newline at end of file" carries no line; skip it.
            Some('\\') => continue,
            _ => {
                let (o, n) = (old_no, new_no);
                old_no += 1;
                new_no += 1;
                (SnippetKind::Context, Some(o), Some(n))
            }
        };
        out.push(SnippetLine {
            old_no: old,
            new_no: new,
            kind,
            text: line.get(1..).unwrap_or("").to_owned(),
        });
    }
    if out.len() > max_lines {
        out.drain(0..out.len() - max_lines);
    }
    out
}

/// Read the start line numbers from a `@@ -a,b +c,d @@` header tail (the text after
/// the leading `@@`): the old-side `a` and new-side `c`.
fn parse_hunk_header(rest: &str) -> Option<(u32, u32)> {
    let (mut old, mut new) = (None, None);
    for tok in rest.split_whitespace() {
        if let Some(s) = tok.strip_prefix('-') {
            old = s.split(',').next().and_then(|n| n.parse().ok());
        } else if let Some(s) = tok.strip_prefix('+') {
            new = s.split(',').next().and_then(|n| n.parse().ok());
        }
    }
    Some((old?, new?))
}

/// Format an ISO-8601 UTC timestamp as a compact relative age ("just now", "5m
/// ago", "23h ago", "2 days ago", "3 mo ago", "2 yr ago") against `now_secs` (Unix
/// epoch seconds). Unparseable input yields an empty string so the caller can hide
/// the line. Used by the review detail's "Created" line.
pub fn relative_age(iso: &str, now_secs: i64) -> String {
    let Some(then) = epoch_secs(iso) else {
        return String::new();
    };
    let d = (now_secs - then).max(0);
    if d < 60 {
        "just now".to_owned()
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86_400 {
        format!("{}h ago", d / 3600)
    } else if d < 2_592_000 {
        let days = d / 86_400;
        if days == 1 {
            "1 day ago".to_owned()
        } else {
            format!("{days} days ago")
        }
    } else if d < 31_536_000 {
        format!("{} mo ago", d / 2_592_000)
    } else {
        let yr = d / 31_536_000;
        if yr == 1 {
            "1 yr ago".to_owned()
        } else {
            format!("{yr} yr ago")
        }
    }
}

/// Parse the leading `YYYY-MM-DDTHH:MM:SS` of an ISO-8601 timestamp into Unix epoch
/// seconds, treating it as UTC (both forges return UTC — GitHub `…Z`, Bitbucket
/// `…+00:00`). Sub-second and offset suffixes are ignored.
fn epoch_secs(iso: &str) -> Option<i64> {
    let year: i64 = iso.get(0..4)?.parse().ok()?;
    let month: i64 = iso.get(5..7)?.parse().ok()?;
    let day: i64 = iso.get(8..10)?.parse().ok()?;
    let hour: i64 = iso.get(11..13)?.parse().ok()?;
    let min: i64 = iso.get(14..16)?.parse().ok()?;
    let sec: i64 = iso.get(17..19)?.parse().ok()?;
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + min * 60 + sec)
}

/// Days since the Unix epoch for a proleptic-Gregorian `(y, m, d)` — Howard
/// Hinnant's `days_from_civil` (public-domain), valid across the dates a forge returns.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The `(forge, repo, number)` identity a PR is deduped on. Two worktrees of one
/// root share a remote, so the same PR can surface from several queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PrKey {
    forge_kind: ForgeKind,
    repo_label: String,
    number: u64,
}

impl PullRequest {
    fn key(&self) -> PrKey {
        PrKey {
            forge_kind: self.forge_kind,
            repo_label: self.repo_label.clone(),
            number: self.number,
        }
    }
}

impl ForgeKind {
    /// Discriminator + `owner/repo` display label for the `Forge` behind a query.
    pub fn of(forge: &Forge) -> (ForgeKind, String) {
        match forge {
            Forge::GitHub { owner, repo } => (ForgeKind::GitHub, format!("{owner}/{repo}")),
            Forge::Bitbucket { workspace, repo } => {
                (ForgeKind::Bitbucket, format!("{workspace}/{repo}"))
            }
        }
    }
}

/// Collapse PRs to one entry per `(forge, repo, number)`, preserving first-seen
/// order; `Mine` wins over `ToReview` if a source returns a PR under both (§1).
pub fn dedupe(prs: Vec<PullRequest>) -> Vec<PullRequest> {
    let mut out: Vec<PullRequest> = Vec::with_capacity(prs.len());
    let mut seen: std::collections::HashMap<PrKey, usize> = std::collections::HashMap::new();
    for pr in prs {
        match seen.get(&pr.key()) {
            Some(&idx) => {
                if out[idx].role == PrRole::ToReview && pr.role == PrRole::Mine {
                    out[idx] = pr;
                }
            }
            None => {
                seen.insert(pr.key(), out.len());
                out.push(pr);
            }
        }
    }
    out
}

/// One row of the stacked-PR layout for a single role group: which PR (`idx` into
/// the `prs` slice), plus the gutter connectors the list draws to the left of it.
/// A PR is *stacked on* another when its `dest_branch` is that other PR's
/// `source_branch` in the same repo (pull-requests.md §5) — the chain renders as an
/// indented tree, base first. A root (or any unstacked PR) has `elbow_last == None`
/// and `verticals` empty, so it draws flush-left exactly like before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackRow {
    pub idx: usize,
    /// One flag per ancestor gutter column (`0..depth-1`): `true` where an ancestor
    /// still has a sibling below, so a `│` runs through this row.
    pub verticals: Vec<bool>,
    /// `Some(is_last)` draws the `├`/`└` elbow at column `verticals.len()`; `None`
    /// for a stack root, drawn flush-left with no elbow.
    pub elbow_last: Option<bool>,
}

impl StackRow {
    /// Indentation level: 0 for a stack root (and any unstacked PR), +1 per ancestor.
    pub fn depth(&self) -> usize {
        self.verticals.len() + usize::from(self.elbow_last.is_some())
    }
}

/// Lay a role group out as stacked trees: a PR whose `dest_branch` equals another
/// listed PR's `source_branch` (same forge + repo) hangs under it. Roots keep their
/// original relative order, each immediately followed by its descendants (pre-order,
/// children in listed order); an unstacked list comes back unchanged. `indices` are
/// positions into `prs` (one role group); the returned rows carry those same `idx`.
pub fn stacked_rows(prs: &[PullRequest], indices: &[usize]) -> Vec<StackRow> {
    let n = indices.len();
    let mut source_of: std::collections::HashMap<(ForgeKind, &str, &str), usize> =
        std::collections::HashMap::with_capacity(n);
    for (pos, &idx) in indices.iter().enumerate() {
        let p = &prs[idx];
        source_of
            .entry((
                p.forge_kind,
                p.repo_label.as_str(),
                p.source_branch.as_str(),
            ))
            .or_insert(pos);
    }
    let parent: Vec<Option<usize>> = indices
        .iter()
        .enumerate()
        .map(|(pos, &idx)| {
            let p = &prs[idx];
            source_of
                .get(&(p.forge_kind, p.repo_label.as_str(), p.dest_branch.as_str()))
                .copied()
                .filter(|&par| par != pos)
        })
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots: Vec<usize> = Vec::new();
    for (pos, &par) in parent.iter().enumerate() {
        match par {
            Some(par) => children[par].push(pos),
            None => roots.push(pos),
        }
    }

    let mut out = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    // Explicit DFS stack: (pos, ancestor verticals, elbow). Children pushed in reverse
    // so siblings still emit in their listed order.
    let mut stack: Vec<(usize, Vec<bool>, Option<bool>)> = roots
        .iter()
        .rev()
        .map(|&root| (root, Vec::new(), None))
        .collect();
    while let Some((pos, verticals, elbow_last)) = stack.pop() {
        if visited[pos] {
            continue;
        }
        visited[pos] = true;
        let kids = &children[pos];
        let mut child_vert = verticals.clone();
        if let Some(last) = elbow_last {
            child_vert.push(!last);
        }
        for (i, &child) in kids.iter().enumerate().rev() {
            stack.push((child, child_vert.clone(), Some(i + 1 == kids.len())));
        }
        out.push(StackRow {
            idx: indices[pos],
            verticals,
            elbow_last,
        });
    }
    // A node reachable only through a cycle never sat under a root; emit it flush so
    // no row is dropped.
    for (pos, &done) in visited.iter().enumerate() {
        if !done {
            out.push(StackRow {
                idx: indices[pos],
                verticals: Vec::new(),
                elbow_last: None,
            });
        }
    }
    out
}

/// How many other listed PRs target this one's source branch — the **Blocks N**
/// flag a list row wears when merging it unblocks a stack (pull-requests.md §5).
/// Same forge + repo pairing as `stacked_rows`; self never counts.
pub fn blocked_count(prs: &[PullRequest], idx: usize) -> usize {
    let Some(base) = prs.get(idx) else {
        return 0;
    };
    prs.iter()
        .enumerate()
        .filter(|&(pos, p)| {
            pos != idx
                && p.forge_kind == base.forge_kind
                && p.repo_label == base.repo_label
                && p.dest_branch == base.source_branch
        })
        .count()
}

/// Which actionability band a PR falls into in the browse list (pull-requests.md
/// §5). The list groups by **what the PR is waiting on**, not by role or date, so
/// the rows the user can act on lead the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    /// I am a requested reviewer and the PR is not blocked back on its author.
    WaitingOnMyReview,
    /// Approved with no failing check — merging is all that is left.
    ReadyToMerge,
    /// The ball is in the author's court: draft, changes requested, or red CI.
    WaitingOnAuthor,
    /// Still moving: reviewers have not ruled yet, or CI is running.
    InReview,
}

impl ActionGroup {
    /// Display order of the list's sections.
    pub const ALL: [ActionGroup; 4] = [
        ActionGroup::WaitingOnMyReview,
        ActionGroup::ReadyToMerge,
        ActionGroup::WaitingOnAuthor,
        ActionGroup::InReview,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ActionGroup::WaitingOnMyReview => "Waiting on your review",
            ActionGroup::ReadyToMerge => "Ready to merge",
            ActionGroup::WaitingOnAuthor => "Waiting on the author",
            ActionGroup::InReview => "In review",
        }
    }

    /// Band a PR belongs to. First match wins, so a PR blocked on its author never
    /// masquerades as reviewable, and a review I still owe outranks an approval
    /// someone else already gave.
    pub fn of(pr: &PullRequest) -> ActionGroup {
        if pr.state == PrState::Draft
            || pr.review == Review::ChangesRequested
            || pr.checks == Checks::Failing
        {
            ActionGroup::WaitingOnAuthor
        } else if pr.role == PrRole::ToReview {
            ActionGroup::WaitingOnMyReview
        } else if pr.review == Review::Approved && pr.checks != Checks::Pending {
            ActionGroup::ReadyToMerge
        } else {
            ActionGroup::InReview
        }
    }
}

/// The browse list's tab bar (pull-requests.md §5). Every fetched PR is open by
/// construction (§1), so the tabs are views over the same cache — no extra query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListTab {
    #[default]
    Open,
    ToReview,
    Mine,
    Drafts,
}

impl ListTab {
    pub const ALL: [ListTab; 4] = [
        ListTab::Open,
        ListTab::ToReview,
        ListTab::Mine,
        ListTab::Drafts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ListTab::Open => "Open",
            ListTab::ToReview => "To review",
            ListTab::Mine => "Mine",
            ListTab::Drafts => "Drafts",
        }
    }

    pub fn accepts(self, pr: &PullRequest) -> bool {
        match self {
            ListTab::Open => true,
            ListTab::ToReview => pr.role == PrRole::ToReview,
            ListTab::Mine => pr.role == PrRole::Mine,
            ListTab::Drafts => pr.state == PrState::Draft,
        }
    }
}

/// Free-text match for the list's search field (pull-requests.md §5), over the
/// fields the header offers to search: title, number, author, branches and repo.
/// Case-insensitive; a blank query matches everything.
pub fn matches_search(pr: &PullRequest, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    let needle = needle.strip_prefix('#').unwrap_or(&needle);
    let haystacks = [
        pr.title.as_str(),
        pr.author.as_str(),
        pr.source_branch.as_str(),
        pr.dest_branch.as_str(),
        pr.repo_label.as_str(),
    ];
    haystacks
        .iter()
        .any(|field| field.to_lowercase().contains(needle))
        || pr.number.to_string().contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_makes_author_mine_even_when_also_reviewer() {
        assert_eq!(PrRole::classify(true, true), Some(PrRole::Mine));
        assert_eq!(PrRole::classify(true, false), Some(PrRole::Mine));
        assert_eq!(PrRole::classify(false, true), Some(PrRole::ToReview));
        assert_eq!(PrRole::classify(false, false), None);
    }

    #[test]
    fn relative_age_buckets_by_elapsed_time() {
        let iso = "2024-06-20T12:00:00Z";
        let base = epoch_secs(iso).unwrap();
        assert_eq!(relative_age(iso, base + 30), "just now");
        assert_eq!(relative_age(iso, base + 5 * 60), "5m ago");
        assert_eq!(relative_age(iso, base + 3 * 3600), "3h ago");
        assert_eq!(relative_age(iso, base + 86_400), "1 day ago");
        assert_eq!(relative_age(iso, base + 5 * 86_400), "5 days ago");
        assert_eq!(relative_age(iso, base + 60 * 86_400), "2 mo ago");
        assert_eq!(relative_age(iso, base + 800 * 86_400), "2 yr ago");
        assert!(relative_age("not-a-date", base).is_empty());
    }

    #[test]
    fn forge_kind_of_yields_discriminator_and_label() {
        let gh = Forge::GitHub {
            owner: "acme".to_owned(),
            repo: "webapp".to_owned(),
        };
        assert_eq!(
            ForgeKind::of(&gh),
            (ForgeKind::GitHub, "acme/webapp".to_owned())
        );
        let bb = Forge::Bitbucket {
            workspace: "team".to_owned(),
            repo: "repo".to_owned(),
        };
        assert_eq!(
            ForgeKind::of(&bb),
            (ForgeKind::Bitbucket, "team/repo".to_owned())
        );
    }

    fn pr(forge_kind: ForgeKind, repo_label: &str, number: u64, role: PrRole) -> PullRequest {
        PullRequest {
            forge_kind,
            repo_label: repo_label.to_owned(),
            number,
            title: format!("PR {number}"),
            role,
            state: PrState::Open,
            author: "someone".to_owned(),
            source_branch: "feature".to_owned(),
            dest_branch: "main".to_owned(),
            url: String::new(),
            updated_at: String::new(),
            checks: Checks::None,
            review: Review::None,
            reviewers: Vec::new(),
            labels: Vec::new(),
            diffstat: None,
            comment_count: None,
        }
    }

    #[test]
    fn dedupe_keeps_first_seen_order_and_collapses_duplicates() {
        let prs = vec![
            pr(ForgeKind::GitHub, "acme/web", 1, PrRole::ToReview),
            pr(ForgeKind::GitHub, "acme/web", 2, PrRole::Mine),
            pr(ForgeKind::GitHub, "acme/web", 1, PrRole::ToReview),
        ];
        let out = dedupe(prs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].number, 1);
        assert_eq!(out[1].number, 2);
    }

    #[test]
    fn a_draft_waits_on_its_author_whatever_its_role() {
        let mut p = pr(ForgeKind::GitHub, "acme/web", 1, PrRole::ToReview);
        p.state = PrState::Draft;
        assert_eq!(ActionGroup::of(&p), ActionGroup::WaitingOnAuthor);
    }

    #[test]
    fn changes_requested_and_red_ci_wait_on_the_author() {
        let mut changes = pr(ForgeKind::GitHub, "acme/web", 1, PrRole::Mine);
        changes.review = Review::ChangesRequested;
        assert_eq!(ActionGroup::of(&changes), ActionGroup::WaitingOnAuthor);

        let mut failing = pr(ForgeKind::GitHub, "acme/web", 2, PrRole::ToReview);
        failing.checks = Checks::Failing;
        assert_eq!(ActionGroup::of(&failing), ActionGroup::WaitingOnAuthor);
    }

    #[test]
    fn a_review_i_owe_outranks_an_approval_someone_else_gave() {
        let mut p = pr(ForgeKind::GitHub, "acme/web", 1, PrRole::ToReview);
        p.review = Review::Approved;
        p.checks = Checks::Passing;
        assert_eq!(ActionGroup::of(&p), ActionGroup::WaitingOnMyReview);
    }

    #[test]
    fn approved_and_green_is_ready_to_merge_but_pending_ci_is_not() {
        let mut ready = pr(ForgeKind::GitHub, "acme/web", 1, PrRole::Mine);
        ready.review = Review::Approved;
        ready.checks = Checks::Passing;
        assert_eq!(ActionGroup::of(&ready), ActionGroup::ReadyToMerge);

        let mut running = ready.clone();
        running.checks = Checks::Pending;
        assert_eq!(ActionGroup::of(&running), ActionGroup::InReview);
    }

    #[test]
    fn my_pr_awaiting_a_verdict_is_in_review() {
        let p = pr(ForgeKind::GitHub, "acme/web", 1, PrRole::Mine);
        assert_eq!(ActionGroup::of(&p), ActionGroup::InReview);
    }

    #[test]
    fn tabs_filter_by_role_and_draft_state() {
        let mine = pr(ForgeKind::GitHub, "acme/web", 1, PrRole::Mine);
        let to_review = pr(ForgeKind::GitHub, "acme/web", 2, PrRole::ToReview);
        let mut draft = pr(ForgeKind::GitHub, "acme/web", 3, PrRole::Mine);
        draft.state = PrState::Draft;

        assert!(ListTab::Open.accepts(&mine) && ListTab::Open.accepts(&to_review));
        assert!(ListTab::Mine.accepts(&mine) && !ListTab::Mine.accepts(&to_review));
        assert!(ListTab::ToReview.accepts(&to_review) && !ListTab::ToReview.accepts(&mine));
        assert!(ListTab::Drafts.accepts(&draft) && !ListTab::Drafts.accepts(&mine));
    }

    #[test]
    fn search_spans_title_author_branches_repo_and_number() {
        let mut p = pr(ForgeKind::GitHub, "acme/web", 1284, PrRole::Mine);
        p.title = "Dedupe webhook deliveries".to_owned();
        p.author = "Thomas Lenoir".to_owned();
        p.source_branch = "feat/webhook-dedupe".to_owned();

        assert!(matches_search(&p, ""));
        assert!(matches_search(&p, "  "));
        assert!(matches_search(&p, "WEBHOOK"));
        assert!(matches_search(&p, "lenoir"));
        assert!(matches_search(&p, "feat/"));
        assert!(matches_search(&p, "acme"));
        assert!(matches_search(&p, "1284"));
        assert!(matches_search(&p, "#1284"));
        assert!(!matches_search(&p, "proration"));
    }

    fn branched(number: u64, source: &str, dest: &str) -> PullRequest {
        let mut p = pr(ForgeKind::GitHub, "acme/web", number, PrRole::Mine);
        p.source_branch = source.to_owned();
        p.dest_branch = dest.to_owned();
        p
    }

    #[test]
    fn stacked_rows_leaves_an_unstacked_list_unchanged() {
        let prs = vec![
            branched(1, "a", "main"),
            branched(2, "b", "main"),
            branched(3, "c", "develop"),
        ];
        let rows = stacked_rows(&prs, &[0, 1, 2]);
        assert_eq!(rows.iter().map(|r| r.idx).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(rows
            .iter()
            .all(|r| r.depth() == 0 && r.elbow_last.is_none()));
    }

    #[test]
    fn stacked_rows_nests_a_chain_base_first_with_growing_depth() {
        // Listed out of order (top, base, mid); dest→source links them.
        let prs = vec![
            branched(3, "c", "b"),
            branched(1, "a", "main"),
            branched(2, "b", "a"),
        ];
        let rows = stacked_rows(&prs, &[0, 1, 2]);
        assert_eq!(rows.iter().map(|r| r.idx).collect::<Vec<_>>(), [1, 2, 0]);
        assert_eq!(
            rows.iter().map(|r| r.depth()).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(rows[0].elbow_last, None);
        assert_eq!(rows[1].elbow_last, Some(true));
        assert_eq!(rows[2].elbow_last, Some(true));
        assert_eq!(rows[2].verticals, vec![false]);
    }

    #[test]
    fn stacked_rows_marks_siblings_and_runs_a_vertical_past_a_non_last_branch() {
        // base ← {A, B}; A ← A1. A is not last, so A1 keeps a vertical in A's column.
        let prs = vec![
            branched(1, "a", "main"),
            branched(2, "b", "a"),
            branched(3, "c", "a"),
            branched(4, "d", "b"),
        ];
        let rows = stacked_rows(&prs, &[0, 1, 2, 3]);
        assert_eq!(rows.iter().map(|r| r.idx).collect::<Vec<_>>(), [0, 1, 3, 2]);
        assert_eq!(rows[1].elbow_last, Some(false)); // A (├, sibling B below)
        assert_eq!(rows[2].verticals, vec![true]); // A1 keeps A's column filled
        assert_eq!(rows[3].elbow_last, Some(true)); // B (└, last)
    }

    #[test]
    fn stacked_rows_does_not_link_across_repos() {
        let mut other = branched(2, "b", "a");
        other.repo_label = "acme/other".to_owned();
        let prs = vec![branched(1, "a", "main"), other];
        let rows = stacked_rows(&prs, &[0, 1]);
        assert!(rows.iter().all(|r| r.depth() == 0));
    }

    #[test]
    fn dedupe_lets_mine_win_over_to_review_regardless_of_order() {
        let review_first = dedupe(vec![
            pr(ForgeKind::GitHub, "acme/web", 7, PrRole::ToReview),
            pr(ForgeKind::GitHub, "acme/web", 7, PrRole::Mine),
        ]);
        assert_eq!(review_first.len(), 1);
        assert_eq!(review_first[0].role, PrRole::Mine);

        let mine_first = dedupe(vec![
            pr(ForgeKind::GitHub, "acme/web", 7, PrRole::Mine),
            pr(ForgeKind::GitHub, "acme/web", 7, PrRole::ToReview),
        ]);
        assert_eq!(mine_first.len(), 1);
        assert_eq!(mine_first[0].role, PrRole::Mine);
    }

    fn comment(author: &str, body: &str, path: Option<&str>, line: Option<u32>) -> PrComment {
        PrComment {
            author: author.to_owned(),
            body: body.to_owned(),
            path: path.map(str::to_owned),
            old_lineno: None,
            new_lineno: line,
            id: None,
            parent_id: None,
            context: None,
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }
    }

    #[test]
    fn hunk_snippet_numbers_lines_and_tags_kinds() {
        let hunk = "@@ -40,3 +40,4 @@ fn check(t: &Token) {\n fn check(t: &Token) {\n-    if t.exp < now {\n+    if t.exp <= now {\n+        bail();";
        let snippet = hunk_snippet(hunk, 10);
        assert_eq!(
            snippet,
            vec![
                SnippetLine {
                    old_no: Some(40),
                    new_no: Some(40),
                    kind: SnippetKind::Context,
                    text: "fn check(t: &Token) {".to_owned(),
                },
                SnippetLine {
                    old_no: Some(41),
                    new_no: None,
                    kind: SnippetKind::Deleted,
                    text: "    if t.exp < now {".to_owned(),
                },
                SnippetLine {
                    old_no: None,
                    new_no: Some(41),
                    kind: SnippetKind::Added,
                    text: "    if t.exp <= now {".to_owned(),
                },
                SnippetLine {
                    old_no: None,
                    new_no: Some(42),
                    kind: SnippetKind::Added,
                    text: "        bail();".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn hunk_snippet_keeps_only_the_tail_ending_at_the_anchor() {
        let hunk = "@@ -1,5 +1,5 @@\n a\n b\n c\n d\n e";
        let snippet = hunk_snippet(hunk, 2);
        assert_eq!(snippet.len(), 2);
        assert_eq!(snippet[0].text, "d");
        assert_eq!(snippet[1].text, "e");
        assert_eq!(snippet[1].new_no, Some(5));
    }

    #[test]
    fn forge_threads_groups_inline_by_file_and_line_skipping_conversation() {
        let comments = vec![
            comment("alice", "general note", None, None),
            comment("bob", "nit", Some("a.rs"), Some(3)),
            comment("carol", "reply", Some("a.rs"), Some(3)),
            comment("dave", "other file", Some("b.rs"), Some(9)),
        ];
        let threads = forge_threads(&comments);
        assert_eq!(threads.len(), 2, "conversation comment is excluded");
        let a_line3 = &threads["a.rs"][&(None, Some(3))];
        assert_eq!(a_line3.len(), 2);
        assert_eq!(a_line3[0].author, "bob");
        assert_eq!(a_line3[1].body, "reply");
        assert_eq!(threads["b.rs"][&(None, Some(9))][0].author, "dave");
    }

    #[test]
    fn dedupe_distinguishes_same_number_across_repos_and_forges() {
        let prs = vec![
            pr(ForgeKind::GitHub, "acme/web", 1, PrRole::Mine),
            pr(ForgeKind::GitHub, "acme/api", 1, PrRole::Mine),
            pr(ForgeKind::Bitbucket, "acme/web", 1, PrRole::Mine),
        ];
        assert_eq!(dedupe(prs).len(), 3);
    }

    #[test]
    fn status_enums_default_to_none() {
        assert_eq!(Checks::default(), Checks::None);
        assert_eq!(Review::default(), Review::None);
        assert_eq!(PrDetail::default(), PrDetail::default());
    }
}
