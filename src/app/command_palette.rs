//! App side of the command palette (keybindings.md §1): which commands apply to
//! the current state, the rows of each screen, and what a row does. Every action
//! goes through the path its button or menu already takes.

use lucide_icons::Icon;

use super::render::armed_force_push;
use super::*;
use crate::command_palette::{Command, CommandPalette, Screen};
use crate::git::branch::CheckoutTarget;
use crate::git::stash::StashEntry;
use crate::ui::command_palette::{command_palette_modal, Leading, PaletteRow, PaletteView};
use crate::ui::graph_view::StashTarget;
use crate::ui::run_panel::{RunPanelAction, RunStatus};

const STOP_SERVER_LABEL: &str = "Stop server";
const DROP_STASH_LABEL: &str = "Drop stash";
const CURRENT_BADGE: &str = "current";

enum PaletteItem {
    Command(Command),
    Worktree(usize),
    Server(RunTarget),
    Stash(StashEntry),
    Branch(String),
    Opener(WorkspaceOpener),
    PullRequest(usize),
    Agent(usize),
}

impl HelmApp {
    /// `Cmd+P`: opens on the command list, closes when already open; inert while
    /// another modal holds the screen.
    pub(super) fn toggle_command_palette(&mut self) {
        match self.modal {
            None => self.modal = Some(Modal::CommandPalette(CommandPalette::default())),
            Some(Modal::CommandPalette(_)) => self.modal = None,
            Some(_) => {}
        }
    }

    pub(super) fn render_command_palette(
        &mut self,
        ui: &mut egui::Ui,
        palette: &theme::Palette,
        ctx: &egui::Context,
    ) {
        let Some(Modal::CommandPalette(state)) = &self.modal else {
            return;
        };
        let screen = state.screen();
        let scope = self.palette_scope(screen);
        let recent = match screen {
            Screen::Commands => self.recent_commands(),
            _ => Vec::new(),
        };
        let entries = self.palette_entries(screen, &recent);
        let loaded = entries.is_some();
        let (mut items, rows): (Vec<PaletteItem>, Vec<PaletteRow>) =
            entries.into_iter().flatten().unzip();
        let Some(Modal::CommandPalette(state)) = self.modal.as_mut() else {
            return;
        };
        let view = PaletteView {
            rows: loaded.then_some(&rows[..]),
            scope,
            recent_rows: recent.len(),
        };
        let action = command_palette_modal(ui, palette, state, &view);
        if action.dismiss {
            self.modal = None;
        } else if let Some(index) = action.remove {
            self.remove_palette_item(items.swap_remove(index), ctx);
        } else if let Some(index) = action.activate {
            self.activate_palette_item(items.swap_remove(index), palette, ctx);
        }
    }

    fn palette_entries(
        &mut self,
        screen: Screen,
        recent: &[Command],
    ) -> Option<Vec<(PaletteItem, PaletteRow)>> {
        let entries = match screen {
            Screen::Commands => self.command_entries(recent),
            Screen::SwitchTo => self.worktree_entries(),
            Screen::RunningServers => self
                .running_servers()
                .into_iter()
                .map(|target| {
                    let row = self.server_row(&target);
                    (PaletteItem::Server(target), row)
                })
                .collect(),
            Screen::Stashes => self.stash_entries()?,
            Screen::Branches => self.branch_entries()?,
            Screen::Editors => self.opener_entries(),
            Screen::PullRequests => self.pull_request_entries(),
            Screen::Agents => self.agent_entries(),
        };
        Some(entries)
    }

    /// The available commands, `recent` first: a fuzzy tie then goes to the one run lately.
    fn command_entries(&mut self, recent: &[Command]) -> Vec<(PaletteItem, PaletteRow)> {
        let run_badge = self.keymap.shortcut_for(Action::Run).map(|s| s.display());
        let others = self
            .available_commands()
            .into_iter()
            .filter(|command| !recent.contains(command));
        recent
            .iter()
            .copied()
            .chain(others)
            .map(|command| {
                let mut row = PaletteRow::command(
                    command_icon(command),
                    command.label(),
                    command.opened_screen().is_some(),
                );
                if matches!(command, Command::RunServer | Command::RelaunchServer) {
                    row.badge = run_badge.clone();
                }
                (PaletteItem::Command(command), row)
            })
            .collect()
    }

