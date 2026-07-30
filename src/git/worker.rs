use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use crate::git::branch::{self, Branch};
use crate::git::commit_detail::{self, CommitDetail};
use crate::git::conflict::{self, ConflictFile};
use crate::git::diff::{self, DiffSource, FileDiff};
use crate::git::edit::{self, EditError, EditRequest, Landing};
use crate::git::graph::{self, Graph};
use crate::git::rebase::{self, RebaseCommit, RebaseStep};
use crate::git::status::{load_repo, op_in_progress, op_summary, OpSummary, RepoStatus};
use crate::git::sync::{self, PullMode, SyncError, SyncOutcome};
use crate::git::{commit, discard, stage, stash, tag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitCommand {
    Status,
    Stage(String),
    Unstage(String),
    StageAll,
    UnstageAll,
    Discard(String),
    DiscardAll,
    Commit(String),
    /// Amends **HEAD**'s message (git.md §5, commit-detail reword): a message-only
    /// amend — tree and author preserved, committer refreshed. Replies with a
    /// status snapshot like any mutation; the app reloads the graph and re-selects
    /// the moved HEAD.
    AmendMessage(String),
    /// Computes a file's diff for the overlay view (M6-3). `staged` selects the
    /// source: `false` ⇒ Unstaged (WT vs index), `true` ⇒ Staged (index vs HEAD).
    Diff {
        path: String,
        staged: bool,
    },
    StageHunk {
        path: String,
        hunk: usize,
    },
    UnstageHunk {
        path: String,
        hunk: usize,
    },
    StageLines {
        path: String,
        hunk: usize,
        lines: Vec<usize>,
    },
    UnstageLines {
        path: String,
        hunk: usize,
        lines: Vec<usize>,
    },
    /// Discards hunk `hunk` of an **unstaged** file (git.md §4): reverts that
    /// hunk's working-tree change to the index content. Destructive — the app
    /// confirms it first.
    DiscardHunk {
        path: String,
        hunk: usize,
    },
    /// Loads the commit graph (M9-1) off the UI thread. `limit` bounds the
    /// pagination (0 ⇒ module default).
    Graph {
        limit: usize,
    },
    /// Lists the commits `onto..HEAD` for the interactive-rebase page (git.md
    /// §9) — the page opens with a loader and fills on the reply.
    RebaseTodo {
        onto: String,
    },
    /// Loads a commit's detail (M9-2): metadata + files changed vs first parent.
    CommitDetail(git2::Oid),
    /// Loads the **read-only** diff of a file at the commit vs its first parent (M9-2/M6-1).
    CommitFileDiff {
        oid: git2::Oid,
        path: String,
    },
    /// Reads every conflicted file from the index merge stages for the conflict
    /// editor's file rail (conflicts.md §6/§8) — like `Diff`, a read that returns
    /// its own payload. One reply for the whole rail (the per-kind staleness gate
    /// would otherwise drop all but the newest of N per-file reads).
    ReadConflicts,
    /// Writes the conflict editor's resolution (conflicts.md §2/§8): `content`
    /// replaces the file and clears its merge stages; `None` is a delete
    /// resolution. Mutates the index, so it replies with a status snapshot.
    ResolveFile {
        path: String,
        content: Option<String>,
    },
    /// Resolves a binary / oversize conflict by taking one whole side from the
    /// index (conflicts.md §5). Mutates the index, so it replies with a snapshot.
    ResolveFileSide {
        path: String,
        ours: bool,
    },
    /// Checks out a branch from a graph chip — local, or remote via DWIM (local
    /// namesake, created and tracked as needed); dirty working tree ⇒ automatic
    /// stash first (untracked included).
    Checkout(String),
    /// Stashes the entire working tree (untracked included) (M12-4).
    Stash,
    /// Stashes only the given paths — both staged and unstaged changes of each,
    /// untracked included (WIP sidebar context menu, git.md §3). One git
    /// invocation so the selection lands in a **single** stash.
    StashFiles(Vec<String>),
    /// Applies then drops `stash@{0}`; conflict ⇒ stash kept + error.
    StashPop,
    /// Applies then drops the stash whose **stash commit** is the oid (graph
    /// stash row menu); same conflict rule as `StashPop`.
    StashPopAt(git2::Oid),
    /// Applies the stash whose **stash commit** is the oid **without dropping it**
    /// (graph stash row menu, git.md §9): the no-drop twin of `StashPopAt`, the
    /// stash stays either way.
    StashApplyAt(git2::Oid),
    /// Drops the stash whose **stash commit** is the oid (graph stash row menu,
    /// confirmed by a modal on the app side).
    StashDropAt(git2::Oid),
    /// Creates the branch on HEAD and checks it out (M12-5, Branch popover).
    CreateBranch(String),
    /// Creates a local branch at the commit pointed to by `at` (graph chip
    /// context menu, git.md §9) **without** checking it out — HEAD is untouched.
    /// `at` is the source ref, fully qualified on the app side.
    CreateBranchAt {
        name: String,
        at: String,
    },
    /// Creates a **lightweight** tag `name` on the commit `at` (graph row menu,
    /// git.md §9) — no checkout, no push. Duplicate/invalid name ⇒ clean failure.
    CreateTagAt {
        name: String,
        at: git2::Oid,
    },
    /// Deletes the **local** branch (graph context menu, confirmed by a modal);
    /// current branch or checked out elsewhere ⇒ clean failure.
    DeleteBranch(String),
    /// Renames the **local** branch `from` to `to` (graph chip menu, git.md §9):
    /// `git branch -m` semantics — HEAD follows when it is the current branch,
    /// upstream config moves with it; duplicate/invalid ⇒ clean failure (never
    /// a forced overwrite).
    RenameBranch {
        from: String,
        to: String,
    },
    /// **Detached** checkout on a tag's commit (graph tag menu, git.md §9): same
    /// automatic stash as the branch checkout, menu-only.
    CheckoutTag(String),
    /// Deletes the **local** tag (graph tag menu, confirmed by a modal). The
    /// remote-side deletion, if asked, runs first on the sync runner.
    DeleteTag(String),
    /// Writes one inline-editor buffer back to the working tree (git.md §4). The
    /// request anchors the edit in the file's own line numbering: the write only
    /// happens while those lines still read exactly as they were opened, so a file
    /// that moved on disk under the editor is refused, not overwritten.
    EditFile(EditRequest),
    /// Moves the current branch to `target` (graph row menu, git.md §9): Soft /
    /// Mixed run directly, Hard is confirmed by a modal on the app side. Local
    /// `git2` reset, no network; detached HEAD is gated out in the UI.
    Reset {
        target: git2::Oid,
        mode: git2::ResetType,
    },
}

impl GitCommand {
    /// `true` for commands that write (index, worktree, refs): they drive the
    /// busy state of the graph toolbar
    /// (D-2026-06-03-toolbar-loader-commandes-git).
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            GitCommand::Status
                | GitCommand::Diff { .. }
                | GitCommand::Graph { .. }
                | GitCommand::RebaseTodo { .. }
                | GitCommand::CommitDetail(_)
                | GitCommand::CommitFileDiff { .. }
                | GitCommand::ReadConflicts
        )
    }

    /// `true` for commit-addressed reads (detail / file diff of an immutable
    /// commit): they answer a click and no queued command can change their
    /// result, so the worker serves them **before** the queued poll reads
    /// (status/graph) — the sidebar must not wait for a reload backlog.
    /// `RebaseTodo` also jumps: it answers a click and the rebase page must
    /// open instantly even behind a slow graph reload; if an overtaken
    /// mutation moves HEAD, the plan is stale — the execution re-derives and
    /// refuses (`sync::interactive_rebase`), never a silently wrong todo.
    pub fn jumps_queue(&self) -> bool {
        matches!(
            self,
            GitCommand::CommitDetail(_)
                | GitCommand::CommitFileDiff { .. }
                | GitCommand::RebaseTodo { .. }
        )
    }

    /// `true` for the two refresh reads (poll cadence, reload behind a
    /// mutation, manual Refresh): they only update views already on screen, so
    /// a click's working-tree diff may overtake them (`next_index`) — they
    /// change nothing it reads.
    pub fn refresh_read(&self) -> bool {
        matches!(self, GitCommand::Status | GitCommand::Graph { .. })
    }

    /// Reply variant this command resolves to — the slot its generation stamps
    /// (M17-13).
    pub fn result_kind(&self) -> ResultKind {
        match self {
            GitCommand::EditFile(_) => ResultKind::Edit,
            GitCommand::Diff { .. } => ResultKind::Diff,
            GitCommand::Graph { .. } => ResultKind::Graph,
            GitCommand::RebaseTodo { .. } => ResultKind::RebaseTodo,
            GitCommand::CommitDetail(_) => ResultKind::CommitDetail,
            GitCommand::CommitFileDiff { .. } => ResultKind::CommitFileDiff,
            GitCommand::ReadConflicts => ResultKind::Conflicts,
            _ => ResultKind::Status,
        }
    }
}

