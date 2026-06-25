use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::agent_watch::{AgentBadge, PaneAgentState};
use crate::ai::{AiProvider, AiRunner};
use crate::git::ai_rebase::{AiRebaseReport, AiRebaseRequest, AiRebaseRunner};
use crate::git::branch::Branch;
use crate::git::commit_detail::CommitDetail;
use crate::git::diff::FileDiff;
use crate::git::graph::{self, Graph, LaneCache};
use crate::git::rebase::RebaseCommit;
use crate::git::status::RepoStatus;
use crate::git::worker::{
    self, FetchRunner, GitCommand, GitResult, GitWorker, MutationLock, RepoSnapshot, ResultKind,
    SyncCommand, SyncRunner,
};
use crate::git::worktree::{DeleteReply, DeleteRequest, DeleteRunner};
use crate::keybindings::{Action, Keymap, Shortcut};
use crate::persistence::Prefs;
use crate::terminal::emu::FontZoom;
use crate::terminal::layout::{Dir, Layout, Orient, PaneId, Rect};
use crate::terminal::links::{Editor, LinkAction};
use crate::terminal::palette::TermPalette;
use crate::terminal::pane::Pane;
use crate::theme::{self, ThemeMode};
use crate::ui::ai_rebase_modal::{ai_rebase_modal, ai_rebase_report_modal, AiRebasePage};
use crate::ui::conflict_view::{
    conflict_view, ConflictEditorAction, ConflictEditorState, ResolveRequest,
};
use crate::ui::diff_view::{diff_view, DiffViewState};
use crate::ui::feedback_modal::{feedback_modal, FeedbackPage};
use crate::ui::git_panel::{abort_op_modal, discard_hunk_modal, GitIntent, GitPanelState};
use crate::ui::graph_toolbar::{
    force_push_modal, graph_toolbar, reset_hard_modal, sync_error_message, sync_success_message,
    BusyAction, PullDefault, ToolbarAction, ToolbarState,
};
use crate::ui::graph_view::{
    delete_branch_modal, delete_stash_modal, delete_tag_modal, graph_view, BranchEditor,
    BranchEditorTarget, DeleteBranchTarget, GraphAction, GraphSearch, GraphViewState, StashTarget,
    WipRow,
};
use crate::ui::preferences::{preferences_page, KeyboardState, PreferencesSection, UpdatesView};
use crate::ui::rebase_view::{rebase_view, RebasePage, RebasePageAction};
use crate::ui::repo_sidebar::{
    delete_worktree_modal, CreateSelection, DeleteModalAction, DeletePrompt, ProjectHeader,
    ProjectVisibility, RepoRow, SidebarAction, SidebarItem,
};
use crate::ui::tab_bar::{tab_bar, TabBarAction, TabRename};
use crate::ui::terminal_view::{
    cell_metrics, terminal_tree, terminal_view, terminal_view_preview, terminal_view_readonly,
};
use crate::ui::toast::{toast_overlay, Toasts};
use crate::ui::{central_empty_state, central_switch, root_layout, TITLEBAR_HEIGHT};
use crate::update::{self, UpdateOutcome, UpdateRunner};
use crate::workspace::{GroupSync, Repo, TabId, Workspace};
use crate::workspace_launcher::{
    installed_openers, open_workspace as launch_workspace, resolve_default, WorkspaceOpener,
};

const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;
/// Git status refresh cadence: the worker is re-queried at a fixed interval to
/// reflect external changes (editing, git from the terminal).
const GIT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
/// Worktree discovery/purge cadence while the window is focused (worktrees.md §4):
/// a 4th sync trigger so a worktree created from a terminal appears without a
/// defocus/refocus round-trip. Off-focus the focus-regain trigger already covers it.
const GROUP_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// Workspace PR refresh cadence while the cockpit is open and focused
/// (pull-requests.md §6): network-bound (`gh`/`curl`), so far coarser than the
/// git/worktree ticks.
const PR_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
type Panes = HashMap<PaneId, TerminalState>;

mod keys;
use keys::{action_pressed, open_agents_pressed, overlay_or_command};
pub use keys::{
    focus_zone, route_cycle_repo_keys, route_layout_keys, route_select_repo_keys, route_tab_keys,
    route_zoom_keys,
};

mod git_session;
pub use git_session::command_failure_message;
use git_session::{
    repainter, AgentEntry, CommitDraft, DiffSource, DiffState, GitSession, PaneKey, RepoCaches,
    RepoKey,
};

mod render;

/// What the central area renders (M9-4). The header switch toggles between the
/// terminal (MVP core) and the Git graph (post-MVP, read-only). Graph rendering
/// arrives in M9-5; switching to `Graph` reveals the git sidebar and triggers a
/// `Graph` load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CentralMode {
    #[default]
    Terminal,
    Graph,
    /// Cross-repo agents dashboard in the central area (specs/agents.md §5): the
    /// project sidebar stays, the per-repo git panel is hidden.
    Agents,
    /// Workspace pull-requests cockpit in the central area
    /// (specs/pull-requests.md §6): my PRs and PRs to review across the
    /// workspace repos, fetched on entry and on a focused tick.
    PullRequests,
}

/// Target of a pending Delete worktree: what's needed to re-run with force after the
/// dirty modal (the libgit2 name is re-resolved by path in the thread).
struct PendingDelete {
    root: PathBuf,
    path: PathBuf,
    label: String,
    prompt: DeletePrompt,
}

/// Worktree creation modal state: sources are pre-filtered by the domain and the
/// selected source is revalidated by the async runner before writing.
struct PendingCreate {
    root: PathBuf,
    root_label: String,
    /// Per-project worktree base captured at open (worktrees.md §6); threads into
    /// the source list, the path preview and the create request.
    base: Option<PathBuf>,
    sources: Option<Vec<crate::git::worktree::WorktreeSource>>,
    /// Names already taken (lowercased) and the base label, both filled from the
    /// async source reply: they gate and label the on-the-fly "Create branch" row.
    taken: HashSet<String>,
    base_branch: String,
    selected: Option<CreateSelection>,
    error: Option<String>,
    view: crate::ui::repo_sidebar::CreateWorktreeState,
}

/// Create request for the modal's current selection: an existing source by index,
/// or the on-the-fly new branch typed in the filter. Only a folder name diverging
/// from the followed branch travels — the domain re-derives the path otherwise.
fn create_request_from(
    pending: &PendingCreate,
) -> Option<(
    PathBuf,
    crate::git::worktree::CreateSource,
    Option<String>,
    Option<PathBuf>,
)> {
    use crate::git::worktree::CreateSource;
    let (source, follow) = match pending.selected? {
        CreateSelection::Source(index) => {
            let source = pending.sources.as_ref()?.get(index)?;
            (
                CreateSource::Existing(source.name.clone()),
                source.local_branch.clone(),
            )
        }
        CreateSelection::NewBranch => {
            let branch = pending.view.query.trim().to_owned();
            if branch.is_empty() {
                return None;
            }
            (CreateSource::NewBranch(branch.clone()), branch)
        }
    };
    let name = pending.view.effective_name(&follow);
    let custom = (name != follow).then(|| name.to_owned());
    Some((pending.root.clone(), source, custom, pending.base.clone()))
}

/// Edit buffers for the active project's settings while the Preferences "Project"
/// section is open (worktrees.md §6); rebuilt when the active project changes.
/// The page mutates these; the app writes them back to `prefs` on change.
struct ProjectSettingsEdit {
    root: PathBuf,
    worktree_base: String,
    post_create: String,
    run_command: String,
    base_port: String,
}

/// One-shot post-create command (worktrees.md §6): set when a worktree is created
/// with a non-empty script, consumed the moment that worktree's first terminal
/// goes live — the script is typed into it, the env exported invisibly.
struct PendingPostCreate {
    /// Canonical worktree path; matched against the active repo before injecting.
    worktree_path: PathBuf,
    script: String,
    env: Vec<(&'static str, String)>,
}

/// At most one confirmation modal on screen (M17-8): a single field makes the
/// invariant structural — confirm dispatches per variant, dismiss closes
/// whichever is open.
enum Modal {
    /// Delete worktree awaiting arbitration (worktrees.md §6): dirty modal or
    /// refusal (locked / git error).
    DeleteWorktree(PendingDelete),
    /// Create worktree from a source branch selected in the sidebar modal.
    CreateWorktree(PendingCreate),
    /// Branch deletion (local or remote) from the graph context menu (git.md §9).
    DeleteBranch(DeleteBranchTarget),
    /// Stash deletion from a stash row's context menu (git.md §9).
    DropStash(StashTarget),
    /// Tag deletion from the graph tag menu (git.md §9): names the tag;
    /// `also_remote` mirrors the "Also delete on origin" checkbox — checked, the
    /// remote deletion runs first on the sync runner, then the local one.
    DeleteTag { tag: String, also_remote: bool },
    /// Hard reset from the graph row menu (git.md §9): names the `branch` it
    /// moves and the `short` target sha; destructive (the working tree is
    /// overwritten), confirmed before the `git2` reset runs on the worker.
    ResetHard {
        branch: String,
        target: git2::Oid,
        short: String,
    },
    /// Abort of the merge/rebase in progress (banner button, git.md §10):
    /// resolutions in progress are discarded — confirmed before anything runs.
    AbortOp,
    /// Force push (with lease) from the toolbar Push chevron (git.md §10): names
    /// the `branch` and its `remote`; confirmed before `--force-with-lease` runs.
    ForcePush { branch: String, remote: String },
    /// Discard of a single unstaged hunk from the diff view (git.md §4): reverting
    /// the working tree cannot be undone — confirmed before it runs. `path` is the
    /// open file, captured when the intent is raised.
    DiscardHunk { path: String, hunk: usize },
    /// AI rebase recap (git.md §9): commits to replay + extra AI instructions;
    /// Start hands the request to the session's AI rebase runner.
    AiRebase(AiRebasePage),
    /// AI rebase report: the provider's account once the run completed, under
    /// the outcome verified on the repo.
    AiRebaseReport(AiRebaseReport),
    /// Feedback report (specs/feedback.md): Suggestion/Bug + description, filed
    /// as a GitHub issue on the helm repo via the browser.
    Feedback(FeedbackPage),
    /// Release notes shown once after an update (update.md §9): renders the
    /// bundled `release-notes.md`; carries no data.
    WhatsNew,
}

/// The only truly exclusive route (M17-10): Preferences replaces the 3 zones
/// full-window (preferences.md §2); everything else — sidebars, central mode,
/// overlays — composes within `Main`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Main,
    Preferences,
}

impl Page {
    /// ⌘, and the header gear both toggle the same route.
    fn toggled(self) -> Page {
        match self {
            Page::Main => Page::Preferences,
            Page::Preferences => Page::Main,
        }
    }
}

/// The two sidebars are independent (⌘B / ⌘G can both be open) — grouped for
/// persistence, **never** collapsed into one exclusive enum (M17-10).
#[derive(Clone, Copy, PartialEq, Eq)]
struct SidebarVisibility {
    workspace: bool,
    git: bool,
}