    fn recent_commands(&mut self) -> Vec<Command> {
        let available = self.available_commands();
        self.prefs
            .command_usage
            .recent(&available, crate::ui::pull_requests_view::now_epoch_secs())
    }

    fn available_commands(&mut self) -> Vec<Command> {
        let run_status = self.active_run_status();
        Command::ALL
            .into_iter()
            .filter(|&command| self.is_command_available(command, run_status.as_ref()))
            .collect()
    }

    fn record_command_run(&mut self, command: Command) {
        let now = crate::ui::pull_requests_view::now_epoch_secs();
        self.persist(move |mut prefs| {
            prefs.command_usage.record(command, now);
            prefs
        });
    }

    fn is_command_available(&self, command: Command, run_status: Option<&RunStatus>) -> bool {
        let active = self.workspace.active();
        let git = self.git.as_ref();
        let named_branch = git.is_some_and(|g| matches!(g.branch, Branch::Named(_)));
        let can_sync = git.is_some_and(|g| !g.git_missing && g.has_remote) && named_branch;
        match command {
            Command::SwitchTo => !self.workspace.is_empty(),
            Command::RunningServers
            | Command::ToggleTheme
            | Command::SendFeedback
            | Command::WhatsNew
            | Command::OpenOnPhone => true,
            Command::StopPhoneAccess => self.is_phone_access_on(),
            Command::RunServer => run_status.is_some_and(|s| *s != RunStatus::Running),
            Command::RelaunchServer | Command::StopServer => {
                run_status == Some(&RunStatus::Running)
            }
            Command::StashChanges => {
                git.is_some_and(|g| g.status.changed_file_count() > 0) && named_branch
            }
            Command::Stashes => git.is_some_and(|g| g.stash_count > 0),
            Command::Pull | Command::Push | Command::Fetch => can_sync,
            Command::ForcePush => can_sync && armed_force_push(git).is_some(),
            Command::CheckoutBranch => git.is_some_and(|g| !g.op_in_progress),
            Command::CreateWorktree => active.is_some(),
            Command::DeleteWorktree => {
                active.is_some_and(|i| self.workspace.parent_root(i).is_some())
            }
            Command::OpenInEditor | Command::RevealInFinder | Command::CopyPath => {
                self.workspace.active_repo().is_some()
            }
            Command::PullRequests => self.pr_cache.loaded && !self.workspace.is_empty(),
            Command::Agents => !self.caches.agents.is_empty(),
        }
    }

    fn active_run_status(&mut self) -> Option<RunStatus> {
        let target = self.run_target_at(self.workspace.active()?)?;
        Some(run_status_of(self.caches.run_panes.get_mut(&target.key)))
    }

    /// The active worktree on the screens whose rows act on it.
    fn palette_scope(&self, screen: Screen) -> Option<(Icon, String)> {
        if !matches!(
            screen,
            Screen::Commands | Screen::Stashes | Screen::Branches | Screen::Editors
        ) {
            return None;
        }
        let index = self.workspace.active()?;
        let project = self.workspace.project_name(index)?;
        let key = self.caches.keys.get(index)?;
        Some(match self.caches.branch_labels.get(key) {
            Some(branch) => (Icon::GitBranch, format!("{project} · {branch}")),
            None => (Icon::Folder, project),
        })
    }

    fn worktree_entries(&self) -> Vec<(PaletteItem, PaletteRow)> {
        let active = self.workspace.active();
        (0..self.workspace.len())
            .filter_map(|index| {
                let repo = self.workspace.repo(index)?;
                if repo.bare {
                    return None;
                }
                let branch = self
                    .caches
                    .keys
                    .get(index)
                    .and_then(|k| self.caches.branch_labels.get(k));
                let context = match branch {
                    Some(branch) => (Icon::GitBranch, branch.clone()),
                    None => (Icon::Folder, repo.name.clone()),
                };
                let row = PaletteRow {
                    leading: Leading::Icon(Icon::Folder),
                    title: self.workspace.project_name(index).unwrap_or_default(),
                    context: Some(context),
                    detail: Some(home_relative(&repo.path)),
                    badge: (active == Some(index)).then(|| CURRENT_BADGE.to_owned()),
                    opens_screen: false,
                    enter_label: "Switch",
                    remove_label: None,
                };
                Some((PaletteItem::Worktree(index), row))
            })
            .collect()
    }