/// One slot per `GitResult` variant: the staleness gate (M17-13) compares a
/// reply's generation against the **latest request of the same kind** — a
/// status burst never invalidates an in-flight graph, and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    Status,
    Diff,
    Graph,
    RebaseTodo,
    CommitDetail,
    CommitFileDiff,
    Conflicts,
    Edit,
}

const RESULT_KINDS: usize = 8;

#[derive(Debug)]
pub enum GitResult {
    /// Reply for status and mutating commands. `source` identifies the original
    /// command: the app routes a failure to its surface (inline error in the
    /// Branch popover for `CreateBranch`, contextual toast otherwise).
    Status {
        source: GitCommand,
        result: Result<RepoSnapshot, git2::Error>,
    },
    Diff(Result<FileDiff, git2::Error>),
    /// Echoes the requested `limit` (data role only since M17-13: staleness
    /// rides the generation tag).
    Graph {
        limit: usize,
        result: Result<Graph, git2::Error>,
    },
    /// Carries the requested `onto`: the rebase page adopts only the reply
    /// that targets its own onto (a reopened page never inherits the plan of
    /// the previous target).
    RebaseTodo {
        onto: String,
        result: Result<Vec<RebaseCommit>, git2::Error>,
    },
    CommitDetail(Result<CommitDetail, git2::Error>),
    /// Carries the requested `oid`: the open diff adopts only the reply that
    /// targets its commit (and its path).
    CommitFileDiff {
        oid: git2::Oid,
        result: Result<FileDiff, git2::Error>,
    },
    /// Every conflicted file of the index — the editor's file rail
    /// (conflicts.md §8), adopted by the open editor while it is loading.
    Conflicts {
        result: Result<Vec<ConflictFile>, git2::Error>,
    },
    /// Outcome of an inline-editor save (git.md §4). Typed rather than folded into a
    /// `Status` failure: the open editor arbitrates on it — `Diverged` raises the
    /// divergence notice, a `NotStaged` landing is a toast over a *successful* save.
    /// Carries no snapshot; the app re-reads status and diff behind it.
    Edit {
        request: EditRequest,
        result: Result<Landing, EditError>,
    },
}

impl GitResult {
    pub fn kind(&self) -> ResultKind {
        match self {
            GitResult::Status { .. } => ResultKind::Status,
            GitResult::Diff(_) => ResultKind::Diff,
            GitResult::Graph { .. } => ResultKind::Graph,
            GitResult::RebaseTodo { .. } => ResultKind::RebaseTodo,
            GitResult::CommitDetail(_) => ResultKind::CommitDetail,
            GitResult::CommitFileDiff { .. } => ResultKind::CommitFileDiff,
            GitResult::Conflicts { .. } => ResultKind::Conflicts,
            GitResult::Edit { .. } => ResultKind::Edit,
        }
    }

