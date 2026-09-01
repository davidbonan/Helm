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
    /// Current-user display names for the conversation composer avatar (§11),
    /// resolved alongside the identity above and so only `Some` on the first reply;
    /// the app keeps its last-good value on later replies.
    pub github_name: Option<String>,
    pub bitbucket_name: Option<String>,
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
    /// Unix seconds of the last folded reply, for the header's "· 2 min ago" note.
    /// `None` until the first one lands.
    pub refreshed_at: Option<i64>,
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
        self.refreshed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .ok();
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
/// its changed files (pull-requests.md §5). Network only when the listed tips
/// (`source_commit` / `dest_commit`) are not already in the repo: then one fetch of
/// the PR head and the base branch into `FETCH_HEAD` — **no local branch is
/// written** (read-only).
#[derive(Debug, Clone)]
pub struct PrFilesRequest {
    pub key: PrReviewKey,
    pub root: PathBuf,
    pub forge_kind: ForgeKind,
    pub number: u64,
    pub source_branch: String,
    pub dest_branch: String,
    pub source_commit: String,
    pub dest_commit: String,
}

/// The resolved review base/head and the changed-files list.
#[derive(Debug, Clone)]
pub struct PrFilesLoaded {
    pub base: git2::Oid,
    pub head: git2::Oid,
    pub files: Vec<crate::git::commit_detail::CommitFile>,
}

/// The commit range a `CommitFiles` request recomputes the changed files over — both
/// already present locally after the initial PR-head fetch, so the recompute is I/O-free
/// (per-commit diff: T5).
#[derive(Debug, Clone, Copy)]
pub enum CommitRange {
    /// One commit: `base = its first parent` (the commit itself if it is a root), so
    /// the diff is exactly what that commit introduced.
    Commit(git2::Oid),
    /// An explicit oid range — the stored three-dot anchors when returning to "All
    /// commits".
    Range { base: git2::Oid, head: git2::Oid },
}

/// A review fetch: the changed-files list (network), a per-commit/range files recompute
/// (local), or an embedded image. One file's diff is a `PrFileDiffRequest`: batched
/// through the runner's bounded pool rather than a thread per request.
#[derive(Debug, Clone)]
pub enum PrReviewRequest {
    Files(PrFilesRequest),
    CommitFiles {
        key: PrReviewKey,
        root: PathBuf,
        /// Echoed back so a stale reply for a since-changed selection is dropped
        /// (`None` ⇒ "All commits").
        selection: Option<String>,
        range: CommitRange,
    },
    /// An image a markdown body / comment embeds (pull-requests.md §11). Not keyed on
    /// a review: the same asset can be linked from any PR, and the cache is the URL's.
    Image {
        url: String,
        forge_kind: ForgeKind,
        /// Bitbucket identity for the Basic header, empty on GitHub.
        bitbucket_email: String,
    },
}

/// One file's local diff over a fetched range (pull-requests.md §11).
#[derive(Debug, Clone)]
pub struct PrFileDiffRequest {
    pub key: PrReviewKey,
    pub root: PathBuf,
    pub base: git2::Oid,
    pub head: git2::Oid,
    pub path: String,
}

/// The single reply per review request, echoing the key (and path/selection) for adoption.
pub enum PrReviewReply {
    Files {
        key: PrReviewKey,
        result: Result<PrFilesLoaded, String>,
    },
    CommitFiles {
        key: PrReviewKey,
        selection: Option<String>,
        result: Result<PrFilesLoaded, String>,
    },
    FileDiff {
        key: PrReviewKey,
        path: String,
        result: Result<crate::git::diff::FileDiff, String>,
    },
    Image {
        url: String,
        result: Result<Vec<u8>, String>,
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
///
/// File diffs go through a **bounded pool** (`DIFF_WORKERS`) fed by `DiffQueue`
/// rather than a thread per file: the Files tab asks for every file of a PR on
/// one frame, and a hundred threads each opening the repo contended with the
/// frame instead of finishing sooner. The newest batch is served first, in its
/// own order — the PR just opened ahead of the one just left, and the file the
/// user is looking at ahead of the rest of its column.
pub struct PrReviewRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<PrReviewReply>,
    results_rx: Receiver<PrReviewReply>,
    diffs: std::sync::Arc<DiffQueue>,
}

/// Threads serving the file-diff queue: a diff is a few ms of CPU, so a handful
/// keeps the column filling without starving the UI thread of a core.
const DIFF_WORKERS: usize = 4;

/// The file-diff work queue shared by the runner and its workers: newest batch at
/// the front, `closed` once the runner is dropped so the workers exit.
#[derive(Default)]
struct DiffQueue {
    jobs: std::sync::Mutex<DiffJobs>,
    ready: std::sync::Condvar,
}

#[derive(Default)]
struct DiffJobs {
    pending: std::collections::VecDeque<PrFileDiffRequest>,
    closed: bool,
}

impl DiffQueue {
    /// Puts `batch` ahead of everything pending, keeping the batch's own order.
    fn push_front(&self, batch: Vec<PrFileDiffRequest>) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        for job in batch.into_iter().rev() {
            jobs.pending.push_front(job);
        }
        self.ready.notify_all();
    }

