//! Background git session: drains the worker's replies into the per-repo
//! caches (status / diff / graph / commit detail / rebase plan) consumed by the
//! UI, plus the diff-overlay state and cache keys (git.md, architecture.md).

use super::*;

/// Stable repo identity (M17-11): path canonicalized once at key creation — cache
/// keys survive workspace reorders and removals, no positional reindexing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RepoKey(PathBuf);

impl RepoKey {
    pub(crate) fn of(path: &Path) -> RepoKey {
        RepoKey(canonical_path(path))
    }
}

/// Live terminal sessions are keyed by **(repo, tab)** identity: each tab of a repo
/// carries its own pane tree (terminal.md §10), hence its own set of PTYs. Switching
/// tab or repo does not touch the sets of the other keys.
pub(crate) type PaneKey = (RepoKey, TabId);

/// Commit-drafting state of a repo that is **not** the active one, preserved
/// across switches (the `GitSession` is re-spawned on switch). Parked on
/// switch-out, restored on switch-back: a typed message never shows under
/// another repo, and an in-flight AI generation is not cancelled — its runner
/// (background thread + result channel) lives on here until the repo is
/// reopened, where the queued suggestion is drained into the inputs.
pub(crate) struct CommitDraft {
    pub(crate) subject: String,
    pub(crate) description: String,
    pub(crate) ai: AiRunner,
}

/// One running (or recently finished) agent, for the cross-repo dashboard
/// (specs/agents.md). Rebuilt each agent-watch tick from `panes` +
/// `agent_watch`; carries the stable identity needed to focus it on click and
/// the labels the page renders.
#[derive(Clone)]
pub(crate) struct AgentEntry {
    pub(crate) repo_key: RepoKey,
    /// Project name = the group root's name; a root and its worktrees share it so
    /// the dashboard groups them under one card (specs/agents.md §5).
    pub(crate) group_name: String,
    pub(crate) branch: Option<String>,
    pub(crate) tab_id: TabId,
    pub(crate) tab_name: String,
    pub(crate) pane_id: PaneId,
    pub(crate) agent: &'static str,
    pub(crate) badge: AgentBadge,
    /// Monotonic stamp (`activity::now_ms`) of the pane's last spontaneous output
    /// — the page renders "finished <ago>" off the live clock.
    pub(crate) last_output_ms: u64,
}

/// Per-repo UI state under one roof (M17-11), reconciled by `sync` after every
/// workspace mutation. Stable keys mean a removal/regroup/close only ever *drops*
/// entries — nothing shifts.
#[derive(Default)]
pub(crate) struct RepoCaches {
    /// Workspace order → key, rebuilt by `sync`: per-frame lookups never touch the
    /// filesystem (canonicalization happens once per mutation).
    pub(crate) keys: Vec<RepoKey>,
    pub(crate) panes: HashMap<PaneKey, Panes>,
    /// The per-worktree Run terminal process (git.md §3): one PTY per repo key,
    /// shared by every tab of that worktree. Spawned on demand by the Run button,
    /// dropped (killing its tree) on Stop or when the worktree leaves the workspace.
    pub(crate) run_panes: HashMap<RepoKey, TerminalState>,
    /// Per-pane agent state machine (specs/agents.md), ticked at the poll cadence;
    /// keys aligned on `panes`, pruned at tick.
    pub(crate) agent_watch: HashMap<(PaneKey, PaneId), PaneAgentState>,
    /// Aggregated badge per repo (sidebar), recomputed at tick.
    pub(crate) agent_badges: HashMap<RepoKey, AgentBadge>,
    /// Flat list of agents across every repo (cross-repo dashboard), rebuilt at
    /// tick and read by the Agents page; ordered by workspace then pane.
    pub(crate) agents: Vec<AgentEntry>,
    /// Current branch per repo, for the sidebar's second line. Refreshed on sync
    /// triggers + on every workspace mutation; the active row also follows the live
    /// `GitSession` snapshot (checkout from the terminal or the graph).
    pub(crate) branch_labels: HashMap<RepoKey, String>,
    /// Uncommitted line stats `(additions, deletions)` per dirty repo, for the
    /// sidebar's right-edge `+N −M`. Membership = dirty (value may be `(0, 0)` for a
    /// change with no countable lines). Refreshed for all repos on sync triggers
    /// (`workspace_dirty_stats`); the active repo also follows its live `GitSession`
    /// status every frame.
    pub(crate) dirty: HashMap<RepoKey, (usize, usize)>,
    /// Last graph (+ page size) per left-behind repo: switching back to a repo in
    /// Graph mode displays it immediately while the worker reloads a fresh graph — no
    /// more loader on switch-back.
    pub(crate) graph_cache: HashMap<RepoKey, (Graph, usize)>,
    /// Per-repo commit-drafting state parked while the repo is inactive (message
    /// draft + AI runner, see [`CommitDraft`]): the active repo's draft lives in
    /// `git_panel_state` and its runner in the `GitSession`.
    pub(crate) commit_drafts: HashMap<RepoKey, CommitDraft>,
    /// Memoized graph lanes (M10-8): content-addressed (reconciles by comparison,
    /// including on repo switch), nothing to re-key.
    pub(crate) lane_cache: LaneCache,
}