pub struct HelmApp {
    theme_mode: ThemeMode,
    /// Theme families per mode (`theme::PRESETS`), persisted: a single choice
    /// recolors chrome, terminal and diff together.
    light_theme: String,
    dark_theme: String,
    caches: RepoCaches,
    font_zoom: FontZoom,
    central_mode: CentralMode,
    /// Agent picked in the cross-repo dashboard (specs/agents.md §5): its pane is
    /// mirrored live in the dashboard's right panel. The stable triple survives
    /// the per-tick rebuild of `caches.agents`; a selection whose tab/pane closed
    /// is dropped (and the most urgent agent re-picked) each frame.
    selected_agent: Option<(RepoKey, TabId, PaneId)>,
    /// Cross-repo dashboard layout (specs/agents.md §5): master-detail list or the
    /// multi-terminal column grid. Persisted; toggled from the dashboard header.
    agents_view: crate::ui::agents_view::AgentsViewMode,
    /// Shared flat/tree mode of the Git file lists — WIP sections and the
    /// commit-detail "Files changed" (specs/git.md, M40). Persisted; toggled from
    /// either header.
    git_file_view: crate::ui::file_list::FileViewMode,
    /// Shared column width of the dashboard's columns view (specs/agents.md §5),
    /// live source for rendering; mirrored into `Prefs` on drag (persisted).
    agents_column_width: f32,
    /// Shared terminal-card height of the dashboard's columns view; live source
    /// for rendering, mirrored into `Prefs` on drag (persisted).
    agents_terminal_height: f32,
    /// Height of the Run terminal strip in the git sidebar (git.md §3); live source
    /// for rendering, mirrored into `Prefs` on drag (persisted).
    run_panel_height: f32,
    /// Run terminal strip folded to its header (git.md §3); persisted.
    run_panel_collapsed: bool,
    /// Inline edit buffer of the Run command, while the header's pencil is active
    /// (git.md §3); `None` when not editing. Committing writes it to the project's
    /// `ProjectSettings.run_command`.
    run_command_edit: Option<String>,
    /// Inline edit buffer of the worktree's `$PORT` override, while the header's
    /// port chip is active (git.md §3); `None` when not editing. Mutually exclusive
    /// with `run_command_edit`.
    run_port_edit: Option<String>,
    /// Wheel `End` withheld by `rewrite_wheel_phases` until the next frame's
    /// hook: replayed there (after egui zeroed its per-frame scroll buffer) so
    /// the gesture reset never destroys motion already accumulated this frame.
    deferred_wheel_end: Option<egui::Event>,
    sidebars: SidebarVisibility,
    page: Page,
    /// Active section of the Preferences page — session memory only
    /// (preferences.md §5): not persisted, defaults to Appearance on each launch.
    preferences_section: PreferencesSection,
    /// Keyboard-section recorder (preferences.md §4): while armed, `Esc` and the
    /// preferences toggle are captured as combos instead of acting.
    keyboard_prefs: KeyboardState,
    workspace: Workspace,
    git: Option<GitSession>,
    git_panel_state: GitPanelState,
    diff: Option<DiffState>,
    /// Interactive-rebase page (git.md §9), replacing the graph while open:
    /// created on the menu click (loading), filled by the worker's `RebaseTodo`
    /// reply, dropped on Start/Cancel and on repo switch (stale plan).
    rebase_page: Option<RebasePage>,
    /// In-app conflict editor (conflicts.md §2), taking over the central zone:
    /// opened from the banner's Resolve or a conflicted row, filled by the
    /// worker's `ReadConflicts` reply, dropped on Close / Continue / op end /
    /// repo switch. Exists only while an operation is in progress.
    conflict_editor: Option<ConflictEditorState>,
    /// Last-used launcher app (shown on the main button) and the apps actually
    /// installed on this Mac (the menu choices), detected once at startup.
    workspace_opener: WorkspaceOpener,
    installed_openers: Vec<WorkspaceOpener>,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    tab_rename: Option<TabRename>,
    modal: Option<Modal>,
    /// Worktree deletion running on a **dedicated thread**: pruning the folder (dirty
    /// status included) took long seconds on a large worktree and froze the UI thread.
    /// Created on the first Delete (it carries the `ctx` repaint).
    worktree_delete: Option<DeleteRunner>,
    /// Branch-source enumeration for the Create worktree modal runs off the UI thread:
    /// large repos can have many refs/worktrees and opening the modal must stay instant.
    worktree_sources: Option<crate::git::worktree::SourceRunner>,
    /// Worktree creation also runs off the UI thread: checking out a branch can touch
    /// many files and must not freeze rendering.
    worktree_create: Option<crate::git::worktree::CreateRunner>,
    /// PR Checkout's off-thread fetch (pull-requests.md §7): brings the PR source
    /// branch local, then hands off to `worktree_create`.
    worktree_checkout: Option<crate::pull_requests::runner::CheckoutRunner>,
    /// One-shot post-create script injected into a freshly created worktree's first
    /// terminal (worktrees.md §6).
    pending_post_create: Option<PendingPostCreate>,
    /// Edit buffers of the Preferences "Project" section, scoped to the active
    /// project (worktrees.md §6).
    project_settings_edit: Option<ProjectSettingsEdit>,
    /// Project the "Project" section configures — session memory (preferences.md
    /// §5): seeded to the active project each time the page opens, switchable via
    /// the section's picker. `None` falls back to the first workspace project.
    selected_project: Option<PathBuf>,
    /// Default operation of the Pull split-button (git.md §10), persisted in
    /// `prefs.toml` (M12-7): loaded at boot, saved on change in the menu.
    pull_default: PullDefault,
    /// Provider + instructions of the AI commit message, persisted in `prefs.toml`:
    /// loaded at boot, saved on change in Preferences.
    ai_provider: AiProvider,
    ai_instructions: String,
    /// Provider of the AI rebase (git.md §9), configured separately from the
    /// commit-message one — the rebase invocation is agentic (runs git itself).
    ai_rebase_provider: AiProvider,
    /// CLI the in-diff review's "Send to {agent}" button launches (M-RC),
    /// persisted in `prefs.toml`: loaded at boot, saved on change in Preferences.
    review_agent_command: String,
    /// In-diff review comments accumulated per repo (M-RC), in memory only: the
    /// active repo's set feeds the diff view and the `Send` prompt.
    review: HashMap<RepoKey, crate::review::FileComments>,
    /// IDE opening terminal Cmd+click file links (terminal.md §12), persisted in
    /// `prefs.toml`: loaded at boot, saved on change in Preferences.
    editor: Editor,
    /// Native banner on agent completion (specs/agents.md), persisted in
    /// `prefs.toml`: loaded at boot, toggled in Preferences.
    notify_on_agent_completion: bool,
    /// Branch editor (M12-6): opened by the toolbar button, rendered by `graph_view`
    /// on the HEAD row; stays open while waiting for the worker, which writes the
    /// inline error into it or closes it on success.
    branch_editor: BranchEditor,
    /// In-graph commit search (⌘F, git.md §9): floating box rendered by
    /// `graph_view`, filters the loaded commits and cycles through the matches.
    /// Reset whenever the central mode switches.
    graph_search: GraphSearch,
    /// Markdown layout cache shared by the What's new modal and the Preferences
    /// release-notes block (update.md §9.4) — both render the same bundled file.
    commonmark_cache: egui_commonmark::CommonMarkCache,
    /// Global notifications (git.md §10): outcomes of git actions (persistent errors,
    /// auto-expiring network successes), rendered as a bottom-right overlay in **all**
    /// modes — the old banner was only visible in Graph.
    toasts: Toasts,
    last_agent_poll: f64,
    last_group_poll: f64,
    /// Workspace PR fetch running off the UI thread (pull-requests.md §6): `gh`
    /// and `curl` calls plus libgit2 remote resolution must not freeze rendering.
    /// Created lazily on the first refresh (it carries the `ctx` repaint).
    pr_runner: Option<crate::pull_requests::runner::PrRunner>,
    /// Last PR fetch result shown in the cockpit, refreshed in place each reply.
    pr_cache: crate::pull_requests::runner::PrCache,
    last_pr_poll: f64,
    /// Selected PR row in the cockpit, indexing `pr_cache.pull_requests`; drives
    /// the detail panel. Session-only, like the cache itself.
    pr_selected: Option<usize>,
    /// Width of the cockpit's detail panel (pull-requests.md §5); live source for
    /// rendering, mirrored into `Prefs` on drag (persisted).
    pr_detail_width: f32,
    /// Bitbucket account email bound to the Preferences field (pull-requests.md
    /// §3): live mirror of `prefs.bitbucket_email`, persisted on edit.
    bitbucket_email: String,
    /// Transient Bitbucket token typed in Preferences, before "Save" writes it to
    /// the Keychain (pull-requests.md §3) — never persisted, cleared after a save
    /// and each time the page opens.
    bitbucket_token_input: String,
    /// Prefs file to rewrite. `None` (constructors) = ephemeral app: tests and headless
    /// verification must never touch the user's real TOML; only `run()` injects the
    /// persisted path.
    prefs_path: Option<PathBuf>,
    /// In-memory prefs, single base for every `persist` (M17-6): the TOML is no
    /// longer re-read per change, and writes are debounced (`prefs_dirty_at`).
    prefs: Prefs,
    /// Resolved shortcuts (keybindings.md §6): `prefs.keybindings` over the spec
    /// defaults, rebuilt whenever the keybindings prefs change.
    keymap: Keymap,
    /// Last `persist` call; trailing debounce — flushed once `PREFS_DEBOUNCE`
    /// elapses without a new change, and unconditionally on save/exit.
    prefs_dirty_at: Option<Instant>,
    /// In-app updater (update.md): created on the first frame (it carries the `ctx`
    /// repaint) with a silent boot check — a no-op outside an `.app` bundle.
    update_runner: Option<UpdateRunner>,
    /// Off-thread refresh of the sidebar branch labels + dirty stats (sync triggers):
    /// the full per-dirty-repo diff this pass runs cost ~1-2s on a large/dirty
    /// workspace and froze the focus-regain frame when it lived on the UI thread.
    /// Created lazily by `run_group_sync` (it carries the `ctx` repaint).
    group_refresh: Option<GroupRefreshRunner>,
    /// Last fullscreen state pushed to the native titlebar accessory
    /// (`titlebar::sync_fullscreen`): the strip must hide in fullscreen, where
    /// the reveal band would otherwise draw it on the opaque system bar.
    titlebar_fullscreen: Option<bool>,
    /// Frame-pacing instrumentation (`HELM_FRAME_LOG=1`), `None` otherwise.
    frame_log: Option<crate::frame_log::FrameLog>,
}

impl Default for HelmApp {
    fn default() -> Self {
        Self::from_prefs(Prefs::default())
    }
}

impl HelmApp {
    pub fn pane_count(&self) -> usize {
        self.workspace
            .active_layout()
            .map_or(0, |layout| layout.pane_ids().len())
    }

    /// Closes tab `tab` of the active repo: its id vanishes from the workspace and
    /// the cache sync drops its PTY set (terminal.md §11). Closing the last tab does
    /// not remove the repo — it restarts on a fresh id, so the old set is dropped
    /// and the key repopulates on the next render.
    pub fn close_active_tab(&mut self, tab: usize) -> bool {
        if !self.workspace.close_tab(tab) {
            return false;
        }
        self.caches.sync(&self.workspace);
        true
    }

    /// Rebuilds the application state from the persisted preferences (M7-5,
    /// architecture §4): ordered repos + active one, theme, widths and open/closed
    /// state of the sidebars. PTYs and window geometry are not restored (eframe handles
    /// geometry).
    pub fn from_prefs(prefs: Prefs) -> Self {
        let snapshot = prefs.clone();
        let mut workspace = Workspace::new();
        for project in prefs.projects {
            let collapsed = project.collapsed;
            let hidden = project.hidden;
            let root = Repo::new(project.root.clone());
            let root_index = if project.worktrees.is_empty() {
                workspace.add(root)
            } else {
                let children = project.worktrees.into_iter().map(Repo::new).collect();
                let root_index = workspace.add_group(root, children);
                workspace.set_collapsed(root_index, collapsed);
                root_index
            };
            workspace.set_user_hidden(root_index, hidden);
        }
        if let Some(active) = prefs.active {
            let index = workspace.repos().position(|r| r.path == active);
            if let Some(index) = index {
                workspace.set_active(index);
            }
        }
        let mut caches = RepoCaches::default();
        caches.sync(&workspace);
        let installed_openers = installed_openers();
        let workspace_opener = resolve_default(prefs.workspace_opener, &installed_openers);
        Self {
            theme_mode: prefs.theme,
            light_theme: prefs.light_theme,
            dark_theme: prefs.dark_theme,
            caches,
            font_zoom: FontZoom::default(),
            central_mode: CentralMode::default(),
            selected_agent: None,
            agents_view: prefs.agents_view,
            git_file_view: prefs.git_file_view,
            agents_column_width: prefs.agents_column_width,
            agents_terminal_height: prefs.agents_terminal_height,
            run_panel_height: prefs.run_panel_height,
            run_panel_collapsed: prefs.run_panel_collapsed,
            run_command_edit: None,
            run_port_edit: None,
            deferred_wheel_end: None,
            sidebars: SidebarVisibility {
                workspace: prefs.show_workspace,
                git: prefs.show_git,
            },
            page: Page::Main,
            preferences_section: PreferencesSection::default(),
            keyboard_prefs: KeyboardState::default(),
            workspace,
            git: None,
            git_panel_state: GitPanelState::default(),
            diff: None,
            rebase_page: None,
            conflict_editor: None,
            workspace_opener,
            installed_openers,
            left_sidebar_width: prefs.left_sidebar_width,
            right_sidebar_width: prefs.right_sidebar_width,
            tab_rename: None,
            modal: None,
            worktree_delete: None,
            worktree_sources: None,
            worktree_create: None,
            worktree_checkout: None,
            pending_post_create: None,
            project_settings_edit: None,
            selected_project: None,
            pull_default: prefs.pull_default,
            ai_provider: prefs.ai_provider,
            ai_instructions: prefs.ai_instructions,
            ai_rebase_provider: prefs.ai_rebase_provider,
            review_agent_command: prefs.review_agent_command,
            review: HashMap::new(),
            editor: prefs.editor,
            notify_on_agent_completion: prefs.notify_on_agent_completion,
            branch_editor: BranchEditor::default(),
            graph_search: GraphSearch::default(),
            commonmark_cache: egui_commonmark::CommonMarkCache::default(),
            toasts: Toasts::default(),
            last_agent_poll: 0.0,
            last_group_poll: 0.0,
            pr_runner: None,
            pr_cache: crate::pull_requests::runner::PrCache::default(),
            last_pr_poll: 0.0,
            pr_selected: None,
            pr_detail_width: prefs.pr_detail_width,
            bitbucket_email: prefs.bitbucket_email.clone(),
            bitbucket_token_input: String::new(),
            prefs_path: None,
            keymap: snapshot.keymap(),
            prefs: snapshot,
            prefs_dirty_at: None,
            update_runner: None,
            group_refresh: None,
            titlebar_fullscreen: None,
            frame_log: crate::frame_log::FrameLog::from_env(),
        }
    }