    /// Blocks until a job is available; `None` once the queue is closed.
    fn pop_front(&self) -> Option<PrFileDiffRequest> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(job) = jobs.pending.pop_front() {
                return Some(job);
            }
            if jobs.closed {
                return None;
            }
            jobs = self.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn close(&self) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.closed = true;
        jobs.pending.clear();
        self.ready.notify_all();
    }
}

impl PrReviewRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        let on_event: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(on_event);
        let diffs = std::sync::Arc::new(DiffQueue::default());
        for _ in 0..DIFF_WORKERS {
            let queue = std::sync::Arc::clone(&diffs);
            let tx = results_tx.clone();
            let on_event = std::sync::Arc::clone(&on_event);
            std::thread::spawn(move || {
                while let Some(job) = queue.pop_front() {
                    if tx.send(run_file_diff(job)).is_err() {
                        break;
                    }
                    on_event();
                }
            });
        }
        Self {
            on_event,
            results_tx,
            results_rx,
            diffs,
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

    /// Queues a batch of file diffs ahead of what is still pending, served in the
    /// batch's order by the pool.
    pub fn request_file_diffs(&self, batch: Vec<PrFileDiffRequest>) {
        self.diffs.push_front(batch);
    }

    pub fn try_recv(&self) -> Option<PrReviewReply> {
        self.results_rx.try_recv().ok()
    }
}

impl Drop for PrReviewRunner {
    fn drop(&mut self) {
        self.diffs.close();
    }
}

fn run_file_diff(job: PrFileDiffRequest) -> PrReviewReply {
    let result = git2::Repository::open(&job.root)
        .map_err(|e| e.message().to_owned())
        .and_then(|repo| {
            crate::git::diff::pr_file_diff(&repo, job.base, job.head, &job.path)
                .map_err(|e| e.message().to_owned())
        });
    PrReviewReply::FileDiff {
        key: job.key,
        path: job.path,
        result,
    }
}

fn run_review(request: PrReviewRequest) -> PrReviewReply {
    match request {
        PrReviewRequest::Files(req) => PrReviewReply::Files {
            key: req.key.clone(),
            result: load_pr_files(&req),
        },
        PrReviewRequest::CommitFiles {
            key,
            root,
            selection,
            range,
        } => PrReviewReply::CommitFiles {
            key,
            selection,
            result: load_commit_files(&root, range),
        },
        PrReviewRequest::Image {
            url,
            forge_kind,
            bitbucket_email,
        } => {
            let result = fetch_image(&url, forge_kind, &bitbucket_email);
            PrReviewReply::Image { url, result }
        }
    }
}

/// Largest embedded image helm will pull: a review body links screenshots, not
/// archives, and the bytes cross a channel and become a GPU texture.
const MAX_IMAGE_BYTES: u64 = 8_000_000;

/// Fetches an embedded image's bytes over `curl`. The forge credentials only travel
/// to the **forge's own hosts** (`image_auth`) — an image body can name any host on
/// the internet, and a Basic header handed to one of them is a leaked credential.
/// curl drops the header itself if a redirect leaves that host.
fn fetch_image(url: &str, forge_kind: ForgeKind, bitbucket_email: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("unsupported image URL".to_owned());
    }
    // A repo download linked from a comment answers 401 on the **website** host even
    // with the credentials: only the API serves it (it 302s to a signed S3 link, which
    // curl then follows without the header).
    let rewritten = bitbucket_download_api_url(url);
    let url = rewritten.as_deref().unwrap_or(url);
    let mut args = vec![
        "--silent".to_owned(),
        "--location".to_owned(),
        "--fail".to_owned(),
        "--connect-timeout".to_owned(),
        "10".to_owned(),
        "--max-time".to_owned(),
        "30".to_owned(),
        "--max-filesize".to_owned(),
        MAX_IMAGE_BYTES.to_string(),
        // The status goes to stderr (`%{stderr}`) so a failure can name itself instead
        // of reading "unavailable" twice; the body stays alone on stdout.
        "--write-out".to_owned(),
        "%{stderr}%{http_code}".to_owned(),
    ];
    let auth = image_auth(url, forge_kind, bitbucket_email);
    if auth.is_some() {
        args.push("--config".to_owned());
        args.push("-".to_owned());
    }
    args.push(url.to_owned());

    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("curl")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "curl not available".to_owned())?;
    if let (Some(header), Some(mut stdin)) = (auth, child.stdin.take()) {
        let _ = stdin.write_all(format!("header = \"Authorization: {header}\"\n").as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|_| "image fetch failed".to_owned())?;
    if !out.status.success() {
        let status = String::from_utf8_lossy(&out.stderr);
        let status = status.trim();
        return Err(match status.parse::<u32>() {
            Ok(code) if code >= 400 => format!("HTTP {code}"),
            _ => "fetch failed".to_owned(),
        });
    }
    if out.stdout.is_empty() {
        return Err("empty image".to_owned());
    }
    Ok(out.stdout)
}