    /// `true` when the payload is adoptable state (`Ok`): only those go through
    /// the staleness gate. Errors always surface — they report on the command
    /// (toast, inline editor error), not on superseded state. Exception:
    /// a `RebaseTodo` error IS state (the page's clean error screen), so it is
    /// gated too — a stale failure must not clobber a page reopened on the
    /// same target while the fresh reply is in flight.
    pub fn carries_state(&self) -> bool {
        match self {
            GitResult::Status { result, .. } => result.is_ok(),
            GitResult::Diff(result) => result.is_ok(),
            GitResult::Graph { result, .. } => result.is_ok(),
            GitResult::RebaseTodo { .. } => true,
            GitResult::CommitDetail(result) => result.is_ok(),
            GitResult::CommitFileDiff { result, .. } => result.is_ok(),
            GitResult::Conflicts { result } => result.is_ok(),
            // Never state: a save's outcome reports on the command that ran it, and
            // must reach the editor even with a newer flush already in flight.
            GitResult::Edit { .. } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSnapshot {
    pub status: RepoStatus,
    pub branch: Branch,
    /// Stash count (M12-4): enables/disables **Pop** in the toolbar.
    pub stash_count: usize,
    /// At least one remote configured (M12-6): without a remote, Pull/Push are grayed out.
    pub has_remote: bool,
    /// Remote name of the current branch's upstream, if any (git.md §10): drives
    /// the force-push gating (greyed without it) and names the remote in its
    /// confirmation modal.
    pub upstream_remote: Option<String>,
    /// Tip of the remote-tracking ref a force push would overwrite
    /// (`refs/remotes/<remote>/<branch>`, git.md §10): the oid helm displays, and
    /// the lease the confirmation modal is armed on. `None` ⇒ nothing recorded to
    /// overwrite, force push greyed out.
    pub upstream_oid: Option<git2::Oid>,
    /// Merge / rebase in progress (M12-8): banner in the status sidebar.
    pub op_in_progress: bool,
    /// One-line summary of that op (conflicts.md §2): verb + best-effort
    /// source/target branch names for the conflict panel header. `None` outside
    /// merge/rebase/cherry-pick/revert.
    pub op: Option<OpSummary>,
    /// Cloud forge behind `origin` (git.md §9), `None` when there is no `origin`
    /// or its host is unrecognized: gates the **Create pull request** graph entry
    /// and carries the workspace/owner + repo that builds its URL.
    pub pr_remote: Option<crate::git::forge::Forge>,
}

#[derive(Clone, Default)]
pub struct MutationLock {
    locked: Arc<AtomicBool>,
}

impl MutationLock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only peek, never acquires: the background fetch (which runs
    /// lock-free) uses it to defer to an in-flight mutation without racing a
    /// user op for the lock.
    pub(crate) fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Acquire)
    }

    pub(crate) fn try_acquire(&self) -> Option<MutationGuard> {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| MutationGuard {
                locked: Arc::clone(&self.locked),
            })
    }
}

pub(crate) struct MutationGuard {
    locked: Arc<AtomicBool>,
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub struct GitWorker {
    commands: Option<Sender<(u64, GitCommand)>>,
    results: Receiver<(u64, GitResult)>,
    handle: Option<JoinHandle<()>>,
    /// Abandoned session (repo switch): the thread skips the remaining reads in
    /// its queue — no one will read their results anymore.
    cancelled: Arc<AtomicBool>,
    /// Sent commands whose reply hasn't been drained yet (the worker replies
    /// exactly once per command, in order **per kind** — commit-addressed reads
    /// jump the queue): lets us know whether a mutation is in progress without
    /// a dedicated signal from the thread.
    in_flight: Mutex<VecDeque<GitCommand>>,
    /// Generation tag (M17-13): every send stamps the next value; the thread
    /// echoes it on the reply.
    generation: AtomicU64,
    /// Latest generation sent, per result kind — the reference the staleness
    /// gate compares replies against.
    latest: [AtomicU64; RESULT_KINDS],
}

impl GitWorker {
    pub fn spawn(repo_path: &Path, on_event: impl Fn() + Send + Sync + 'static) -> Self {
        Self::spawn_with_lock(repo_path, MutationLock::new(), on_event)
    }

    pub fn spawn_with_lock(
        repo_path: &Path,
        mutation_lock: MutationLock,
        on_event: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let repo_path = repo_path.to_path_buf();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<(u64, GitCommand)>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<(u64, GitResult)>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let handle = std::thread::spawn(move || {
            run(&repo_path, cmd_rx, res_tx, on_event, &flag, mutation_lock)
        });
        Self {
            commands: Some(cmd_tx),
            results: res_rx,
            handle: Some(handle),
            cancelled,
            in_flight: Mutex::new(VecDeque::new()),
            generation: AtomicU64::new(0),
            latest: Default::default(),
        }
    }

    pub fn send(&self, command: GitCommand) {
        if let Some(commands) = &self.commands {
            let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
            let kind = command.result_kind();
            let queued = command.clone();
            if commands.send((generation, command)).is_ok() {
                self.latest[kind as usize].store(generation, Ordering::Relaxed);
                self.in_flight.lock().unwrap().push_back(queued);
            }
        }
    }

    /// `true` when a newer request of the same kind was sent after the one this
    /// reply answers: its payload is already outdated, the fresher reply is in
    /// flight (M17-13).
    pub fn superseded(&self, generation: u64, kind: ResultKind) -> bool {
        generation < self.latest[kind as usize].load(Ordering::Relaxed)
    }

    pub fn has_pending(&self, kind: ResultKind) -> bool {
        self.in_flight
            .lock()
            .unwrap()
            .iter()
            .any(|command| command.result_kind() == kind)
    }

    pub fn try_recv(&self) -> Option<(u64, GitResult)> {
        let result = self.results.try_recv().ok();
        if let Some((_, reply)) = &result {
            self.settle(reply.kind());
        }
        result
    }

    pub fn recv(&self) -> Option<(u64, GitResult)> {
        let result = self.results.recv().ok();
        if let Some((_, reply)) = &result {
            self.settle(reply.kind());
        }
        result
    }

    /// Drops the in-flight command this reply answers: the oldest of its kind —
    /// replies are FIFO per kind, but commit-addressed reads overtake the other
    /// kinds (`jumps_queue`), so a blind front pop would unpair the queue.
    fn settle(&self, kind: ResultKind) {
        let mut in_flight = self.in_flight.lock().unwrap();
        if let Some(index) = in_flight
            .iter()
            .position(|command| command.result_kind() == kind)
        {
            in_flight.remove(index);
        }
    }

    /// `true` while a `Commit` awaits its reply: the commit button shows a
    /// spinner and ignores clicks — a double-click (or repeated ⌘Enter) must
    /// not enqueue a second commit that would fail with "nothing staged".
    pub fn has_pending_commit(&self) -> bool {
        self.in_flight
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, GitCommand::Commit(_)))
    }

    /// First **mutating** command awaiting a reply, if any: the graph toolbar
    /// shows a loader and grays out all its buttons meanwhile
    /// (D-2026-06-03-toolbar-loader-commandes-git). Reads (status, diff, graph)
    /// don't count — the poll doesn't make the toolbar flicker.
    pub fn pending_mutation(&self) -> Option<GitCommand> {
        self.in_flight
            .lock()
            .unwrap()
            .iter()
            .find(|command| command.mutates())
            .cloned()
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        // Only join if a mutation is at stake (it must complete — write safety):
        // joining unconditionally blocked the UI thread on a repo switch while
        // the worker drained the poll backlog (status + graph of the left repo).
        self.cancelled.store(true, Ordering::Relaxed);
        let pending_mutation = self.pending_mutation().is_some();
        self.commands = None;
        if let Some(handle) = self.handle.take() {
            if pending_mutation {
                let _ = handle.join();
            }
        }
    }
}