impl RepoCaches {
    /// Clears every live pane's "painted this frame" flag (called at frame start).
    /// The render path re-sets it for the panes it actually draws; a pane left
    /// false keeps reading into its grid without pacing the event loop.
    pub(crate) fn clear_pane_visibility(&self) {
        for panes in self.panes.values() {
            for state in panes.values() {
                if let TerminalState::Live(pane) = state {
                    pane.set_visible(false);
                }
            }
        }
        for state in self.run_panes.values() {
            if let TerminalState::Live(pane) = state {
                pane.set_visible(false);
            }
        }
    }

    /// Single reconciliation point (M17-11): rebuild the order→key alignment and
    /// drop every entry whose repo or tab no longer exists in the workspace.
    /// Dropping a pane set kills its process trees (`Pty::drop`).
    pub(crate) fn sync(&mut self, workspace: &Workspace) {
        self.keys = workspace.repos().map(|r| RepoKey::of(&r.path)).collect();
        let live_tabs: std::collections::HashSet<PaneKey> = workspace
            .all_tab_ids()
            .filter_map(|(i, id)| Some((self.keys.get(i)?.clone(), id)))
            .collect();
        self.panes.retain(|key, _| live_tabs.contains(key));
        self.run_panes.retain(|key, _| self.keys.contains(key));
        self.agent_watch
            .retain(|(key, _), _| live_tabs.contains(key));
        self.agent_badges.retain(|key, _| self.keys.contains(key));
        self.branch_labels.retain(|key, _| self.keys.contains(key));
        self.dirty.retain(|key, _| self.keys.contains(key));
        self.graph_cache.retain(|key, _| self.keys.contains(key));
        self.commit_drafts.retain(|key, _| self.keys.contains(key));
    }

    /// Adopts a fresh `workspace_branches` pass (workspace order), dropping the
    /// unversioned/bare/unreadable entries (`None`) like the absent keys.
    pub(crate) fn set_branch_labels(&mut self, labels: Vec<Option<String>>) {
        self.branch_labels = self
            .keys
            .iter()
            .zip(labels)
            .filter_map(|(key, label)| Some((key.clone(), label?)))
            .collect();
    }

    /// Adopts a fresh `workspace_dirty_stats` pass (workspace order): keeps the dirty
    /// repos with their `(additions, deletions)`, drops the clean ones (`None`). The
    /// active repo's live overlay is re-applied on the next frame.
    pub(crate) fn set_dirty_stats(&mut self, stats: Vec<Option<(usize, usize)>>) {
        self.dirty = self
            .keys
            .iter()
            .zip(stats)
            .filter_map(|(key, stat)| Some((key.clone(), stat?)))
            .collect();
    }

    /// Pane-set key of `(repo index, tab index)`, `None` if either is stale.
    pub(crate) fn pane_key(
        &self,
        workspace: &Workspace,
        index: usize,
        tab: usize,
    ) -> Option<PaneKey> {
        Some((self.keys.get(index)?.clone(), workspace.tab_id(index, tab)?))
    }
}

/// Diff open in the central area: one open/adopt/close lifecycle for both sources
/// (M17-9). `Esc` or a repo switch closes it; a re-stage refreshes a working-tree
/// `loaded` via a new Diff command, leaving Graph mode closes a commit diff.
pub(crate) struct DiffState {
    pub(crate) source: DiffSource,
    pub(crate) path: String,
    pub(crate) loaded: Option<FileDiff>,
    /// `loaded` comes from the previously open file (arrow-key navigation): kept
    /// displayed frozen during the worker round-trip — otherwise the central area
    /// falls back to terminal/graph for a few frames (flash). Reset (view included)
    /// when the requested diff arrives.
    pub(crate) inherited: bool,
    pub(crate) view: DiffViewState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSource {
    /// Status-section overlay (M6-3): workdir vs index (`staged == false`) or index
    /// vs HEAD (`staged == true`); granular staging allowed.
    WorkingTree { staged: bool },
    /// Commit file vs first parent, **fullscreen and read-only** in Graph mode
    /// (M9-7, git.md §9): a worker reply is adopted only if it targets this commit
    /// **and** this path (never one from a previous click).
    Commit(git2::Oid),
}

impl DiffState {
    /// Single open path (M17-9): content already displayed for the **same source
    /// kind** stays frozen on screen during the worker round-trip (`inherited`);
    /// switching kinds starts fresh — inheriting across kinds would render the
    /// other source's content under the wrong chrome (staging buttons on a commit
    /// file).
    pub(crate) fn open(slot: &mut Option<DiffState>, source: DiffSource, path: String) {
        let kept = slot.take().filter(|open| {
            open.loaded.is_some()
                && std::mem::discriminant(&open.source) == std::mem::discriminant(&source)
        });
        let inherited = kept.is_some();
        let (loaded, view) = kept
            .map(|open| (open.loaded, open.view))
            .unwrap_or_default();
        *slot = Some(DiffState {
            source,
            path,
            loaded,
            inherited,
            view,
        });
    }

