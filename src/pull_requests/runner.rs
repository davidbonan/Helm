//! One-shot PR fetch runner (pull-requests.md §6, architecture §3 runner
//! contract): a detached thread per request, gated by `in_flight`, one reply
//! drained each frame with a repaint. It resolves the workspace repos into
//! `Forge`s, fans the per-forge queries (`gh` for GitHub, `curl` for Bitbucket
//! Cloud), classifies roles against the per-session identity, and returns the
//! deduped `Vec<PullRequest>` plus a per-source status.
//!
//! The command/URL *construction* is the pure `plan` below — unit-tested without
//! the network; the thread merely runs the plan and parses the replies.

use std::path::{Path, PathBuf};
use std::process::Command;

use crossbeam_channel::{Receiver, Sender};

use crate::git::forge::{parse_remote, Forge};
use crate::pull_requests::model::{
    dedupe, DraftComment, ForgeKind, PrRole, PullRequest, ReviewVerdict,
};
use crate::pull_requests::{bitbucket, creds, github};

/// Usability of one source for the cockpit's inline hints (spec §3/§5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SourceStatus {
    /// No repository of this forge in the workspace — nothing to show, no hint.
    #[default]
    Absent,
    /// Queried successfully.
    Ok,
    /// Unusable; carries the one-line hint to surface.
    Unavailable(String),
}

/// One external query the runner runs for a forge, captured so command/URL
/// construction is testable without the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrQuery {
    /// A `gh` invocation (program is `gh`); roles come from the cached login.
    Gh {
        repo_label: String,
        args: Vec<String>,
    },
    /// A Bitbucket REST GET; the Basic-auth header is added at execution. The
    /// `role` is the query's filter (author/reviewer), assigned to every PR it
    /// returns since the list reply carries no reviewer roster to re-derive it.
    Bitbucket {
        repo_label: String,
        url: String,
        role: PrRole,
    },
}

/// What the UI thread asks the runner to fetch.
#[derive(Debug, Clone)]
pub struct PrRequest {
    /// Distinct workspace project roots; the worker resolves each `origin`.
    pub roots: Vec<PathBuf>,
    /// Bitbucket account email (`Prefs`); empty ⇒ the Bitbucket source is off.
    pub bitbucket_email: String,
}

/// The single reply per request.
#[derive(Debug, Clone)]
pub struct PrReply {
    pub github: SourceStatus,
    pub bitbucket: SourceStatus,
    /// Rows fetched per source: `Some` replaces the cache's rows for that forge;
    /// `None` means the source failed transiently, so the cache keeps its
    /// last-good rows and flags the view stale (pull-requests.md §6).
    pub github_rows: Option<Vec<PullRequest>>,
    pub bitbucket_rows: Option<Vec<PullRequest>>,
    /// Identity resolved this run (cached by the runner for the next request).
    pub github_login: Option<String>,
    pub bitbucket_uuid: Option<String>,
}

/// The cockpit's view of the last fetch: the deduped PRs and each source's
/// usability, refreshed in place on every reply.
#[derive(Debug, Clone, Default)]
pub struct PrCache {
    pub pull_requests: Vec<PullRequest>,
    pub github: SourceStatus,
    pub bitbucket: SourceStatus,
    /// `false` until the first reply lands — drives the cold-start fetch on entry.
    pub loaded: bool,
    /// At least one source served cached rows on the last refresh because its
    /// query failed transiently (pull-requests.md §6) — drives the "stale" hint.
    pub stale: bool,
}

impl PrCache {
    /// Fold a reply in, keeping a source's prior rows when it didn't answer so a
    /// transient failure never blanks the list (pull-requests.md §6).
    pub fn apply(&mut self, reply: PrReply) {
        let stale = reply.github_rows.is_none() || reply.bitbucket_rows.is_none();
        let mut rows = reply
            .github_rows
            .unwrap_or_else(|| self.rows_of(ForgeKind::GitHub));
        rows.extend(
            reply
                .bitbucket_rows
                .unwrap_or_else(|| self.rows_of(ForgeKind::Bitbucket)),
        );
        self.pull_requests = dedupe(rows);
        self.github = reply.github;
        self.bitbucket = reply.bitbucket;
        self.loaded = true;
        self.stale = stale;
    }

    fn rows_of(&self, kind: ForgeKind) -> Vec<PullRequest> {
        self.pull_requests
            .iter()
            .filter(|pr| pr.forge_kind == kind)
            .cloned()
            .collect()
    }
}

/// The per-forge list queries, in sidebar order. Both forges need two searches
/// (authored + review-requested) because neither list reply lets the other role
/// be re-derived; Bitbucket needs the resolved account uuid, so its queries are
/// emitted only once `bitbucket_uuid` is known. Pure — the worker runs whatever
/// this returns.
pub fn plan(forges: &[(Forge, String)], bitbucket_uuid: Option<&str>) -> Vec<PrQuery> {
    let mut queries = Vec::new();
    for (forge, label) in forges {
        match forge {
            Forge::GitHub { .. } => {
                queries.push(PrQuery::Gh {
                    repo_label: label.clone(),
                    args: github::list_authored_args(label),
                });
                queries.push(PrQuery::Gh {
                    repo_label: label.clone(),
                    args: github::list_review_requested_args(label),
                });
            }
            Forge::Bitbucket { workspace, repo } => {
                if let Some(uuid) = bitbucket_uuid {
                    queries.push(PrQuery::Bitbucket {
                        repo_label: label.clone(),
                        url: bitbucket::authored_url(workspace, repo, uuid),
                        role: PrRole::Mine,
                    });
                    queries.push(PrQuery::Bitbucket {
                        repo_label: label.clone(),
                        url: bitbucket::reviewing_url(workspace, repo, uuid),
                        role: PrRole::ToReview,
                    });
                }
            }
        }
    }
    queries
}

