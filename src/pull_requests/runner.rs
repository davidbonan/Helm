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
use crate::pull_requests::model::{dedupe, ForgeKind, PullRequest};
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
    /// A Bitbucket REST GET; the Basic-auth header is added at execution.
    Bitbucket { repo_label: String, url: String },
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
    pub pull_requests: Vec<PullRequest>,
    pub github: SourceStatus,
    pub bitbucket: SourceStatus,
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
}

impl PrCache {
    pub fn apply(&mut self, reply: PrReply) {
        self.pull_requests = reply.pull_requests;
        self.github = reply.github;
        self.bitbucket = reply.bitbucket;
        self.loaded = true;
    }
}

/// The per-forge list queries, in sidebar order. GitHub needs two searches
/// (`author:@me`, `review-requested:@me`); Bitbucket one list call, only when
/// configured. Pure — the worker runs whatever this returns.
pub fn plan(forges: &[(Forge, String)], bitbucket_configured: bool) -> Vec<PrQuery> {
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
                if bitbucket_configured {
                    queries.push(PrQuery::Bitbucket {
                        repo_label: label.clone(),
                        url: bitbucket::pull_requests_url(workspace, repo),
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

/// The worker body: resolve forges, query each source, classify + dedupe.
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
    let bitbucket_configured = has_bitbucket && !request.bitbucket_email.is_empty();

    let mut pull_requests = Vec::new();
    let mut github = SourceStatus::Absent;
    let mut bitbucket = SourceStatus::Absent;

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
                    github = SourceStatus::Ok;
                    for query in plan(&forges, bitbucket_configured) {
                        if let PrQuery::Gh { repo_label, args } = query {
                            if let Some(json) = run_stdout("gh", &args) {
                                if let Ok(mut prs) = github::parse_list(&json, login, &repo_label) {
                                    pull_requests.append(&mut prs);
                                }
                            }
                        }
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
                        CurlResult::Failed => {
                            bitbucket =
                                SourceStatus::Unavailable("Bitbucket unreachable".to_owned())
                        }
                    }
                }
                if let Some(uuid) = &bitbucket_uuid {
                    bitbucket = SourceStatus::Ok;
                    for query in plan(&forges, true) {
                        if let PrQuery::Bitbucket { repo_label, url } = query {
                            match curl_get(&url, &header) {
                                CurlResult::Ok(json) => {
                                    if let Ok(mut prs) =
                                        bitbucket::parse_list(&json, uuid, &repo_label)
                                    {
                                        pull_requests.append(&mut prs);
                                    }
                                }
                                CurlResult::Unauthorized => {
                                    bitbucket = SourceStatus::Unavailable(
                                        "Bitbucket token invalid or expired".to_owned(),
                                    )
                                }
                                CurlResult::Failed => {
                                    bitbucket = SourceStatus::Unavailable(
                                        "Bitbucket unreachable".to_owned(),
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    PrReply {
        pull_requests: dedupe(pull_requests),
        github,
        bitbucket,
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

enum CurlResult {
    Ok(String),
    Unauthorized,
    Failed,
}

/// `curl` the URL with a Basic-auth header, splitting the trailing `%{http_code}`
/// the `update.rs` idiom uses to tell 200 / 401 / other apart.
fn curl_get(url: &str, auth_header: &str) -> CurlResult {
    let args = [
        "-s".to_owned(),
        "-H".to_owned(),
        format!("Authorization: {auth_header}"),
        "-w".to_owned(),
        "\n%{http_code}".to_owned(),
        url.to_owned(),
    ];
    let Ok(out) = Command::new("curl").args(args).output() else {
        return CurlResult::Failed;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    match code.trim() {
        "200" => CurlResult::Ok(body.to_owned()),
        "401" => CurlResult::Unauthorized,
        _ => CurlResult::Failed,
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
        let queries = plan(&[github("acme/web")], false);
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
    fn plan_emits_bitbucket_list_only_when_configured() {
        let forges = [bitbucket("team/repo")];
        assert!(plan(&forges, false).is_empty());
        assert_eq!(
            plan(&forges, true),
            vec![PrQuery::Bitbucket {
                repo_label: "team/repo".to_owned(),
                url: bitbucket::pull_requests_url("team", "repo"),
            }]
        );
    }

    #[test]
    fn plan_mixes_sources_in_order() {
        let queries = plan(&[github("acme/web"), bitbucket("team/repo")], true);
        assert_eq!(queries.len(), 3);
        assert!(matches!(queries[0], PrQuery::Gh { .. }));
        assert!(matches!(queries[1], PrQuery::Gh { .. }));
        assert!(matches!(
            &queries[2],
            PrQuery::Bitbucket { url, .. }
            if url == "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests?state=OPEN&pagelen=50"
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
}