    // Seeding seam: opening a repo in prod goes through the native `rfd` dialog
    // (NSOpenPanel), undrivable headless. This constructor injects an already-populated
    // workspace for headless verification. See the `headless-verify` skill.
    pub fn with_workspace(workspace: Workspace) -> Self {
        let mut app = Self {
            workspace,
            ..Default::default()
        };
        app.caches.sync(&app.workspace);
        app.caches
            .set_branch_labels(workspace_branches(&app.workspace));
        app.caches
            .set_dirty_stats(workspace_dirty_stats(&app.workspace));
        app
    }

    /// Parks the active session's per-repo state before the session is dropped on a
    /// repo switch (or release): the last graph (redrawn instantly on return) and the
    /// commit-drafting state — the message draft (so it never shows under another repo)
    /// and the AI runner (so an in-flight generation survives the switch instead of
    /// being cancelled with the dropped session). Empties `git_panel_state` so the next
    /// repo starts from its own draft.
    fn park_active_session(&mut self) {
        let Some(old) = self.git.take() else {
            return;
        };
        if let Some(graph) = old.graph {
            self.caches
                .graph_cache
                .insert(old.key.clone(), (graph, old.graph_limit));
        }
        self.caches.commit_drafts.insert(
            old.key,
            CommitDraft {
                subject: std::mem::take(&mut self.git_panel_state.subject),
                description: std::mem::take(&mut self.git_panel_state.description),
                ai: old.ai,
            },
        );
    }

    /// Aligns the git session on the active repo: re-spawn on switch, release for a
    /// non-git repo or in the absence of an active repo. A repo change also closes the
    /// diff overlay view (git.md §4) and the fullscreen commit diff (§9).
    fn sync_git_session(&mut self, ctx: &egui::Context) {
        let target = self
            .workspace
            .active()
            .and_then(|i| self.workspace.repo(i).map(|r| (i, r)));
        match target {
            Some((index, repo)) => {
                let needs_spawn = self.git.as_ref().map(|g| g.index) != Some(index);
                if needs_spawn {
                    // Owned before parking: `park_active_session` reborrows all of `self`,
                    // while `repo` still borrows `self.workspace`.
                    let path = repo.path.clone();
                    // Park the left-behind repo's state (graph for an instant redraw,
                    // commit draft + AI runner so a draft never shows under another repo
                    // and an in-flight generation is not cancelled).
                    self.park_active_session();
                    // Restore this repo's parked draft (the active draft lives in
                    // `git_panel_state`), reattaching its runner — a fresh one for a repo
                    // opened for the first time this session.
                    let key = RepoKey::of(&path);
                    let ai = match self.caches.commit_drafts.remove(&key) {
                        Some(draft) => {
                            self.git_panel_state.subject = draft.subject;
                            self.git_panel_state.description = draft.description;
                            draft.ai
                        }
                        None => AiRunner::new(&path, repainter(ctx)),
                    };
                    let mut session = GitSession::spawn(index, &path, ctx, ai);
                    if let Some((graph, limit)) = self.caches.graph_cache.remove(&session.key) {
                        // HEAD may have moved during the absence (checkout in the repo's
                        // terminal): the scroll-to-head auto-scroll waits for the fresh
                        // graph rather than targeting a stale row.
                        session.graph = Some(graph);
                        session.graph_limit = limit;
                        session.graph_fresh = false;
                    }
                    // Already in Graph mode (repo switch, reopen): graph requested
                    // **before** the status — the worker is sequential and the status
                    // (full scan + line stats) would delay the visible central content.
                    if self.central_mode == CentralMode::Graph {
                        session.reload_graph();
                    }
                    session.worker.send(GitCommand::Status);
                    self.git = Some(session);
                    self.diff = None;
                    self.branch_editor = BranchEditor::default();
                    // The plan targets the left repo's refs: always stale here.
                    self.rebase_page = None;
                    // The editor's rail belongs to the previous repo's index.
                    self.conflict_editor = None;
                    self.close_ai_rebase_modal();
                }
            }
            None => {
                self.park_active_session();
                self.diff = None;
                self.branch_editor = BranchEditor::default();
                self.rebase_page = None;
                self.conflict_editor = None;
                self.close_ai_rebase_modal();
            }
        }
        let graph_mode = self.central_mode == CentralMode::Graph;
        if let Some(git) = &mut self.git {
            let now = ctx.input(|i| i.time);
            git.poll(now, self.diff.as_ref(), graph_mode);
            git.poll_background_fetch(now);
            git.drain_sync(graph_mode, &mut self.toasts, now);
            git.drain_ai(&mut self.git_panel_state, &mut self.toasts, now);
            git.drain(
                &mut self.diff,
                &mut self.branch_editor,
                &mut self.git_panel_state,
                &mut self.rebase_page,
                &mut self.conflict_editor,
                &mut self.modal,
                &mut self.toasts,
                now,
            );
            // End of an AI rebase: refresh status (+ graph in Graph mode) and
            // show the provider's report under its verified outcome — a report
            // modal takes precedence over whatever modal is open (losing the
            // account of a history rewrite would be worse). Failures go to a
            // toast; the banner tells a rebase left in progress either way.
            if let Some(reply) = git.ai_rebase.try_recv() {
                git.worker.send(GitCommand::Status);
                if graph_mode {
                    git.reload_graph();
                }
                match reply {
                    Ok(report) => self.modal = Some(Modal::AiRebaseReport(report)),
                    Err(err) => self.toasts.error(err.message(), now),
                }
            }
            // Wake the idle app so the next poll fires (reactive mode).
            ctx.request_repaint_after(GIT_POLL_INTERVAL);
        }
    }

    /// Closes the AI rebase surfaces on repo switch: the recap targets the left
    /// repo's refs and the report describes it — both stale here. The other
    /// modal variants survive the switch unchanged.
    fn close_ai_rebase_modal(&mut self) {
        if matches!(
            self.modal,
            Some(Modal::AiRebase(_) | Modal::AiRebaseReport(_))
        ) {
            self.modal = None;
        }
    }