/// Resolve project roots to `(Forge, repo_label)`, deduped by `Forge` so several
/// worktrees of one remote are queried once (spec §1).
pub fn forges_of_roots(roots: &[PathBuf]) -> Vec<(Forge, String)> {
    let mut out: Vec<(Forge, String)> = Vec::new();
    for root in roots {
        let Some(pair) = forge_of_root(root) else {
            continue;
        };
        if !out.iter().any(|(forge, _)| *forge == pair.0) {
            out.push(pair);
        }
    }
    out
}

fn forge_of_root(root: &Path) -> Option<(Forge, String)> {
    let repo = git2::Repository::open(root).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    let forge = parse_remote(remote.url().ok()?)?;
    let (_, label) = ForgeKind::of(&forge);
    Some((forge, label))
}

/// `(forge_kind, repo_label)` of a project root's `origin`, or `None` when it has
/// no recognized cloud remote — the reverse map a PR uses to find its repo (§7).
pub fn forge_kind_of_root(root: &Path) -> Option<(ForgeKind, String)> {
    let (forge, label) = forge_of_root(root)?;
    Some((ForgeKind::of(&forge).0, label))
}

/// The workspace project root whose `origin` forge owns this PR (§7), out of the
/// precomputed `(root, forge_kind, repo_label)` of each project. `None` ⇒ no
/// workspace repo matches and Checkout has nothing to fetch into.
pub fn match_pr_root(
    roots: &[(PathBuf, ForgeKind, String)],
    forge_kind: ForgeKind,
    repo_label: &str,
) -> Option<PathBuf> {
    roots
        .iter()
        .find(|(_, kind, label)| *kind == forge_kind && label == repo_label)
        .map(|(root, _, _)| root.clone())
}

/// Index of the workspace row already sitting on `source_branch` within the
/// matched project (§7 "already checked out"): `rows` are
/// `(index, project_root, branch_label)` for every workspace repo, and only rows
/// of `project_root` count — the first match wins, else `None` ⇒ a fetch+create.
pub fn matching_worktree(
    rows: &[(usize, PathBuf, String)],
    project_root: &Path,
    source_branch: &str,
) -> Option<usize> {
    rows.iter()
        .find(|(_, root, branch)| root == project_root && branch == source_branch)
        .map(|(index, _, _)| *index)
}

/// The `git fetch origin <refspec>` argument that brings a PR's source branch up
/// as a **local** branch (§7) so `CreateRunner` can put a worktree on it. GitHub
/// fetches the PR head (forks included) into a same-named local branch; Bitbucket
/// fetches `origin`'s source branch into a same-named local one. A bare remote
/// ref (`<source>`) would land only `refs/remotes/origin/<source>`, which
/// `CreateSource::Existing(<source>)` can't resolve — hence the `<dst>` half.
pub fn fetch_refspec(forge_kind: ForgeKind, number: u64, source_branch: &str) -> String {
    match forge_kind {
        ForgeKind::GitHub => format!("pull/{number}/head:{source_branch}"),
        ForgeKind::Bitbucket => format!("{source_branch}:{source_branch}"),
    }
}

/// What the UI asks the checkout runner to make available before creating a
/// worktree (§7): a PR's source branch, local in `root`'s repo.
#[derive(Debug, Clone)]
pub struct CheckoutRequest {
    pub root: PathBuf,
    pub forge_kind: ForgeKind,
    pub number: u64,
    pub source_branch: String,
}

/// The single reply: `Ok` once the branch is local, else a one-line error.
#[derive(Debug, Clone)]
pub struct CheckoutReply {
    pub request: CheckoutRequest,
    pub result: Result<(), String>,
}

/// Detached one-shot fetch for Checkout (§7, architecture §3 runner contract):
/// ensures the PR source branch is local — reusing an existing same-name branch,
/// else fetching it from `origin` — so the worktree create can follow. Network
/// I/O, off the UI thread; one in-flight at a time.
pub struct CheckoutRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<CheckoutReply>,
    results_rx: Receiver<CheckoutReply>,
    in_flight: bool,
}

impl CheckoutRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: std::sync::Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: false,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight
    }

    /// Spawn the detached fetch; `false` when one is already running.
    pub fn request(&mut self, request: CheckoutRequest) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = ensure_branch_local(&request);
            let _ = tx.send(CheckoutReply { request, result });
            on_event();
        });
        true
    }

    pub fn try_recv(&mut self) -> Option<CheckoutReply> {
        let reply = self.results_rx.try_recv().ok()?;
        self.in_flight = false;
        Some(reply)
    }
}