/// A Bitbucket **repo download** (`bitbucket.org/{ws}/{repo}/downloads/{file}`) as the
/// API path that actually serves it to a credentialed client — the website host answers
/// `401` to Basic auth, while `api.bitbucket.org` redirects to a signed link. `None`
/// for any other URL, which is fetched as written (pull-requests.md §11).
fn bitbucket_download_api_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://bitbucket.org/")
        .or_else(|| url.strip_prefix("http://bitbucket.org/"))?;
    let (path, query) = rest.split_once(['?', '#']).unwrap_or((rest, ""));
    let mut parts = path.splitn(4, '/');
    let workspace = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    if parts.next()? != "downloads" {
        return None;
    }
    let file = parts.next().filter(|s| !s.is_empty())?;
    let _ = query;
    Some(format!(
        "https://api.bitbucket.org/2.0/repositories/{workspace}/{repo}/downloads/{file}"
    ))
}

/// The `Authorization` value to send with an image request, or `None` when the URL
/// is not on a host the forge credentials belong to.
fn image_auth(url: &str, forge_kind: ForgeKind, bitbucket_email: &str) -> Option<String> {
    let authority = url.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, h)| h)
        .split(':')
        .next()?
        .to_ascii_lowercase();
    match forge_kind {
        // Attachments under github.com itself need the session; the CDN hosts serve
        // pre-signed URLs and must not see the token.
        ForgeKind::GitHub => (host == "github.com" || host.ends_with(".github.com"))
            .then(|| run_stdout("gh", &["auth".to_owned(), "token".to_owned()]))
            .flatten()
            .map(|token| format!("Bearer {}", token.trim())),
        ForgeKind::Bitbucket => {
            if host != "bitbucket.org" && !host.ends_with(".bitbucket.org") {
                return None;
            }
            (!bitbucket_email.is_empty())
                .then(|| creds::read_token(bitbucket_email))
                .flatten()
                .map(|token| bitbucket::basic_auth_header(bitbucket_email, &token))
        }
    }
}

fn load_pr_files(req: &PrFilesRequest) -> Result<PrFilesLoaded, String> {
    // A ref name beginning with '-' must never reach the CLI as a flag.
    if req.source_branch.starts_with('-') || req.dest_branch.starts_with('-') {
        return Err("invalid branch name".to_owned());
    }
    let repo = git2::Repository::open(&req.root).map_err(|e| e.message().to_owned())?;
    let (head, dest) = match listed_tips(&repo, req) {
        Some(tips) => tips,
        None => fetch_tips(&repo, req)?,
    };

    // Three-dot base: the merge-base, so the diff excludes commits already on the
    // destination. Unrelated histories ⇒ fall back to the destination tip.
    let base = repo.merge_base(dest, head).unwrap_or(dest);
    let files = crate::git::diff::pr_changed_files(&repo, base, head)
        .map_err(|e| e.message().to_owned())?;
    Ok(PrFilesLoaded { base, head, files })
}

/// The two tips exactly as the forge listed them, when both objects are already in
/// the repo — a PR opened before, or a branch the user works on here. No network:
/// the file list is on screen at once (pull-requests.md §5).
fn listed_tips(repo: &git2::Repository, req: &PrFilesRequest) -> Option<(git2::Oid, git2::Oid)> {
    if req.source_commit.is_empty() || req.dest_commit.is_empty() {
        return None;
    }
    let head = revparse_commit(repo, &req.source_commit)?;
    let dest = revparse_commit(repo, &req.dest_commit)?;
    Some((head, dest))
}