    /// 1 s tick of agent detection (specs/agents.md): probes the foreground process
    /// group of each live pane, advances its state machine, then aggregates per
    /// workspace entry for the sidebar badge.
    fn update_agent_watch(&mut self, ctx: &egui::Context) {
        if self.caches.panes.is_empty() {
            self.caches.agent_badges.clear();
            self.caches.agents.clear();
            return;
        }
        // Idle wake-up: transitions to the green state / disappearance happen on
        // **silence** — no output would trigger the repaint.
        ctx.request_repaint_after(GIT_POLL_INTERVAL);
        let now = ctx.input(|i| i.time);
        if now - self.last_agent_poll < GIT_POLL_INTERVAL.as_secs_f64() {
            return;
        }
        self.last_agent_poll = now;

        // Seeing a pane acknowledges its green — but only on the terminal view: the
        // dashboard lists completions, so reading it must not auto-ack them.
        let focused_key = (self.page == Page::Main
            && !matches!(
                self.central_mode,
                CentralMode::Agents | CentralMode::PullRequests
            )
            && ctx.input(|i| i.focused))
        .then(|| {
            let index = self.workspace.active()?;
            let tab = self.workspace.active_tab()?;
            self.caches.pane_key(&self.workspace, index, tab)
        })
        .flatten();
        let now_ms = crate::terminal::activity::now_ms();
        // Previous tick's per-pane badge + agent name: the rising edge into
        // `Done` fires the notification, and a just-departed agent (the green
        // outlives the probe by one tolerated absent tick) keeps its name.
        let prev: HashMap<(PaneKey, PaneId), (AgentBadge, &'static str)> = self
            .caches
            .agents
            .iter()
            .map(|e| {
                (
                    ((e.repo_key.clone(), e.tab_id), e.pane_id),
                    (e.badge, e.agent),
                )
            })
            .collect();
        let repo_names: HashMap<RepoKey, String> = self
            .caches
            .keys
            .iter()
            .enumerate()
            .filter_map(|(i, key)| Some((key.clone(), self.workspace.repo(i)?.name.clone())))
            .collect();
        let group_names: HashMap<RepoKey, String> = self
            .caches
            .keys
            .iter()
            .enumerate()
            .filter_map(|(i, key)| Some((key.clone(), self.workspace.project_name(i)?)))
            .collect();
        self.caches.agent_watch.retain(|(key, pane), _| {
            self.caches
                .panes
                .get(key)
                .is_some_and(|panes| panes.contains_key(pane))
        });
        let mut badges: HashMap<RepoKey, AgentBadge> = HashMap::new();
        let mut entries: Vec<AgentEntry> = Vec::new();
        let mut notifications: Vec<(String, String)> = Vec::new();
        for (key, panes) in &self.caches.panes {
            for (pane_id, state) in panes {
                let TerminalState::Live(pane) = state else {
                    continue;
                };
                let snapshot = pane.activity().snapshot();
                let agent = pane
                    .foreground_pgid()
                    .and_then(crate::agent_watch::probe::foreground_agent);
                let badge = self
                    .caches
                    .agent_watch
                    .entry((key.clone(), *pane_id))
                    .or_default()
                    .tick(
                        agent.is_some(),
                        &snapshot,
                        focused_key.as_ref() == Some(key),
                        now_ms,
                    );
                let slot = badges.entry(key.0.clone()).or_insert(AgentBadge::None);
                *slot = (*slot).max(badge);
                if badge == AgentBadge::None {
                    continue;
                }
                let prev_entry = prev.get(&(key.clone(), *pane_id));
                let agent = agent
                    .or_else(|| prev_entry.map(|(_, name)| *name))
                    .unwrap_or("agent");
                let prev_badge = prev_entry.map_or(AgentBadge::None, |(b, _)| *b);
                let repo_name = repo_names.get(&key.0).cloned().unwrap_or_default();
                let group_name = group_names.get(&key.0).cloned().unwrap_or_default();
                let branch = self.caches.branch_labels.get(&key.0).cloned();
                if self.notify_on_agent_completion
                    && crate::agent_watch::newly_completed(prev_badge, badge)
                {
                    notifications.push(crate::notify::completion_message(
                        agent,
                        &repo_name,
                        branch.as_deref(),
                    ));
                }
                entries.push(AgentEntry {
                    repo_key: key.0.clone(),
                    group_name,
                    branch,
                    tab_id: key.1,
                    tab_name: self
                        .workspace
                        .tab_label(key.1)
                        .unwrap_or_else(|| "Terminal".to_owned()),
                    pane_id: *pane_id,
                    agent,
                    badge,
                    last_output_ms: snapshot.last_spont_output_ms,
                });
            }
        }
        // `panes` is a map (arbitrary order); the dashboard groups *consecutive*
        // rows by project, so order by workspace position (a root and its
        // worktrees sit adjacent there) then tab/pane.
        entries.sort_by_key(|e| {
            let repo = self
                .caches
                .keys
                .iter()
                .position(|k| k == &e.repo_key)
                .unwrap_or(usize::MAX);
            (repo, e.tab_id, e.pane_id.0)
        });
        self.caches.agent_badges = badges;
        self.caches.agents = entries;
        for (title, body) in notifications {
            std::thread::spawn(move || {
                let _ = crate::notify::notify(&title, &body);
            });
        }

        // Tab auto-naming (terminal.md §4): name each tab after the current
        // activity of its focused pane; the workspace keeps it sticky.
        for (key, panes) in &self.caches.panes {
            let tab_id = key.1;
            let candidate = self
                .workspace
                .tab_focus(tab_id)
                .and_then(|pane_id| panes.get(&pane_id))
                .and_then(|state| match state {
                    TerminalState::Live(pane) => Some(pane),
                    _ => None,
                })
                .and_then(name_candidate);
            self.workspace
                .refresh_auto_name(tab_id, candidate.as_deref());
        }
    }

    /// Re-reads the effective sidebar widths after render and persists them if the user
    /// resized them (M7-5, architecture §4). With `persist_egui_memory` disabled, the
    /// TOML is the single source of truth for the widths.
    fn persist_sidebar_widths_if_changed(&mut self, ctx: &egui::Context) {
        let left = crate::ui::left_sidebar_width(ctx).unwrap_or(self.left_sidebar_width);
        let right = crate::ui::right_sidebar_width(ctx).unwrap_or(self.right_sidebar_width);
        if left != self.left_sidebar_width || right != self.right_sidebar_width {
            self.left_sidebar_width = left;
            self.right_sidebar_width = right;
            self.persist(|prefs| Prefs {
                left_sidebar_width: left,
                right_sidebar_width: right,
                ..prefs
            });
        }
        // The Run strip height (git.md §3) only when expanded: a folded strip is
        // pinned to its header, whose height must not overwrite the remembered one.
        if !self.run_panel_collapsed {
            let height = crate::ui::run_panel_height(ctx).unwrap_or(self.run_panel_height);
            if height != self.run_panel_height {
                self.run_panel_height = height;
                self.persist(|prefs| Prefs {
                    run_panel_height: height,
                    ..prefs
                });
            }
        }
    }

    /// Persists the open/closed state of the sidebars when a toggle during the frame
    /// (⌘B / ⌘G, header buttons, entry into Graph) changed it — restored by
    /// `from_prefs` on the next launch.
    fn persist_sidebar_visibility_if_changed(&mut self, was: SidebarVisibility) {
        let sidebars = self.sidebars;
        if sidebars != was {
            self.persist(move |prefs| Prefs {
                show_workspace: sidebars.workspace,
                show_git: sidebars.git,
                ..prefs
            });
        }
    }

    /// Records the change in the in-memory prefs; the TOML write is debounced
    /// (one write per burst — a sidebar drag was one read+write per frame). The
    /// dirty stamp is only armed with an injected path, so test/headless apps
    /// never schedule a flush.
    fn persist(&mut self, update: impl FnOnce(Prefs) -> Prefs) {
        self.prefs = update(std::mem::take(&mut self.prefs));
        if self.prefs_path.is_some() {
            self.prefs_dirty_at = Some(Instant::now());
        }
    }

    fn flush_prefs(&mut self) {
        if self.prefs_dirty_at.take().is_none() {
            return;
        }
        let Some(path) = &self.prefs_path else {
            return;
        };
        if let Err(err) = self.prefs.save_to(path) {
            eprintln!("helm: cannot save prefs {}: {err}", path.display());
        }
    }

    fn flush_prefs_if_due(&mut self, ctx: &egui::Context) {
        let Some(dirty_at) = self.prefs_dirty_at else {
            return;
        };
        match prefs_flush_wait(dirty_at, Instant::now()) {
            None => self.flush_prefs(),
            // No event may come before the deadline: book the frame that will
            // perform the flush.
            Some(wait) => ctx.request_repaint_after(wait),
        }
    }

    /// Sync trigger (worktrees.md §4): startup, window focus regain, after a Delete
    /// worktree (M11-7).
    fn run_group_sync(&mut self, ctx: &egui::Context) {
        let outcome = sync_workspace_groups(&mut self.workspace);
        self.caches.sync(&self.workspace);
        if outcome.changed {
            let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
            self.persist(move |_| next);
        }
        // Branch labels + dirty stats are read off the UI thread (`poll_group_refresh`
        // adopts the reply): the full diff per dirty repo froze the frame here.
        let probes = workspace_probes(&self.workspace);
        self.group_refresh
            .get_or_insert_with(|| GroupRefreshRunner::new(repainter(ctx)))
            .request(probes);
    }

    /// Adopts a completed off-thread branch/dirty refresh; called every frame.
    fn poll_group_refresh(&mut self) {
        let refreshed = self
            .group_refresh
            .as_mut()
            .and_then(GroupRefreshRunner::try_recv);
        if let Some(refreshed) = refreshed {
            self.apply_group_refresh(refreshed);
        }
    }

    /// Merges a refresh keyed by `RepoKey` (workspace order may have changed since the
    /// snapshot was taken): touches only still-present repos, leaving any added in the
    /// meantime to their own refresh. `None` clears the entry (clean / unversioned /
    /// bare), `Some` sets it; the active repo's live overlay re-applies next frame.
    fn apply_group_refresh(&mut self, refreshed: Vec<RepoRefresh>) {
        for refresh in refreshed {
            if !self.caches.keys.contains(&refresh.key) {
                continue;
            }
            match refresh.branch {
                Some(label) => {
                    self.caches.branch_labels.insert(refresh.key.clone(), label);
                }
                None => {
                    self.caches.branch_labels.remove(&refresh.key);
                }
            }
            match refresh.dirty {
                Some(stat) => {
                    self.caches.dirty.insert(refresh.key.clone(), stat);
                }
                None => {
                    self.caches.dirty.remove(&refresh.key);
                }
            }
        }
    }

    fn project_root_for_index(&self, index: usize) -> Option<PathBuf> {
        self.workspace
            .parent_root(index)
            .map(Path::to_path_buf)
            .or_else(|| self.workspace.repo(index).map(|repo| repo.path.clone()))
    }

    /// Distinct project roots across the workspace, in sidebar order — the choices
    /// of the Preferences "Project" picker. A group root and its worktrees collapse
    /// to the one shared root (`project_root_for_index`).
    fn workspace_project_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for index in 0..self.workspace.len() {
            if let Some(root) = self.project_root_for_index(index) {
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
        roots
    }

    /// Toggles the Preferences page; entering it seeds the Project section's picker
    /// with the active project (preferences.md §5).
    fn toggle_preferences_page(&mut self) {
        self.page = self.page.toggled();
        if self.page == Page::Preferences {
            self.selected_project = self
                .workspace
                .active()
                .and_then(|index| self.project_root_for_index(index));
            self.bitbucket_token_input.clear();
        }
    }

    fn open_create_worktree_modal(&mut self, index: usize, ctx: &egui::Context) {
        let Some(root) = self.project_root_for_index(index) else {
            return;
        };
        let root_label = self
            .workspace
            .repo(index)
            .map(|repo| repo.name.clone())
            .unwrap_or_else(|| root.display().to_string());
        let base = self.project_worktree_base(&root);
        let request = crate::git::worktree::SourceRequest {
            root: root.clone(),
            base: base.clone(),
        };
        let requested = self.source_runner(ctx).request(request);
        self.modal = Some(Modal::CreateWorktree(PendingCreate {
            root,
            root_label,
            base,
            sources: None,
            taken: HashSet::new(),
            base_branch: String::new(),
            selected: None,
            error: (!requested).then(|| "Another branch list is already loading".to_owned()),
            view: Default::default(),
        }));
    }

    /// Configured worktree base of the project rooted at `root` (worktrees.md §6),
    /// or `None` for the default `<root>.worktrees`.
    fn project_worktree_base(&self, root: &Path) -> Option<PathBuf> {
        self.prefs
            .project_settings(root)
            .and_then(|s| s.worktree_base.clone())
    }

    /// Run command for the project rooted at `root` (git.md §3): the per-project
    /// override when set, else auto-detected from `workdir`'s manifest. Empty when
    /// neither yields one — the Run panel then prompts for an explicit command.
    /// Takes `prefs` rather than `&self` so the caller can compute it while other
    /// `self` fields stay mutably borrowed for the frame's render closures.
    fn resolved_run_command(prefs: &Prefs, root: &Path, workdir: &Path) -> String {
        prefs
            .project_settings(root)
            .map(|s| s.run_command.clone())
            .filter(|c| !c.trim().is_empty())
            .or_else(|| crate::run::detect_run_command(workdir))
            .unwrap_or_default()
    }

    /// `$PORT` value for this worktree (git.md §3), or `None` when `command` doesn't
    /// consume the placeholder. The manual per-worktree override wins; otherwise the
    /// group's base port (default 3000) plus the worktree's offset within the group.
    fn resolved_run_port(
        prefs: &Prefs,
        root: &Path,
        workdir: &Path,
        offset: usize,
        command: &str,
    ) -> Option<u16> {
        if !crate::run::uses_port(command) {
            return None;
        }
        let settings = prefs.project_settings(root);
        let base = settings
            .and_then(|s| s.base_port)
            .unwrap_or(crate::run::DEFAULT_BASE_PORT);
        let override_port = settings.and_then(|s| s.port_overrides.get(workdir).copied());
        Some(crate::run::resolved_port(base, offset, override_port))
    }

    /// Applies a Run strip intent (git.md §3). Run/Relaunch drop any live pane
    /// first so its process tree is killed (`Pty::drop`) before a fresh one spawns;
    /// Stop just drops it. Collapse and the inline command edit persist to prefs —
    /// the command is shared by the whole project group.
    fn apply_run_intent(&mut self, intent: RunIntent, ctx: &egui::Context) {
        let RunIntent {
            key,
            cwd,
            root,
            command,
            launch_command,
            port,
            action,
        } = intent;
        if action.run || action.relaunch {
            self.caches.run_panes.remove(&key);
            self.caches
                .run_panes
                .insert(key, open_run_terminal(ctx, &cwd, &launch_command));
            ctx.request_repaint();
        } else if action.stop {
            self.caches.run_panes.remove(&key);
        }
        if action.toggle_collapsed {
            self.run_panel_collapsed = !self.run_panel_collapsed;
            let run_panel_collapsed = self.run_panel_collapsed;
            self.persist(move |prefs| Prefs {
                run_panel_collapsed,
                ..prefs
            });
        }
        if action.begin_edit {
            self.run_port_edit = None;
            self.run_command_edit = Some(command);
        } else if action.commit_edit {
            if let Some(value) = self.run_command_edit.take() {
                let run_command = value.trim().to_owned();
                self.persist(move |mut prefs| {
                    let existing = prefs.project_settings(&root);
                    let worktree_base = existing.and_then(|s| s.worktree_base.clone());
                    let post_create = existing.map(|s| s.post_create.clone()).unwrap_or_default();
                    prefs.set_project_settings(root, worktree_base, post_create, run_command);
                    prefs
                });
            }
        } else if action.cancel_edit {
            self.run_command_edit = None;
        } else if action.begin_port_edit {
            self.run_command_edit = None;
            self.run_port_edit = Some(port.map(|p| p.to_string()).unwrap_or_default());
        } else if action.commit_port_edit {
            if let Some(value) = self.run_port_edit.take() {
                // An empty or unparsable field clears the override back to auto.
                let override_port = value.trim().parse::<u16>().ok();
                self.persist(move |mut prefs| {
                    prefs.set_worktree_port(&root, &cwd, override_port);
                    prefs
                });
            }
        } else if action.cancel_port_edit {
            self.run_port_edit = None;
        }
    }

    fn active_repo_key(&self) -> Option<RepoKey> {
        let index = self.workspace.active()?;
        self.caches.keys.get(index).cloned()
    }

    /// Applies a review action raised by the diff view (M-RC): the editing
    /// intents mutate the active repo's in-memory comment store; `SendToAgent`
    /// spawns the agent tab.
    fn apply_review_intent(&mut self, intent: crate::review::ReviewIntent, ctx: &egui::Context) {
        use crate::review::ReviewIntent;
        match intent {
            ReviewIntent::SaveComment { file, comment } => {
                if let Some(key) = self.active_repo_key() {
                    crate::review::add_comment(self.review.entry(key).or_default(), &file, comment);
                }
            }
            ReviewIntent::DeleteComment { file, line } => {
                if let Some(key) = self.active_repo_key() {
                    if let Some(store) = self.review.get_mut(&key) {
                        crate::review::delete_comment(store, &file, line);
                    }
                }
            }
            ReviewIntent::SendToAgent => self.send_review_to_agent(ctx),
        }
    }

    /// Opens the aggregated review of the active repo in a fresh terminal tab
    /// running the agent CLI (M-RC). The pane is pre-inserted under the new tab's
    /// key so the render loop adopts it instead of spawning a plain shell, then
    /// the central area switches to the terminal to surface it. The repo's
    /// comments are cleared once handed off — the new tab is the only signal.
    fn send_review_to_agent(&mut self, ctx: &egui::Context) {
        let Some(index) = self.workspace.active() else {
            return;
        };
        let Some(key) = self.caches.keys.get(index).cloned() else {
            return;
        };
        let Some(store) = self.review.get(&key) else {
            return;
        };
        if crate::review::count(store) == 0 {
            return;
        }
        let prompt = crate::review::build_review_prompt(store);
        let Some(cwd) = self.workspace.repo(index).map(|r| r.path.clone()) else {
            return;
        };
        let Some(tab) = self.workspace.add_tab() else {
            return;
        };
        let (Some(tab_id), Some(pane_id)) = (
            self.workspace.tab_id(index, tab),
            self.workspace.active_layout().map(|l| l.focus()),
        ) else {
            return;
        };
        self.workspace.rename_tab(tab, &self.review_agent_command);
        let pane = open_agent_terminal(ctx, &cwd, &self.review_agent_command, &prompt);
        self.caches
            .panes
            .entry((key.clone(), tab_id))
            .or_default()
            .insert(pane_id, pane);
        self.review.remove(&key);
        self.central_mode = CentralMode::Terminal;
        self.diff = None;
    }

    /// Arms the one-shot post-create injection (worktrees.md §6) when the project
    /// configured a non-empty script; the `HELM_*` env is exported on the new
    /// pane (set on spawn, not echoed). No-op for an empty script.
    fn arm_post_create(
        &mut self,
        request: &crate::git::worktree::CreateRequest,
        created: &crate::git::worktree::CreatedWorktree,
        worktree_path: PathBuf,
    ) {
        let script = self
            .prefs
            .project_settings(&request.root)
            .map(|s| s.post_create.trim().to_owned())
            .unwrap_or_default();
        if script.is_empty() {
            return;
        }
        self.pending_post_create = Some(PendingPostCreate {
            worktree_path,
            script,
            env: vec![
                ("HELM_WORKTREE_PATH", created.path.display().to_string()),
                ("HELM_WORKTREE_BRANCH", created.source.local_branch.clone()),
                ("HELM_PROJECT_ROOT", request.root.display().to_string()),
                ("HELM_SOURCE_BRANCH", created.source.name.clone()),
            ],
        });
    }

    /// Keeps the Project section's edit buffers in sync with the active project:
    /// rebuilt from prefs when the project changes (or cleared with no repo open),
    /// left untouched while the same project is being edited (worktrees.md §6).
    fn ensure_project_edit(&mut self, root: Option<&Path>) {
        let Some(root) = root else {
            self.project_settings_edit = None;
            return;
        };
        let stale = self
            .project_settings_edit
            .as_ref()
            .is_none_or(|e| e.root != root);
        if !stale {
            return;
        }
        let settings = self.prefs.project_settings(root);
        self.project_settings_edit = Some(ProjectSettingsEdit {
            root: root.to_path_buf(),
            worktree_base: settings
                .and_then(|s| s.worktree_base.as_ref())
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            post_create: settings.map(|s| s.post_create.clone()).unwrap_or_default(),
            run_command: settings.map(|s| s.run_command.clone()).unwrap_or_default(),
            base_port: settings
                .and_then(|s| s.base_port)
                .map(|p| p.to_string())
                .unwrap_or_default(),
        });
    }

    /// Native folder picker for the worktree base (worktrees.md §6), seeded at the
    /// current base (if absolute) or the project root; the choice updates the edit
    /// buffer and persists. Cancelling is a no-op.
    fn pick_worktree_base(&mut self) {
        let (root, post_create, run_command, start) = match self.project_settings_edit.as_ref() {
            Some(edit) => {
                let typed = edit.worktree_base.trim();
                let start = if !typed.is_empty() && Path::new(typed).is_absolute() {
                    PathBuf::from(typed)
                } else {
                    edit.root.clone()
                };
                (
                    edit.root.clone(),
                    edit.post_create.clone(),
                    edit.run_command.clone(),
                    start,
                )
            }
            None => return,
        };
        let Some(path) = rfd::FileDialog::new().set_directory(&start).pick_folder() else {
            return;
        };
        if let Some(edit) = self.project_settings_edit.as_mut() {
            edit.worktree_base = path.display().to_string();
        }
        self.persist(move |mut prefs| {
            prefs.set_project_settings(root, Some(path), post_create, run_command);
            prefs
        });
    }

    fn source_runner(&mut self, ctx: &egui::Context) -> &mut crate::git::worktree::SourceRunner {
        self.worktree_sources
            .get_or_insert_with(|| crate::git::worktree::SourceRunner::new(repainter(ctx)))
    }

    fn request_create_worktree(
        &mut self,
        root: PathBuf,
        source: crate::git::worktree::CreateSource,
        name: Option<String>,
        base: Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        let request = crate::git::worktree::CreateRequest {
            root,
            source,
            name,
            base,
        };
        if !self.create_runner(ctx).request(request) {
            self.toasts.error(
                "Another worktree creation is already in progress",
                ctx.input(|i| i.time),
            );
        }
    }

    fn create_runner(&mut self, ctx: &egui::Context) -> &mut crate::git::worktree::CreateRunner {
        self.worktree_create
            .get_or_insert_with(|| crate::git::worktree::CreateRunner::new(repainter(ctx)))
    }

    fn checkout_runner(
        &mut self,
        ctx: &egui::Context,
    ) -> &mut crate::pull_requests::runner::CheckoutRunner {
        self.worktree_checkout.get_or_insert_with(|| {
            crate::pull_requests::runner::CheckoutRunner::new(repainter(ctx))
        })
    }

    /// Checkout a PR (pull-requests.md §7): activate an existing worktree already
    /// on the source branch, else fetch the branch off-thread then create one.
    fn request_pr_checkout(
        &mut self,
        pr: &crate::pull_requests::model::PullRequest,
        ctx: &egui::Context,
    ) {
        use crate::pull_requests::runner::{forge_kind_of_root, match_pr_root, matching_worktree};
        let now = ctx.input(|i| i.time);
        let roots: Vec<(PathBuf, crate::pull_requests::model::ForgeKind, String)> = self
            .workspace_project_roots()
            .into_iter()
            .filter_map(|root| forge_kind_of_root(&root).map(|(kind, label)| (root, kind, label)))
            .collect();
        let Some(root) = match_pr_root(&roots, pr.forge_kind, &pr.repo_label) else {
            self.toasts
                .error("No workspace repo matches this pull request", now);
            return;
        };
        let rows: Vec<(usize, PathBuf, String)> = (0..self.workspace.len())
            .filter_map(|index| {
                let project = self.project_root_for_index(index)?;
                let key = RepoKey::of(&self.workspace.repo(index)?.path);
                let branch = self.caches.branch_labels.get(&key)?.clone();
                Some((index, project, branch))
            })
            .collect();
        if let Some(index) = matching_worktree(&rows, &root, &pr.source_branch) {
            self.workspace.set_active(index);
            let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
            self.persist(move |_| next);
            self.central_mode = CentralMode::Terminal;
            ctx.request_repaint();
            return;
        }
        let request = crate::pull_requests::runner::CheckoutRequest {
            root,
            forge_kind: pr.forge_kind,
            number: pr.number,
            source_branch: pr.source_branch.clone(),
        };
        if !self.checkout_runner(ctx).request(request) {
            self.toasts
                .error("Another checkout is already in progress", now);
        }
    }

    /// The fetch landed: hand the now-local branch to `CreateRunner`, whose drain
    /// then activates the new worktree (pull-requests.md §7); leaving the cockpit
    /// for that worktree's terminal. A fetch failure surfaces one line.
    fn drain_worktree_checkout(&mut self, ctx: &egui::Context) {
        let Some(reply) = self
            .worktree_checkout
            .as_mut()
            .and_then(crate::pull_requests::runner::CheckoutRunner::try_recv)
        else {
            return;
        };
        let crate::pull_requests::runner::CheckoutReply { request, result } = reply;
        match result {
            Ok(()) => {
                let base = self.project_worktree_base(&request.root);
                let source = crate::git::worktree::CreateSource::Existing(request.source_branch);
                self.request_create_worktree(request.root, source, None, base, ctx);
                self.central_mode = CentralMode::Terminal;
            }
            Err(message) => {
                self.toasts.error(
                    format!("Checkout failed — {message}"),
                    ctx.input(|i| i.time),
                );
            }
        }
    }

    fn pr_runner(&mut self, ctx: &egui::Context) -> &mut crate::pull_requests::runner::PrRunner {
        self.pr_runner
            .get_or_insert_with(|| crate::pull_requests::runner::PrRunner::new(repainter(ctx)))
    }

    /// Kick a workspace PR fetch (pull-requests.md §6): the detached runner resolves
    /// each project's `origin` and queries the forges against the cached identity.
    /// A no-op while one is already in flight.
    fn refresh_pull_requests(&mut self, ctx: &egui::Context) {
        let request = crate::pull_requests::runner::PrRequest {
            roots: self.workspace_project_roots(),
            bitbucket_email: self.prefs.bitbucket_email.clone(),
        };
        self.pr_runner(ctx).request(request);
    }

    fn poll_pr_runner(&mut self) {
        if let Some(reply) = self
            .pr_runner
            .as_mut()
            .and_then(crate::pull_requests::runner::PrRunner::try_recv)
        {
            self.pr_cache.apply(reply);
        }
    }

    /// Store the typed Bitbucket token in the Keychain (pull-requests.md §3) under
    /// the configured email, then re-fetch so the source status reflects the new
    /// creds. The token never reaches `prefs`; the input is cleared on success.
    fn save_bitbucket_token(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let email = self.bitbucket_email.trim().to_owned();
        if email.is_empty() {
            self.toasts.error("Enter your Bitbucket email first", now);
            return;
        }
        let token = self.bitbucket_token_input.trim().to_owned();
        if token.is_empty() {
            return;
        }
        if crate::pull_requests::creds::store_token(&email, &token) {
            self.bitbucket_token_input.clear();
            self.toasts.success("Bitbucket token saved", now);
            self.refresh_pull_requests(ctx);
        } else {
            self.toasts
                .error("Couldn't save the Bitbucket token to the Keychain", now);
        }
    }

    fn request_delete_worktree(&mut self, index: usize, ctx: &egui::Context) {
        let Some(root) = self.workspace.parent_root(index).map(Path::to_path_buf) else {
            return;
        };
        let Some(repo) = self.workspace.repo(index) else {
            return;
        };
        let request = DeleteRequest {
            root,
            path: repo.path.clone(),
            label: repo.name.clone(),
            force: false,
        };
        self.delete_runner(ctx).request(request);
    }

    fn delete_runner(&mut self, ctx: &egui::Context) -> &mut DeleteRunner {
        self.worktree_delete
            .get_or_insert_with(|| DeleteRunner::new(repainter(ctx)))
    }

    fn drain_worktree_sources(&mut self) {
        let Some(reply) = self
            .worktree_sources
            .as_mut()
            .and_then(crate::git::worktree::SourceRunner::try_recv)
        else {
            return;
        };
        let crate::git::worktree::SourceReply { request, result } = reply;
        let Some(Modal::CreateWorktree(pending)) = self.modal.as_mut() else {
            return;
        };
        if pending.root != request.root {
            return;
        }
        match result {
            Ok(options) => {
                pending.selected =
                    (!options.sources.is_empty()).then_some(CreateSelection::Source(0));
                pending.sources = Some(options.sources);
                pending.taken = options.taken;
                pending.base_branch = options.base;
                pending.error = None;
            }
            Err(err) => {
                pending.sources = Some(Vec::new());
                pending.taken = HashSet::new();
                pending.base_branch = String::new();
                pending.selected = None;
                pending.error = Some(err.message().to_owned());
            }
        }
    }

    fn drain_worktree_create(&mut self, ctx: &egui::Context) {
        let Some(reply) = self
            .worktree_create
            .as_mut()
            .and_then(crate::git::worktree::CreateRunner::try_recv)
        else {
            return;
        };
        let crate::git::worktree::CreateReply { request, result } = reply;
        let now = ctx.input(|i| i.time);
        match result {
            Ok(created) => {
                if matches!(self.modal, Some(Modal::CreateWorktree(_))) {
                    self.modal = None;
                }
                self.run_group_sync(ctx);
                let created_path = canonical_path(&created.path);
                let created_index = (0..self.workspace.len()).find(|&index| {
                    self.workspace
                        .repo(index)
                        .is_some_and(|repo| canonical_path(&repo.path) == created_path)
                });
                if let Some(index) = created_index {
                    self.workspace.set_active(index);
                    let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
                    self.persist(move |_| next);
                    self.arm_post_create(&request, &created, created_path);
                }
                if let Some(git) = self.git.as_mut() {
                    git.worker.send(GitCommand::Status);
                    if self.central_mode == CentralMode::Graph {
                        git.graph_fresh = false;
                        git.reload_graph();
                    }
                }
                self.toasts.success(
                    format!("Created worktree — {}", created.source.local_branch),
                    now,
                );
            }
            Err(err) => {
                let message = format!("Create worktree failed — {}", err.message());
                if let Some(Modal::CreateWorktree(pending)) = self.modal.as_mut() {
                    if pending.root == request.root && pending.selected.is_some() {
                        pending.error = Some(message);
                        return;
                    }
                }
                self.toasts.error(message, now);
            }
        }
    }

    /// Drains the reply from the deletion thread (one op at a time): success ⇒ group
    /// sync (purge the row), dirty ⇒ confirmation modal, locked / git error ⇒ refusal
    /// modal (worktrees.md §6).
    fn drain_worktree_delete(&mut self, ctx: &egui::Context) {
        use crate::git::worktree::DeleteError;
        let Some(reply) = self
            .worktree_delete
            .as_mut()
            .and_then(DeleteRunner::try_recv)
        else {
            return;
        };
        let DeleteReply { request, result } = reply;
        let refused = |label: &str, reason: String| DeletePrompt::Refused {
            label: label.to_owned(),
            reason,
        };
        let prompt = match result {
            Ok(()) => {
                // Only the worktree modal may be dismissed by a background
                // success — never an unrelated one opened in the meantime.
                if matches!(self.modal, Some(Modal::DeleteWorktree(_))) {
                    self.modal = None;
                }
                self.run_group_sync(ctx);
                return;
            }
            Err(DeleteError::Dirty(files)) => DeletePrompt::Dirty {
                label: request.label.clone(),
                files,
            },
            Err(DeleteError::Locked(reason)) => refused(
                &request.label,
                reason.unwrap_or_else(|| "Worktree is locked".to_owned()),
            ),
            Err(DeleteError::Git(err)) => refused(&request.label, err.message().to_owned()),
        };
        self.modal = Some(Modal::DeleteWorktree(PendingDelete {
            root: request.root,
            path: request.path,
            label: request.label,
            prompt,
        }));
    }
}

/// Returns the chosen folders refused because they are not git repositories (§2), for
/// an error toast at the call site.
fn pick_and_add_folders(workspace: &mut Workspace, caches: &mut RepoCaches) -> Vec<PathBuf> {
    let Some(paths) = rfd::FileDialog::new().pick_folders() else {
        return Vec::new();
    };
    let rejected = add_picked_folders(workspace, paths).rejected;
    caches.sync(workspace);
    rejected
}

/// Error-toast text for folders refused at import (§2): names them when few, counts
/// them otherwise.
fn non_git_toast(rejected: &[PathBuf]) -> String {
    match rejected {
        [path] => format!(
            "“{}” is not a git repository",
            path.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_else(|| path.to_string_lossy())
        ),
        _ => format!("{} folders are not git repositories", rejected.len()),
    }
}

/// Current branch (HEAD) of each workspace repo, aligned on its order. None =
/// unversioned, bare (no working tree to reflect) or unreadable. Metadata read only —
/// same cost class as the sync triggers (UI-thread deviation noted in M11-6).
pub fn workspace_branches(workspace: &Workspace) -> Vec<Option<String>> {
    workspace
        .repos()
        .map(|r| {
            if r.bare {
                return None;
            }
            let repo = git2::Repository::open(&r.path).ok()?;
            crate::git::branch::current(&repo)
                .ok()
                .map(|b| b.label().to_owned())
        })
        .collect()
}

/// Uncommitted line stats `(additions, deletions)` per workspace repo, aligned on
/// its order; `None` for a clean / bare / unreadable repo. Clean repos pay only the
/// cheap `is_dirty` probe — the full `load_repo` diff (a patch per file) runs solely
/// for the dirty ones. Same UI-thread cost class as `workspace_branches`, run on the
/// sync triggers only.
pub fn workspace_dirty_stats(workspace: &Workspace) -> Vec<Option<(usize, usize)>> {
    workspace
        .repos()
        .map(|r| {
            if r.bare {
                return None;
            }
            let repo = git2::Repository::open(&r.path).ok()?;
            if !crate::git::status::is_dirty(&repo).unwrap_or(false) {
                return None;
            }
            crate::git::status::load_repo(&repo)
                .ok()
                .map(|status| status.total_line_stats())
        })
        .collect()
}

/// One repo's identity plus the bits the off-thread refresh reads from it.
#[derive(Clone)]
struct RepoProbe {
    key: RepoKey,
    path: PathBuf,
    bare: bool,
}

/// Outcome of probing one repo off the UI thread: its branch label and uncommitted
/// line stats. `None` ⇒ absent (unversioned / bare / clean / unreadable) and clears
/// the cache entry on adoption.
struct RepoRefresh {
    key: RepoKey,
    branch: Option<String>,
    dirty: Option<(usize, usize)>,
}

fn workspace_probes(workspace: &Workspace) -> Vec<RepoProbe> {
    workspace
        .repos()
        .map(|r| RepoProbe {
            key: RepoKey::of(&r.path),
            path: r.path.clone(),
            bare: r.bare,
        })
        .collect()
}

/// Branch label + dirty stats in a single repo open (merging the `workspace_branches`
/// / `workspace_dirty_stats` reads): a clean repo pays only the cheap `is_dirty` probe,
/// the full per-file diff runs solely for the dirty ones.
fn probe_repo(probe: &RepoProbe) -> RepoRefresh {
    let mut refresh = RepoRefresh {
        key: probe.key.clone(),
        branch: None,
        dirty: None,
    };
    if probe.bare {
        return refresh;
    }
    let Ok(repo) = git2::Repository::open(&probe.path) else {
        return refresh;
    };
    refresh.branch = crate::git::branch::current(&repo)
        .ok()
        .map(|b| b.label().to_owned());
    if crate::git::status::is_dirty(&repo).unwrap_or(false) {
        refresh.dirty = crate::git::status::load_repo(&repo)
            .ok()
            .map(|status| status.total_line_stats());
    }
    refresh
}

/// Runs the workspace-wide branch/dirty refresh on a dedicated thread (the full diff
/// per dirty repo froze the focus-regain frame on the UI thread); skips a request
/// while one is in flight, like the worktree runners.
struct GroupRefreshRunner {
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    results_tx: crossbeam_channel::Sender<Vec<RepoRefresh>>,
    results_rx: crossbeam_channel::Receiver<Vec<RepoRefresh>>,
    in_flight: bool,
}

impl GroupRefreshRunner {
    fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: std::sync::Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: false,
        }
    }