/// Reuse an existing same-name local branch, else `git fetch origin <refspec>` to
/// create it. The reuse path also avoids a non-fast-forward fetch onto a branch
/// the user already has locally (§7 "existing same-name local branch is reused").
fn ensure_branch_local(request: &CheckoutRequest) -> Result<(), String> {
    // A ref name beginning with '-' must never reach the CLI as a flag.
    if request.source_branch.starts_with('-') {
        return Err(format!("invalid branch name '{}'", request.source_branch));
    }
    let repo = git2::Repository::open(&request.root).map_err(|err| err.message().to_owned())?;
    if repo
        .find_branch(&request.source_branch, git2::BranchType::Local)
        .is_ok()
    {
        return Ok(());
    }
    let refspec = fetch_refspec(request.forge_kind, request.number, &request.source_branch);
    match crate::git::cli::run(&request.root, &["fetch", "origin", &refspec]) {
        Ok(out) if out.success() => Ok(()),
        Ok(out) => Err(out.stderr.trim().to_owned()),
        Err(err) => Err(format!("{err:?}")),
    }
}

/// Identity a review reply is matched against so the UI adopts only the data for
/// the PR still selected (the runner is fire-and-forget, several may be in flight).
/// `Hash` so it can also key the per-PR review cache (pull-requests.md §11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrReviewKey {
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
}

/// Ask the review runner to resolve a PR's `merge-base(dest, head)..head` and list
/// its changed files (pull-requests.md §5). Network: fetches the PR head and the
/// base branch into `FETCH_HEAD` — **no local branch is written** (read-only).
#[derive(Debug, Clone)]
pub struct PrFilesRequest {
    pub key: PrReviewKey,
    pub root: PathBuf,
    pub forge_kind: ForgeKind,
    pub number: u64,
    pub source_branch: String,
    pub dest_branch: String,
}

/// The resolved review base/head and the changed-files list.
#[derive(Debug, Clone)]
pub struct PrFilesLoaded {
    pub base: git2::Oid,
    pub head: git2::Oid,
    pub files: Vec<crate::git::commit_detail::CommitFile>,
}

/// A review fetch: the changed-files list (network) or one file's diff (local).
#[derive(Debug, Clone)]
pub enum PrReviewRequest {
    Files(PrFilesRequest),
    FileDiff {
        key: PrReviewKey,
        root: PathBuf,
        base: git2::Oid,
        head: git2::Oid,
        path: String,
    },
}

/// The single reply per review request, echoing the key (and path) for adoption.
pub enum PrReviewReply {
    Files {
        key: PrReviewKey,
        result: Result<PrFilesLoaded, String>,
    },
    FileDiff {
        key: PrReviewKey,
        path: String,
        result: Result<crate::git::diff::FileDiff, String>,
    },
}

/// The bare `git fetch origin <ref>` that lands a PR's head in `FETCH_HEAD`
/// without writing a local branch (the read-only review path, vs `fetch_refspec`
/// which Checkout uses to materialize a branch). GitHub fetches the PR head ref
/// (forks included); Bitbucket the source branch on `origin`.
pub fn review_head_ref(forge_kind: ForgeKind, number: u64, source_branch: &str) -> String {
    match forge_kind {
        ForgeKind::GitHub => format!("pull/{number}/head"),
        ForgeKind::Bitbucket => source_branch.to_owned(),
    }
}

/// Off-thread PR review fetch (pull-requests.md §5, architecture §3 runner
/// contract). Fire-and-forget — unlike the gated list/checkout runners, several
/// requests may be in flight (the file-list load then per-file diffs), each
/// adopted by its echoed `PrReviewKey`/path.
pub struct PrReviewRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<PrReviewReply>,
    results_rx: Receiver<PrReviewReply>,
}

impl PrReviewRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: std::sync::Arc::new(on_event),
            results_tx,
            results_rx,
        }
    }

    pub fn request(&self, request: PrReviewRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let reply = run_review(request);
            let _ = tx.send(reply);
            on_event();
        });
    }

    pub fn try_recv(&self) -> Option<PrReviewReply> {
        self.results_rx.try_recv().ok()
    }
}

fn run_review(request: PrReviewRequest) -> PrReviewReply {
    match request {
        PrReviewRequest::Files(req) => PrReviewReply::Files {
            key: req.key.clone(),
            result: load_pr_files(&req),
        },
        PrReviewRequest::FileDiff {
            key,
            root,
            base,
            head,
            path,
        } => {
            let result = git2::Repository::open(&root)
                .map_err(|e| e.message().to_owned())
                .and_then(|repo| {
                    crate::git::diff::pr_file_diff(&repo, base, head, &path)
                        .map_err(|e| e.message().to_owned())
                });
            PrReviewReply::FileDiff { key, path, result }
        }
    }
}