    /// Single adoption path (M17-9): inherited content gives way to the requested
    /// diff (fresh view, like a direct open); a poll-driven reload (on-disk change,
    /// git.md §8) instead reconciles the selection, dropping and signaling what
    /// became invalid.
    pub(crate) fn adopt(&mut self, file: FileDiff) {
        if self.inherited {
            self.view.clear();
            self.inherited = false;
        }
        self.view.reconcile(&file);
        self.loaded = Some(file);
    }
}

/// Git session of the active repo: libgit2 worker (off the UI thread, architecture §3)
/// + last received snapshot (status + branch). Re-spawned on repo switch.
pub(crate) struct GitSession {
    pub(crate) index: usize,
    /// Stable repo identity: graph-cache key on switch, branch-label key live.
    pub(crate) key: RepoKey,
    pub(crate) worker: GitWorker,
    /// Network ops (M12-3) on dedicated threads: the sequential worker and the poll
    /// are not blocked; one op at a time (`busy`).
    pub(crate) sync: SyncRunner,
    /// Silent background fetch (git.md §7): refreshes `refs/remotes/*` on a fixed
    /// cadence so the graph reflects the real remote position without a manual pull.
    pub(crate) fetch: FetchRunner,
    /// AI generation of the commit message, off the UI thread; one request at a time
    /// (`busy` ⇒ spinner on the commit card button).
    pub(crate) ai: AiRunner,
    /// AI rebase (git.md §9), off the UI thread; holds the repo's mutation lock
    /// for the whole run — staging, commits and sync ops are refused meanwhile.
    pub(crate) ai_rebase: AiRebaseRunner,
    pub(crate) status: RepoStatus,
    pub(crate) branch: Branch,
    /// Stash count + presence of a remote (worker snapshot, M12-6): drive the
    /// Pop / Pull / Push states of the graph toolbar.
    pub(crate) stash_count: usize,
    pub(crate) has_remote: bool,
    /// Remote name of the current branch's upstream (worker snapshot, git.md §10):
    /// `None` ⇒ force push greyed out; otherwise names the remote in its modal.
    pub(crate) upstream_remote: Option<String>,
    /// Cloud forge behind `origin` (worker snapshot, git.md §9): `Some` ⇒ the
    /// **Create pull request** graph entry is offered and this builds its URL.
    pub(crate) pr_remote: Option<crate::git::forge::Forge>,
    /// Merge / rebase in progress (M12-8, worker snapshot): banner in the status
    /// sidebar, including for an op started from the terminal.
    pub(crate) op_in_progress: bool,
    /// Summary of the in-progress op (worker snapshot, conflicts.md §2): verb +
    /// source/target for the conflict panel header. `None` outside an op.
    pub(crate) op: Option<crate::git::status::OpSummary>,
    /// `git` binary missing from PATH (detected at spawn, M12-9): toolbar network
    /// actions greyed out with a tooltip.
    pub(crate) git_missing: bool,
    /// `false` until the first status snapshot lands: `status` still holds the
    /// `default()` placeholder — the sidebar shows a loader, not a clean tree.
    pub(crate) status_loaded: bool,
    /// Anti-spam for **polled** failures (status / graph re-run at a fixed cadence):
    /// one toast per failure episode, rearmed on the first success.
    pub(crate) status_error_seen: bool,
    pub(crate) graph_error_seen: bool,
    /// Last received graph (M9-1) rendered in Graph mode; commit selected by click
    /// (intent M9-5) whose detail (meta + files, M9-2) shows in the right sidebar
    /// (M9-6). `None` until a graph has arrived: the view shows a loader, not
    /// **No commits** (reserved for a truly empty repo).
    pub(crate) graph: Option<Graph>,
    /// Page size requested from the worker (M9-8); **Load more** grows it by one
    /// `graph::PAGE_SIZE` slice then reloads (explicit pagination).
    pub(crate) graph_limit: usize,
    /// Commit selected by click (M9-5). `None` ⇒ the WIP row is the implicit
    /// selection: the right sidebar keeps the status sections (M10-7) — the
    /// graph never shows an empty "select a commit" state.
    pub(crate) selected_commit: Option<git2::Oid>,
    pub(crate) detail: Option<CommitDetail>,
    /// One-shot: on the next graph render, scroll to the HEAD row (armed at spawn and
    /// on every entry into Graph mode, consumed via `GraphAction::scrolled_to_head`).
    pub(crate) scroll_to_head: bool,
    /// `false` when the displayed graph may no longer reflect the repo's HEAD (cache
    /// from a switch, entry into Graph mode after a stint in the terminal, checkout in
    /// flight): consuming `scroll_to_head` then waits for the next fresh graph —
    /// otherwise the one-shot would be consumed on the row of the **old** HEAD and the
    /// scroll would miss the real one.
    pub(crate) graph_fresh: bool,
    pub(crate) last_poll: f64,
}

/// Worker → UI wakeup: the callback every background runner gets so a reply
/// landing off-frame still schedules a paint.
pub(crate) fn repainter(ctx: &egui::Context) -> impl Fn() + Send + Sync + 'static {
    let ctx = ctx.clone();
    move || ctx.request_repaint()
}