    /// Every worktree whose Run strip process is alive, in sidebar order.
    fn running_servers(&mut self) -> Vec<RunTarget> {
        let mut running = Vec::new();
        for index in 0..self.workspace.len() {
            let Some(key) = self.caches.keys.get(index) else {
                continue;
            };
            let status = run_status_of(self.caches.run_panes.get_mut(key));
            if status != RunStatus::Running {
                continue;
            }
            running.extend(self.run_target_at(index));
        }
        running
    }

    fn server_row(&self, target: &RunTarget) -> PaletteRow {
        let branch = self.caches.branch_labels.get(&target.key);
        let context = match branch {
            Some(branch) => (Icon::GitBranch, branch.clone()),
            None => (Icon::Folder, file_name_label(&target.path)),
        };
        PaletteRow {
            leading: Leading::Running,
            title: target.project.clone(),
            context: Some(context),
            detail: Some(target.launch_command.clone()),
            badge: target.port.map(|port| format!(":{port}")),
            opens_screen: false,
            enter_label: "Show",
            remove_label: Some(STOP_SERVER_LABEL),
        }
    }

    /// `None` until the worker answered the `Refs` read sent on entering the screen.
    fn stash_entries(&self) -> Option<Vec<(PaletteItem, PaletteRow)>> {
        let refs = self.git.as_ref().and_then(|g| g.refs.as_ref())?;
        let entries = refs
            .stashes
            .iter()
            .enumerate()
            .map(|(index, stash)| {
                let row = PaletteRow {
                    leading: Leading::Icon(Icon::Archive),
                    title: stash.message.clone(),
                    context: None,
                    detail: None,
                    badge: Some(format!("stash@{{{index}}}")),
                    opens_screen: false,
                    enter_label: "Apply",
                    remove_label: Some(DROP_STASH_LABEL),
                };
                (PaletteItem::Stash(stash.clone()), row)
            })
            .collect();
        Some(entries)
    }

    fn branch_entries(&self) -> Option<Vec<(PaletteItem, PaletteRow)>> {
        let refs = self.git.as_ref().and_then(|g| g.refs.as_ref())?;
        let entries = refs
            .checkout_targets
            .iter()
            .map(|CheckoutTarget { name, remote }| {
                let icon = if *remote {
                    Icon::Cloud
                } else {
                    Icon::GitBranch
                };
                let row = PaletteRow {
                    leading: Leading::Icon(icon),
                    title: name.clone(),
                    context: None,
                    detail: None,
                    badge: remote.then(|| "remote".to_owned()),
                    opens_screen: false,
                    enter_label: "Checkout",
                    remove_label: None,
                };
                (PaletteItem::Branch(name.clone()), row)
            })
            .collect();
        Some(entries)
    }

    fn opener_entries(&self) -> Vec<(PaletteItem, PaletteRow)> {
        self.installed_openers
            .iter()
            .map(|&opener| {
                let mut row = PaletteRow::command(Icon::AppWindow, opener.label(), false);
                row.enter_label = "Open";
                row.badge = (opener == self.workspace_opener).then(|| "last used".to_owned());
                (PaletteItem::Opener(opener), row)
            })
            .collect()
    }

    fn pull_request_entries(&self) -> Vec<(PaletteItem, PaletteRow)> {
        use crate::pull_requests::model::PrState;
        self.pr_cache
            .pull_requests
            .iter()
            .enumerate()
            .map(|(index, pr)| {
                let row = PaletteRow {
                    leading: Leading::Icon(Icon::GitPullRequest),
                    title: pr.title.clone(),
                    context: Some((Icon::Folder, format!("{}#{}", pr.repo_label, pr.number))),
                    detail: Some(format!(
                        "{} · {} → {}",
                        pr.author, pr.source_branch, pr.dest_branch
                    )),
                    badge: (pr.state == PrState::Draft).then(|| "draft".to_owned()),
                    opens_screen: false,
                    enter_label: "Open",
                    remove_label: None,
                };
                (PaletteItem::PullRequest(index), row)
            })
            .collect()
    }