fn load_pr_files(req: &PrFilesRequest) -> Result<PrFilesLoaded, String> {
    // A ref name beginning with '-' must never reach the CLI as a flag.
    if req.source_branch.starts_with('-') || req.dest_branch.starts_with('-') {
        return Err("invalid branch name".to_owned());
    }
    let head_ref = review_head_ref(req.forge_kind, req.number, &req.source_branch);
    fetch_origin(&req.root, &head_ref)?;
    let repo = git2::Repository::open(&req.root).map_err(|e| e.message().to_owned())?;
    let head = revparse_commit(&repo, "FETCH_HEAD")
        .ok_or_else(|| format!("cannot resolve PR head '{head_ref}'"))?;

    // The base branch tip: prefer a fresh fetch into FETCH_HEAD, else the
    // remote-tracking ref or a same-name local branch already present.
    let dest_fetched = fetch_origin(&req.root, &req.dest_branch).is_ok();
    let dest = dest_fetched
        .then(|| revparse_commit(&repo, "FETCH_HEAD"))
        .flatten()
        .or_else(|| revparse_commit(&repo, &format!("origin/{}", req.dest_branch)))
        .or_else(|| revparse_commit(&repo, &req.dest_branch))
        .ok_or_else(|| format!("cannot resolve base branch '{}'", req.dest_branch))?;

    // Three-dot base: the merge-base, so the diff excludes commits already on the
    // destination. Unrelated histories ⇒ fall back to the destination tip.
    let base = repo.merge_base(dest, head).unwrap_or(dest);
    let files = crate::git::diff::pr_changed_files(&repo, base, head)
        .map_err(|e| e.message().to_owned())?;
    Ok(PrFilesLoaded { base, head, files })
}

fn fetch_origin(root: &Path, refspec: &str) -> Result<(), String> {
    match crate::git::cli::run(root, &["fetch", "origin", refspec]) {
        Ok(out) if out.success() => Ok(()),
        Ok(out) => Err(out.stderr.trim().to_owned()),
        Err(err) => Err(format!("{err:?}")),
    }
}

fn revparse_commit(repo: &git2::Repository, name: &str) -> Option<git2::Oid> {
    repo.revparse_single(name)
        .ok()?
        .peel_to_commit()
        .ok()
        .map(|c| c.id())
}

/// Ask the detail runner for one PR's body / comments / check runs (the forge
/// API data the cockpit lazily loads on selection, pull-requests.md §5).
#[derive(Debug, Clone)]
pub struct PrDetailRequest {
    pub key: PrReviewKey,
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub bitbucket_email: String,
}

/// The single reply per detail request, echoing the key for adoption.
#[derive(Debug, Clone)]
pub struct PrDetailReply {
    pub key: PrReviewKey,
    pub result: Result<crate::pull_requests::model::PrDetail, String>,
}

/// Off-thread per-PR detail fetch (`gh pr view` / Bitbucket REST). Fire-and-forget
/// like `PrReviewRunner` — replies are adopted by their echoed `PrReviewKey`, so a
/// fast switch between PRs never drops the new selection's fetch.
pub struct PrDetailRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<PrDetailReply>,
    results_rx: Receiver<PrDetailReply>,
}

impl PrDetailRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: std::sync::Arc::new(on_event),
            results_tx,
            results_rx,
        }
    }

    pub fn request(&self, request: PrDetailRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = fetch_detail(&request);
            let _ = tx.send(PrDetailReply {
                key: request.key,
                result,
            });
            on_event();
        });
    }

    pub fn try_recv(&self) -> Option<PrDetailReply> {
        self.results_rx.try_recv().ok()
    }
}

fn fetch_detail(req: &PrDetailRequest) -> Result<crate::pull_requests::model::PrDetail, String> {
    match req.forge_kind {
        ForgeKind::GitHub => {
            if !command_ok("gh", &github::auth_status_args()) {
                return Err("Install gh and run `gh auth login`".to_owned());
            }
            let json = run_stdout("gh", &github::view_args(&req.repo_label, req.number))
                .ok_or_else(|| format!("gh pr view {} failed", req.number))?;
            let mut detail = github::parse_detail(&json).map_err(|e| e.to_string())?;
            // Inline review comments are a separate REST resource; missing them is
            // non-fatal (the conversation + diff still render).
            if let Some(inline) = run_stdout(
                "gh",
                &github::review_comments_args(&req.repo_label, req.number),
            )
            .and_then(|j| github::parse_review_comments(&j).ok())
            {
                detail.comments.extend(inline);
            }
            Ok(detail)
        }
        ForgeKind::Bitbucket => {
            let (workspace, repo) = req
                .repo_label
                .split_once('/')
                .ok_or_else(|| format!("malformed repo label '{}'", req.repo_label))?;
            let email = &req.bitbucket_email;
            let token = (!email.is_empty())
                .then(|| creds::read_token(email))
                .flatten()
                .ok_or_else(|| "Set a Bitbucket email and token in Preferences".to_owned())?;
            let header = bitbucket::basic_auth_header(email, &token);
            let detail_json = curl_body(
                &bitbucket::pull_request_url(workspace, repo, req.number),
                &header,
            )?;
            let body = bitbucket::parse_body(&detail_json).map_err(|e| e.to_string())?;
            let mut comments = Vec::new();
            let mut next = Some(bitbucket::comments_url(workspace, repo, req.number));
            while let Some(url) = next {
                let page = curl_body(&url, &header)?;
                comments.extend(bitbucket::parse_comments(&page).map_err(|e| e.to_string())?);
                next = bitbucket::next_page(&page);
            }
            let mut commits = Vec::new();
            let mut next = Some(bitbucket::commits_url(workspace, repo, req.number));
            while let Some(url) = next {
                let page = curl_body(&url, &header)?;
                commits.extend(bitbucket::parse_commits(&page).map_err(|e| e.to_string())?);
                next = bitbucket::next_page(&page);
            }
            // Bitbucket lists commits newest-first; flip to the oldest-first invariant.
            commits.reverse();
            Ok(crate::pull_requests::model::PrDetail {
                body,
                comments,
                check_runs: Vec::new(),
                commits,
            })
        }
    }
}