    fn request(&mut self, probes: Vec<RepoProbe>) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        let tx = self.results_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let refreshed: Vec<RepoRefresh> = probes.iter().map(probe_repo).collect();
            let _ = tx.send(refreshed);
            on_event();
        });
        true
    }

    fn try_recv(&mut self) -> Option<Vec<RepoRefresh>> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.in_flight = false;
        }
        reply
    }
}

/// Result of an Open Folder import: the `sync_group` mappings to apply to the per-repo
/// indexed states, and the chosen paths `rejected` because they are not git repositories
/// (Open Folder is git-only, overview.md §3.1) — surfaced as an error toast.
pub struct ImportOutcome {
    pub syncs: Vec<GroupSync>,
    pub rejected: Vec<PathBuf>,
}

/// Resolved import (worktrees.md §2): each git folder is brought back to its root,
/// deduplicated by canonicalized root, added as a full group (root + worktrees), and
/// the chosen path becomes the active row. A non-git folder is refused (§2, §3.1).
pub fn add_picked_folders(workspace: &mut Workspace, paths: Vec<PathBuf>) -> ImportOutcome {
    let mut syncs = Vec::new();
    let mut rejected = Vec::new();
    for path in paths {
        match crate::git::worktree::resolve_root(&path) {
            Ok(root) => syncs.extend(add_resolved_project(workspace, &root, &path)),
            Err(_) => rejected.push(path),
        }
    }
    ImportOutcome { syncs, rejected }
}