fn run(
    repo_path: &Path,
    commands: Receiver<(u64, GitCommand)>,
    results: Sender<(u64, GitResult)>,
    on_event: impl Fn(),
    cancelled: &AtomicBool,
    mutation_lock: MutationLock,
) {
    let repo = git2::Repository::open(repo_path);
    let mut queue: VecDeque<(u64, GitCommand)> = VecDeque::new();
    loop {
        if queue.is_empty() {
            match commands.recv() {
                Ok(message) => queue.push_back(message),
                Err(_) => break,
            }
        }
        while let Ok(message) = commands.try_recv() {
            queue.push_back(message);
        }
        let Some((generation, command)) = queue.remove(next_index(&queue)) else {
            continue;
        };
        // Abandoned session: queued reads are skipped, but an already-requested
        // mutation still applies (a Stage clicked just before the switch isn't
        // lost) — without a snapshot or reply.
        if cancelled.load(Ordering::Relaxed) {
            if command.mutates() {
                if let Ok(repo) = repo.as_ref() {
                    let _ = mutate_with_lock(repo, &command, &mutation_lock);
                }
            }
            continue;
        }
        let repo = repo
            .as_ref()
            .map_err(|err| git2::Error::from_str(err.message()));
        let result = dispatch(repo, &command, &mutation_lock);
        if results.send((generation, result)).is_err() {
            break;
        }
        on_event();
    }
}

/// Position of the next command to serve: the first commit-addressed read if
/// any (`jumps_queue` — it answers a click and must not wait for the poll
/// backlog), else a working-tree `Diff` that only refresh reads separate from
/// the front (see below), else the queue's front. Commands of the **same kind**
/// never overtake each other: per-kind FIFO holds, the staleness gate (M17-13)
/// and the per-kind `in_flight` settlement stay exact.
///
/// The `Diff` case answers a click too, but its result depends on the index and
/// the worktree: it may only overtake `refresh_read` commands (status/graph),
/// never a queued mutation. Without it a file opened from the sidebar waits for
/// the poll backlog it landed behind — a status snapshot plus a graph reload,
/// 150–450 ms on a large repo.
fn next_index(queue: &VecDeque<(u64, GitCommand)>) -> usize {
    if let Some(index) = queue.iter().position(|(_, command)| command.jumps_queue()) {
        return index;
    }
    queue
        .iter()
        .position(|(_, command)| matches!(command, GitCommand::Diff { .. }))
        .filter(|index| {
            queue
                .iter()
                .take(*index)
                .all(|(_, command)| command.refresh_read())
        })
        .unwrap_or(0)
}

/// Read commands (`Diff`, `Graph`, `CommitDetail`, `CommitFileDiff`) reply with
/// their own variant without touching the index; any other command mutates the
/// index then replies with a status snapshot (refresh). A repo-open failure
/// follows the same command→variant routing (a single match).
fn dispatch(
    repo: Result<&git2::Repository, git2::Error>,
    command: &GitCommand,
    mutation_lock: &MutationLock,
) -> GitResult {
    match command {
        GitCommand::Diff { path, staged } => {
            let source = if *staged {
                DiffSource::Staged
            } else {
                DiffSource::Unstaged
            };
            GitResult::Diff(repo.and_then(|repo| diff::file_diff(repo, path, source)))
        }
        GitCommand::Graph { limit } => GitResult::Graph {
            limit: *limit,
            result: repo.and_then(|repo| graph::load_repo(repo, *limit)),
        },
        GitCommand::RebaseTodo { onto } => GitResult::RebaseTodo {
            onto: onto.clone(),
            result: repo.and_then(|repo| rebase::rebase_commits(repo, onto)),
        },
        GitCommand::CommitDetail(oid) => {
            GitResult::CommitDetail(repo.and_then(|repo| commit_detail::load_repo(repo, *oid)))
        }
        GitCommand::CommitFileDiff { oid, path } => GitResult::CommitFileDiff {
            oid: *oid,
            result: repo.and_then(|repo| diff::commit_file_diff(repo, *oid, path)),
        },
        GitCommand::ReadConflicts => GitResult::Conflicts {
            result: repo.and_then(conflict::read_conflicts),
        },
        // The only mutation answered by its own variant: the editor needs the typed
        // outcome, not a snapshot (`GitResult::Edit`). It still takes the mutation
        // lock — hence a git failure mapped into `EditError`, whose `Io` prints the
        // message verbatim.
        GitCommand::EditFile(request) => GitResult::Edit {
            request: request.clone(),
            result: repo
                .and_then(|repo| mutation_guard(command, mutation_lock).map(|guard| (repo, guard)))
                .map_err(|err| EditError::Io(err.message().to_owned()))
                .and_then(|(repo, _guard)| edit::flush(repo, request)),
        },
        _ => GitResult::Status {
            source: command.clone(),
            result: repo.and_then(|repo| apply(repo, command, mutation_lock)),
        },
    }
}

fn apply(
    repo: &git2::Repository,
    command: &GitCommand,
    mutation_lock: &MutationLock,
) -> Result<RepoSnapshot, git2::Error> {
    let _guard = mutation_guard(command, mutation_lock)?;
    mutate(repo, command)?;
    snapshot(repo)
}

fn mutate_with_lock(
    repo: &git2::Repository,
    command: &GitCommand,
    mutation_lock: &MutationLock,
) -> Result<(), git2::Error> {
    let _guard = mutation_guard(command, mutation_lock)?;
    // The inline save mutates like the rest, but it answers with its own reply variant
    // and therefore has its own executor: `mutate` refuses it outright. Reached on the
    // abandoned-session path, where the buffer flushed by a repo switch must still land.
    if let GitCommand::EditFile(request) = command {
        return edit::flush(repo, request)
            .map(|_| ())
            .map_err(|err| git2::Error::from_str(&err.to_string()));
    }
    mutate(repo, command)
}

fn mutation_guard(
    command: &GitCommand,
    mutation_lock: &MutationLock,
) -> Result<Option<MutationGuard>, git2::Error> {
    if command.mutates() {
        mutation_lock
            .try_acquire()
            .map(Some)
            .ok_or_else(|| git2::Error::from_str("another Git operation is in progress"))
    } else {
        Ok(None)
    }
}