/// `curl_get` reduced to `Result<body, message>` for the single-PR detail fetch.
fn curl_body(url: &str, auth_header: &str) -> Result<String, String> {
    match curl_get(url, auth_header) {
        CurlResult::Ok(body) => Ok(body),
        CurlResult::Unauthorized => Err("Bitbucket token invalid or expired".to_owned()),
        CurlResult::HttpError(message) => Err(message),
        CurlResult::Failed => Err("Bitbucket unreachable".to_owned()),
    }
}

/// Submit one review from the cockpit (pull-requests.md §11): the verdict plus the
/// drafted line comments and optional summary, posted in one request.
#[derive(Debug, Clone)]
pub struct PrPostRequest {
    pub key: PrReviewKey,
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub bitbucket_email: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub comments: Vec<DraftComment>,
}

/// The single reply per post request, echoing the key for adoption.
#[derive(Debug, Clone)]
pub struct PrPostReply {
    pub key: PrReviewKey,
    pub result: Result<(), String>,
}

/// Off-thread review submission, fire-and-forget like `PrDetailRunner`. The reply
/// carries the echoed key so the UI re-fetches the right PR's detail on success.
pub struct PrPostRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<PrPostReply>,
    results_rx: Receiver<PrPostReply>,
}

impl PrPostRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: std::sync::Arc::new(on_event),
            results_tx,
            results_rx,
        }
    }

    pub fn request(&self, request: PrPostRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = post_review(&request);
            let _ = tx.send(PrPostReply {
                key: request.key,
                result,
            });
            on_event();
        });
    }

    pub fn try_recv(&self) -> Option<PrPostReply> {
        self.results_rx.try_recv().ok()
    }
}

fn post_review(req: &PrPostRequest) -> Result<(), String> {
    match req.forge_kind {
        ForgeKind::GitHub => {
            if !command_ok("gh", &github::auth_status_args()) {
                return Err("Install gh and run `gh auth login`".to_owned());
            }
            let body = github::submit_review_body(req.verdict, &req.summary, &req.comments);
            run_stdin(
                "gh",
                &github::submit_review_args(&req.repo_label, req.number),
                &body,
            )
            .map(|_| ())
            .map_err(|e| {
                let e = e.trim();
                if e.is_empty() {
                    "gh review submit failed".to_owned()
                } else {
                    e.to_owned()
                }
            })
        }
        ForgeKind::Bitbucket => {
            let (workspace, repo) = req
                .repo_label
                .split_once('/')
                .ok_or_else(|| format!("malformed repo label '{}'", req.repo_label))?;
            let email = &req.bitbucket_email;
            let token = (!email.is_empty())
                .then(|| creds::read_token(email))
                .flatten()
                .ok_or_else(|| "Set a Bitbucket email and token in Preferences".to_owned())?;
            let header = bitbucket::basic_auth_header(email, &token);
            let comments_url = bitbucket::post_comment_url(workspace, repo, req.number);
            for c in &req.comments {
                curl_post_ok(
                    &comments_url,
                    &header,
                    &bitbucket::inline_comment_body(&c.path, c.line, &c.body),
                )?;
            }
            if !req.summary.trim().is_empty() {
                curl_post_ok(
                    &comments_url,
                    &header,
                    &bitbucket::summary_comment_body(&req.summary),
                )?;
            }
            match req.verdict {
                ReviewVerdict::Approve => curl_post_ok(
                    &bitbucket::approve_url(workspace, repo, req.number),
                    &header,
                    "",
                )?,
                ReviewVerdict::RequestChanges => curl_post_ok(
                    &bitbucket::request_changes_url(workspace, repo, req.number),
                    &header,
                    "",
                )?,
                ReviewVerdict::Comment => {}
            }
            Ok(())
        }
    }
}

pub struct PrRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<PrReply>,
    results_rx: Receiver<PrReply>,
    in_flight: bool,
    github_login: Option<String>,
    bitbucket_uuid: Option<String>,
}

impl PrRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: std::sync::Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: false,
            github_login: None,
            bitbucket_uuid: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight
    }

    /// Spawn the detached fetch; `false` when one is already running.
    pub fn request(&mut self, request: PrRequest) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        let github_login = self.github_login.clone();
        let bitbucket_uuid = self.bitbucket_uuid.clone();
        std::thread::spawn(move || {
            let reply = fetch(request, github_login, bitbucket_uuid);
            let _ = tx.send(reply);
            on_event();
        });
        true
    }

    /// Drain the reply (if any); clearing `in_flight` re-arms the runner and the
    /// resolved identity is cached for the next request.
    pub fn try_recv(&mut self) -> Option<PrReply> {
        let reply = self.results_rx.try_recv().ok()?;
        self.in_flight = false;
        if reply.github_login.is_some() {
            self.github_login = reply.github_login.clone();
        }
        if reply.bitbucket_uuid.is_some() {
            self.bitbucket_uuid = reply.bitbucket_uuid.clone();
        }
        Some(reply)
    }
}