/// One `git fetch origin <head> <dest>` lands both tips in a single round trip.
/// The PR head is `FETCH_HEAD`'s first line; the destination is the listed commit
/// once fetched, else the remote-tracking ref or a same-name local branch. A
/// destination the remote no longer has fails the whole fetch, so the head alone
/// is retried before giving up.
fn fetch_tips(
    repo: &git2::Repository,
    req: &PrFilesRequest,
) -> Result<(git2::Oid, git2::Oid), String> {
    let head_ref = review_head_ref(req.forge_kind, req.number, &req.source_branch);
    if fetch_origin(&req.root, &[&head_ref, &req.dest_branch]).is_err() {
        fetch_origin(&req.root, &[&head_ref])?;
    }
    let head = revparse_commit(repo, "FETCH_HEAD")
        .ok_or_else(|| format!("cannot resolve PR head '{head_ref}'"))?;
    let dest = (!req.dest_commit.is_empty())
        .then(|| revparse_commit(repo, &req.dest_commit))
        .flatten()
        .or_else(|| revparse_commit(repo, &format!("origin/{}", req.dest_branch)))
        .or_else(|| revparse_commit(repo, &req.dest_branch))
        .ok_or_else(|| format!("cannot resolve base branch '{}'", req.dest_branch))?;
    Ok((head, dest))
}

/// Recompute the changed files for a single commit (`parent..commit`) or an explicit
/// oid range — both already fetched, so no network. A root commit (no parent) diffs
/// against itself, yielding an empty delta.
fn load_commit_files(root: &Path, range: CommitRange) -> Result<PrFilesLoaded, String> {
    let repo = git2::Repository::open(root).map_err(|e| e.message().to_owned())?;
    let (base, head) = match range {
        CommitRange::Range { base, head } => (base, head),
        CommitRange::Commit(head) => {
            let commit = repo.find_commit(head).map_err(|e| e.message().to_owned())?;
            let base = commit.parent(0).map(|p| p.id()).unwrap_or(head);
            (base, head)
        }
    };
    let files = crate::git::diff::pr_changed_files(&repo, base, head)
        .map_err(|e| e.message().to_owned())?;
    Ok(PrFilesLoaded { base, head, files })
}