fn add_resolved_project(
    workspace: &mut Workspace,
    root: &Path,
    chosen: &Path,
) -> Option<GroupSync> {
    let listing = crate::git::worktree::list(root).ok();
    let bare = listing.as_ref().is_some_and(|l| l.bare);
    let children: Vec<Repo> = listing
        .map(|listing| {
            listing
                .worktrees
                .into_iter()
                // Prunable = removed from disk (worktrees.md §8): never imported.
                .filter(|w| !w.prunable)
                .map(|w| Repo::new(w.path))
                .collect()
        })
        .unwrap_or_default();

    // sync_group compares raw paths: we find the entry by canonicalized root but pass
    // it its path as stored.
    let existing = workspace
        .repos()
        .position(|r| canonical_path(&r.path) == *root);
    let stored_root = existing.and_then(|i| workspace.repo(i).map(|r| r.path.clone()));
    let sync = match stored_root {
        Some(stored) => {
            if let Some(index) = existing {
                workspace.set_bare(index, bare);
            }
            workspace.sync_group(&stored, children)
        }
        None => {
            let root_repo = Repo {
                bare,
                ..Repo::new(root.to_path_buf())
            };
            // A fresh import has no manual order to honour: seed it alphabetically
            // (worktrees.md §3); later reorders and discoveries are order-preserving.
            let mut children = children;
            children.sort_by_key(|r| r.name.to_lowercase());
            workspace.add_group(root_repo, children);
            None
        }
    };

    let chosen = canonical_path(chosen);
    let index = workspace
        .repos()
        .position(|r| canonical_path(&r.path) == chosen);
    if let Some(index) = index {
        workspace.set_active(index);
    }
    sync
}