/// The worker body: resolve forges, query each source, tag roles + dedupe. Each
/// source yields `Some(rows)` once it answered, or `None` when a query failed
/// transiently so the cache keeps its last-good rows for that forge (§6).
fn fetch(
    request: PrRequest,
    mut github_login: Option<String>,
    mut bitbucket_uuid: Option<String>,
) -> PrReply {
    let forges = forges_of_roots(&request.roots);
    let has_github = forges
        .iter()
        .any(|(f, _)| matches!(f, Forge::GitHub { .. }));
    let has_bitbucket = forges
        .iter()
        .any(|(f, _)| matches!(f, Forge::Bitbucket { .. }));

    let mut github = SourceStatus::Absent;
    let mut github_rows: Option<Vec<PullRequest>> = Some(Vec::new());
    let mut bitbucket = SourceStatus::Absent;
    let mut bitbucket_rows: Option<Vec<PullRequest>> = Some(Vec::new());

    // GitHub — availability via `gh auth status`, identity via `gh api user`.
    if has_github {
        if !command_ok("gh", &github::auth_status_args()) {
            github = SourceStatus::Unavailable("Install gh and run `gh auth login`".to_owned());
        } else {
            if github_login.is_none() {
                github_login =
                    run_stdout("gh", &github::current_login_args()).map(|s| s.trim().to_owned());
            }
            match &github_login {
                Some(login) if !login.is_empty() => {
                    let mut rows = Vec::new();
                    let mut complete = true;
                    for query in plan(&forges, None) {
                        if let PrQuery::Gh { repo_label, args } = query {
                            match run_stdout("gh", &args) {
                                Some(json) => {
                                    if let Ok(mut prs) =
                                        github::parse_list(&json, login, &repo_label)
                                    {
                                        rows.append(&mut prs);
                                    }
                                }
                                // A failed `gh pr list` would drop rows silently;
                                // keep the cached set rather than show a partial one.
                                None => complete = false,
                            }
                        }
                    }
                    if complete {
                        github = SourceStatus::Ok;
                        github_rows = Some(dedupe(rows));
                    } else {
                        github = SourceStatus::Unavailable(
                            "GitHub unreachable — showing cached results".to_owned(),
                        );
                        github_rows = None;
                    }
                }
                _ => {
                    github =
                        SourceStatus::Unavailable("Could not resolve GitHub identity".to_owned())
                }
            }
        }
    }

    // Bitbucket — creds from Prefs email + Keychain token; identity via /2.0/user.
    if has_bitbucket {
        let email = &request.bitbucket_email;
        let token = (!email.is_empty())
            .then(|| creds::read_token(email))
            .flatten();
        match token {
            None => {
                bitbucket = SourceStatus::Unavailable(
                    "Set a Bitbucket email and token in Preferences".to_owned(),
                )
            }
            Some(token) => {
                let header = bitbucket::basic_auth_header(email, &token);
                if bitbucket_uuid.is_none() {
                    match curl_get(&bitbucket::current_user_url(), &header) {
                        CurlResult::Ok(json) => {
                            bitbucket_uuid = bitbucket::parse_current_user(&json)
                        }
                        CurlResult::Unauthorized => {
                            bitbucket = SourceStatus::Unavailable(
                                "Bitbucket token invalid or expired".to_owned(),
                            )
                        }
                        CurlResult::HttpError(message) => {
                            bitbucket = SourceStatus::Unavailable(message);
                            bitbucket_rows = None;
                        }
                        CurlResult::Failed => {
                            bitbucket =
                                SourceStatus::Unavailable("Bitbucket unreachable".to_owned());
                            bitbucket_rows = None;
                        }
                    }
                }
                if let Some(uuid) = &bitbucket_uuid {
                    let mut rows = Vec::new();
                    let mut complete = true;
                    let mut unauthorized = false;
                    for query in plan(&forges, Some(uuid.as_str())) {
                        if let PrQuery::Bitbucket {
                            repo_label,
                            url,
                            role,
                        } = query
                        {
                            let mut next = Some(url);
                            while let Some(page_url) = next {
                                match curl_get(&page_url, &header) {
                                    CurlResult::Ok(json) => {
                                        if let Ok(mut prs) =
                                            bitbucket::parse_list(&json, &repo_label, role)
                                        {
                                            rows.append(&mut prs);
                                        }
                                        next = bitbucket::next_page(&json);
                                    }
                                    CurlResult::Unauthorized => {
                                        unauthorized = true;
                                        next = None;
                                    }
                                    CurlResult::HttpError(_) | CurlResult::Failed => {
                                        complete = false;
                                        next = None;
                                    }
                                }
                            }
                        }
                        if unauthorized {
                            break;
                        }
                    }
                    if unauthorized {
                        bitbucket = SourceStatus::Unavailable(
                            "Bitbucket token invalid or expired".to_owned(),
                        );
                    } else if complete {
                        bitbucket = SourceStatus::Ok;
                        bitbucket_rows = Some(dedupe(rows));
                    } else {
                        bitbucket = SourceStatus::Unavailable(
                            "Bitbucket unreachable — showing cached results".to_owned(),
                        );
                        bitbucket_rows = None;
                    }
                }
            }
        }
    }

    PrReply {
        github,
        bitbucket,
        github_rows,
        bitbucket_rows,
        github_login,
        bitbucket_uuid,
    }
}