fn fetch_origin(root: &Path, refspecs: &[&str]) -> Result<(), String> {
    let mut args = vec!["fetch", "origin"];
    args.extend_from_slice(refspecs);
    match crate::git::cli::run(root, &args) {
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

/// A detail request answers in up to two replies, each echoing the key for
/// adoption: `Partial` as soon as the forge returns the PR itself — comments and
/// commits still on their way — then `Complete` with everything (or the failure).
/// A surface still empty paints the partial at once; one already showing a detail
/// keeps it until `Complete`, so a staleness refetch never blanks the threads.
#[derive(Debug, Clone)]
pub enum PrDetailReply {
    Partial {
        key: PrReviewKey,
        detail: crate::pull_requests::model::PrDetail,
    },
    Complete {
        key: PrReviewKey,
        result: Result<crate::pull_requests::model::PrDetail, String>,
    },
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
            let partial_tx = tx.clone();
            let partial_key = request.key.clone();
            let partial_event = std::sync::Arc::clone(&on_event);
            let result = fetch_detail(&request, &|detail| {
                let _ = partial_tx.send(PrDetailReply::Partial {
                    key: partial_key.clone(),
                    detail,
                });
                partial_event();
            });
            let _ = tx.send(PrDetailReply::Complete {
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

/// The forge calls a detail needs are independent of each other, so they run at
/// once and the reply lands when the slowest returns, not their sum; `on_partial`
/// fires as soon as the PR itself is parsed. GitHub: `gh pr view` (body, checks,
/// conversation, commits) beside the two inline-comment resources. Bitbucket: the
/// PR beside its paginated comments and commits.
fn fetch_detail(
    req: &PrDetailRequest,
    on_partial: &(dyn Fn(crate::pull_requests::model::PrDetail) + Sync),
) -> Result<crate::pull_requests::model::PrDetail, String> {
    match req.forge_kind {
        ForgeKind::GitHub => std::thread::scope(|scope| {
            // Inline review comments are a separate REST resource; missing them is
            // non-fatal (the conversation + diff still render). Resolution + the
            // thread node id live on the GraphQL review thread, joined back by
            // `databaseId`; also non-fatal.
            let inline = scope.spawn(|| {
                run_stdout(
                    "gh",
                    &github::review_comments_args(&req.repo_label, req.number),
                )
                .and_then(|j| github::parse_review_comments(&j).ok())
            });
            let threads = scope.spawn(|| {
                run_stdout(
                    "gh",
                    &github::review_threads_args(&req.repo_label, req.number),
                )
                .and_then(|j| github::parse_review_threads(&j).ok())
            });
            let json = run_stdout("gh", &github::view_args(&req.repo_label, req.number))
                .ok_or_else(|| format!("gh pr view {} failed", req.number))?;
            let mut detail = github::parse_detail(&json).map_err(|e| e.to_string())?;
            on_partial(detail.clone());
            if let Some(mut inline) = joined(inline)? {
                if let Some(threads) = joined(threads)? {
                    github::apply_thread_resolution(&mut inline, &threads);
                }
                detail.comments.extend(inline);
            }
            Ok(detail)
        }),
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
            std::thread::scope(|scope| {
                let comments = scope.spawn(|| {
                    collect_pages(
                        bitbucket::comments_url(workspace, repo, req.number),
                        &header,
                        bitbucket::parse_comments,
                    )
                });
                let commits = scope.spawn(|| {
                    collect_pages(
                        bitbucket::commits_url(workspace, repo, req.number),
                        &header,
                        bitbucket::parse_commits,
                    )
                });
                let detail_json = curl_body(
                    &bitbucket::pull_request_url(workspace, repo, req.number),
                    &header,
                )?;
                let mut detail = crate::pull_requests::model::PrDetail {
                    body: bitbucket::parse_body(&detail_json).map_err(|e| e.to_string())?,
                    created_at: bitbucket::parse_created_on(&detail_json)
                        .map_err(|e| e.to_string())?,
                    ..Default::default()
                };
                on_partial(detail.clone());
                detail.comments = joined(comments)??;
                detail.commits = joined(commits)??;
                // Bitbucket lists commits newest-first; flip to the oldest-first invariant.
                detail.commits.reverse();
                Ok(detail)
            })
        }
    }
}

/// A scoped fetch thread's value; a panic in it is reported, not propagated.
fn joined<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> Result<T, String> {
    handle
        .join()
        .map_err(|_| "detail fetch thread panicked".to_owned())
}

/// Walks a paginated Bitbucket collection from `first` to the last page.
fn collect_pages<T>(
    first: String,
    auth_header: &str,
    parse: fn(&str) -> serde_json::Result<Vec<T>>,
) -> Result<Vec<T>, String> {
    let mut items = Vec::new();
    let mut next = Some(first);
    while let Some(url) = next {
        let page = curl_body(&url, auth_header)?;
        items.extend(parse(&page).map_err(|e| e.to_string())?);
        next = bitbucket::next_page(&page);
    }
    Ok(items)
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

/// Reply to one existing PR comment thread (pull-requests.md §11): the thread
/// root's forge id and the reply body, posted off-thread like `PrPostRequest`.
#[derive(Debug, Clone)]
pub struct PrReplyRequest {
    pub key: PrReviewKey,
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub bitbucket_email: String,
    pub comment_id: u64,
    pub body: String,
}

/// Add or reply to a conversation-level comment (pull-requests.md §11): `parent` is
/// `None` for a new top-level comment, `Some(id)` to nest under one (Bitbucket only —
/// GitHub issue comments are flat and ignore it).
#[derive(Debug, Clone)]
pub struct PrConversationRequest {
    pub key: PrReviewKey,
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub bitbucket_email: String,
    pub parent: Option<u64>,
    pub body: String,
}

/// Resolve or reopen one existing review thread (pull-requests.md §11): `thread_id`
/// is the GitHub review-thread node id (`None` on Bitbucket), `comment_id` the thread
/// root's numeric id (Bitbucket's resolve handle), `resolved` the target state.
#[derive(Debug, Clone)]
pub struct PrResolveRequest {
    pub key: PrReviewKey,
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub bitbucket_email: String,
    pub thread_id: Option<String>,
    pub comment_id: u64,
    pub resolved: bool,
}

/// Merge one PR on its forge (pull-requests.md §5). Carries no strategy choice: the
/// plain merge commit is the one both forges agree on, and picking between squash and
/// rebase is a repository policy the cockpit does not own.
#[derive(Debug, Clone)]
pub struct PrMergeRequest {
    pub key: PrReviewKey,
    pub forge_kind: ForgeKind,
    pub repo_label: String,
    pub number: u64,
    pub bitbucket_email: String,
}

/// Which write the post runner carried, so the UI clears the review draft only on
/// a submitted review — a posted reply or conversation comment leaves it untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrPostKind {
    Review,
    Reply,
    Conversation,
    Resolve,
    Merge,
}

/// The single reply per post request, echoing the key for adoption.
#[derive(Debug, Clone)]
pub struct PrPostReply {
    pub key: PrReviewKey,
    pub kind: PrPostKind,
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
                kind: PrPostKind::Review,
                result,
            });
            on_event();
        });
    }

    pub fn request_merge(&self, request: PrMergeRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = post_merge(&request);
            let _ = tx.send(PrPostReply {
                key: request.key,
                kind: PrPostKind::Merge,
                result,
            });
            on_event();
        });
    }

    pub fn request_reply(&self, request: PrReplyRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = post_reply(&request);
            let _ = tx.send(PrPostReply {
                key: request.key,
                kind: PrPostKind::Reply,
                result,
            });
            on_event();
        });
    }

    pub fn request_conversation(&self, request: PrConversationRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = post_conversation(&request);
            let _ = tx.send(PrPostReply {
                key: request.key,
                kind: PrPostKind::Conversation,
                result,
            });
            on_event();
        });
    }

    pub fn request_resolve(&self, request: PrResolveRequest) {
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = post_resolve(&request);
            let _ = tx.send(PrPostReply {
                key: request.key,
                kind: PrPostKind::Resolve,
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

/// Post one reply to an existing comment thread (pull-requests.md §11). GitHub
/// replies on the review-comment endpoint (`…/comments/{id}/replies`); Bitbucket
/// posts to the PR comments collection with a `parent` id (the same URL that takes
/// a new comment), so the reply nests under the thread.
fn post_reply(req: &PrReplyRequest) -> Result<(), String> {
    match req.forge_kind {
        ForgeKind::GitHub => {
            if !command_ok("gh", &github::auth_status_args()) {
                return Err("Install gh and run `gh auth login`".to_owned());
            }
            let body = github::reply_comment_body(&req.body);
            run_stdin(
                "gh",
                &github::reply_comment_args(&req.repo_label, req.number, req.comment_id),
                &body,
            )
            .map(|_| ())
            .map_err(|e| {
                let e = e.trim();
                if e.is_empty() {
                    "gh reply failed".to_owned()
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
            let url = bitbucket::post_comment_url(workspace, repo, req.number);
            curl_post_ok(
                &url,
                &header,
                &bitbucket::reply_comment_body(req.comment_id, &req.body),
            )
        }
    }
}

/// Post a conversation-level comment (pull-requests.md §11). GitHub adds it on the
/// flat issue-comments endpoint (replies aren't threaded there); Bitbucket posts to
/// the PR comments collection, nesting under `parent` when the user replied to a card.
fn post_conversation(req: &PrConversationRequest) -> Result<(), String> {
    match req.forge_kind {
        ForgeKind::GitHub => {
            if !command_ok("gh", &github::auth_status_args()) {
                return Err("Install gh and run `gh auth login`".to_owned());
            }
            let body = github::reply_comment_body(&req.body);
            run_stdin(
                "gh",
                &github::issue_comment_args(&req.repo_label, req.number),
                &body,
            )
            .map(|_| ())
            .map_err(|e| {
                let e = e.trim();
                if e.is_empty() {
                    "gh comment failed".to_owned()
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
            let url = bitbucket::post_comment_url(workspace, repo, req.number);
            let payload = match req.parent {
                Some(parent) => bitbucket::reply_comment_body(parent, &req.body),
                None => bitbucket::summary_comment_body(&req.body),
            };
            curl_post_ok(&url, &header, &payload)
        }
    }
}

/// Resolve or reopen one review thread (pull-requests.md §11). GitHub toggles it
/// with a GraphQL mutation on the thread node id; Bitbucket POSTs to the thread's
/// `resolve` endpoint to resolve and DELETEs the same URL to reopen.
fn post_resolve(req: &PrResolveRequest) -> Result<(), String> {
    match req.forge_kind {
        ForgeKind::GitHub => {
            if !command_ok("gh", &github::auth_status_args()) {
                return Err("Install gh and run `gh auth login`".to_owned());
            }
            let thread_id = req
                .thread_id
                .as_deref()
                .ok_or_else(|| "missing review-thread id".to_owned())?;
            run_stdin(
                "gh",
                &github::resolve_thread_args(thread_id, req.resolved),
                "",
            )
            .map(|_| ())
            .map_err(|e| {
                let e = e.trim();
                if e.is_empty() {
                    "gh resolve failed".to_owned()
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
            let url = bitbucket::resolve_comment_url(workspace, repo, req.number, req.comment_id);
            if req.resolved {
                curl_post_ok(&url, &header, "")
            } else {
                curl_delete_ok(&url, &header)
            }
        }
    }
}

/// Merge the PR on its forge (pull-requests.md §5): `gh pr merge --merge` on GitHub,
/// `POST …/pullrequests/{id}/merge` on Bitbucket. Mirrors `post_resolve`'s auth and
/// error shaping so a missing CLI or token reads the same everywhere.
fn post_merge(req: &PrMergeRequest) -> Result<(), String> {
    match req.forge_kind {
        ForgeKind::GitHub => {
            if !command_ok("gh", &github::auth_status_args()) {
                return Err("Install gh and run `gh auth login`".to_owned());
            }
            // `gh pr merge` refuses on an unmergeable PR; its stderr is the useful
            // message, so route through the stdin runner that surfaces it.
            run_stdin("gh", &github::merge_args(&req.repo_label, req.number), "")
                .map(|_| ())
                .map_err(|e| {
                    let e = e.trim();
                    if e.is_empty() {
                        "gh merge failed".to_owned()
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
            let url = bitbucket::merge_url(workspace, repo, req.number);
            curl_post_ok(&url, &header, &bitbucket::merge_body())
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
    let mut github_name: Option<String> = None;
    let mut bitbucket_name: Option<String> = None;

    // GitHub — availability via `gh auth status`, identity via `gh api user`.
    if has_github {
        if !command_ok("gh", &github::auth_status_args()) {
            github = SourceStatus::Unavailable("Install gh and run `gh auth login`".to_owned());
        } else {
            if github_login.is_none() {
                github_login =
                    run_stdout("gh", &github::current_login_args()).map(|s| s.trim().to_owned());
                github_name = run_stdout("gh", &github::current_name_args())
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty());
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
                            bitbucket_uuid = bitbucket::parse_current_user(&json);
                            bitbucket_name = bitbucket::parse_current_user_display_name(&json);
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
        github_name,
        bitbucket_name,
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

/// `curl -X DELETE` reduced to `Result<(), message>` — the unresolve path, which
/// returns 204 (no body).
fn curl_delete_ok(url: &str, auth_header: &str) -> Result<(), String> {
    match run_curl(
        &[
            "-X".to_owned(),
            "DELETE".to_owned(),
            "-w".to_owned(),
            "\n%{http_code}".to_owned(),
            url.to_owned(),
        ],
        auth_header,
    ) {
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
    run_curl(&curl_post_args(url, body), auth_header)
}

/// curl options for a POST. The approve / request-changes / resolve endpoints
/// take no body: sending `-d ""` with a JSON content-type makes Bitbucket parse
/// `""` as JSON and reply 400, so an empty body posts as a bare POST with
/// neither the header nor `-d`.
fn curl_post_args(url: &str, body: &str) -> Vec<String> {
    let mut args = vec!["-X".to_owned(), "POST".to_owned()];
    if !body.is_empty() {
        args.push("-H".to_owned());
        args.push("Content-Type: application/json".to_owned());
        args.push("-d".to_owned());
        args.push(body.to_owned());
    }
    args.push("-w".to_owned());
    args.push("\n%{http_code}".to_owned());
    args.push(url.to_owned());
    args
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
        // 204 is the unresolve (DELETE) success — no body.
        "200" | "201" | "204" => CurlResult::Ok(body.to_owned()),
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

    /// A screenshot linked from a comment lives under the repo's downloads: fetched on
    /// the website host it answers 401 even with the credentials, so it is asked of the
    /// API instead (pull-requests.md §11).
    #[test]
    fn a_bitbucket_repo_download_is_fetched_through_the_api() {
        assert_eq!(
            bitbucket_download_api_url(
                "https://bitbucket.org/acme/web/downloads/step-06-final.png"
            )
            .as_deref(),
            Some("https://api.bitbucket.org/2.0/repositories/acme/web/downloads/step-06-final.png"),
        );
        // Anything else is fetched as written — including the signed link the API hands
        // back, and any host a body happens to name.
        assert_eq!(
            bitbucket_download_api_url("https://bitbucket.org/acme/web/src/main/logo.png"),
            None,
        );
        assert_eq!(
            bitbucket_download_api_url("https://bbuseruploads.s3.amazonaws.com/x/step.png"),
            None,
        );
        assert_eq!(
            bitbucket_download_api_url("https://example.test/a.png"),
            None
        );
    }

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
            source_commit: String::new(),
            dest_commit: String::new(),
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
            github_name: None,
            bitbucket_name: None,
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

    fn commit_file(
        repo: &git2::Repository,
        parent: Option<git2::Oid>,
        name: &str,
        content: &str,
    ) -> git2::Oid {
        commit_file_on(repo, Some("HEAD"), parent, name, content)
    }

    /// A commit HEAD does not move to — a branch diverging from the one HEAD is on.
    fn commit_file_detached(
        repo: &git2::Repository,
        parent: git2::Oid,
        name: &str,
        content: &str,
    ) -> git2::Oid {
        commit_file_on(repo, None, Some(parent), name, content)
    }

    fn commit_file_on(
        repo: &git2::Repository,
        update_ref: Option<&str>,
        parent: Option<git2::Oid>,
        name: &str,
        content: &str,
    ) -> git2::Oid {
        let root = repo.workdir().unwrap();
        std::fs::write(root.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let parents: Vec<git2::Commit> = parent
            .into_iter()
            .map(|oid| repo.find_commit(oid).unwrap())
            .collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(update_ref, &sig, &sig, name, &tree, &parent_refs)
            .unwrap()
    }

    #[test]
    fn load_commit_files_isolates_a_single_commit_from_the_cumulative_range() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let c1 = commit_file(&repo, None, "a.txt", "1\n");
        let c2 = commit_file(&repo, Some(c1), "a.txt", "1\n2\n");
        let c3 = commit_file(&repo, Some(c2), "b.txt", "new\n");

        // A single commit diffs against its parent: c3 only added b.txt.
        let single = load_commit_files(tmp.path(), CommitRange::Commit(c3)).unwrap();
        assert_eq!(single.base, c2);
        assert_eq!(single.head, c3);
        let single_paths: Vec<&str> = single.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(single_paths, vec!["b.txt"]);

        // The explicit c1..c3 range is cumulative: both files changed.
        let range =
            load_commit_files(tmp.path(), CommitRange::Range { base: c1, head: c3 }).unwrap();
        let mut range_paths: Vec<&str> = range.files.iter().map(|f| f.path.as_str()).collect();
        range_paths.sort_unstable();
        assert_eq!(range_paths, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn load_commit_files_root_commit_yields_empty_delta() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let root_commit = commit_file(&repo, None, "a.txt", "1\n");
        let loaded = load_commit_files(tmp.path(), CommitRange::Commit(root_commit)).unwrap();
        assert_eq!(loaded.base, root_commit);
        assert!(loaded.files.is_empty());
    }

    fn files_request(root: &Path, source_commit: &str, dest_commit: &str) -> PrFilesRequest {
        PrFilesRequest {
            key: PrReviewKey {
                forge_kind: ForgeKind::GitHub,
                repo_label: "acme/webapp".to_owned(),
                number: 1,
            },
            root: root.to_path_buf(),
            forge_kind: ForgeKind::GitHub,
            number: 1,
            source_branch: "feature".to_owned(),
            dest_branch: "main".to_owned(),
            source_commit: source_commit.to_owned(),
            dest_commit: dest_commit.to_owned(),
        }
    }

    /// Both listed tips already in the repo ⇒ the files come back with no network:
    /// the fixture has no `origin`, so any fetch attempt would fail the load. The
    /// range is the three-dot one — the destination's own drift (a.txt) stays out.
    #[test]
    fn load_pr_files_serves_listed_tips_without_fetching() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let fork = commit_file(&repo, None, "a.txt", "1\n");
        let head = commit_file(&repo, Some(fork), "b.txt", "new\n");
        let dest = commit_file_detached(&repo, fork, "a.txt", "1\n2\n");

        // Bitbucket lists abbreviated hashes; GitHub full ones — both resolve.
        let short_head = &head.to_string()[..12];
        let loaded =
            load_pr_files(&files_request(tmp.path(), short_head, &dest.to_string())).unwrap();
        assert_eq!(loaded.head, head);
        assert_eq!(loaded.base, fork);
        let paths: Vec<&str> = loaded.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["b.txt"]);
    }

    /// A tip the repo does not hold (or none listed) is the fetch path — which this
    /// origin-less fixture cannot serve, so the load reports the fetch failure
    /// rather than answering from stale local refs.
    #[test]
    fn load_pr_files_fetches_when_a_listed_tip_is_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let only = commit_file(&repo, None, "a.txt", "1\n");
        let unknown = "0".repeat(40);
        assert!(load_pr_files(&files_request(tmp.path(), &unknown, &only.to_string())).is_err());
        assert!(load_pr_files(&files_request(tmp.path(), "", "")).is_err());
    }

    fn diff_job(path: &str) -> PrFileDiffRequest {
        PrFileDiffRequest {
            key: PrReviewKey {
                forge_kind: ForgeKind::GitHub,
                repo_label: "acme/webapp".to_owned(),
                number: 1,
            },
            root: PathBuf::new(),
            base: git2::Oid::ZERO_SHA1,
            head: git2::Oid::ZERO_SHA1,
            path: path.to_owned(),
        }
    }

    /// The pool serves the newest batch first, each batch in its own order — the PR
    /// just opened ahead of the one just left, the selected file ahead of its column.
    #[test]
    fn diff_queue_serves_the_newest_batch_first_in_order() {
        let queue = DiffQueue::default();
        queue.push_front(vec![diff_job("old/1"), diff_job("old/2")]);
        queue.push_front(vec![diff_job("new/1"), diff_job("new/2")]);
        let served: Vec<String> = std::iter::from_fn(|| queue.pop_front().map(|j| j.path))
            .take(4)
            .collect();
        assert_eq!(served, vec!["new/1", "new/2", "old/1", "old/2"]);
        queue.close();
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn empty_body_post_omits_content_type_and_data() {
        let args = curl_post_args("https://api.bitbucket.org/x/approve", "");
        assert!(!args.iter().any(|a| a == "-d"));
        assert!(!args.iter().any(|a| a == "Content-Type: application/json"));
        assert_eq!(args.first().map(String::as_str), Some("-X"));
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://api.bitbucket.org/x/approve")
        );
    }

    #[test]
    fn json_body_post_sends_content_type_and_data() {
        let args = curl_post_args("https://api.bitbucket.org/x/comments", "{\"a\":1}");
        assert!(args.iter().any(|a| a == "Content-Type: application/json"));
        assert!(args.iter().any(|a| a == "{\"a\":1}"));
    }
}