fn mutate(repo: &git2::Repository, command: &GitCommand) -> Result<(), git2::Error> {
    match command {
        GitCommand::Status => {}
        GitCommand::Stage(path) => stage::stage(repo, path)?,
        GitCommand::Unstage(path) => stage::unstage(repo, path)?,
        GitCommand::StageAll => stage::stage_all(repo)?,
        GitCommand::UnstageAll => stage::unstage_all(repo)?,
        GitCommand::Discard(path) => discard::discard_file(repo, path)?,
        GitCommand::DiscardAll => discard::discard_all(repo)?,
        GitCommand::Commit(message) => {
            commit::commit(repo, message)?;
        }
        GitCommand::AmendMessage(message) => {
            commit::amend_message(repo, message)?;
        }
        GitCommand::Checkout(name) => branch::checkout(repo, name)?,
        GitCommand::Stash => stash::stash(repo)?,
        GitCommand::StashFiles(paths) => stash::stash_paths(repo, paths)?,
        GitCommand::StashPop => stash::pop(repo)?,
        GitCommand::StashPopAt(oid) => stash::pop_at(repo, *oid)?,
        GitCommand::StashApplyAt(oid) => stash::apply_at(repo, *oid)?,
        GitCommand::StashDropAt(oid) => stash::drop_at(repo, *oid)?,
        GitCommand::CreateBranch(name) => branch::create_and_checkout(repo, name)?,
        GitCommand::CreateBranchAt { name, at } => branch::create_at(repo, name, at)?,
        GitCommand::CreateTagAt { name, at } => tag::create_lightweight(repo, name, *at)?,
        GitCommand::DeleteBranch(name) => branch::delete_local(repo, name)?,
        GitCommand::RenameBranch { from, to } => branch::rename(repo, from, to)?,
        GitCommand::CheckoutTag(name) => tag::checkout_detached(repo, name)?,
        GitCommand::DeleteTag(name) => tag::delete(repo, name)?,
        GitCommand::Reset { target, mode } => branch::reset(repo, *target, *mode)?,
        GitCommand::StageHunk { path, hunk } => stage::stage_hunk(repo, path, *hunk)?,
        GitCommand::UnstageHunk { path, hunk } => stage::unstage_hunk(repo, path, *hunk)?,
        GitCommand::StageLines { path, hunk, lines } => {
            stage::stage_lines(repo, path, *hunk, lines)?
        }
        GitCommand::UnstageLines { path, hunk, lines } => {
            stage::unstage_lines(repo, path, *hunk, lines)?
        }
        GitCommand::DiscardHunk { path, hunk } => stage::discard_hunk(repo, path, *hunk)?,
        GitCommand::ResolveFile { path, content } => {
            conflict::resolve_file(repo, path, content.as_deref())?
        }
        GitCommand::ResolveFileSide { path, ours } => {
            conflict::resolve_file_side(repo, path, *ours)?
        }
        GitCommand::Diff { .. }
        | GitCommand::Graph { .. }
        | GitCommand::RebaseTodo { .. }
        | GitCommand::CommitDetail(_)
        | GitCommand::CommitFileDiff { .. }
        | GitCommand::ReadConflicts
        | GitCommand::EditFile(_) => {
            unreachable!("commands with their own reply variant never reach apply")
        }
    }
    Ok(())
}

fn snapshot(repo: &git2::Repository) -> Result<RepoSnapshot, git2::Error> {
    let upstream = upstream_remote(repo);
    Ok(RepoSnapshot {
        status: load_repo(repo)?,
        branch: branch::current(repo)?,
        stash_count: stash::count(repo)?,
        has_remote: !repo.remotes()?.is_empty(),
        upstream_oid: upstream
            .as_deref()
            .and_then(|remote| remote_tracking_oid(repo, remote)),
        upstream_remote: upstream,
        op_in_progress: op_in_progress(repo),
        op: op_summary(repo),
        pr_remote: pr_remote(repo),
    })
}

/// Cloud forge of the `origin` remote (git.md §9), parsed from its URL — `None`
/// without an `origin` or for an unrecognized host. No network: just a config read.
fn pr_remote(repo: &git2::Repository) -> Option<crate::git::forge::Forge> {
    let remote = repo.find_remote("origin").ok()?;
    crate::git::forge::parse_remote(remote.url().ok()?)
}

/// Remote name of the current branch's upstream (`branch.<name>.remote`), or
/// `None` if HEAD is detached/unborn or the branch has no tracking config.
fn upstream_remote(repo: &git2::Repository) -> Option<String> {
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return None;
    }
    let name = head.shorthand().ok()?;
    let local = repo.find_branch(name, git2::BranchType::Local).ok()?;
    local.upstream().ok()?;
    repo.config()
        .ok()?
        .get_string(&format!("branch.{name}.remote"))
        .ok()
}