fn command_ok(program: &str, args: &[String]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_stdout(program: &str, args: &[String]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run `program` feeding `input` on stdin (the `gh api --input -` path); returns
/// stdout on success, else stderr (so the forge's own error reaches the UI).
fn run_stdin(program: &str, args: &[String], input: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or_else(|| "failed to open stdin".to_owned())?
        .write_all(input.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// `curl_post` reduced to `Result<(), message>` for the write path; the success
/// body is discarded (we re-fetch the detail afterwards).
fn curl_post_ok(url: &str, auth_header: &str, body: &str) -> Result<(), String> {
    match curl_post(url, auth_header, body) {
        CurlResult::Ok(_) => Ok(()),
        CurlResult::Unauthorized => Err("Bitbucket token invalid or expired".to_owned()),
        CurlResult::HttpError(message) => Err(message),
        CurlResult::Failed => Err("Bitbucket unreachable".to_owned()),
    }
}

enum CurlResult {
    Ok(String),
    Unauthorized,
    /// An HTTP error reply (403, 404, 5xx…): authenticated but refused, carrying
    /// Bitbucket's own reason so the UI can name it (not "unreachable").
    HttpError(String),
    /// curl could not produce an HTTP response at all (couldn't run, or `000`).
    Failed,
}

/// `curl` the URL with a Basic-auth header, splitting the trailing `%{http_code}`
/// the `update.rs` idiom uses to tell 200 / 401 / error / no-response apart.
fn curl_get(url: &str, auth_header: &str) -> CurlResult {
    run_curl(
        &["-w".to_owned(), "\n%{http_code}".to_owned(), url.to_owned()],
        auth_header,
    )
}

/// `curl -X POST` the URL with the Basic-auth header and a JSON body; comment
/// creation returns 201 and approve/request-changes return 200, so both pass.
fn curl_post(url: &str, auth_header: &str, body: &str) -> CurlResult {
    run_curl(
        &[
            "-X".to_owned(),
            "POST".to_owned(),
            "-H".to_owned(),
            "Content-Type: application/json".to_owned(),
            "-d".to_owned(),
            body.to_owned(),
            "-w".to_owned(),
            "\n%{http_code}".to_owned(),
            url.to_owned(),
        ],
        auth_header,
    )
}

/// Spawn `curl` with timeouts, feeding the Basic-auth header through a `--config`
/// file on stdin so the token never lands on the argv (visible to any `ps`); the
/// per-call options stay on the argv.
fn run_curl(args: &[String], auth_header: &str) -> CurlResult {
    use std::io::Write;
    use std::process::Stdio;
    let base = [
        "--silent".to_owned(),
        "--connect-timeout".to_owned(),
        "10".to_owned(),
        "--max-time".to_owned(),
        "30".to_owned(),
        "--config".to_owned(),
        "-".to_owned(),
    ];
    let Ok(mut child) = Command::new("curl")
        .args(base.iter().chain(args))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return CurlResult::Failed;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("header = \"Authorization: {auth_header}\"\n").as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
        return CurlResult::Failed;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    match code.trim() {
        "200" | "201" => CurlResult::Ok(body.to_owned()),
        "401" => CurlResult::Unauthorized,
        "000" | "" => CurlResult::Failed,
        code => CurlResult::HttpError(
            bitbucket::parse_error_message(body)
                .unwrap_or_else(|| format!("Bitbucket error (HTTP {code})")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github(label: &str) -> (Forge, String) {
        let (owner, repo) = label.split_once('/').unwrap();
        (
            Forge::GitHub {
                owner: owner.to_owned(),
                repo: repo.to_owned(),
            },
            label.to_owned(),
        )
    }

    fn bitbucket(label: &str) -> (Forge, String) {
        let (workspace, repo) = label.split_once('/').unwrap();
        (
            Forge::Bitbucket {
                workspace: workspace.to_owned(),
                repo: repo.to_owned(),
            },
            label.to_owned(),
        )
    }

    #[test]
    fn plan_fans_two_gh_queries_per_github_repo() {
        let queries = plan(&[github("acme/web")], None);
        assert_eq!(
            queries,
            vec![
                PrQuery::Gh {
                    repo_label: "acme/web".to_owned(),
                    args: github::list_authored_args("acme/web"),
                },
                PrQuery::Gh {
                    repo_label: "acme/web".to_owned(),
                    args: github::list_review_requested_args("acme/web"),
                },
            ]
        );
    }

    #[test]
    fn plan_emits_two_bitbucket_queries_only_once_the_uuid_is_known() {
        let forges = [bitbucket("team/repo")];
        // No uuid yet (identity unresolved) ⇒ no Bitbucket query.
        assert!(plan(&forges, None).is_empty());
        assert_eq!(
            plan(&forges, Some("{me}")),
            vec![
                PrQuery::Bitbucket {
                    repo_label: "team/repo".to_owned(),
                    url: bitbucket::authored_url("team", "repo", "{me}"),
                    role: PrRole::Mine,
                },
                PrQuery::Bitbucket {
                    repo_label: "team/repo".to_owned(),
                    url: bitbucket::reviewing_url("team", "repo", "{me}"),
                    role: PrRole::ToReview,
                },
            ]
        );
    }

    #[test]
    fn plan_mixes_sources_in_order() {
        let queries = plan(&[github("acme/web"), bitbucket("team/repo")], Some("{me}"));
        assert_eq!(queries.len(), 4);
        assert!(matches!(queries[0], PrQuery::Gh { .. }));
        assert!(matches!(queries[1], PrQuery::Gh { .. }));
        assert!(matches!(
            &queries[2],
            PrQuery::Bitbucket { role, .. } if *role == PrRole::Mine
        ));
        assert!(matches!(
            &queries[3],
            PrQuery::Bitbucket { role, .. } if *role == PrRole::ToReview
        ));
    }

    #[test]
    fn fetch_refspec_lands_a_local_branch_per_forge() {
        assert_eq!(
            fetch_refspec(ForgeKind::GitHub, 128, "feature/login"),
            "pull/128/head:feature/login"
        );
        assert_eq!(
            fetch_refspec(ForgeKind::Bitbucket, 7, "feature/login"),
            "feature/login:feature/login"
        );
    }

    #[test]
    fn match_pr_root_keys_on_forge_kind_and_label() {
        let roots = vec![
            (
                PathBuf::from("/ws/web"),
                ForgeKind::GitHub,
                "acme/web".to_owned(),
            ),
            (
                PathBuf::from("/ws/web-bb"),
                ForgeKind::Bitbucket,
                "acme/web".to_owned(),
            ),
        ];
        assert_eq!(
            match_pr_root(&roots, ForgeKind::Bitbucket, "acme/web"),
            Some(PathBuf::from("/ws/web-bb"))
        );
        assert_eq!(match_pr_root(&roots, ForgeKind::GitHub, "acme/api"), None);
    }

    #[test]
    fn matching_worktree_finds_a_row_on_the_branch_within_the_project() {
        let rows = vec![
            (0, PathBuf::from("/ws/web"), "main".to_owned()),
            (1, PathBuf::from("/ws/web"), "feature/login".to_owned()),
            // Same branch name, different project ⇒ must not match.
            (2, PathBuf::from("/ws/api"), "feature/login".to_owned()),
        ];
        assert_eq!(
            matching_worktree(&rows, Path::new("/ws/web"), "feature/login"),
            Some(1)
        );
        assert_eq!(
            matching_worktree(&rows, Path::new("/ws/web"), "absent"),
            None
        );
    }

    fn row(forge: ForgeKind, repo: &str, number: u64) -> PullRequest {
        use crate::pull_requests::model::{Checks, PrState, Review};
        PullRequest {
            forge_kind: forge,
            repo_label: repo.to_owned(),
            number,
            title: String::new(),
            role: PrRole::Mine,
            state: PrState::Open,
            author: String::new(),
            source_branch: String::new(),
            dest_branch: String::new(),
            url: String::new(),
            updated_at: String::new(),
            checks: Checks::None,
            review: Review::None,
            reviewers: Vec::new(),
        }
    }

    fn reply(
        github_rows: Option<Vec<PullRequest>>,
        bitbucket_rows: Option<Vec<PullRequest>>,
    ) -> PrReply {
        PrReply {
            github: SourceStatus::Ok,
            bitbucket: SourceStatus::Ok,
            github_rows,
            bitbucket_rows,
            github_login: None,
            bitbucket_uuid: None,
        }
    }

    #[test]
    fn apply_replaces_rows_and_clears_stale_when_both_sources_answer() {
        let mut cache = PrCache::default();
        cache.apply(reply(
            Some(vec![row(ForgeKind::GitHub, "acme/web", 1)]),
            Some(vec![row(ForgeKind::Bitbucket, "team/repo", 2)]),
        ));
        assert_eq!(cache.pull_requests.len(), 2);
        assert!(cache.loaded);
        assert!(!cache.stale);
    }

    #[test]
    fn apply_keeps_a_failed_sources_prior_rows_and_flags_stale() {
        let mut cache = PrCache::default();
        cache.apply(reply(
            Some(vec![row(ForgeKind::GitHub, "acme/web", 1)]),
            Some(vec![row(ForgeKind::Bitbucket, "team/repo", 2)]),
        ));

        // GitHub fails transiently (None): its prior row survives; Bitbucket refreshes.
        cache.apply(reply(
            None,
            Some(vec![
                row(ForgeKind::Bitbucket, "team/repo", 2),
                row(ForgeKind::Bitbucket, "team/repo", 3),
            ]),
        ));
        assert!(cache.stale);
        assert!(cache
            .pull_requests
            .iter()
            .any(|pr| pr.forge_kind == ForgeKind::GitHub && pr.number == 1));
        assert_eq!(
            cache
                .pull_requests
                .iter()
                .filter(|pr| pr.forge_kind == ForgeKind::Bitbucket)
                .count(),
            2
        );
    }
}