impl GitSession {
    /// Sends no command: the caller orders the first load itself (graph before status
    /// in Graph mode — the worker is sequential).
    pub(crate) fn spawn(index: usize, path: &Path, ctx: &egui::Context, ai: AiRunner) -> Self {
        let now = ctx.input(|i| i.time);
        let mutation_lock = MutationLock::new();
        let worker = GitWorker::spawn_with_lock(path, mutation_lock.clone(), repainter(ctx));
        let sync = SyncRunner::new_with_lock(path, mutation_lock.clone(), repainter(ctx));
        let fetch = FetchRunner::new(path, now, repainter(ctx));
        let ai_rebase = AiRebaseRunner::new(path, mutation_lock, repainter(ctx));
        Self {
            index,
            key: RepoKey::of(path),
            worker,
            sync,
            fetch,
            ai,
            ai_rebase,
            status: RepoStatus::default(),
            branch: Branch::Named(String::new()),
            stash_count: 0,
            has_remote: false,
            upstream_remote: None,
            pr_remote: None,
            op_in_progress: false,
            op: None,
            git_missing: crate::git::cli::locate_git().is_none(),
            status_loaded: false,
            status_error_seen: false,
            graph_error_seen: false,
            graph: None,
            graph_limit: graph::PAGE_SIZE,
            selected_commit: None,
            detail: None,
            scroll_to_head: true,
            graph_fresh: true,
            last_poll: now,
        }
    }

    /// Poll: re-requests the status once the interval has elapsed, independent of any
    /// interaction. The result comes back via the worker (callback → repaint). If a
    /// diff view is open, its diff is also reloaded to reflect a possible on-disk
    /// change (git.md §8). In Graph mode, the graph is reloaded at the same cadence: a
    /// commit made in the terminal or a HEAD moved outside the app appears without
    /// going through a switch (git.md §9).
    pub(crate) fn poll(&mut self, now: f64, diff: Option<&DiffState>, graph_mode: bool) {
        if now - self.last_poll >= GIT_POLL_INTERVAL.as_secs_f64() {
            // Each kind re-polls only once the previous same-kind request has
            // been drained: on a repo slower than the cadence, the queue would
            // otherwise grow without bound (the worker computing snapshots the
            // staleness gate then throws away).
            if !self.worker.has_pending(ResultKind::Status) {
                self.worker.send(GitCommand::Status);
            }
            if let Some(DiffState {
                source: DiffSource::WorkingTree { staged },
                path,
                ..
            }) = diff
            {
                if !self.worker.has_pending(ResultKind::Diff) {
                    self.worker.send(GitCommand::Diff {
                        path: path.clone(),
                        staged: *staged,
                    });
                }
            }
            if graph_mode && !self.worker.has_pending(ResultKind::Graph) {
                self.reload_graph();
            }
            self.last_poll = now;
        }
    }

    /// Silent background fetch (git.md §7) on its own cadence: defers to an in-flight
    /// manual network op or AI rebase (which also move the remote refs), otherwise
    /// refreshes `refs/remotes/*` so the next poll-driven graph reload shows the real
    /// remote position without a manual pull.
    pub(crate) fn poll_background_fetch(&mut self, now: f64) {
        if self.sync.busy() || self.ai_rebase.busy() {
            return;
        }
        self.fetch.tick(now, self.has_remote);
    }

