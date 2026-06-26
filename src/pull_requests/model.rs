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
}

/// A single comment in a PR's thread. Conversation comments leave `path` and both
/// line anchors empty; **inline** review comments anchor to a diff row, carrying
/// the side they were left on (`old_lineno` for the deleted side, `new_lineno` for
/// the added/context side) so the overlay can place them on the right row.
/// `parent_id` links a reply to the comment it answers (pull-requests.md §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrComment {
    pub author: String,
    pub body: String,
    pub path: Option<String>,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub id: Option<u64>,
    pub parent_id: Option<u64>,
}

/// A single CI check run shown in the detail panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    pub status: Checks,
}

/// Lazily-fetched detail for the selected PR (pull-requests.md §5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrDetail {
    pub body: String,
    pub comments: Vec<PrComment>,
    pub check_runs: Vec<CheckRun>,
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
            });
    }
    threads
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
        }
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