/// Tip of `refs/remotes/<remote>/<current branch>` — the ref a force push of the
/// current branch overwrites (git.md §10). HEAD is known to be a branch here:
/// [`upstream_remote`] returns `None` otherwise.
fn remote_tracking_oid(repo: &git2::Repository, remote: &str) -> Option<git2::Oid> {
    let head = repo.head().ok()?;
    let name = head.shorthand().ok()?;
    repo.find_reference(&format!("refs/remotes/{remote}/{name}"))
        .ok()?
        .target()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncCommand {
    FetchAll,
    Pull(PullMode),
    Push,
    /// Force-pushes `branch` to its upstream with a lease (toolbar Push chevron,
    /// git.md §10): `git push --force-with-lease=<ref>:<lease>`. Both fields are
    /// captured when the confirmation modal is armed — `lease` is the
    /// remote-tracking oid helm was displaying, so the push refuses if the remote
    /// moved since. Same execution rules as `Push` (never bare `--force`).
    ForcePush {
        branch: String,
        lease: git2::Oid,
    },
    /// Rebases the current branch onto the named ref (graph context menu,
    /// git.md §9). Local op via the `git` subprocess — same execution rules as
    /// Pull/Push (dedicated thread, one op at a time, toasts on completion).
    Rebase(String),
    /// Executes the plan of the interactive-rebase page (git.md §9): `git
    /// rebase -i` with the injected todo — same execution rules as `Rebase`.
    /// `current` is the branch the page was opened on: the execution refuses
    /// when HEAD no longer names it (checkout from the terminal meanwhile).
    InteractiveRebase {
        current: String,
        onto: String,
        steps: Vec<RebaseStep>,
    },
    /// Merges the named ref into the current branch (graph context menu,
    /// git.md §9). Local op via the `git` subprocess — same execution rules as
    /// `Rebase` (dedicated thread, one op at a time, toasts on completion).
    Merge(String),
    /// Replays the commit on the current branch (`git cherry-pick <sha>`, graph
    /// row menu — git.md §9). Local op, same execution rules as `Rebase`.
    CherryPick(String),
    /// Commits the inverse of the commit on the current branch (`git revert
    /// --no-edit <sha>`, graph row menu — git.md §9). Same rules as `CherryPick`.
    Revert(String),
    /// Aborts the merge/rebase in progress (banner button, git.md §10): the
    /// abort flavor follows the repo state on the domain side.
    AbortOp,
    /// Continues the merge/rebase after its conflicts are resolved (conflict
    /// editor "Continue", conflicts.md §2/§5): the `--continue` flavor follows
    /// the repo state on the domain side, non-interactively.
    ContinueOp,
    /// Deletes the branch on its remote (`git push <remote> --delete`, graph
    /// context menu, confirmed by a modal): a network op, same execution rules
    /// as Pull/Push (git.md §10).
    DeleteRemoteBranch(String),
    /// Combined menu entry: delete the remote first, then enqueue the local
    /// deletion only if the network side succeeded.
    DeleteRemoteThenLocalBranch {
        remote: String,
        local: String,
    },
    /// Pushes a tag to `origin` (`git push origin <tag>`, graph tag menu — git.md
    /// §9): a network op, same execution rules as Push (§10).
    PushTag(String),
    /// "Also delete on origin" tag deletion: removes the tag on `origin` first,
    /// then enqueues the local deletion only if the network side succeeded —
    /// never a silent half (busy ⇒ nothing happens, refusal toast on the app
    /// side). Local-only deletion goes straight to the worker (`DeleteTag`).
    DeleteRemoteThenLocalTag(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReply {
    pub command: SyncCommand,
    pub result: Result<SyncOutcome, SyncError>,
}

/// Runs network operations (git.md §10) on a **dedicated thread per op**: the
/// sequential git worker and the status poll (§7) are never blocked. **One op at
/// a time**: `request` ignores until the previous one has been drained (`busy`).
/// The thread is not joined: abandoning the session mid-op lets the subprocess
/// finish in the background, its reply discarded.
pub struct SyncRunner {
    repo_path: PathBuf,
    on_event: Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<SyncReply>,
    results_rx: Receiver<SyncReply>,
    in_flight: Option<SyncCommand>,
    mutation_lock: MutationLock,
}

impl SyncRunner {
    pub fn new(repo_path: &Path, on_event: impl Fn() + Send + Sync + 'static) -> Self {
        Self::new_with_lock(repo_path, MutationLock::new(), on_event)
    }

    pub fn new_with_lock(
        repo_path: &Path,
        mutation_lock: MutationLock,
        on_event: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            repo_path: repo_path.to_path_buf(),
            on_event: Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: None,
            mutation_lock,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Op in progress, if any: the toolbar (M12-6) shows the spinner on the
    /// relevant button and disables the other network actions.
    pub fn in_flight(&self) -> Option<SyncCommand> {
        self.in_flight.clone()
    }

    /// Launches the op; returns `false` (request ignored) if an op is in progress.
    pub fn request(&mut self, command: SyncCommand) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        let Some(guard) = self.mutation_lock.try_acquire() else {
            return false;
        };
        self.in_flight = Some(command.clone());
        let path = self.repo_path.clone();
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            // Release the lock before broadcasting the reply (as GitWorker does):
            // recv() unblocks on the send, so a caller that re-requests right after
            // draining must find the lock already free.
            let result = {
                let _guard = guard;
                execute_sync(&path, &command)
            };
            let _ = tx.send(SyncReply { command, result });
            on_event();
        });
        true
    }

    pub fn try_recv(&mut self) -> Option<SyncReply> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }

    pub fn recv(&mut self) -> Option<SyncReply> {
        let reply = self.results_rx.recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }
}

/// Cadence of the silent background fetch (git.md §7).
const BACKGROUND_FETCH_INTERVAL: Duration = Duration::from_secs(10);

/// Granularity of the sleep between ticks: the runner wakes this often to notice
/// a dropped session, so a repo switch does not leave a thread idling ten seconds.
const FETCH_STOP_POLL: Duration = Duration::from_millis(200);

/// Silent `git fetch --all` of the active repo every `BACKGROUND_FETCH_INTERVAL`,
/// on its **own thread with its own clock**: keeps `refs/remotes/*` fresh so the graph
/// — reloaded by the poll (git.md §7) — shows the real remote position without a manual
/// fetch/pull. Driving it from the frame loop instead tied it to `egui`'s time, which
/// stops advancing as soon as the window is hidden or occluded (macOS delivers no
/// frames): the refs then froze for as long as the app stayed in the background.
///
/// Lock-free on purpose: `sync::background_fetch_all` disables auto-maintenance, so a
/// fetch only writes loose `refs/remotes` + objects (never a repack / `packed-refs`
/// rewrite), disjoint from the index and local refs the mutation lock guards — holding
/// it would instead spuriously fail user staging/commits on every tick. A tick is
/// skipped (never queued) while that lock is held: a manual network op / AI rebase
/// moves the same remote refs, and `is_locked` only peeks — acquiring it would race
/// the user op for the lock.
/// Failures (offline / auth) are swallowed: the fetch stays invisible until it moves
/// a ref the graph then shows. Sequential by construction: a fetch slower than the
/// interval delays the next one instead of stacking. The thread is not joined:
/// abandoning the session mid-fetch lets the subprocess finish, its result dropped.
pub struct FetchRunner {
    stop: Arc<AtomicBool>,
}

impl FetchRunner {
    pub fn new(
        repo_path: &Path,
        mutation_lock: MutationLock,
        on_event: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::with_interval(
            repo_path,
            mutation_lock,
            BACKGROUND_FETCH_INTERVAL,
            on_event,
        )
    }

    /// Seam: the tests drive the cadence instead of waiting ten real seconds.
    pub fn with_interval(
        repo_path: &Path,
        mutation_lock: MutationLock,
        interval: Duration,
        on_event: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let path = repo_path.to_path_buf();
        let stopped = Arc::clone(&stop);
        std::thread::spawn(move || {
            while sleep_until_due(&stopped, interval) {
                // A repo with no remote costs one `Repository::open` per tick
                // (`ensure_remote`), no network — no snapshot to wait for.
                if mutation_lock.is_locked() {
                    continue;
                }
                let _ = sync::background_fetch_all(&path);
                on_event();
            }
        });
        Self { stop }
    }
}