    /// Requests the graph at the current pagination.
    pub(crate) fn reload_graph(&self) {
        self.worker.send(GitCommand::Graph {
            limit: self.graph_limit,
        });
    }

    /// Mutating command with the graph reloaded behind it (worker FIFO): the
    /// affected rows/chips update without waiting for the poll
    /// (D-2026-06-03-graph-stash-rows).
    pub(crate) fn send_then_reload_graph(&self, command: GitCommand) {
        self.worker.send(command);
        self.reload_graph();
    }

    /// Hands a CLI-git op to the runner (one at a time, git.md §10): busy ⇒
    /// the shared refusal toast, nothing queued. `true` when accepted.
    pub(crate) fn request_sync(
        &mut self,
        command: SyncCommand,
        toasts: &mut Toasts,
        now: f64,
    ) -> bool {
        let accepted = self.sync.request(command);
        if !accepted {
            toasts.error("Another Git operation is in progress", now);
        }
        accepted
    }

    /// Drains the worker's results: the staleness gate (M17-13), then one
    /// handler per result domain (M17-14) — a new `GitResult` variant touches
    /// one small fn, not a god-match.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn drain(
        &mut self,
        diff: &mut Option<DiffState>,
        editor: &mut BranchEditor,
        panel: &mut GitPanelState,
        rebase_page: &mut Option<RebasePage>,
        conflict_editor: &mut Option<ConflictEditorState>,
        modal: &mut Option<Modal>,
        toasts: &mut Toasts,
        now: f64,
    ) {
        while let Some((generation, result)) = self.worker.try_recv() {
            // Single staleness gate (M17-13): an `Ok` payload whose request was
            // superseded by a newer same-kind request is dropped — the fresher
            // reply is already in flight. Errors always flow through: they
            // report on the command (toast, inline editor error), and a
            // `CreateBranch` failure must surface even with a status poll
            // queued behind it. Exception: a `RebaseTodo` error is page state
            // and is gated like one (`GitResult::carries_state`).
            if result.carries_state() && self.worker.superseded(generation, result.kind()) {
                continue;
            }
            match result {
                GitResult::Status { source, result } => {
                    self.on_status(source, result, editor, panel, toasts, now)
                }
                GitResult::Diff(result) => Self::on_diff(result, diff, toasts, now),
                GitResult::Graph { result, .. } => self.on_graph(result, diff, toasts, now),
                GitResult::RebaseTodo { onto, result } => {
                    Self::on_rebase_todo(onto, result, rebase_page, modal)
                }
                GitResult::CommitDetail(result) => self.on_commit_detail(result, toasts, now),
                GitResult::CommitFileDiff { oid, result } => {
                    Self::on_commit_file_diff(oid, result, diff, toasts, now)
                }
                GitResult::Conflicts { result } => {
                    Self::on_conflicts(result, conflict_editor, toasts, now)
                }
            }
        }
    }

    /// Status snapshots feed the panel. The pending Branch editor (M12-6)
    /// resolves here: snapshot on the requested branch ⇒ success (closed),
    /// `CreateBranch` error (duplicate…) ⇒ inline error. The **Create branch**
    /// flavor opened from a chip (`CreateBranchAt`, no checkout ⇒ HEAD unchanged)
    /// resolves on its own command — editor closed on success, inline error on
    /// failure. Any other failure goes to a toast (git.md §10), routed by its
    /// originating command.
    fn on_status(
        &mut self,
        source: GitCommand,
        result: Result<RepoSnapshot, git2::Error>,
        editor: &mut BranchEditor,
        panel: &mut GitPanelState,
        toasts: &mut Toasts,
        now: f64,
    ) {
        match result {
            Ok(snapshot) => {
                self.status = snapshot.status;
                self.branch = snapshot.branch;
                self.stash_count = snapshot.stash_count;
                self.has_remote = snapshot.has_remote;
                self.upstream_remote = snapshot.upstream_remote;
                self.pr_remote = snapshot.pr_remote;
                self.op_in_progress = snapshot.op_in_progress;
                self.op = snapshot.op;
                self.status_loaded = true;
                self.status_error_seen = false;
                if matches!(source, GitCommand::Commit(_)) {
                    panel.subject.clear();
                    panel.description.clear();
                }
                if editor.pending && self.branch.label() == editor.name.trim() {
                    *editor = BranchEditor::default();
                }
                // Create branch from a chip / Create tag / Rename of a
                // **non-current** branch succeeded: HEAD did not move, so the
                // close is keyed on the command (not a branch comparison). The
                // current-branch rename closes above — HEAD now carries the new
                // name, matching `editor.name`.
                if editor.pending
                    && matches!(
                        source,
                        GitCommand::CreateBranchAt { .. }
                            | GitCommand::CreateTagAt { .. }
                            | GitCommand::RenameBranch { .. }
                    )
                {
                    *editor = BranchEditor::default();
                }
            }
            Err(err) => match source {
                // The failure is routed by its originating command: only a
                // `CreateBranch`/`CreateBranchAt` feeds the editor's inline error —
                // a concurrent stash/pop can no longer be blamed on it.
                GitCommand::CreateBranch(_)
                | GitCommand::CreateBranchAt { .. }
                | GitCommand::CreateTagAt { .. }
                | GitCommand::RenameBranch { .. }
                    if editor.pending =>
                {
                    editor.pending = false;
                    editor.error = Some(err.message().to_owned());
                }
                // Failure of the **polled** status (repo become unreadable…): one
                // toast per episode, rearmed on the next success.
                GitCommand::Status => {
                    if !self.status_error_seen {
                        self.status_error_seen = true;
                        toasts.error(command_failure_message(&source, &err), now);
                    }
                }
                _ => toasts.error(command_failure_message(&source, &err), now),
            },
        }
    }

    /// Working-tree diff for the overlay (M6-3): adopted only if the overlay
    /// still targets that file. An overlay still empty — or frozen on the
    /// previous file — will never arrive on error: closed with a toast; a poll
    /// reload that fails keeps the displayed content.
    fn on_diff(
        result: Result<FileDiff, git2::Error>,
        diff: &mut Option<DiffState>,
        toasts: &mut Toasts,
        now: f64,
    ) {
        match result {
            Ok(file) => {
                if let Some(open) = diff.as_mut() {
                    if matches!(open.source, DiffSource::WorkingTree { .. })
                        && open.path == file.path
                    {
                        open.adopt(file);
                    }
                }
            }
            Err(err) => {
                if diff.as_ref().is_some_and(|open| {
                    matches!(open.source, DiffSource::WorkingTree { .. })
                        && (open.loaded.is_none() || open.inherited)
                }) {
                    *diff = None;
                    toasts.error(format!("Failed to load the diff — {}", err.message()), now);
                }
            }
        }
    }

    /// Graph (M9-1) rendered in the central area in Graph mode (M9-5); a
    /// selection now absent from the new set is dropped (along with its
    /// detail). The fullscreen diff carries its own oid (which may differ from
    /// the selection): it is dropped only if ITS commit disappeared. The graph
    /// is reloaded by the poll ⇒ one error toast per episode; an auto-scroll
    /// waiting for a fresh graph is disarmed if a (stale) graph is displayed —
    /// otherwise a persistent failure would pin the view to the old HEAD's row.
    fn on_graph(
        &mut self,
        result: Result<Graph, git2::Error>,
        diff: &mut Option<DiffState>,
        toasts: &mut Toasts,
        now: f64,
    ) {
        match result {
            Ok(graph) => {
                self.graph_error_seen = false;
                // Page extended to the HEAD commit on the domain side (branch
                // checked out beyond the limit): the requested size realigns on the
                // received size, otherwise **Load more** would restart from a page
                // smaller than the one displayed.
                self.graph_limit = self.graph_limit.max(graph.page_len());
                self.graph_fresh = true;
                if let Some(oid) = self.selected_commit {
                    if !graph.commits.iter().any(|c| c.oid == oid) {
                        self.selected_commit = None;
                        self.detail = None;
                    }
                }
                if let Some(DiffSource::Commit(oid)) = diff.as_ref().map(|open| open.source) {
                    if !graph.commits.iter().any(|c| c.oid == oid) {
                        *diff = None;
                    }
                }
                self.graph = Some(graph);
            }
            Err(err) => {
                if self.graph.is_some() {
                    self.scroll_to_head = false;
                }
                if !self.graph_error_seen {
                    self.graph_error_seen = true;
                    toasts.error(format!("Failed to load the graph — {}", err.message()), now);
                }
            }
        }
    }

    /// Commit list for the interactive-rebase page **or** the AI rebase recap
    /// modal (git.md §9) — both surfaces share the worker command and are
    /// mutually exclusive on screen: adopted only by the one still loading
    /// **this** target — a surface reopened on another branch never inherits
    /// the previous plan. A failure (unknown ref, capped range) lands as the
    /// surface's clean error state, not a toast: it is what the click opened.
    fn on_rebase_todo(
        onto: String,
        result: Result<Vec<RebaseCommit>, git2::Error>,
        page: &mut Option<RebasePage>,
        modal: &mut Option<Modal>,
    ) {
        if let Some(open) = page.as_mut() {
            if open.onto == onto && open.loading {
                match result {
                    Ok(commits) => open.adopt(commits),
                    Err(err) => open.fail(err.message()),
                }
            }
            return;
        }
        let Some(Modal::AiRebase(recap)) = modal.as_mut() else {
            return;
        };
        if recap.onto != onto || !recap.loading {
            return;
        }
        match result {
            Ok(commits) => recap.adopt(commits),
            Err(err) => recap.fail(err.message()),
        }
    }

    /// `ReadConflicts` reply (conflicts.md §8): fills the open editor's rail while
    /// it is loading. An empty set (the op finalised meanwhile) or a read error
    /// closes the editor; a reply arriving after Close / reopen is ignored — only
    /// a loading editor adopts, mirroring `on_rebase_todo`.
    fn on_conflicts(
        result: Result<Vec<crate::git::conflict::ConflictFile>, git2::Error>,
        editor: &mut Option<ConflictEditorState>,
        toasts: &mut Toasts,
        now: f64,
    ) {
        let Some(open) = editor.as_mut() else {
            return;
        };
        if !open.loading() {
            return;
        }
        match result {
            Ok(files) if files.is_empty() => *editor = None,
            Ok(files) => open.adopt(files),
            Err(err) => {
                *editor = None;
                toasts.error(err.message(), now);
            }
        }
    }

    /// Detail of the selected commit (M9-2) rendered in the right sidebar
    /// (M9-6); we adopt only the detail of the still-selected commit.
    fn on_commit_detail(
        &mut self,
        result: Result<CommitDetail, git2::Error>,
        toasts: &mut Toasts,
        now: f64,
    ) {
        match result {
            Ok(detail) => {
                if self.selected_commit == Some(detail.meta.oid) {
                    self.detail = Some(detail);
                }
            }
            Err(err) => {
                toasts.error(
                    format!("Failed to load the commit details — {}", err.message()),
                    now,
                );
            }
        }
    }

    /// Fullscreen commit diff (M9-7, read-only): attached to the open diff if
    /// it targets this commit and this file. On error, a diff still empty (or
    /// inherited) is closed with a toast — without this, the file click stayed
    /// without effect (the wait never resolved).
    fn on_commit_file_diff(
        oid: git2::Oid,
        result: Result<FileDiff, git2::Error>,
        diff: &mut Option<DiffState>,
        toasts: &mut Toasts,
        now: f64,
    ) {
        match result {
            Ok(file) => {
                if let Some(open) = diff.as_mut() {
                    if open.source == DiffSource::Commit(oid) && open.path == file.path {
                        open.adopt(file);
                    }
                }
            }
            Err(err) => {
                if diff.as_ref().is_some_and(|open| {
                    open.source == DiffSource::Commit(oid)
                        && (open.loaded.is_none() || open.inherited)
                }) {
                    *diff = None;
                    toasts.error(
                        format!("Failed to load the file diff — {}", err.message()),
                        now,
                    );
                }
            }
        }
    }

    /// Git command in progress, as seen by the graph toolbar: network op first
    /// (spinner on its button), otherwise the first mutating command pending in the
    /// worker. Any command ⇒ loader + all other buttons greyed out
    /// (D-2026-06-03-toolbar-loader-commandes-git).
    pub(crate) fn busy_action(&self) -> Option<BusyAction> {
        // The AI rebase holds the mutation lock for its whole run: named
        // end-of-row chip (elapsed time + Cancel), all buttons greyed out.
        if self.ai_rebase.busy() {
            return Some(BusyAction::AiRebase {
                seconds: self.ai_rebase.elapsed().unwrap_or_default().as_secs(),
                cancelling: self.ai_rebase.cancelling(),
            });
        }
        match self.sync.in_flight() {
            Some(SyncCommand::Push | SyncCommand::ForcePush) => Some(BusyAction::Push),
            // Remote deletion and rebase: no dedicated button — generic end-of-row
            // spinner, all buttons greyed out. Exhaustive on purpose: a future
            // command must pick its spinner, not inherit Pull's.
            Some(
                SyncCommand::DeleteRemoteBranch(_)
                | SyncCommand::DeleteRemoteThenLocalBranch { .. }
                | SyncCommand::Rebase(_)
                | SyncCommand::InteractiveRebase { .. }
                | SyncCommand::Merge(_)
                | SyncCommand::CherryPick(_)
                | SyncCommand::Revert(_)
                | SyncCommand::AbortOp
                | SyncCommand::ContinueOp
                | SyncCommand::PushTag(_)
                | SyncCommand::DeleteRemoteThenLocalTag(_),
            ) => Some(BusyAction::Other),
            Some(SyncCommand::FetchAll | SyncCommand::Pull(_)) => Some(BusyAction::Pull),
            None => self.worker.pending_mutation().map(|command| match command {
                GitCommand::Stash => BusyAction::Stash,
                GitCommand::StashPop => BusyAction::Pop,
                GitCommand::CreateBranch(_) => BusyAction::Branch,
                _ => BusyAction::Other,
            }),
        }
    }

    /// End of a network op (M12-3): refresh status + graph, and the outcome goes to a
    /// toast (git.md §10) — auto-expiring success ("Pulled — branch updated"),
    /// persistent failure with the useful message. `Conflicts` **also** toasts: the
    /// sidebar's Merge/Rebase in progress banner shows the lasting state, the toast
    /// says why the op just stopped.
    pub(crate) fn drain_sync(&mut self, graph_mode: bool, toasts: &mut Toasts, now: f64) {
        let replies =
            worker::drain_sync_refresh(&mut self.sync, &self.worker, graph_mode, self.graph_limit);
        for reply in replies {
            match reply.result {
                Ok(outcome) => toasts.success(sync_success_message(reply.command, outcome), now),
                // Upstream gone on the remote (merged elsewhere): pruned in `pull`,
                // surfaced silently — no toast (git.md §10).
                Err(crate::git::sync::SyncError::RemoteBranchGone) => {}
                Err(err) => toasts.error(sync_error_message(reply.command, &err), now),
            }
        }
    }

    /// End of an AI generation: fills the commit card inputs — never an automatic
    /// commit; failure ⇒ persistent toast (git.md §10).
    pub(crate) fn drain_ai(&mut self, panel: &mut GitPanelState, toasts: &mut Toasts, now: f64) {
        while let Some(reply) = self.ai.try_recv() {
            match reply {
                Ok(suggestion) => {
                    panel.subject = suggestion.subject;
                    panel.description = suggestion.description;
                }
                Err(err) => toasts.error(err.message(), now),
            }
        }
    }
}