    fn agent_entries(&self) -> Vec<(PaletteItem, PaletteRow)> {
        self.caches
            .agents
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let row = PaletteRow {
                    leading: Leading::Icon(Icon::Bot),
                    title: entry.group_name.clone(),
                    context: entry.branch.clone().map(|branch| (Icon::GitBranch, branch)),
                    detail: Some(format!("{} · {}", entry.agent, entry.tab_name)),
                    badge: agent_badge_label(entry.badge).map(str::to_owned),
                    opens_screen: false,
                    enter_label: "Focus",
                    remove_label: None,
                };
                (PaletteItem::Agent(index), row)
            })
            .collect()
    }

    fn activate_palette_item(
        &mut self,
        item: PaletteItem,
        palette: &theme::Palette,
        ctx: &egui::Context,
    ) {
        if let PaletteItem::Command(command) = &item {
            self.record_command_run(*command);
            if let Some(screen) = command.opened_screen() {
                self.enter_palette_screen(screen);
                ctx.request_repaint();
                return;
            }
        }
        self.modal = None;
        match item {
            PaletteItem::Command(command) => self.run_command(command, palette, ctx),
            PaletteItem::Worktree(index) => self.select_worktree(index),
            PaletteItem::Server(target) => self.show_server(target),
            PaletteItem::Stash(stash) => {
                if let Some(git) = self.git.as_ref() {
                    git.worker.send(GitCommand::StashApplyAt(stash.oid));
                }
            }
            PaletteItem::Branch(name) => {
                if let Some(git) = self.git.as_mut() {
                    git.checkout(name);
                }
            }
            PaletteItem::Opener(opener) => self.open_active_in(opener),
            PaletteItem::PullRequest(index) => {
                self.central_mode = CentralMode::PullRequests;
                self.open_pr_review(index, ctx);
            }
            PaletteItem::Agent(index) => self.focus_agent(index, ctx),
        }
        ctx.request_repaint();
    }

    fn enter_palette_screen(&mut self, screen: Screen) {
        if matches!(screen, Screen::Stashes | Screen::Branches) {
            if let Some(git) = self.git.as_ref() {
                git.worker.send(GitCommand::Refs);
            }
        }
        if let Some(Modal::CommandPalette(state)) = self.modal.as_mut() {
            state.enter(screen);
        }
    }

    fn remove_palette_item(&mut self, item: PaletteItem, ctx: &egui::Context) {
        match item {
            PaletteItem::Server(target) => self.stop_server(&target, ctx),
            PaletteItem::Stash(stash) => {
                self.modal = Some(Modal::DropStash(StashTarget {
                    oid: stash.oid,
                    summary: stash.message,
                }));
            }
            _ => {}
        }
        ctx.request_repaint();
    }

    /// Commands that open no screen; the palette is already closed, so a command
    /// arming its own confirmation modal takes the screen.
    fn run_command(&mut self, command: Command, palette: &theme::Palette, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match command {
            Command::RunServer => self.drive_active_server(
                RunPanelAction {
                    run: true,
                    ..Default::default()
                },
                ctx,
            ),
            Command::RelaunchServer => self.drive_active_server(
                RunPanelAction {
                    relaunch: true,
                    ..Default::default()
                },
                ctx,
            ),
            Command::StopServer => self.drive_active_server(
                RunPanelAction {
                    stop: true,
                    ..Default::default()
                },
                ctx,
            ),
            Command::StashChanges => {
                if let Some(git) = self.git.as_ref() {
                    git.send_then_reload_graph(GitCommand::Stash);
                }
            }
            Command::Pull => self.request_active_sync(self.pull_default.command(), now),
            Command::Push => self.request_active_sync(SyncCommand::Push, now),
            Command::Fetch => self.request_active_sync(SyncCommand::FetchAll, now),
            Command::ForcePush => {
                self.modal = armed_force_push(self.git.as_ref());
            }
            Command::CreateWorktree => {
                if let Some(index) = self.workspace.active() {
                    self.open_create_worktree_modal(index, ctx);
                }
            }
            Command::DeleteWorktree => {
                if let Some(index) = self.workspace.active() {
                    self.request_delete_worktree(index, BranchCleanup::Keep, ctx);
                }
            }
            Command::RevealInFinder => {
                reveal_in_finder(self.workspace.active_repo().map(|r| r.path.as_path()));
            }
            Command::CopyPath => {
                if let Some(repo) = self.workspace.active_repo() {
                    ctx.copy_text(repo.path.to_string_lossy().into_owned());
                    self.toasts.success("Path copied", now);
                }
            }
            Command::ToggleTheme => self.toggle_theme(palette),
            Command::SendFeedback => {
                self.modal = Some(Modal::Feedback(FeedbackPage::default()));
            }
            Command::WhatsNew => self.modal = Some(Modal::WhatsNew),
            Command::OpenOnPhone => self.open_on_phone(now),
            Command::StopPhoneAccess => self.stop_phone_access(),
            Command::SwitchTo
            | Command::RunningServers
            | Command::Stashes
            | Command::CheckoutBranch
            | Command::OpenInEditor
            | Command::PullRequests
            | Command::Agents => {}
        }
    }

    fn request_active_sync(&mut self, command: SyncCommand, now: f64) {
        if let Some(git) = self.git.as_mut() {
            git.request_sync(command, &mut self.toasts, now);
        }
    }

    /// The active worktree's Run strip, driven like `Cmd+R`: revealed and expanded,
    /// and a missing command opens its inline editor instead of spawning a no-op.
    fn drive_active_server(&mut self, mut action: RunPanelAction, ctx: &egui::Context) {
        let Some(target) = self.workspace.active().and_then(|i| self.run_target_at(i)) else {
            return;
        };
        self.sidebars.git = true;
        self.run_collapsed.insert(target.key.clone(), false);
        if (action.run || action.relaunch) && target.launch_command.trim().is_empty() {
            action = RunPanelAction {
                begin_edit: true,
                ..Default::default()
            };
        }
        self.apply_run_intent(target.intent(action), ctx);
    }

    /// Flips to the opposite of what is on screen — `Auto` resolves to its current look.
    fn toggle_theme(&mut self, palette: &theme::Palette) {
        let theme = if palette.dark {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        self.theme_mode = theme;
        self.persist(move |prefs| Prefs { theme, ..prefs });
    }

    /// Same as a sidebar row click, revealing the row when its group is folded.
    fn select_worktree(&mut self, index: usize) {
        reveal_row(&mut self.workspace, index);
        self.workspace.set_active(index);
        if matches!(
            self.central_mode,
            CentralMode::Agents | CentralMode::PullRequests
        ) {
            self.central_mode = CentralMode::Terminal;
        }
    }

    /// Selects the server's worktree with its Run strip revealed and expanded.
    fn show_server(&mut self, target: RunTarget) {
        let Some(index) = self.caches.keys.iter().position(|k| k == &target.key) else {
            return;
        };
        self.select_worktree(index);
        self.sidebars.git = true;
        self.run_collapsed.insert(target.key, false);
    }

    fn stop_server(&mut self, target: &RunTarget, ctx: &egui::Context) {
        let action = RunPanelAction {
            stop: true,
            ..Default::default()
        };
        self.apply_run_intent(target.intent(action), ctx);
    }
}