fn canonical_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A trimmed, non-empty worktree-base field becomes a path; empty ⇒ `None` (the
/// default `<root>.worktrees`), so an empty entry is never persisted.
fn base_from_field(field: &str) -> Option<PathBuf> {
    let trimmed = field.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Result of a sync pass (worktrees.md §4): mappings to apply to the per-repo indexed
/// states; `changed` ⇒ prefs to rewrite.
pub struct SyncOutcome {
    pub syncs: Vec<GroupSync>,
    pub changed: bool,
}

/// Discovery/purge sync (worktrees.md §4): re-enumerates each group from disk —
/// worktree created outside the app ⇒ added in its alpha position, vanished folder
/// (prunable) ⇒ entry removed, active one folded back to the root. Also folds back into
/// their group the flat entries that are actually worktrees (M11-4 migration, §5).
pub fn sync_workspace_groups(workspace: &mut Workspace) -> SyncOutcome {
    let before: Vec<PathBuf> = workspace.repos().map(|r| r.path.clone()).collect();
    let mut syncs = Vec::new();

    let adopt_active = regroup_stray_worktrees(workspace, &mut syncs);

    let roots: Vec<PathBuf> = (0..workspace.len())
        .filter(|&i| workspace.parent_root(i).is_none())
        .filter_map(|i| workspace.repo(i))
        .map(|r| r.path.clone())
        .collect();
    for stored in roots {
        // Unreadable root (vanished, corrupted): left as is — purged at startup
        // (persistence) and M11-8 edge cases.
        let Ok(listing) = crate::git::worktree::list(&stored) else {
            continue;
        };
        // The bare flag is not persisted: reconciled here (worktrees.md §8).
        let root_index = (0..workspace.len()).find(|&i| {
            workspace.parent_root(i).is_none()
                && workspace.repo(i).is_some_and(|r| r.path == stored)
        });
        if let Some(index) = root_index {
            workspace.set_bare(index, listing.bare);
        }
        let children: Vec<Repo> = listing
            .worktrees
            .into_iter()
            .filter(|w| !w.prunable)
            .map(|w| Repo::new(w.path))
            .collect();
        syncs.extend(workspace.sync_group(&stored, children));
    }

    if let Some(path) = adopt_active {
        let index = workspace
            .repos()
            .position(|r| canonical_path(&r.path) == path);
        if let Some(index) = index {
            workspace.set_active(index);
        }
    }

    let after: Vec<PathBuf> = workspace.repos().map(|r| r.path.clone()).collect();
    SyncOutcome {
        syncs,
        changed: before != after,
    }
}

/// Flat entries that are actually linked worktrees (inherited from the M11-4
/// migration): removed in favor of their group — the root is added if absent, and the
/// enumeration that follows recreates the child. Returns the (canonical) path to
/// reactivate if the removed entry was active.
fn regroup_stray_worktrees(
    workspace: &mut Workspace,
    syncs: &mut Vec<GroupSync>,
) -> Option<PathBuf> {
    let mut adopt_active = None;
    loop {
        let stray = (0..workspace.len())
            .filter(|&i| workspace.parent_root(i).is_none())
            .find_map(|i| {
                let repo = workspace.repo(i)?;
                let root = crate::git::worktree::resolve_root(&repo.path).ok()?;
                (root != canonical_path(&repo.path)).then_some((i, root))
            });
        let Some((index, root)) = stray else {
            return adopt_active;
        };
        if workspace.active() == Some(index) {
            adopt_active = Some(canonical_path(&workspace.repo(index).unwrap().path));
        }
        syncs.push(GroupSync {
            mapping: (0..workspace.len())
                .map(|i| match i.cmp(&index) {
                    std::cmp::Ordering::Less => Some(i),
                    std::cmp::Ordering::Equal => None,
                    std::cmp::Ordering::Greater => Some(i - 1),
                })
                .collect(),
        });
        workspace.remove(index);
        let has_root = workspace.repos().any(|r| canonical_path(&r.path) == root);
        if !has_root {
            workspace.add(Repo::new(root));
        }
    }
}

fn reveal_in_finder(path: Option<&Path>) {
    if let Some(path) = path {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
}

enum TerminalState {
    Live(Pane),
    Failed(String),
}

/// A Run strip intent captured during render and applied once the frame's borrows
/// are released (git.md §3): carries the worktree's key/cwd/root and the run command
/// alongside the buttons the user pressed.
struct RunIntent {
    key: RepoKey,
    cwd: PathBuf,
    root: PathBuf,
    /// Stored run command template — what the inline editor opens and commits.
    command: String,
    /// `command` with `$PORT` resolved for this worktree — what actually spawns.
    launch_command: String,
    /// This worktree's resolved port; seeds the inline override editor.
    port: Option<u16>,
    action: crate::ui::run_panel::RunPanelAction,
}

/// Maps a worktree's run pane to the status the strip displays (git.md §3).
/// Takes `&mut` because the live-vs-exited check reaps the child (`has_exited`).
fn run_status_of(state: Option<&mut TerminalState>) -> crate::ui::run_panel::RunStatus {
    use crate::ui::run_panel::RunStatus;
    match state {
        None => RunStatus::Stopped,
        Some(TerminalState::Live(pane)) => {
            if pane.has_exited() {
                RunStatus::Exited
            } else {
                RunStatus::Running
            }
        }
        Some(TerminalState::Failed(err)) => RunStatus::Failed(err.clone()),
    }
}

/// Activity name for a tab's focused pane (terminal.md §4): the program's OSC
/// title if any, else the foreground process, else the live folder once the
/// shell has left its spawn directory. `None` ⇒ the workspace keeps the sticky
/// name (idle prompt).
fn name_candidate(pane: &Pane) -> Option<String> {
    if let Some(title) = pane.osc_title() {
        let title = title.trim();
        if !title.is_empty() {
            return Some(title.to_owned());
        }
    }
    if let Some(name) = pane
        .foreground_pgid()
        .and_then(crate::agent_watch::probe::foreground_name)
    {
        return Some(name);
    }
    folder_label(pane)
}

/// Leaf folder of the pane's live cwd, only once the shell has left the directory
/// it was spawned in — at the repo root the sidebar entry already names it.
fn folder_label(pane: &Pane) -> Option<String> {
    let live = pane.shell_pid().and_then(crate::terminal::cwd::live_cwd)?;
    if live.as_path() == pane.spawn_cwd() {
        return None;
    }
    live.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Remove on a group root ⇒ the whole group (worktrees.md §6); the disk is never
/// touched. Plain entry ⇒ existing removal. PTYs are dropped by the cache sync
/// that follows every workspace mutation.
fn remove_repo_or_group(workspace: &mut Workspace, index: usize) {
    let Some(root_path) = workspace.repo(index).map(|r| r.path.clone()) else {
        return;
    };
    let mut children: Vec<usize> = (0..workspace.len())
        .filter(|&i| workspace.parent_root(i) == Some(root_path.as_path()))
        .collect();
    children.push(index);
    children.sort_unstable();
    for i in children.into_iter().rev() {
        workspace.remove(i);
    }
}

const PREFS_DEBOUNCE: Duration = Duration::from_millis(500);

/// Trailing debounce decision: `None` ⇒ the flush is due, `Some(wait)` ⇒ check
/// again in `wait`.
fn prefs_flush_wait(dirty_at: Instant, now: Instant) -> Option<Duration> {
    let elapsed = now.duration_since(dirty_at);
    (elapsed < PREFS_DEBOUNCE).then(|| PREFS_DEBOUNCE - elapsed)
}

fn prefs_from_workspace(mut prefs: Prefs, workspace: &Workspace) -> Prefs {
    let projects = workspace.to_projects();
    // Drop per-project settings orphaned by a Remove-from-sidebar / purge.
    let roots: Vec<PathBuf> = projects.iter().map(|p| p.root.clone()).collect();
    prefs.retain_project_settings(&roots);
    Prefs {
        projects,
        active: workspace.active_repo().map(|r| r.path.clone()),
        ..prefs
    }
}

fn open_terminal(ctx: &egui::Context, cwd: &Path) -> TerminalState {
    open_terminal_with_env(ctx, cwd, &[])
}

/// PTY output redraw deadline. Requesting the repaint *after* a delay — instead
/// of immediately — matters beyond burst coalescing: an immediate
/// `request_repaint` puts eframe in `ControlFlow::Poll` (the runloop never
/// sleeps between frames) and macOS then delivers scroll events in clumps,
/// the judder felt while scrolling during a streaming task. A deadline maps to
/// `ControlFlow::WaitUntil`: the runloop sleeps and drains input evenly.
///
/// 33 ms, not 16: egui subtracts `predicted_dt` (a constant 1/60 s — eframe
/// never sets it) from every deadline, and anything ≤ 16.7 ms saturates to zero,
/// silently turning the deadline back into an immediate request (worse, even:
/// zero-delay requests book two passes). 33 ms nets out to ~16 ms of real sleep:
/// still ~60 FPS redraw under the reader throttle, but through `WaitUntil`.
const TERMINAL_REDRAW_INTERVAL: Duration = Duration::from_millis(33);

/// Repaint wakeup the reader fires on output. Visibility gating lives in the
/// pane's reader callback (`Pane::set_visible`): an off-screen pane stamps its
/// grid but never reaches this, so it does not pace the event loop.
fn repaint_pacer(ctx: &egui::Context) -> impl Fn() + Send + Sync + 'static {
    let repaint_ctx = ctx.clone();
    move || {
        crate::frame_log::note_pty_wakeup();
        repaint_ctx.request_repaint_after(TERMINAL_REDRAW_INTERVAL);
    }
}

/// Login-shell pane with extra environment exported on spawn (worktrees.md §6):
/// the post-create `HELM_*` vars reach the script without echoing `export` lines.
fn open_terminal_with_env(
    ctx: &egui::Context,
    cwd: &Path,
    env: &[(&str, String)],
) -> TerminalState {
    let mut cmd =
        crate::terminal::pty::login_shell_command(crate::terminal::pty::shell_program(), cwd);
    for (key, value) in env {
        cmd.env(key, value);
    }
    match Pane::from_command(cmd, INITIAL_ROWS, INITIAL_COLS, repaint_pacer(ctx)) {
        Ok(pane) => TerminalState::Live(pane),
        Err(err) => TerminalState::Failed(err.to_string()),
    }
}

/// Run terminal pane (git.md §3): a login shell that executes `command` then exits,
/// its output mirrored read-only in the sidebar strip. Dropping the pane kills the
/// process tree (`Pty::drop`); Stop and Relaunch both go through that drop.
fn open_run_terminal(ctx: &egui::Context, cwd: &Path, command: &str) -> TerminalState {
    let cmd =
        crate::terminal::pty::run_command(crate::terminal::pty::shell_program(), cwd, command);
    match Pane::from_command(cmd, INITIAL_ROWS, INITIAL_COLS, repaint_pacer(ctx)) {
        Ok(pane) => TerminalState::Live(pane),
        Err(err) => TerminalState::Failed(err.to_string()),
    }
}

/// Agent pane behind a Send-to-agent action (M-RC): an interactive login shell in
/// `cwd` with the aggregated review prompt exported (`HELM_REVIEW_PROMPT`), into
/// which the configured CLI invocation is fed. Running the agent as a job of the
/// shell — not as the pane's root process — keeps the terminal usable after the
/// agent exits (Ctrl+C returns to the shell prompt instead of a dead pane).
fn open_agent_terminal(
    ctx: &egui::Context,
    cwd: &Path,
    program: &str,
    prompt: &str,
) -> TerminalState {
    let mut cmd =
        crate::terminal::pty::login_shell_command(crate::terminal::pty::shell_program(), cwd);
    cmd.env(crate::terminal::pty::REVIEW_PROMPT_ENV, prompt);
    match Pane::from_command(cmd, INITIAL_ROWS, INITIAL_COLS, repaint_pacer(ctx)) {
        Ok(pane) => {
            let _ = pane.feed(crate::terminal::pty::agent_invocation(program).as_bytes());
            TerminalState::Live(pane)
        }
        Err(err) => TerminalState::Failed(err.to_string()),
    }
}

/// Hands the post-create script to `bash` through a quoted heredoc instead of
/// typing it into the pane's interactive shell (often zsh): the quoted delimiter
/// blocks the interactive shell's history expansion / globbing, so a real bash
/// script (`#!/usr/bin/env bash`, `set -euo pipefail`, …) runs verbatim
/// (worktrees.md §6). The `HELM_*` env exported on the pane is inherited by bash.
fn post_create_payload(script: &str) -> String {
    let body = script.trim_end_matches('\n');
    format!("bash -s <<'HELM_POST_CREATE_EOF'\n{body}\nHELM_POST_CREATE_EOF\n")
}

fn fallback_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

fn repo_path_missing(path: &Path) -> bool {
    !path.exists()
}

/// Rewrites trackpad wheel phases so egui loses no motion at gesture
/// boundaries. In egui 0.34 (`WheelState::on_wheel_event`) a `Start` event
/// discards its own delta and an `End` event wipes the motion already
/// accumulated this frame — and winit (macOS) stamps `Start`/`End` on both the
/// touch gesture and the momentum stream, so every flick crosses four such
/// boundaries: the view hitches, then lurches.
///
/// - `Start` is split into `Start` (zero delta) + `Move` (the delta): egui's
///   `InTouch` status still engages (direct, low-latency application) and the
///   first delta still paints.
/// - `End`/`Cancel` is withheld and replayed at the **front of the next
///   frame's events**, where egui's per-frame scroll buffer is freshly zeroed:
///   the gesture reset still happens, but destroys nothing. A `Start` in the
///   same frame cancels the withheld reset — touch→momentum hand-offs stay
///   one continuous gesture.
///
/// Mouse-only scrolling (no phases, `Move` events) passes through untouched,
/// keeping egui's notch smoothing.
fn rewrite_wheel_phases(events: &mut Vec<egui::Event>, deferred_end: &mut Option<egui::Event>) {
    let mut out = Vec::with_capacity(events.len() + 2);
    out.extend(deferred_end.take());
    for event in events.drain(..) {
        let egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
            phase,
        } = event
        else {
            out.push(event);
            continue;
        };
        let moved = egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
            phase: egui::TouchPhase::Move,
        };
        let boundary = egui::Event::MouseWheel {
            unit,
            delta: egui::Vec2::ZERO,
            modifiers,
            phase,
        };
        match phase {
            egui::TouchPhase::Start => {
                *deferred_end = None;
                out.push(boundary);
                if delta != egui::Vec2::ZERO {
                    out.push(moved);
                }
            }
            egui::TouchPhase::Move => out.push(moved),
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if delta != egui::Vec2::ZERO {
                    out.push(moved);
                }
                *deferred_end = Some(boundary);
            }
        }
    }
    *events = out;
}

impl eframe::App for HelmApp {
    // Sidebar widths live in our TOML (architecture §4): egui's memory persistence is
    // turned off so it doesn't become a second competing source of truth. Window
    // geometry stays handled by eframe (`persist_window`).
    fn persist_egui_memory(&self) -> bool {
        false
    }