/// Sleeps one interval in `FETCH_STOP_POLL` slices; `false` once the session is
/// abandoned, which ends the loop.
fn sleep_until_due(stop: &AtomicBool, interval: Duration) -> bool {
    let deadline = Instant::now() + interval;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(FETCH_STOP_POLL.min(interval));
    }
    !stop.load(Ordering::Relaxed)
}

impl Drop for FetchRunner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sync_follow_up(
    command: &SyncCommand,
    result: &Result<SyncOutcome, SyncError>,
) -> Option<GitCommand> {
    match (command, result) {
        (SyncCommand::DeleteRemoteThenLocalBranch { local, .. }, Ok(_)) => {
            Some(GitCommand::DeleteBranch(local.clone()))
        }
        (SyncCommand::DeleteRemoteThenLocalTag(name), Ok(_)) => {
            Some(GitCommand::DeleteTag(name.clone()))
        }
        _ => None,
    }
}

fn execute_sync(path: &Path, command: &SyncCommand) -> Result<SyncOutcome, SyncError> {
    match command {
        SyncCommand::FetchAll => sync::fetch_all(path),
        SyncCommand::Pull(mode) => sync::pull(path, *mode),
        SyncCommand::Push => sync::push(path),
        SyncCommand::ForcePush { branch, lease } => sync::force_push(path, branch, *lease),
        SyncCommand::Rebase(onto) => sync::rebase_onto(path, onto),
        SyncCommand::InteractiveRebase {
            current,
            onto,
            steps,
        } => sync::interactive_rebase(path, current, onto, steps),
        SyncCommand::Merge(from) => sync::merge(path, from),
        SyncCommand::CherryPick(sha) => sync::cherry_pick(path, sha),
        SyncCommand::Revert(sha) => sync::revert(path, sha),
        SyncCommand::AbortOp => sync::abort_op(path),
        SyncCommand::ContinueOp => sync::continue_op(path),
        SyncCommand::DeleteRemoteBranch(name) => sync::delete_remote_branch(path, name),
        SyncCommand::DeleteRemoteThenLocalBranch { remote, local } => {
            let repo = git2::Repository::open(path)
                .map_err(|err| SyncError::Other(err.message().to_owned()))?;
            branch::validate_local_deletable(&repo, local)
                .map_err(|err| SyncError::Other(err.message().to_owned()))?;
            sync::delete_remote_branch(path, remote)
        }
        SyncCommand::PushTag(name) => sync::push_tag(path, name),
        SyncCommand::DeleteRemoteThenLocalTag(name) => sync::delete_remote_tag(path, name),
    }
}