fn command_icon(command: Command) -> Icon {
    match command {
        Command::SwitchTo => Icon::ArrowLeftRight,
        Command::RunningServers => Icon::Server,
        Command::RunServer => Icon::Play,
        Command::RelaunchServer => Icon::RotateCcw,
        Command::StopServer => Icon::Square,
        Command::StashChanges => Icon::Archive,
        Command::Stashes => Icon::ArchiveRestore,
        Command::Pull => Icon::ArrowDown,
        Command::Push => Icon::ArrowUp,
        Command::ForcePush => Icon::ShieldAlert,
        Command::Fetch => Icon::CloudDownload,
        Command::CheckoutBranch => Icon::GitBranch,
        Command::CreateWorktree => Icon::GitBranchPlus,
        Command::DeleteWorktree => Icon::Trash2,
        Command::OpenInEditor => Icon::ExternalLink,
        Command::RevealInFinder => Icon::FolderOpen,
        Command::CopyPath => Icon::Copy,
        Command::PullRequests => Icon::GitPullRequest,
        Command::Agents => Icon::Bot,
        Command::OpenOnPhone => Icon::QrCode,
        Command::StopPhoneAccess => Icon::PhoneOff,
        Command::ToggleTheme => Icon::SunMoon,
        Command::SendFeedback => Icon::MessageSquare,
        Command::WhatsNew => Icon::Sparkles,
    }
}

fn agent_badge_label(badge: AgentBadge) -> Option<&'static str> {
    match badge {
        AgentBadge::None => None,
        AgentBadge::Idle => Some("idle"),
        AgentBadge::Done => Some("done"),
        AgentBadge::Working => Some("working"),
    }
}

fn file_name_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn home_relative(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