    /// Pending prefs survive a quit landing before the debounce deadline:
    /// eframe calls `save` on shutdown (and periodically), `on_exit` last.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.flush_prefs();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_prefs();
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        rewrite_wheel_phases(&mut raw_input.events, &mut self.deferred_wheel_end);
        if self.frame_log.is_some() {
            let wheel = raw_input
                .events
                .iter()
                .filter(|e| matches!(e, egui::Event::MouseWheel { .. }))
                .count() as u64;
            if wheel > 0 {
                crate::frame_log::note_wheel_events(wheel);
            }
        }
        // The withheld End needs a next frame to be replayed in; an idle app
        // (gesture stops, no other event) would otherwise sit on it until the
        // 1 Hz heartbeat.
        if self.deferred_wheel_end.is_some() {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(log) = self.frame_log.as_mut() {
            log.begin_frame(_frame.info().cpu_usage);
        }
        #[cfg(target_os = "macos")]
        {
            let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
            if self.titlebar_fullscreen != Some(fullscreen) {
                self.titlebar_fullscreen = Some(fullscreen);
                titlebar::sync_fullscreen(_frame, fullscreen);
            }
            if fullscreen {
                // winit only reports the exit at the very end of the transition
                // (windowDidExitFullScreen), after the last resize-driven frame:
                // without a heartbeat no frame ever observes the return to
                // windowed, leaving the accessory hidden and the sidebar toggle
                // under the reappeared traffic lights.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        let preset = theme::apply(
            ui.ctx(),
            self.theme_mode,
            &self.light_theme,
            &self.dark_theme,
        );
        let palette = preset.palette;
        let term_palette = preset.term;

        let ctx = ui.ctx().clone();
        // Reset every pane's "painted this frame" flag; the render path re-sets it
        // for panes it actually draws (render_pane). A pane left false (background
        // repo/tab, or hidden behind Graph/Preferences) keeps reading into its grid
        // but its reader never repaints the window.
        self.caches.clear_pane_visibility();

        // Before the Preferences gate: update events are drained every frame in
        // all modes (update.md §4/§7).
        self.poll_update_runner(&ctx);
        self.poll_group_refresh();

        // Sync triggers on focus regain (M11-6) and on a periodic tick while focused
        // (worktrees.md §4): active even with the Preferences page open — the app keeps
        // living behind the page. The tick is gated on focus because off-focus the
        // focus-regain trigger already covers the user coming back, and an unfocused app
        // should sleep rather than poll.
        let focus_regained = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(true)))
        });
        let now = ctx.input(|i| i.time);
        let focused = ctx.input(|i| i.focused);
        if focused {
            // Wake the idle-but-focused app so the next tick fires (reactive mode).
            ctx.request_repaint_after(GROUP_POLL_INTERVAL);
        }
        let tick_due = focused && now - self.last_group_poll >= GROUP_POLL_INTERVAL.as_secs_f64();
        if focus_regained || tick_due {
            self.run_group_sync(&ctx);
            self.last_group_poll = now;
        }

        // PR cockpit refresh (pull-requests.md §6): a cold fetch on entry, then a
        // focused tick — same focus rationale as the group sync. Reuses
        // `focus_regained` so coming back to the app refreshes the open cockpit.
        // The Preferences "Pull Requests" section warms the same cache to show
        // each source's live status (pull-requests.md §3).
        let pr_surface_open = self.central_mode == CentralMode::PullRequests
            || (self.page == Page::Preferences
                && self.preferences_section == PreferencesSection::PullRequests);
        if pr_surface_open {
            let cold = !self.pr_cache.loaded;
            let pr_due = focused && now - self.last_pr_poll >= PR_POLL_INTERVAL.as_secs_f64();
            if cold || focus_regained || pr_due {
                self.refresh_pull_requests(&ctx);
                self.last_pr_poll = now;
            }
            if focused {
                ctx.request_repaint_after(PR_POLL_INTERVAL);
            }
        }

        // While the Keyboard recorder is armed, the toggle is captured as a combo
        // instead of acting (preferences.md §4).
        let recording_shortcut =
            self.page == Page::Preferences && self.keyboard_prefs.recording.is_some();
        let toggle_preferences =
            !recording_shortcut && action_pressed(&ctx, &self.keymap, Action::TogglePreferences);
        if toggle_preferences {
            self.toggle_preferences_page();
        }
        // Full-window Preferences page (preferences.md §2): exclusive active zone —
        // the page replaces the 3 zones and the early return leaves the global
        // shortcuts inert, but destroys nothing: PTYs, git workers and central state
        // stay alive. `Esc` closes (effective on the next frame, the event never
        // reaches the other zones).
        if self.page == Page::Preferences {
            self.render_preferences(ui, palette, &ctx);
            if let Some(log) = self.frame_log.as_mut() {
                log.end_frame("prefs");
            }
            return;
        }
        let sidebars_were = self.sidebars;
        let keys = self.handle_keys(&ctx);
        self.poll_workers(&ctx);
        let actions = self.render_page(ui, palette, term_palette, &ctx, keys.toggle_graph);
        // A terminal Cmd+click (terminal.md §12) is executed here, where `self` is
        // fully borrowed: the URL/file is handed to the configured editor (or
        // macOS `open`), and a failure surfaces as a toast naming the command.
        if let Some(link) = actions.open_link {
            if let Err(err) = crate::terminal::links::execute(&link, self.editor.template()) {
                let now = ctx.input(|i| i.time);
                self.toasts.error(err.message(), now);
            }
        }
        self.render_modals(ui, palette, &ctx);
        // ⌘O converges here (and no longer upstream): a single site that imports **and**
        // persists — the keyboard import did not rewrite the prefs before M11-5.
        if actions.open_folder || keys.open_dialog {
            let rejected = pick_and_add_folders(&mut self.workspace, &mut self.caches);
            if !rejected.is_empty() {
                let now = ctx.input(|i| i.time);
                self.toasts.error(non_git_toast(&rejected), now);
            }
            let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
            self.persist(move |_| next);
            self.caches
                .set_branch_labels(workspace_branches(&self.workspace));
            self.caches
                .set_dirty_stats(workspace_dirty_stats(&self.workspace));
        }
        // A folder dropped from Finder onto the empty-state central card imports it
        // like Open Folder. Gated to the empty workspace: with a repo open the
        // terminal owns the file drop (it pastes the path), so the two never compete.
        if self.workspace.is_empty() {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            if !dropped.is_empty() {
                let rejected = add_picked_folders(&mut self.workspace, dropped).rejected;
                self.caches.sync(&self.workspace);
                if !rejected.is_empty() {
                    let now = ctx.input(|i| i.time);
                    self.toasts.error(non_git_toast(&rejected), now);
                }
                let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
                self.persist(move |_| next);
                self.caches
                    .set_branch_labels(workspace_branches(&self.workspace));
                self.caches
                    .set_dirty_stats(workspace_dirty_stats(&self.workspace));
            }
        }
        // The gear toggles the Preferences page, rendered on the next frame by the
        // exclusive branch at the top of `ui()` (preferences.md §2).
        if actions.toggle_preferences {
            self.toggle_preferences_page();
            ctx.request_repaint();
        }
        self.flush_persistence(&ctx, sidebars_were);
        // Toasts (git.md §10): above everything, in all modes — a git action error
        // stays visible even outside the Graph view. The single toast action is
        // the updater Install (update.md §6).
        if toast_overlay(&ctx, &palette, &mut self.toasts) {
            if let Some(runner) = self.update_runner.as_mut() {
                runner.request_install();
            }
        }
        if let Some(log) = self.frame_log.as_mut() {
            log.end_frame(match self.central_mode {
                CentralMode::Terminal => "term",
                CentralMode::Graph => "graph",
                CentralMode::Agents => "agents",
                CentralMode::PullRequests => "prs",
            });
        }
    }
}

/// Keyboard reads consumed later in the frame (M17-12).
struct FrameKeys {
    toggle_graph: bool,
    open_dialog: bool,
}

/// Page-render outcomes applied after the modals (M17-12).
struct PageActions {
    open_folder: bool,
    toggle_preferences: bool,
    /// A terminal Cmd+click resolved to a link this frame (terminal.md §12),
    /// executed back in `ui()` where `self` is fully borrowed (toasts + editor).
    open_link: Option<LinkAction>,
}

fn rect(area: egui::Rect) -> Rect {
    Rect {
        x: area.min.x,
        y: area.min.y,
        w: area.width(),
        h: area.height(),
    }
}

fn native_options() -> eframe::NativeOptions {
    // winit ignores the window icon on macOS, but eframe applies it to the Dock
    // at runtime (setApplicationIconImage) — useful in dev, before the .app bundle.
    // Variant with Apple margin: same visual size as the neighboring icons.
    let icon = eframe::icon_data::from_png_bytes(include_bytes!(
        "../../assets/brand/icons/icon-dock-512.png"
    ))
    .expect("embedded icon PNG invalid");
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_icon(icon),
        ..Default::default()
    }
}

/// Pin the event-loop thread (where `update`/rendering runs) to the highest
/// interactive QoS. Without it macOS schedules it at the default class, level
/// with the `claude` children: several streaming agents saturate the cores and
/// deschedule the UI thread mid-frame, the stutter felt while scrolling.
#[cfg(target_os = "macos")]
fn raise_ui_thread_priority() {
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
}

/// Window-gesture tools (Swish…) decide "is the cursor on a titlebar?" through
/// the Accessibility API, where the egui-drawn header is plain content: only
/// the native band (MACOS_TITLEBAR_INSET, 28 pt) qualifies. A bottom titlebar
/// accessory grows that AX zone to the full visual header (TITLEBAR_HEIGHT)
/// without changing rendering or mouse routing.
#[cfg(target_os = "macos")]
mod titlebar {
    use objc2::rc::{Allocated, Retained};
    use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
    use objc2_app_kit::{
        NSLayoutAttribute, NSResponder, NSTitlebarAccessoryViewController, NSView,
    };
    use objc2_foundation::{MainThreadMarker, NSObject, NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    declare_class!(
        struct PassThroughView;

        unsafe impl ClassType for PassThroughView {
            #[inherits(NSResponder, NSObject)]
            type Super = NSView;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "HelmTitlebarPassThroughView";
        }

        impl DeclaredClass for PassThroughView {}

        unsafe impl PassThroughView {
            #[method_id(initWithFrame:)]
            fn init_with_frame(this: Allocated<Self>, frame: NSRect) -> Option<Retained<Self>> {
                let this = this.set_ivars(());
                unsafe { msg_send_id![super(this), initWithFrame: frame] }
            }

            // Mouse-invisible: clicks in the strip keep reaching the egui
            // header below; only the accessibility geometry grows.
            #[method_id(hitTest:)]
            fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
                None
            }
        }
    );

    /// Height of the strip below the native 28 pt band, up to the bottom of the
    /// egui-drawn header.
    fn extra_height() -> f64 {
        f64::from(crate::ui::TITLEBAR_HEIGHT - crate::ui::MACOS_TITLEBAR_INSET)
    }

    fn nswindow(source: &impl HasWindowHandle) -> Option<Retained<objc2_app_kit::NSWindow>> {
        let RawWindowHandle::AppKit(appkit) = source.window_handle().ok()?.as_raw() else {
            return None;
        };
        let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    pub(super) fn extend_native_titlebar(cc: &eframe::CreationContext<'_>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(window) = nswindow(cc) else {
            return;
        };

        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, extra_height()));
        let accessory: Retained<PassThroughView> =
            unsafe { msg_send_id![mtm.alloc(), initWithFrame: frame] };
        let controller = unsafe { NSTitlebarAccessoryViewController::init(mtm.alloc()) };
        unsafe {
            controller.setView(&accessory);
            controller.setLayoutAttribute(NSLayoutAttribute::Bottom);
            window.addTitlebarAccessoryViewController(&controller);
        }
    }

    /// The fullscreen reveal band (cursor at the top of the screen) includes
    /// titlebar accessories and draws them on the opaque system bar — the strip
    /// must disappear there and come back windowed. AppKit also stretches the
    /// accessory view during the transition, so its height is re-pinned on the
    /// way back.
    pub(super) fn sync_fullscreen(frame: &eframe::Frame, fullscreen: bool) {
        let Some(window) = nswindow(frame) else {
            return;
        };
        let controllers = unsafe { window.titlebarAccessoryViewControllers() };
        let Some(controller) = (unsafe { controllers.firstObject() }) else {
            return;
        };
        unsafe {
            controller.setHidden(fullscreen);
            if !fullscreen {
                let view = controller.view();
                let width = view.frame().size.width;
                view.setFrameSize(NSSize::new(width, extra_height()));
            }
        }
    }
}

/// Marker fencing the PATH printed by the login shell, isolating its value from
/// any banner an rc file may emit on startup.
#[cfg(target_os = "macos")]
const SHELL_PATH_MARKER: &str = "__helm_path__";

/// Finder and the Dock launch a `.app` with launchd's minimal PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), which omits Homebrew and the user-installed
/// CLIs we shell out to — the AI providers (`claude`, `codex`, `opencode`) and a
/// Homebrew `git`. The login shell is the only place the user's real PATH is
/// defined, so the bundled app adopts it once at startup; a terminal launch
/// already inherits the right PATH and skips this.
#[cfg(target_os = "macos")]
fn import_login_shell_path() {
    let in_bundle = std::env::current_exe()
        .map(|exe| exe_in_app_bundle(&exe))
        .unwrap_or(false);
    if !in_bundle {
        return;
    }
    if let Some(path) = login_shell_path() {
        std::env::set_var("PATH", path);
    }
}

/// Runs the login shell interactively (`-ilc`) so it sources the user's profile,
/// then prints `$PATH` fenced by [`SHELL_PATH_MARKER`]. Bounded so a slow or
/// hanging profile cannot block launch; on any failure we keep the inherited PATH.
#[cfg(target_os = "macos")]
fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned());
    let script = format!(
        "printf %s {SHELL_PATH_MARKER}; printf %s \"$PATH\"; printf %s {SHELL_PATH_MARKER}"
    );
    let output = crate::git::cli::run_program_with_timeout(
        Path::new(&shell),
        Path::new("/"),
        &["-ilc", &script],
        Duration::from_secs(5),
        &[],
    )
    .ok()?;
    if !output.success() {
        return None;
    }
    parse_marked_path(&output.stdout)
}

/// Extracts the PATH fenced between the two markers, dropping rc-file noise around
/// it; `None` when the fence is absent or wraps an empty value.
#[cfg(target_os = "macos")]
fn parse_marked_path(output: &str) -> Option<String> {
    let after = output.split_once(SHELL_PATH_MARKER)?.1;
    let value = after.split_once(SHELL_PATH_MARKER)?.0.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// True when the running executable lives inside a `…/Foo.app/…` bundle (Finder /
/// Dock launch) rather than a bare `cargo run` / dev binary.
#[cfg(target_os = "macos")]
fn exe_in_app_bundle(exe: &Path) -> bool {
    exe.ancestors()
        .any(|dir| dir.extension().is_some_and(|ext| ext == "app"))
}

pub fn run() -> eframe::Result<()> {
    #[cfg(target_os = "macos")]
    import_login_shell_path();
    #[cfg(target_os = "macos")]
    raise_ui_thread_priority();
    eframe::run_native(
        "helm",
        native_options(),
        Box::new(|cc| {
            #[cfg(target_os = "macos")]
            titlebar::extend_native_titlebar(cc);
            theme::install_fonts(&cc.egui_ctx);
            let mut prefs = Prefs::load();
            if prefs.purge_missing_repos() {
                if let Err(err) = prefs.save() {
                    eprintln!("helm: cannot save purged prefs: {err}");
                }
            }
            let mut app = HelmApp::from_prefs(prefs);
            app.prefs_path = crate::persistence::prefs_path();
            app.run_group_sync(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests;