/// Toast message for a failed git command from the worker (git.md §10): the action in
/// plain words, then the git2 message — never the raw message alone.
pub fn command_failure_message(source: &GitCommand, err: &git2::Error) -> String {
    let action = match source {
        GitCommand::Status => "Git status failed",
        GitCommand::Stage(_)
        | GitCommand::StageAll
        | GitCommand::StageHunk { .. }
        | GitCommand::StageLines { .. } => "Stage failed",
        GitCommand::Unstage(_)
        | GitCommand::UnstageAll
        | GitCommand::UnstageHunk { .. }
        | GitCommand::UnstageLines { .. } => "Unstage failed",
        GitCommand::Discard(_) | GitCommand::DiscardAll | GitCommand::DiscardHunk { .. } => {
            "Discard failed"
        }
        GitCommand::Commit(_) => "Commit failed",
        GitCommand::Stash => "Stash failed",
        GitCommand::StashPop | GitCommand::StashPopAt(_) => "Stash pop failed",
        GitCommand::StashApplyAt(_) => "Applying stash failed",
        GitCommand::StashDropAt(_) => "Deleting stash failed",
        GitCommand::Checkout(name) => {
            return format!("Checkout of '{name}' failed — {}", err.message());
        }
        GitCommand::CreateBranch(name) | GitCommand::CreateBranchAt { name, .. } => {
            return format!("Creating branch '{name}' failed — {}", err.message());
        }
        GitCommand::CreateTagAt { name, .. } => {
            return format!("Creating tag '{name}' failed — {}", err.message());
        }
        GitCommand::DeleteBranch(name) => {
            return format!("Deleting branch '{name}' failed — {}", err.message());
        }
        GitCommand::RenameBranch { from, to } => {
            return format!("Renaming '{from}' to '{to}' failed — {}", err.message());
        }
        GitCommand::CheckoutTag(name) => {
            return format!("Checkout of '{name}' failed — {}", err.message());
        }
        GitCommand::DeleteTag(name) => {
            return format!("Deleting tag '{name}' failed — {}", err.message());
        }
        GitCommand::Reset { .. } => "Reset failed",
        GitCommand::ResolveFile { .. } => "Saving the resolution failed",
        GitCommand::ResolveFileSide { .. } => "Taking the side failed",
        // Reads: answered by their own variant, never by `Status`.
        GitCommand::Diff { .. }
        | GitCommand::Graph { .. }
        | GitCommand::RebaseTodo { .. }
        | GitCommand::CommitDetail(_)
        | GitCommand::CommitFileDiff { .. }
        | GitCommand::ReadConflicts => "Git command failed",
    };
    format!("{action} — {}", err.message())
}