/// Drains network-op replies: each completed op reloads status + graph (the
/// graph only in Graph mode, same rules as the poll git.md §7). Seam:
/// `GitSession::drain_sync` delegates here, business e2e tests exercise it without egui.
pub fn drain_sync_refresh(
    sync: &mut SyncRunner,
    worker: &GitWorker,
    graph_mode: bool,
    graph_limit: usize,
) -> Vec<SyncReply> {
    let mut replies = Vec::new();
    while let Some(reply) = sync.try_recv() {
        if let Some(command) = sync_follow_up(&reply.command, &reply.result) {
            worker.send(command);
        }
        worker.send(GitCommand::Status);
        if graph_mode {
            worker.send(GitCommand::Graph { limit: graph_limit });
        }
        replies.push(reply);
    }
    replies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_stamps_the_latest_generation_per_result_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let worker = GitWorker::spawn(tmp.path(), || {});
        worker.send(GitCommand::Status); // generation 1
        worker.send(GitCommand::Graph { limit: 50 }); // generation 2
        worker.send(GitCommand::Stage("a.txt".into())); // generation 3, Status kind

        assert!(
            worker.superseded(1, ResultKind::Status),
            "the Stage request supersedes the older Status one (same kind)"
        );
        assert!(!worker.superseded(3, ResultKind::Status));
        assert!(
            !worker.superseded(2, ResultKind::Graph),
            "a status burst never supersedes the in-flight graph request"
        );
    }

    #[test]
    fn a_rebase_todo_error_is_gated_like_state() {
        // The page adopts even the failure (clean error screen): a stale Err
        // from a superseded request must pass the gate, not bypass it.
        let todo_err = GitResult::RebaseTodo {
            onto: "main".into(),
            result: Err(git2::Error::from_str("boom")),
        };
        assert!(todo_err.carries_state());
        // Other reads keep reporting their errors regardless of staleness.
        let status_err = GitResult::Status {
            source: GitCommand::Status,
            result: Err(git2::Error::from_str("boom")),
        };
        assert!(!status_err.carries_state());
    }

    #[test]
    fn every_command_resolves_to_its_reply_kind() {
        assert_eq!(GitCommand::Status.result_kind(), ResultKind::Status);
        assert_eq!(
            GitCommand::Commit("m".into()).result_kind(),
            ResultKind::Status,
            "mutating commands reply with a status snapshot"
        );
        assert_eq!(
            GitCommand::Diff {
                path: "a".into(),
                staged: false
            }
            .result_kind(),
            ResultKind::Diff
        );
        assert_eq!(
            GitCommand::Graph { limit: 1 }.result_kind(),
            ResultKind::Graph
        );
        assert_eq!(
            GitCommand::CommitDetail(git2::Oid::ZERO_SHA1).result_kind(),
            ResultKind::CommitDetail
        );
        assert_eq!(
            GitCommand::CommitFileDiff {
                oid: git2::Oid::ZERO_SHA1,
                path: "a".into()
            }
            .result_kind(),
            ResultKind::CommitFileDiff
        );
    }

    #[test]
    fn commit_addressed_reads_jump_the_poll_backlog() {
        let queue: VecDeque<(u64, GitCommand)> = [
            (1, GitCommand::Status),
            (2, GitCommand::Graph { limit: 50 }),
            (3, GitCommand::CommitDetail(git2::Oid::ZERO_SHA1)),
            (
                4,
                GitCommand::CommitFileDiff {
                    oid: git2::Oid::ZERO_SHA1,
                    path: "a".into(),
                },
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            next_index(&queue),
            2,
            "the commit detail overtakes the queued poll reads"
        );

        let queue: VecDeque<(u64, GitCommand)> = [
            (1, GitCommand::Stage("a.txt".into())),
            (2, GitCommand::CommitDetail(git2::Oid::ZERO_SHA1)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            next_index(&queue),
            1,
            "a commit is immutable: its detail may overtake a queued mutation"
        );
    }

    #[test]
    fn a_working_tree_diff_overtakes_the_refresh_reads_but_never_a_mutation() {
        let diff = GitCommand::Diff {
            path: "a".into(),
            staged: false,
        };

        let queue: VecDeque<(u64, GitCommand)> = [
            (1, GitCommand::Status),
            (2, GitCommand::Graph { limit: 50 }),
            (3, diff.clone()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            next_index(&queue),
            2,
            "status and graph change nothing the diff reads: the click is served first"
        );

        let queue: VecDeque<(u64, GitCommand)> = [
            (1, GitCommand::Stage("a".into())),
            (2, GitCommand::Status),
            (3, diff.clone()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            next_index(&queue),
            0,
            "the diff reads the index the queued staging is about to change: strict FIFO"
        );

        let queue: VecDeque<(u64, GitCommand)> = [
            (1, GitCommand::Status),
            (
                2,
                GitCommand::Diff {
                    path: "first".into(),
                    staged: false,
                },
            ),
            (3, diff),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            next_index(&queue),
            1,
            "two diffs stay in order: per-kind FIFO holds for the staleness gate"
        );
    }

    #[test]
    fn replies_settle_the_in_flight_entry_of_their_kind() {
        // Non-repo path: every command replies (with an error), whatever the
        // serve order — the bookkeeping must pair each reply with the oldest
        // in-flight command of its kind, never blindly with the queue's front.
        let tmp = tempfile::tempdir().unwrap();
        let worker = GitWorker::spawn(tmp.path(), || {});
        worker.send(GitCommand::Status);
        worker.send(GitCommand::CommitDetail(git2::Oid::ZERO_SHA1));

        let (_, first) = worker.recv().expect("first reply");
        assert!(!worker.has_pending(first.kind()));
        let other = if first.kind() == ResultKind::Status {
            ResultKind::CommitDetail
        } else {
            ResultKind::Status
        };
        assert!(worker.has_pending(other), "the other command is in flight");
        let (_, second) = worker.recv().expect("second reply");
        assert_eq!(second.kind(), other);
        assert!(!worker.has_pending(other));
    }

    #[test]
    fn only_ok_payloads_carry_state() {
        let ok = GitResult::Graph {
            limit: 1,
            result: Ok(Graph {
                commits: Vec::new(),
                has_more: false,
            }),
        };
        assert!(ok.carries_state());
        assert_eq!(ok.kind(), ResultKind::Graph);

        let err = GitResult::Status {
            source: GitCommand::CreateBranch("feat".into()),
            result: Err(git2::Error::from_str("exists")),
        };
        assert!(
            !err.carries_state(),
            "errors are never gated — they report on the command"
        );
        assert_eq!(err.kind(), ResultKind::Status);
    }

    #[test]
    fn request_is_ignored_while_busy_and_busy_clears_on_drain() {
        let tmp = tempfile::tempdir().unwrap();
        let mut runner = SyncRunner::new(tmp.path(), || {});
        assert!(!runner.busy());

        assert!(runner.request(SyncCommand::FetchAll));
        assert!(runner.busy());
        assert_eq!(runner.in_flight(), Some(SyncCommand::FetchAll));
        assert!(!runner.request(SyncCommand::Push));

        let reply = runner.recv().unwrap();
        assert_eq!(reply.command, SyncCommand::FetchAll);
        assert!(reply.result.is_err());
        assert!(!runner.busy());

        assert!(runner.request(SyncCommand::Push));
    }

    #[test]
    fn background_fetch_advances_remote_tracking_ref() {
        let (_tmp, a, target) = clone_behind_its_remote();

        // No UI frame is ever pumped here: the runner ticks on its own clock —
        // exactly what keeps the refs fresh while the window is hidden.
        let _fetch =
            FetchRunner::with_interval(&a, MutationLock::new(), Duration::from_millis(50), || {});
        assert!(
            wait_until(200, || tracking_main(&a) == target),
            "background fetch advanced refs/remotes/origin/main without a checkout"
        );
    }

    #[test]
    fn background_fetch_defers_to_a_held_mutation_lock() {
        let (_tmp, a, target) = clone_behind_its_remote();
        let lock = MutationLock::new();
        let guard = lock.try_acquire().expect("free lock");

        let _fetch = FetchRunner::with_interval(&a, lock, Duration::from_millis(50), || {});
        assert!(
            !wait_until(20, || tracking_main(&a) == target),
            "no fetch while a mutation (manual pull, AI rebase) holds the lock"
        );

        drop(guard);
        assert!(
            wait_until(200, || tracking_main(&a) == target),
            "the fetch resumes once the lock is released"
        );
    }

    /// Clone `a` of a bare remote another clone has since advanced: its
    /// `refs/remotes/origin/main` is one commit behind the returned oid.
    fn clone_behind_its_remote() -> (tempfile::TempDir, PathBuf, git2::Oid) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let git = |args: &[&str], cwd: &Path| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        let bare = root.join("bare.git");
        let a = root.join("a");
        let b = root.join("b");
        git(
            &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
            root,
        );
        // Clone A publishes the first commit.
        git(
            &["clone", bare.to_str().unwrap(), a.to_str().unwrap()],
            root,
        );
        std::fs::write(a.join("f.txt"), "1").unwrap();
        git(&["add", "."], &a);
        git(&["commit", "-m", "c1"], &a);
        git(&["push", "-u", "origin", "main"], &a);
        // Clone B advances the remote past A's tracking ref.
        git(
            &["clone", bare.to_str().unwrap(), b.to_str().unwrap()],
            root,
        );
        std::fs::write(b.join("f.txt"), "2").unwrap();
        git(&["commit", "-am", "c2"], &b);
        git(&["push", "origin", "main"], &b);

        let target = git2::Repository::open(&b)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap();
        assert_ne!(tracking_main(&a), target, "A is behind before the fetch");
        (tmp, a, target)
    }

    fn tracking_main(repo: &Path) -> git2::Oid {
        git2::Repository::open(repo)
            .unwrap()
            .find_reference("refs/remotes/origin/main")
            .unwrap()
            .target()
            .unwrap()
    }

    /// Polls `done` every 25 ms, `tries` times (the fetch subprocess is the slow part).
    fn wait_until(tries: usize, done: impl Fn() -> bool) -> bool {
        for _ in 0..tries {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }
}
