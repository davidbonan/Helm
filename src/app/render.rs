//! Per-frame rendering: the `eframe::App::ui` body delegates here for the
//! Preferences page, key routing, worker polling, the 3-zone page, modals
//! and persistence flush (M17-12). Free `render_pane` draws one split leaf.

use super::*;

/// Borrow an open PR review (pull-requests.md §11) as the view struct the cockpit
/// renders. Built in both `root_layout` arms (active repo / none), so it lives
/// here rather than being inlined twice.
fn pr_review_view<'a>(
    r: &'a mut PrReview,
    agent: &'a str,
) -> crate::ui::pull_requests_view::PrReviewView<'a> {
    let diff = match (r.base, r.head, r.selected_file.and_then(|i| r.files.get(i))) {
        (Some(base), Some(head), Some(file)) => r.diffs.get(&(base, head, file.path.clone())),
        _ => None,
    };
    let commits = r
        .detail
        .as_ref()
        .map(|d| d.commits.as_slice())
        .unwrap_or(&[]);
    crate::ui::pull_requests_view::PrReviewView {
        pr: &r.pr,
        detail: r.detail.as_ref(),
        detail_error: r.detail_error.as_deref(),
        files: &r.files,
        files_loading: r.files_loading,
        files_error: r.files_error.as_deref(),
        selected_file: r.selected_file,
        commits,
        selected_commit: r.selected_commit.as_deref(),
        diff,
        diff_loading: r.diff_loading,
        diff_error: r.diff_error.as_deref(),
        diff_view: &mut r.diff_view,
        existing: &r.existing,
        draft: &r.draft,
        agent_notes: &r.agent_notes,
        agent,
        verdict: &mut r.verdict,
        summary: &mut r.summary,
        posting: r.posting,
        post_error: r.post_error.as_deref(),
    }
}

/// `ui()` phases (M17-12), in call order. The order is part of the behavior:
/// the update runner is polled before the Preferences gate (its events are
/// drained in all modes), and the git session syncs **after** key routing so a
/// keyboard repo switch swaps the session within the same frame.
impl HelmApp {
    pub(super) fn poll_update_runner(&mut self, ctx: &egui::Context) {
        // Updater worker (update.md §4/§7): silent boot check on the first frame.
        let first_frame = self.update_runner.is_none();
        let update_runner = self.update_runner.get_or_insert_with(|| {
            let mut runner = UpdateRunner::new(repainter(ctx));
            runner.check_at_boot();
            runner
        });
        match update_runner.poll() {
            Some(UpdateOutcome::Available {
                version,
                at_boot: true,
            }) => {
                let now = ctx.input(|i| i.time);
                self.toasts.info_with_action(
                    format!("Update available v{version}"),
                    "Install",
                    now,
                );
            }
            Some(UpdateOutcome::Installed { bundle }) => {
                // The new bundle is in place: hand over to a fresh process and
                // close this one (update.md §5).
                if update::relaunch(&bundle).is_ok() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    let now = ctx.input(|i| i.time);
                    self.toasts
                        .error("Update installed — relaunch Helm manually", now);
                }
            }
            _ => {}
        }
        if first_frame {
            self.maybe_show_whats_new();
        }
    }

    /// Boot trigger for the What's new modal (update.md §9.3): the first frame
    /// compares the compiled version with the persisted watermark and either shows
    /// the bundled notes (a bump inside a bundle) or silently records a baseline
    /// (first install). Both cases advance `last_seen_version`; out of a bundle, or
    /// when the version is already seen, nothing happens.
    fn maybe_show_whats_new(&mut self) {
        let current = update::current_version();
        let action = update::whats_new_on_boot(
            update::bundle_path().is_some(),
            current,
            &self.prefs.last_seen_version,
        );
        if action == update::WhatsNew::Show {
            self.modal = Some(Modal::WhatsNew);
        }
        if action != update::WhatsNew::Skip {
            let version = current.to_string();
            self.persist(move |prefs| Prefs {
                last_seen_version: version,
                ..prefs
            });
        }
    }

    pub(super) fn render_preferences(
        &mut self,
        ui: &mut egui::Ui,
        palette: theme::Palette,
        ctx: &egui::Context,
    ) {
        // While recording, `Esc` cancels the capture (in the page) instead of
        // closing (preferences.md §4).
        if self.keyboard_prefs.recording.is_none()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.page = Page::Main;
        }
        self.sync_git_session(ctx);
        let updates = UpdatesView {
            version: update::current_version().to_string(),
            state: self
                .update_runner
                .as_ref()
                .map(|runner| runner.state().clone())
                .unwrap_or_default(),
            bundled: update::bundle_path().is_some(),
        };
        // Project section (worktrees.md §6): a picker over the workspace's projects,
        // seeded to the active project on open (`toggle_preferences_page`). The edit
        // buffers are (re)synced from prefs when the picked project changes.
        let project_roots = self.workspace_project_roots();
        let selected_root = self
            .selected_project
            .clone()
            .filter(|root| project_roots.contains(root))
            .or_else(|| project_roots.first().cloned());
        self.ensure_project_edit(selected_root.as_deref());
        let project_labels: Vec<String> = project_roots
            .iter()
            .map(|r| project_root_label(r))
            .collect();
        let selected_index = selected_root
            .as_ref()
            .and_then(|root| project_roots.iter().position(|r| r == root))
            .unwrap_or(0);
        let base_hint = selected_root
            .as_ref()
            .and_then(|r| crate::git::worktree::default_base(r).ok())
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let project_view =
            self.project_settings_edit
                .as_mut()
                .map(|e| crate::ui::preferences::ProjectView {
                    projects: &project_labels,
                    selected: selected_index,
                    worktree_base: &mut e.worktree_base,
                    post_create: &mut e.post_create,
                    run_command: &mut e.run_command,
                    base_port: &mut e.base_port,
                    base_hint: &base_hint,
                });
        let pr_sources = crate::ui::preferences::PrSourcesView {
            github: self.pr_cache.github.clone(),
            bitbucket: self.pr_cache.bitbucket.clone(),
            loaded: self.pr_cache.loaded,
        };
        let action = preferences_page(
            ui,
            &palette,
            &mut self.preferences_section,
            &mut self.theme_mode,
            &mut self.light_theme,
            &mut self.dark_theme,
            &mut self.pull_default,
            &mut self.ai_provider,
            &mut self.ai_instructions,
            &mut self.ai_rebase_provider,
            &mut self.review_agent_command,
            &mut self.editor,
            &mut self.bitbucket_email,
            &mut self.bitbucket_token_input,
            &pr_sources,
            &mut self.notify_on_agent_completion,
            &mut self.keymap,
            &mut self.keyboard_prefs,
            &updates,
            &mut self.commonmark_cache,
            project_view,
        );
        if action.back {
            self.page = Page::Main;
        }
        if action.theme_changed {
            let (theme, light_theme, dark_theme) = (
                self.theme_mode,
                self.light_theme.clone(),
                self.dark_theme.clone(),
            );
            self.persist(move |prefs| Prefs {
                theme,
                light_theme,
                dark_theme,
                ..prefs
            });
        }
        // Same field as the toolbar's radio menu (M12-7): both surfaces
        // read/write `pull_default`, a change on one side is reflected on the
        // other on the next frame.
        if action.pull_changed {
            let pull_default = self.pull_default;
            self.persist(move |prefs| Prefs {
                pull_default,
                ..prefs
            });
        }
        if action.ai_changed {
            let (ai_provider, ai_instructions, ai_rebase_provider, review_agent_command) = (
                self.ai_provider,
                self.ai_instructions.clone(),
                self.ai_rebase_provider,
                self.review_agent_command.clone(),
            );
            self.persist(move |prefs| Prefs {
                ai_provider,
                ai_instructions,
                ai_rebase_provider,
                review_agent_command,
                ..prefs
            });
        }
        if action.editor_changed {
            let editor = self.editor;
            self.persist(move |prefs| Prefs { editor, ..prefs });
        }
        if action.agent_notify_changed {
            let notify_on_agent_completion = self.notify_on_agent_completion;
            self.persist(move |prefs| Prefs {
                notify_on_agent_completion,
                ..prefs
            });
        }
        // Bitbucket email persists like any scalar; the token never touches prefs
        // — "Save" stores it in the Keychain and re-fetches (pull-requests.md §3).
        if action.bitbucket_email_changed {
            let bitbucket_email = self.bitbucket_email.clone();
            self.persist(move |prefs| Prefs {
                bitbucket_email,
                ..prefs
            });
        }
        if action.save_bitbucket_token {
            self.save_bitbucket_token(ctx);
        }
        // Per-project settings (worktrees.md §6): the edit buffers are written
        // back to prefs (an emptied entry is dropped by `set_project_settings`).
        if action.project_changed {
            if let Some(edit) = self.project_settings_edit.as_ref() {
                let root = edit.root.clone();
                let base = base_from_field(&edit.worktree_base);
                let post_create = edit.post_create.clone();
                let run_command = edit.run_command.clone();
                let base_port = edit.base_port.trim().parse::<u16>().ok();
                self.persist(move |mut prefs| {
                    prefs.set_project_settings(root.clone(), base, post_create, run_command);
                    prefs.set_base_port(&root, base_port);
                    prefs
                });
            }
        }
        if action.pick_worktree_base {
            self.pick_worktree_base();
        }
        // The picker rescopes the section to another project; the edit buffers
        // re-sync next frame (`ensure_project_edit` keys on the root).
        if let Some(index) = action.project_selected {
            if let Some(root) = project_roots.get(index) {
                self.selected_project = Some(root.clone());
                ctx.request_repaint();
            }
        }
        // The mutated keymap is already live for routing and badges — only the
        // deviations remain to persist (keybindings.md §6, preferences.md §5).
        if action.keymap_changed {
            let keymap = self.keymap.clone();
            self.persist(move |mut prefs| {
                prefs.set_keybindings(&keymap);
                prefs
            });
        }
        // Updater intents (update.md §6): routed to the runner, which
        // ignores them while busy.
        if action.check_updates {
            if let Some(runner) = self.update_runner.as_mut() {
                runner.request_check();
            }
        }
        if action.install_update {
            if let Some(runner) = self.update_runner.as_mut() {
                runner.request_install();
            }
        }
        if self.page == Page::Main {
            ctx.request_repaint();
        }
        // Toasts above everything, in all modes (git.md §10).
        if toast_overlay(ctx, &palette, &mut self.toasts) {
            if let Some(runner) = self.update_runner.as_mut() {
                runner.request_install();
            }
        }
    }

    /// Activates an agent's repo/tab/pane from the dashboard and returns to the
    /// terminal — the stable `AgentEntry` identity survives reorders, so resolve
    /// the live positions here (`set_active`/`set_active_tab` are index-based).
    fn focus_agent(&mut self, index: usize, ctx: &egui::Context) {
        let Some(entry) = self.caches.agents.get(index).cloned() else {
            return;
        };
        if let Some(repo_pos) = self.caches.keys.iter().position(|k| k == &entry.repo_key) {
            self.workspace.set_active(repo_pos);
            if let Some(tab_pos) = self.workspace.tab_index(repo_pos, entry.tab_id) {
                self.workspace.set_active_tab(tab_pos);
            }
            if let Some(layout) = self.workspace.active_layout_mut() {
                layout.set_focus(entry.pane_id);
            }
        }
        self.central_mode = CentralMode::Terminal;
        ctx.request_repaint();
    }

    /// Validates `selected_agent` against the freshly-rebuilt agent list and, when
    /// unset or stale (its tab/pane closed), auto-selects the most urgent agent
    /// (Working > Done > Idle, ties by workspace order) so the dashboard opens on
    /// a populated panel. Returns the triple to mirror, or `None` when none runs.
    fn resolve_selected_agent(&mut self) -> Option<(RepoKey, TabId, PaneId)> {
        let valid = self.selected_agent.as_ref().is_some_and(|(rk, tid, pid)| {
            self.caches
                .agents
                .iter()
                .any(|e| &e.repo_key == rk && e.tab_id == *tid && e.pane_id == *pid)
        });
        if !valid {
            self.selected_agent = self
                .caches
                .agents
                .iter()
                .enumerate()
                .max_by_key(|(i, e)| (e.badge, std::cmp::Reverse(*i)))
                .map(|(_, e)| (e.repo_key.clone(), e.tab_id, e.pane_id));
        }
        self.selected_agent.clone()
    }

    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) -> FrameKeys {
        if action_pressed(ctx, &self.keymap, Action::ToggleWorkspaceSidebar) {
            self.sidebars.workspace = !self.sidebars.workspace;
        }
        if action_pressed(ctx, &self.keymap, Action::ToggleGitSidebar) {
            // In the PR cockpit the git sidebar is suppressed; ⌘G acts on the
            // changed-files rail instead — the same key hides the same slot.
            if self.central_mode == CentralMode::PullRequests {
                let collapsed = !self.pr_rail_collapsed;
                self.pr_rail_collapsed = collapsed;
                self.persist(move |prefs| Prefs {
                    pr_rail_collapsed: collapsed,
                    ..prefs
                });
            } else {
                self.sidebars.git = !self.sidebars.git;
            }
        }
        // ⌘⇧G (keybindings §1): keyboard equivalent of the header switch — consumed
        // by `render_page` with `switch_request` to share the enter/exit logic.
        let toggle_graph = action_pressed(ctx, &self.keymap, Action::ToggleGraph);
        // In Graph mode, the right sidebar is the only access to the commit detail (and
        // to the status sections via the WIP row): it stays visible — ⌘G and the toggle
        // button don't act on it, consistent with entering Graph which forces it.
        if self.central_mode == CentralMode::Graph {
            self.sidebars.git = true;
        }
        let open_dialog = action_pressed(ctx, &self.keymap, Action::OpenFolder);
        // Cmd+Ctrl+0 opens the Agents dashboard — slot 0 of the positional repo
        // family (keybindings §1). No-op on the empty workspace, where the
        // dashboard and its sidebar entry do not exist (agents.md §5).
        if !self.workspace.is_empty() && open_agents_pressed(ctx) {
            self.central_mode = CentralMode::Agents;
        }
        route_select_repo_keys(ctx, &mut self.workspace);
        route_cycle_repo_keys(ctx, &self.keymap, &mut self.workspace);
        route_tab_keys(ctx, &self.keymap, &mut self.workspace);
        FrameKeys {
            toggle_graph,
            open_dialog,
        }
    }

    pub(super) fn poll_workers(&mut self, ctx: &egui::Context) {
        self.sync_git_session(ctx);
        self.update_agent_watch(ctx);
        self.drain_worktree_sources();
        self.drain_worktree_create(ctx);
        self.drain_worktree_checkout(ctx);
        self.resume_pending_pr_ask(ctx);
        self.drain_worktree_delete(ctx);
        self.poll_pr_runner();
        self.poll_pr_review(ctx);
        self.poll_pr_post(ctx);
        self.git_panel_state.ai_busy = self.git.as_ref().is_some_and(|g| g.ai.busy());
        self.git_panel_state.commit_busy = self
            .git
            .as_ref()
            .is_some_and(|g| g.worker.has_pending_commit());
        self.git_panel_state.status_loading = self.git.as_ref().is_some_and(|g| !g.status_loaded);
        self.git_panel_state.mutation_busy = self
            .git
            .as_ref()
            .is_some_and(|g| g.worker.pending_mutation().is_some());
    }

    pub(super) fn render_page(
        &mut self,
        ui: &mut egui::Ui,
        palette: theme::Palette,
        term_palette: TermPalette,
        ctx: &egui::Context,
        toggle_graph: bool,
    ) -> PageActions {
        let active = self.workspace.active();
        let active_project_root = active.and_then(|index| {
            self.workspace
                .parent_root(index)
                .map(Path::to_path_buf)
                .or_else(|| self.workspace.repo(index).map(|repo| repo.path.clone()))
        });
        // Reminder shown left of the central switch: the project (group root) name
        // and, when the active entry is a linked worktree, the worktree name.
        let project_reminder = active.map(|index| match self.workspace.parent_root(index) {
            Some(root) => (
                root.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                self.workspace.repo(index).map(|r| r.name.clone()),
            ),
            None => (
                self.workspace
                    .repo(index)
                    .map(|r| r.name.clone())
                    .unwrap_or_default(),
                None,
            ),
        });
        let font_size = self.font_zoom.point_size();
        // The active row follows the git session's live snapshot (checkout from the
        // terminal or the graph, edits, commits) without waiting for a sync trigger.
        if let Some(g) = &self.git {
            let label = g.branch.label();
            if !label.is_empty()
                && self.caches.branch_labels.get(&g.key).map(String::as_str) != Some(label)
            {
                self.caches
                    .branch_labels
                    .insert(g.key.clone(), label.to_owned());
            }
            if g.status_loaded {
                if g.status.changed_file_count() > 0 {
                    self.caches
                        .dirty
                        .insert(g.key.clone(), g.status.total_line_stats());
                } else {
                    self.caches.dirty.remove(&g.key);
                }
            }
        }
        let repo_paths: Vec<String> = self
            .workspace
            .repos()
            .map(|r| r.path.to_string_lossy().into_owned())
            .collect();
        let deleting_path = self
            .worktree_delete
            .as_ref()
            .and_then(DeleteRunner::in_flight)
            .map(|req| req.path.clone());
        // Field-path access (not a RepoCaches method): the badges are read by both
        // the rows and the per-project aggregate below, borrowing `agent_badges` only.
        let badges: Vec<AgentBadge> = (0..repo_paths.len())
            .map(|i| {
                self.caches
                    .keys
                    .get(i)
                    .and_then(|key| self.caches.agent_badges.get(key))
                    .copied()
                    .unwrap_or_default()
            })
            .collect();
        // Uncommitted line stats per row (sidebar right-edge `+N −M`), aligned on entry
        // order like `badges`; the active repo's entry is kept live above.
        let stats: Vec<Option<(usize, usize)>> = (0..repo_paths.len())
            .map(|i| {
                self.caches
                    .keys
                    .get(i)
                    .and_then(|key| self.caches.dirty.get(key).copied())
            })
            .collect();
        // `resolve_reorder` reads the full entry layout (folded rows included), so the
        // child-flag mirrors the entry order, not the visible items (worktrees.md §3).
        let child_flags: Vec<bool> = (0..repo_paths.len())
            .map(|i| self.workspace.parent_root(i).is_some())
            .collect();
        // Each root project gets a stable color index — its rank among roots, with
        // worktrees inheriting their root's. Shared with the Agents columns view so a
        // project reads with one color across both surfaces.
        let lane_of_repo: Vec<usize> = {
            let mut lane = 0usize;
            let mut seen_root = false;
            (0..repo_paths.len())
                .map(|i| {
                    if !child_flags[i] {
                        if seen_root {
                            lane += 1;
                        }
                        seen_root = true;
                    }
                    lane
                })
                .collect()
        };
        // The project header surfaces the max activity over its worktrees so a folded
        // group still shows an agent working under it (worktrees.md §1).
        let block_agent: Vec<AgentBadge> = self
            .workspace
            .repos()
            .enumerate()
            .map(|(i, r)| {
                if child_flags[i] {
                    return AgentBadge::None;
                }
                let root = r.path.as_path();
                crate::agent_watch::aggregate(
                    (0..badges.len())
                        .filter(|&j| j == i || self.workspace.parent_root(j) == Some(root))
                        .map(|j| badges[j]),
                )
            })
            .collect();
        let agents_active = self.central_mode == CentralMode::Agents;
        let pr_active = self.central_mode == CentralMode::PullRequests;
        // Resolve the dashboard selection here, before `items` borrows the workspace
        // for the rest of the frame: validates the stored triple against the freshly
        // rebuilt agent list and auto-picks the most urgent agent (panel opens populated).
        let selected_agent = agents_active
            .then(|| self.resolve_selected_agent())
            .flatten();
        let selected_index = selected_agent.as_ref().and_then(|(rk, tid, pid)| {
            self.caches
                .agents
                .iter()
                .position(|e| &e.repo_key == rk && e.tab_id == *tid && e.pane_id == *pid)
        });
        let items: Vec<SidebarItem> = self
            .workspace
            .repos()
            .zip(repo_paths.iter())
            .enumerate()
            .flat_map(|(i, (r, path))| {
                let missing = repo_path_missing(&r.path);
                let is_root = !child_flags[i];
                // A user-hidden project drops out entirely — header and every row;
                // only the eye dropdown still lists it (built separately below).
                let hidden_project = self.workspace.is_in_hidden_project(i);
                let branch = self
                    .caches
                    .keys
                    .get(i)
                    .and_then(|key| self.caches.branch_labels.get(key))
                    .map(String::as_str);
                let header = (is_root && !hidden_project).then(|| {
                    SidebarItem::Header(ProjectHeader {
                        root: i,
                        name: r.name.as_str(),
                        path: path.as_str(),
                        collapsed: self.workspace.is_collapsed(i),
                        lane: lane_of_repo[i],
                        can_create_worktree: !missing,
                        agent: block_agent[i],
                    })
                });
                // A bare root owns only a header; a folded group hides its main and
                // linked rows (worktrees.md §3, §8) — same filter as `nth_visible`.
                let row = (!(hidden_project || self.workspace.is_hidden(i) || (is_root && r.bare)))
                    .then(|| {
                        SidebarItem::Row(RepoRow {
                            index: i,
                            name: r.name.as_str(),
                            path: path.as_str(),
                            missing,
                            main: is_root,
                            branch,
                            deleting: deleting_path.as_deref() == Some(r.path.as_path()),
                            agent: badges[i],
                            stats: stats[i],
                        })
                    });
                header.into_iter().chain(row)
            })
            .collect();
        // Every project (group root) for the eye dropdown — hidden ones included,
        // so a hidden project can be unhidden from the only surface that lists it.
        let project_visibility: Vec<ProjectVisibility> = self
            .workspace
            .repos()
            .enumerate()
            .filter(|(i, _)| !child_flags[*i])
            .map(|(i, r)| ProjectVisibility {
                root: i,
                name: r.name.as_str(),
                hidden: self.workspace.is_user_hidden(i),
            })
            .collect();
        let empty_status = RepoStatus::default();
        let status = self.git.as_ref().map_or(&empty_status, |g| &g.status);
        let graph = self.git.as_ref().and_then(|g| g.graph.as_ref());
        let selected_commit = self.git.as_ref().and_then(|g| g.selected_commit);
        let scroll_to_head = self.git.as_ref().is_some_and(|g| g.scroll_to_head);
        let can_pull_request = self.git.as_ref().is_some_and(|g| g.pr_remote.is_some());
        let commit_detail = self.git.as_ref().and_then(|g| g.detail.as_ref());
        let graph_mode = self.central_mode == CentralMode::Graph;
        // WIP row (M10-7): dirty working tree ⇒ head row of the graph. It is the
        // implicit selection whenever no commit is selected — the right sidebar
        // then keeps the status sections (instead of the commit detail).
        let wip = self.git.as_ref().and_then(|g| {
            let files = g.status.changed_file_count();
            (files > 0).then_some(WipRow {
                files,
                selected: g.selected_commit.is_none(),
            })
        });
        // File whose fullscreen commit diff is open (M9-7): the detail (sidebar)
        // highlights its row and ↑/↓ navigate among the commit's files.
        let commit_diff_file = self.diff.as_ref().and_then(|open| match open.source {
            DiffSource::Commit(oid) => Some((oid, open.path.clone())),
            DiffSource::WorkingTree { .. } => None,
        });
        // ↑/↓ in the graph (keybindings §3): a single consumer of the arrows per frame
        // — an open commit diff (the arrows walk its files) or the status sidebar's
        // armed file nav (status sections shown, i.e. no commit selected) take
        // precedence over the graph.
        let graph_keyboard_nav = commit_diff_file.is_none()
            && !(selected_commit.is_none()
                && self.git_panel_state.file_nav_active
                && self.git_panel_state.selected_file.is_some());
        let branch_label = self
            .git
            .as_ref()
            .map(|g| g.branch.label().to_owned())
            .unwrap_or_default();
        // Graph actions toolbar (M12-6, git.md §10): state computed from the git
        // session (no git logic in the rendering). No session ⇒ no toolbar (non-git,
        // §8).
        let toolbar_state = self.git.as_ref().map(|g| ToolbarState {
            pull_default: self.pull_default,
            busy: g.busy_action(),
            has_remote: g.has_remote,
            has_upstream: g.upstream_remote.is_some(),
            detached: matches!(g.branch, Branch::Detached(_)),
            unborn: matches!(g.branch, Branch::Unborn(_)),
            dirty: g.status.changed_file_count() > 0,
            stash_count: g.stash_count,
            git_missing: g.git_missing,
        });
        // M12-8 status banner (sidebar): merge/rebase in progress. Op outcomes
        // (failures, network successes) go through the toasts (git.md §10).
        let op_in_progress = self.git.as_ref().is_some_and(|g| g.op_in_progress);
        let op = self.git.as_ref().and_then(|g| g.op.as_ref());
        // The conflict editor only exists while an op is in progress; the op ending
        // (Continue/Abort succeeded, or it was finished from the terminal) closes it.
        if !op_in_progress {
            self.conflict_editor = None;
        }
        // Branch editor opening detected after the render (rising edge): it arms the
        // auto-scroll to the HEAD row, which carries the field.
        let branch_editor_was_open = self.branch_editor.open;
        let branch_editor = &mut self.branch_editor;
        let graph_search = &mut self.graph_search;
        let show_workspace = &mut self.sidebars.workspace;
        let workspace_shown = *show_workspace;
        let show_git = &mut self.sidebars.git;
        let default_workspace_opener = self.workspace_opener;
        let installed_openers = self.installed_openers.clone();
        let git_state = &mut self.git_panel_state;
        let diff = &mut self.diff;
        // Any git command running greys the page's Start button out — same
        // rule as the toolbar (computed from the same `busy` state).
        let sync_busy = toolbar_state.as_ref().is_some_and(|s| s.busy.is_some());
        let rebase_page = &mut self.rebase_page;
        let conflict_editor = &mut self.conflict_editor;
        let lane_cache = &mut self.caches.lane_cache;
        let left_sidebar_width = self.left_sidebar_width;
        let right_sidebar_width = self.right_sidebar_width;
        let keymap = &self.keymap;
        let clear_shortcut = self.keymap.shortcut_for(Action::ClearTerminal);

        let mut intents = Vec::new();
        let mut diff_intents = Vec::new();
        let mut review_intents: Vec<crate::review::ReviewIntent> = Vec::new();
        let mut sidebar = SidebarAction::default();
        let mut open_workspace_request = None;
        let mut toggle_preferences_request = false;
        let agents_badge =
            crate::agent_watch::aggregate(self.caches.agents.iter().map(|e| e.badge));
        // Sidebar badge = PRs awaiting my review (the actionable role); authored PRs
        // stay informational (pull-requests.md §2).
        let pr_to_review = self
            .pr_cache
            .pull_requests
            .iter()
            .filter(|pr| pr.role == crate::pull_requests::model::PrRole::ToReview)
            .count();
        // Done-state agents shown as indented child rows under the Agents entry
        // (specs/agents.md §5): `index` is the position in `caches.agents` consumed by
        // `focus_agent` when a row is clicked.
        let done_agents: Vec<crate::ui::repo_sidebar::DoneAgentRow> = self
            .caches
            .agents
            .iter()
            .enumerate()
            .filter(|(_, e)| e.badge == crate::agent_watch::AgentBadge::Done)
            .map(|(index, e)| crate::ui::repo_sidebar::DoneAgentRow {
                index,
                branch: e.branch.clone(),
                tab: e.tab_name.clone(),
            })
            .collect();
        // Dashboard rows borrow `self.caches.agents` (disjoint from `panes`, mutably
        // borrowed in the central closure), so they're built up-front — but only when
        // the dashboard is on screen, to skip the per-frame allocation otherwise.
        let mut agents_select = None;
        let mut agents_focus = None;
        let mut agents_set_view = None;
        let mut agents_set_column_width = None;
        let mut agents_set_terminal_height = None;
        // Column view: a clicked column terminal becomes the single focused agent
        // next frame (merged into `agents_select`).
        let mut terminal_click = None;
        let agents_view = self.agents_view;
        let agents_column_width = self.agents_column_width;
        let agents_terminal_height = self.agents_terminal_height;
        // Set by the dashboard's mirrored terminal when it holds egui focus: gates
        // `Esc` (it must reach the agent as interrupt, not close the dashboard).
        let mut agents_terminal_focused = false;
        // `agent_keys` is index-aligned with `agent_rows`: the column view mirrors
        // each agent's live pane via `caches.panes[(repo, tab)][pane]`.
        let (agent_rows, agent_keys): (
            Vec<crate::ui::agents_view::AgentRow>,
            Vec<(RepoKey, TabId, PaneId)>,
        ) = if agents_active {
            let now_ms = crate::terminal::activity::now_ms();
            self.caches
                .agents
                .iter()
                .map(|e| {
                    let worktree_id = self
                        .caches
                        .keys
                        .iter()
                        .position(|k| k == &e.repo_key)
                        .unwrap_or(usize::MAX);
                    (
                        crate::ui::agents_view::AgentRow {
                            repo: &e.group_name,
                            branch: e.branch.as_deref(),
                            tab: &e.tab_name,
                            agent: e.agent,
                            badge: e.badge,
                            detail: match e.badge {
                                AgentBadge::Working => "Working…".to_owned(),
                                AgentBadge::Done => {
                                    finished_ago(now_ms.saturating_sub(e.last_output_ms))
                                }
                                _ => "Idle".to_owned(),
                            },
                            worktree_id,
                            lane: lane_of_repo.get(worktree_id).copied().unwrap_or(0),
                            stats: self.caches.dirty.get(&e.repo_key).copied(),
                        },
                        (e.repo_key.clone(), e.tab_id, e.pane_id),
                    )
                })
                .unzip()
        } else {
            (Vec::new(), Vec::new())
        };
        let mut open_feedback_request = false;
        let mut open_dialog_requested = false;
        let mut open_link: Option<LinkAction> = None;
        let mut file_menu = crate::ui::file_list::FileMenuOutput::default();
        let mut close_diff = false;
        let mut open_commit_file_request = None;
        let mut pull_default_to_persist = None;
        let mut create_worktree_request = None;
        let mut run_intent: Option<RunIntent> = None;
        // PR cockpit snapshot: the list is cloned only when the cockpit is on
        // screen (mirrors the agents dashboard's gated build above).
        let pr_list: Vec<crate::pull_requests::model::PullRequest> = if pr_active {
            self.pr_cache.pull_requests.clone()
        } else {
            Vec::new()
        };
        let (pr_github_hint, pr_bitbucket_hint, pr_no_repos) = if pr_active {
            use crate::pull_requests::runner::SourceStatus;
            let hint = |status: &SourceStatus| match status {
                SourceStatus::Unavailable(message) => Some(message.clone()),
                _ => None,
            };
            (
                hint(&self.pr_cache.github),
                hint(&self.pr_cache.bitbucket),
                matches!(self.pr_cache.github, SourceStatus::Absent)
                    && matches!(self.pr_cache.bitbucket, SourceStatus::Absent),
            )
        } else {
            (None, None, false)
        };
        let pr_selected = self.pr_selected;
        let pr_detail_width = self.pr_detail_width;
        let pr_rail_collapsed = self.pr_rail_collapsed;
        // The active review surface is taken out of the cache for the frame so it can
        // be borrowed `&mut` inside the central closure; reinserted before the actions
        // that open / close / mutate it run.
        let mut pr_review_local = self
            .pr_active
            .clone()
            .and_then(|key| self.pr_reviews.remove(&key));
        let mut pr_select = None;
        let mut pr_open_url: Option<String> = None;
        let mut pr_checkout = false;
        let mut pr_set_detail_width = None;
        let mut pr_toggle_rail = false;
        let mut pr_back = false;
        let mut pr_close_file = false;
        let mut pr_select_file: Option<usize> = None;
        let mut pr_select_commit: Option<crate::ui::pull_requests_view::CommitSelection> = None;
        let mut pr_open_inline: Option<(usize, Option<u32>)> = None;
        let mut pr_review_intents: Vec<crate::review::ReviewIntent> = Vec::new();
        let mut pr_submit_review = false;
        let pr_agent = self.review_agent_command.clone();
        let mut set_file_view = None;

        // A stale active index makes these accessors return None; degrade to the
        // empty state below instead of panicking mid-frame (M17-4).
        let active_view = active.and_then(|index| {
            let tab = self.workspace.active_tab()?;
            // Inlined `pane_key` (field paths): borrowing all of `caches` here
            // would collide with the `lane_cache` loan above.
            let pane_key = (
                self.caches.keys.get(index)?.clone(),
                self.workspace.tab_id(index, tab)?,
            );
            Some((
                index,
                tab,
                self.workspace.tab_titles()?,
                self.workspace.active_layout()?,
                pane_key,
            ))
        });
        match active_view {
            Some((active, active_tab, tab_titles, layout, pane_key)) => {
                let cwd = self
                    .workspace
                    .repo(active)
                    .map(|r| r.path.clone())
                    .unwrap_or_else(fallback_cwd);
                // A post-create script armed for THIS worktree is consumed as its
                // first pane spawns: the env is set on the pane, the script typed in
                // (worktrees.md §6). A fresh worktree has a single pane.
                let agents_mode = self.central_mode == CentralMode::Agents;
                let take_pc = !agents_mode
                    && matches!(
                        &self.pending_post_create,
                        Some(pc) if pc.worktree_path == canonical_path(&cwd)
                    );
                let post_create = take_pc.then(|| self.pending_post_create.take()).flatten();
                // Run terminal (git.md §3): one process per worktree under `run_key`,
                // the command shared at the project (group root) level. The resolved
                // command and live status are computed here, before the `caches.panes`
                // borrows below take over `self`.
                let run_key = pane_key.0.clone();
                let run_root = active_project_root.clone().unwrap_or_else(|| cwd.clone());
                let run_command_resolved =
                    HelmApp::resolved_run_command(&self.prefs, &run_root, &cwd);
                let run_offset = self.workspace.group_offset(&run_root, &cwd);
                let run_port = HelmApp::resolved_run_port(
                    &self.prefs,
                    &run_root,
                    &cwd,
                    run_offset,
                    &run_command_resolved,
                );
                let run_launch_command = match run_port {
                    Some(port) => crate::run::apply_port(&run_command_resolved, port),
                    None => run_command_resolved.clone(),
                };
                let run_status = run_status_of(self.caches.run_panes.get_mut(&run_key));
                let run_collapsed = self.run_panel_collapsed;
                let run_panel_height = self.run_panel_height;
                // In-diff review (M-RC): the active repo's stored comments feed the
                // diff view; the actions it raises are drained into `review_intents`
                // and applied after the layout closure.
                let empty_comments = crate::review::FileComments::new();
                let review_comments = self.review.get(&run_key).unwrap_or(&empty_comments);
                // Working-tree / commit diffs carry no posted PR threads.
                let no_threads = crate::review::ForgeThreads::new();
                let review_agent = self.review_agent_command.clone();
                let pane_ids = layout.pane_ids();
                // In Agents mode the per-repo terminal tree isn't rendered (the
                // dashboard owns the central area). The list view mirrors the SELECTED
                // agent's pane, the column view mirrors EVERY agent's pane, so the
                // central closure borrows the whole `caches.panes` map (`panes_all`).
                // The active tab's panes are only spawned/fed in Terminal mode — this
                // scoped borrow ends before the map-wide one; they persist in
                // `caches.panes` and respawn on return to Terminal.
                if !agents_mode {
                    let panes = self.caches.panes.entry(pane_key.clone()).or_default();
                    if let (Some(pc), Some(&first)) = (post_create.as_ref(), pane_ids.first()) {
                        panes
                            .entry(first)
                            .or_insert_with(|| open_terminal_with_env(ctx, &cwd, &pc.env));
                    }
                    for id in &pane_ids {
                        panes.entry(*id).or_insert_with(|| open_terminal(ctx, &cwd));
                    }
                    if let Some(pc) = post_create {
                        if let Some(TerminalState::Live(pane)) =
                            pane_ids.first().and_then(|id| panes.get(id))
                        {
                            let _ = pane.feed(post_create_payload(&pc.script).as_bytes());
                        }
                    }
                }
                let panes_all = &mut self.caches.panes;
                // Disjoint `caches` field ⇒ borrowed alongside `panes_all` for the two
                // root_layout closures (the run strip and the central area).
                let run_panes_all = &mut self.caches.run_panes;
                let run_edit = self.run_command_edit.as_mut();
                let run_port_edit = self.run_port_edit.as_mut();
                let mut run_action = crate::ui::run_panel::RunPanelAction::default();

                let mut output = None;
                let mut any_focused = false;
                let mut central_area = egui::Rect::NOTHING;
                let mut tab_action = TabBarAction::default();
                let mut switch_request = None;
                let mut graph_action = GraphAction::default();
                let mut toolbar_action = ToolbarAction::default();
                let mut rebase_page_action = RebasePageAction::default();
                let mut conflict_editor_action = ConflictEditorAction::default();
                let central_mode = self.central_mode;
                let tab_rename = &mut self.tab_rename;
                root_layout(
                    ui,
                    &palette,
                    &items,
                    &child_flags,
                    &project_visibility,
                    Some(active),
                    &branch_label,
                    status,
                    op_in_progress,
                    op,
                    git_state,
                    &mut intents,
                    show_workspace,
                    show_git,
                    graph_mode && selected_commit.is_some(),
                    commit_detail,
                    commit_diff_file.as_ref(),
                    &mut open_commit_file_request,
                    Some(cwd.as_path()),
                    &mut file_menu,
                    self.git_file_view,
                    default_workspace_opener,
                    &installed_openers,
                    &mut open_workspace_request,
                    &mut toggle_preferences_request,
                    &mut open_feedback_request,
                    agents_badge,
                    agents_active,
                    &done_agents,
                    pr_to_review,
                    pr_active,
                    pr_rail_collapsed,
                    &mut pr_toggle_rail,
                    &mut sidebar,
                    left_sidebar_width,
                    right_sidebar_width,
                    keymap,
                    true,
                    run_collapsed,
                    run_panel_height,
                    |ui| {
                        // ⌘R badge next to Run/Relaunch while Cmd is held alone
                        // (keybindings §5); unbound ⇒ no badge.
                        let run_shortcut = ui
                            .input(|i| {
                                let m = i.modifiers;
                                m.command && !m.shift && !m.alt && !m.ctrl
                            })
                            .then(|| keymap.shortcut_for(Action::Run).map(|s| s.display()))
                            .flatten();
                        run_action = crate::ui::run_panel::run_panel(
                            ui,
                            &palette,
                            &run_status,
                            &run_command_resolved,
                            run_port,
                            run_collapsed,
                            run_edit,
                            run_port_edit,
                            run_shortcut.as_deref(),
                            |ui| match run_panes_all.get_mut(&run_key) {
                                Some(TerminalState::Live(pane)) => {
                                    pane.set_visible(true);
                                    pane.set_reply_palette(term_palette);
                                    let exited = pane.has_exited();
                                    let out = terminal_view_readonly(
                                        ui,
                                        pane.grid(),
                                        &term_palette,
                                        font_size,
                                        exited,
                                    );
                                    if let Some(scroll) = out.scroll {
                                        pane.scroll(scroll);
                                    }
                                    if out.size.rows != pane.rows() || out.size.cols != pane.cols()
                                    {
                                        let _ = pane.resize(out.size.rows, out.size.cols);
                                    }
                                }
                                Some(TerminalState::Failed(err)) => {
                                    ui.label(
                                        egui::RichText::new(format!("Run failed: {err}"))
                                            .color(palette.text_muted),
                                    );
                                }
                                None => {}
                            },
                        );
                    },
                    |ui| {
                        // The in-app conflict editor (conflicts.md §3) owns the whole
                        // central area while open, regardless of the current mode.
                        if let Some(state) = conflict_editor.as_mut() {
                            ui.add_space(f32::from(TITLEBAR_HEIGHT));
                            conflict_editor_action = conflict_view(ui, &palette, state, sync_busy);
                        }
                        // The cross-repo dashboard owns the whole central area; it
                        // takes priority over the per-repo diff overlay.
                        else if central_mode == CentralMode::Agents {
                            let action = crate::ui::agents_view::agents_page(
                                ui,
                                &palette,
                                &agent_rows,
                                selected_index,
                                agents_view,
                                agents_column_width,
                                agents_terminal_height,
                                |idx, term_ui, view| match view {
                                    crate::ui::agents_view::TermView::Full => {
                                        if mirror_agent_terminal(
                                            term_ui,
                                            panes_all,
                                            &agent_keys,
                                            idx,
                                            selected_agent.as_ref(),
                                            font_size,
                                            &term_palette,
                                            &palette,
                                            clear_shortcut,
                                            &mut agents_terminal_focused,
                                            &mut open_link,
                                        ) {
                                            terminal_click = Some(idx);
                                        }
                                    }
                                    crate::ui::agents_view::TermView::Preview => {
                                        mirror_agent_preview(
                                            term_ui,
                                            panes_all,
                                            &agent_keys,
                                            idx,
                                            font_size,
                                            &term_palette,
                                        );
                                    }
                                },
                            );
                            agents_set_view = action.set_view;
                            agents_set_column_width = action.set_column_width;
                            agents_set_terminal_height = action.set_terminal_height;
                            agents_select = action.select.or(terminal_click);
                            agents_focus = action.jump;
                        }
                        // The PR cockpit owns the central area like the dashboard, so
                        // the term/graph switch is suppressed and it takes priority
                        // over the per-repo diff overlay; its two-pane body is drawn
                        // by `pull_requests_view`.
                        else if central_mode == CentralMode::PullRequests {
                            let mut review_view = pr_review_local
                                .as_mut()
                                .map(|r| pr_review_view(r, &pr_agent));
                            let hints = crate::ui::pull_requests_view::PrSourceHints {
                                github: pr_github_hint.as_deref(),
                                bitbucket: pr_bitbucket_hint.as_deref(),
                                no_repos: pr_no_repos,
                            };
                            let action = crate::ui::pull_requests_view::pull_requests_page(
                                ui,
                                &palette,
                                &pr_list,
                                pr_selected,
                                &hints,
                                review_view.as_mut(),
                                pr_detail_width,
                                pr_rail_collapsed,
                                self.git_file_view,
                            );
                            pr_select = action.select;
                            pr_open_url = action.open_url;
                            pr_checkout = pr_checkout || action.checkout;
                            pr_set_detail_width = action.set_detail_width;
                            set_file_view = action.set_file_view;
                            pr_back = pr_back || action.back;
                            pr_close_file = pr_close_file || action.close_file;
                            pr_select_file = pr_select_file.or(action.select_file);
                            if pr_select_commit.is_none() {
                                pr_select_commit = action.select_commit;
                            }
                            pr_open_inline = pr_open_inline.or(action.open_inline_comment);
                            pr_review_intents = action.review_intents;
                            pr_submit_review = pr_submit_review || action.submit_review;
                        }
                        // A loaded working-tree file overlays the central area for
                        // Terminal/Graph only — the Agents/PR cockpits above already won.
                        else if let Some(DiffState {
                            source: DiffSource::WorkingTree { staged },
                            loaded: Some(file),
                            view,
                            ..
                        }) = diff.as_mut()
                        {
                            // Replaces the title row (switch); still clear the
                            // macOS traffic-light line above the diff.
                            ui.add_space(f32::from(TITLEBAR_HEIGHT));
                            close_diff = diff_view(
                                ui,
                                &palette,
                                file,
                                crate::ui::diff_view::DiffSurface::WorkingTree { staged: *staged },
                                view,
                                &mut diff_intents,
                                Some(&mut crate::ui::diff_view::DiffReview {
                                    comments: review_comments,
                                    forge: None,
                                    existing: &no_threads,
                                    agent: &review_agent,
                                    intents: &mut review_intents,
                                }),
                            );
                        } else {
                            let (project, worktree) = match &project_reminder {
                                Some((project, worktree)) => {
                                    (Some(project.as_str()), worktree.as_deref())
                                }
                                None => (None, None),
                            };
                            switch_request = central_switch(
                                ui,
                                &palette,
                                central_mode == CentralMode::Graph,
                                keymap,
                                project,
                                worktree,
                                workspace_shown,
                            );
                            match central_mode {
                                CentralMode::Terminal => {
                                    tab_bar(
                                        ui,
                                        &palette,
                                        &tab_titles,
                                        active_tab,
                                        tab_rename,
                                        keymap,
                                        &mut tab_action,
                                    );
                                    central_area = ui.available_rect_before_wrap();
                                    let panes = panes_all.entry(pane_key.clone()).or_default();
                                    output = Some(terminal_tree(
                                        ui,
                                        layout,
                                        &palette,
                                        |ui, id, focused| {
                                            render_pane(
                                                ui,
                                                panes,
                                                id,
                                                focused,
                                                font_size,
                                                &term_palette,
                                                &palette,
                                                clear_shortcut,
                                                &mut any_focused,
                                                &mut open_link,
                                            )
                                        },
                                    ));
                                }
                                CentralMode::Graph => {
                                    // Interactive-rebase page (git.md §9): exclusive
                                    // takeover of the central area while the plan is
                                    // prepared — Start/Cancel arbitrated by the caller.
                                    if let Some(page) = rebase_page.as_mut() {
                                        rebase_page_action =
                                            rebase_view(ui, &palette, page, sync_busy);
                                    }
                                    // Fullscreen commit diff (M9-7, git.md §9): a loaded
                                    // file replaces the graph read-only; otherwise we
                                    // render the graph. Close/`Esc` returns to the graph.
                                    else if let Some(DiffState {
                                        source: DiffSource::Commit(_),
                                        loaded: Some(file),
                                        view,
                                        ..
                                    }) = diff.as_mut()
                                    {
                                        close_diff = diff_view(
                                            ui,
                                            &palette,
                                            file,
                                            crate::ui::diff_view::DiffSurface::Commit,
                                            view,
                                            &mut diff_intents,
                                            Some(&mut crate::ui::diff_view::DiffReview {
                                                comments: review_comments,
                                                forge: None,
                                                existing: &no_threads,
                                                agent: &review_agent,
                                                intents: &mut review_intents,
                                            }),
                                        );
                                    } else {
                                        if let Some(state) = &toolbar_state {
                                            toolbar_action =
                                                graph_toolbar(ui, &palette, state, branch_editor);
                                        }
                                        graph_action = graph_view(
                                            ui,
                                            &palette,
                                            &GraphViewState {
                                                graph,
                                                wip,
                                                selected: selected_commit,
                                                scroll_to_head,
                                                keyboard_nav: graph_keyboard_nav,
                                                can_pull_request,
                                            },
                                            lane_cache,
                                            branch_editor,
                                            graph_search,
                                        );
                                    }
                                }
                                CentralMode::Agents => {}
                                CentralMode::PullRequests => {}
                            }
                        }
                    },
                );
                // ⌘R (keybindings §1) runs the active project, or relaunches it when
                // already running (run+relaunch share one apply path). Reveal the git
                // sidebar and expand the strip so the launch is visible; with no command
                // resolved yet, open the inline editor instead of spawning a no-op shell.
                if action_pressed(ctx, &self.keymap, Action::Run) {
                    self.sidebars.git = true;
                    self.run_panel_collapsed = false;
                    if run_command_resolved.trim().is_empty() {
                        run_action.begin_edit = true;
                    } else {
                        run_action.run = true;
                    }
                }
                // Run strip intents are applied after the match, where the closure
                // borrows are released and `&mut self` is free again (git.md §3).
                if run_action.any() {
                    run_intent = Some(RunIntent {
                        key: run_key,
                        cwd: cwd.clone(),
                        root: run_root,
                        command: run_command_resolved,
                        launch_command: run_launch_command,
                        port: run_port,
                        action: run_action,
                    });
                }
                // The switch click wins; otherwise ⌘⇧G toggles to the other mode.
                let switch_request =
                    switch_request.or(toggle_graph.then(|| central_mode != CentralMode::Graph));
                if let Some(graph) = switch_request {
                    // A mode switch closes the graph search (git.md §9): reopening
                    // Graph later starts fresh.
                    self.graph_search = GraphSearch::default();
                    self.central_mode = if graph {
                        CentralMode::Graph
                    } else {
                        CentralMode::Terminal
                    };
                    if graph {
                        self.sidebars.git = true;
                        if let Some(git) = self.git.as_mut() {
                            // Restarts at the first page on every entry into Graph mode
                            // (M9-8); **Load more** will grow it afterwards. Also
                            // re-arms the auto-scroll to the HEAD row (git.md §9) —
                            // honored on the fresh graph: outside Graph mode the poll
                            // doesn't reload, the displayed one may date from a HEAD
                            // moved in the terminal.
                            git.graph_limit = graph::PAGE_SIZE;
                            git.scroll_to_head = true;
                            git.graph_fresh = false;
                            git.reload_graph();
                            ctx.request_repaint();
                        }
                    } else {
                        // Switching back to Terminal exits the fullscreen commit diff (§9).
                        if matches!(
                            *diff,
                            Some(DiffState {
                                source: DiffSource::Commit(_),
                                ..
                            })
                        ) {
                            *diff = None;
                        }
                    }
                }
                if let (Some(oid), Some(git)) = (graph_action.selected, self.git.as_mut()) {
                    // Selecting a commit (M9-5) ⇒ loads its detail (M9-2) off the UI
                    // thread to show it in the right sidebar (M9-6). Always re-requested:
                    // a re-click acts as a retry after an error reply (drain discards
                    // stale replies). Changing commit drops the old detail to never
                    // render a stale file list.
                    if git.selected_commit != Some(oid) {
                        git.detail = None;
                    }
                    git.selected_commit = Some(oid);
                    git.worker.send(GitCommand::CommitDetail(oid));
                    ctx.request_repaint();
                }
                if graph_action.wip_selected {
                    // Click on the WIP row (M10-7): clears the commit selection — the
                    // right sidebar switches back to the status sections on the next
                    // render (WIP is the implicit selection without a commit).
                    if let Some(git) = self.git.as_mut() {
                        git.selected_commit = None;
                        git.detail = None;
                        ctx.request_repaint();
                    }
                }
                if graph_action.scrolled_to_head {
                    // The one-shot is consumed only if the rendered graph was fresh: on a
                    // stale graph (cache from a switch, reload in flight), the scroll
                    // targeted the old HEAD's row — it must replay on the fresh graph to
                    // come.
                    if let Some(git) = self.git.as_mut() {
                        if git.graph_fresh {
                            git.scroll_to_head = false;
                        }
                    }
                }
                if graph_action.load_more {
                    // **Load more** (M9-8): grows the page by one slice and reloads
                    // (explicit pagination, never silent truncation, git.md §9).
                    if let Some(git) = self.git.as_mut() {
                        git.graph_limit += graph::PAGE_SIZE;
                        git.reload_graph();
                        ctx.request_repaint();
                    }
                }
                if let (Some(branch), Some(git)) = (graph_action.checkout.take(), self.git.as_mut())
                {
                    // Double-click on a local branch chip: checkout (automatic stash if
                    // the tree is dirty), then graph reload — the status snapshot
                    // already comes back from the mutating command itself. Pagination
                    // restarts at the first page: extending to the **new** HEAD
                    // re-covers the target branch (the clicked row stays in page), and a
                    // limit inflated by an old deep branch is not paid on every poll
                    // after returning to a recent branch.
                    git.graph_limit = graph::PAGE_SIZE;
                    git.graph_fresh = false;
                    git.send_then_reload_graph(GitCommand::Checkout(branch));
                    ctx.request_repaint();
                }
                // Graph toolbar intents (M12-6, git.md §10): network ops to the
                // SyncRunner (one at a time; refresh status+graph on return via
                // drain_sync), local actions to the worker (the mutating command itself
                // replies with a status snapshot).
                if let (Some(command), Some(git)) = (toolbar_action.sync, self.git.as_mut()) {
                    git.request_sync(command, &mut self.toasts, ctx.input(|i| i.time));
                    ctx.request_repaint();
                }
                // Force push (Push chevron, git.md §10): a deliberate, one-shot
                // act behind a confirmation modal naming the branch + remote —
                // nothing runs before it. The entry is only emittable with an
                // upstream (`has_upstream`), so the remote is always present.
                if toolbar_action.force_push {
                    if let Some(git) = self.git.as_ref() {
                        if let (Branch::Named(branch), Some(remote)) =
                            (&git.branch, &git.upstream_remote)
                        {
                            self.modal = Some(Modal::ForcePush {
                                branch: branch.clone(),
                                remote: remote.clone(),
                            });
                            ctx.request_repaint();
                        }
                    }
                }
                // Cancel on the AI rebase chip: the runner kills the provider,
                // aborts a rebase left in progress and reports the verified
                // result — the chip shows "Cancelling…" until the reply lands.
                if toolbar_action.cancel_ai_rebase {
                    if let Some(git) = self.git.as_ref() {
                        git.ai_rebase.cancel();
                        ctx.request_repaint();
                    }
                }
                if let Some(default) = toolbar_action.set_default {
                    // Selection **without execution** (git.md §10).
                    self.pull_default = default;
                    pull_default_to_persist = Some(default);
                }
                if let (Some(name), Some(git)) = (graph_action.create_branch, self.git.as_mut()) {
                    git.send_then_reload_graph(GitCommand::CreateBranch(name));
                    ctx.request_repaint();
                }
                // Create branch confirmed in the inline editor opened from a chip
                // (git.md §9): the editor's target holds the source committish; the
                // branch is created at it **without checkout**. The editor stays
                // open until the worker replies (closed on success, inline error on
                // duplicate — resolved in `on_status`). Graph reloaded behind it.
                if let Some(name) = graph_action.create_branch_at {
                    if let (Some(git), Some(target)) =
                        (self.git.as_mut(), self.branch_editor.target.as_ref())
                    {
                        git.send_then_reload_graph(GitCommand::CreateBranchAt {
                            name,
                            at: target.source.clone(),
                        });
                        ctx.request_repaint();
                    }
                }
                // Create tag confirmed in the inline editor (git.md §9): a
                // lightweight tag on the editor's commit (`target.oid`), no
                // checkout and no push. Same open-until-reply contract as Create
                // branch (inline error on duplicate, graph reloaded behind it).
                if let Some(name) = graph_action.create_tag_at {
                    if let (Some(git), Some(target)) =
                        (self.git.as_mut(), self.branch_editor.target.as_ref())
                    {
                        git.send_then_reload_graph(GitCommand::CreateTagAt {
                            name,
                            at: target.oid,
                        });
                        ctx.request_repaint();
                    }
                }
                // Rename confirmed in the inline editor opened from a chip
                // (git.md §9): `git branch -m` semantics on the worker — HEAD
                // follows when it is the current branch, upstream config moves
                // with it. Same open-until-reply contract as Create branch
                // (closed on success, inline error on duplicate); graph reloaded
                // behind it (FIFO).
                if let Some((from, to)) = graph_action.rename_branch {
                    if let Some(git) = self.git.as_mut() {
                        git.send_then_reload_graph(GitCommand::RenameBranch { from, to });
                        ctx.request_repaint();
                    }
                }
                if let Some(branch) = graph_action.create_worktree {
                    if let Some(root) = active_project_root.clone() {
                        create_worktree_request = Some((root, branch));
                        ctx.request_repaint();
                    }
                }
                // Create branch entry of a chip's menu (git.md §9): opens the
                // inline editor on the targeted ref's row (same field as the
                // toolbar Branch button) — its `Enter` creates a local branch at
                // the ref's commit without checkout. Nothing is sent before.
                if let Some(request) = graph_action.open_branch_editor {
                    self.branch_editor = BranchEditor {
                        open: true,
                        target: Some(BranchEditorTarget {
                            oid: request.oid,
                            source: request.source,
                        }),
                        ..BranchEditor::default()
                    };
                    ctx.request_repaint();
                }
                // Create tag entry of a commit row's menu (git.md §9): opens the
                // inline editor (tag mode) on that commit's row — its `Enter` tags
                // the commit. The source committish is unused for tags (created by
                // oid); `target.oid` anchors the field. Nothing is sent before.
                if let Some(oid) = graph_action.open_tag_editor {
                    self.branch_editor = BranchEditor {
                        open: true,
                        tag: true,
                        target: Some(BranchEditorTarget {
                            oid,
                            source: String::new(),
                        }),
                        ..BranchEditor::default()
                    };
                    ctx.request_repaint();
                }
                // Rename entry of a chip's menu (git.md §9): opens the inline
                // editor on the branch's row **pre-filled** with the current name
                // — `Enter` renames. `target.oid` anchors the field (source unused
                // for a rename); `rename` carries the old name. Nothing is sent
                // before.
                if let Some(request) = graph_action.open_rename_editor {
                    self.branch_editor = BranchEditor {
                        open: true,
                        name: request.name.clone(),
                        rename: Some(request.name),
                        target: Some(BranchEditorTarget {
                            oid: request.oid,
                            source: String::new(),
                        }),
                        ..BranchEditor::default()
                    };
                    ctx.request_repaint();
                }
                // Rebase onto entry of the chips' menu (git.md §9): current branch
                // rebased onto the targeted ref — same execution rules as the
                // toolbar's network ops (sync runner, one op at a time, outcome
                // toasts + status/graph refresh via drain_sync).
                if let (Some(onto), Some(git)) = (graph_action.rebase_onto, self.git.as_mut()) {
                    git.request_sync(
                        SyncCommand::Rebase(onto),
                        &mut self.toasts,
                        ctx.input(|i| i.time),
                    );
                    ctx.request_repaint();
                }
                // Merge entry of the chips' menu (git.md §9): the targeted ref
                // merged into the current branch — same execution rules as
                // Rebase onto (sync runner, one op at a time, spinner + toasts).
                if let (Some(branch), Some(git)) = (graph_action.merge, self.git.as_mut()) {
                    git.request_sync(
                        SyncCommand::Merge(branch),
                        &mut self.toasts,
                        ctx.input(|i| i.time),
                    );
                    ctx.request_repaint();
                }
                // Create pull request entry of the chips' menu (git.md §9): the
                // clicked ref is the destination, the current branch the source.
                // Not a git op — the forge's prefilled create-PR page opens in
                // the browser (the entry only appears with a recognized forge).
                if let Some(dest) = graph_action.create_pull_request {
                    let now = ctx.input(|i| i.time);
                    let url =
                        self.git
                            .as_ref()
                            .and_then(|git| match (&git.pr_remote, &git.branch) {
                                (Some(forge), Branch::Named(source)) => {
                                    Some(forge.pull_request_url(source, &dest))
                                }
                                _ => None,
                            });
                    if let Some(url) = url {
                        match crate::terminal::links::open_url(&url) {
                            Ok(()) => self.toasts.success("Opening pull request…", now),
                            Err(err) => self.toasts.error(
                                format!("Couldn't open the pull request — {}", err.message()),
                                now,
                            ),
                        }
                    }
                    ctx.request_repaint();
                }
                // Interactive rebase entry (git.md §9): opens the plan page on
                // the clicked ref — nothing runs before its Start. Refused up
                // front when the rebase could not start anyway: op already in
                // progress (resolve or abort it first) or detached HEAD.
                if let (Some(onto), Some(git)) =
                    (graph_action.interactive_rebase_onto, self.git.as_mut())
                {
                    let now = ctx.input(|i| i.time);
                    if git.op_in_progress {
                        self.toasts.error(
                            "A merge or rebase is already in progress — resolve or abort it first",
                            now,
                        );
                    } else if matches!(git.branch, Branch::Detached(_)) {
                        self.toasts
                            .error("HEAD is detached — check out a branch to rebase", now);
                    } else {
                        self.rebase_page = Some(RebasePage::loading(git.branch.label(), &onto));
                        git.worker.send(GitCommand::RebaseTodo { onto });
                    }
                    ctx.request_repaint();
                }
                // AI rebase entry (git.md §9): opens the recap modal on the
                // clicked ref — nothing runs before its Start. Same up-front
                // refusals as the interactive flavor.
                if let (Some(onto), Some(git)) = (graph_action.ai_rebase_onto, self.git.as_mut()) {
                    let now = ctx.input(|i| i.time);
                    if git.op_in_progress {
                        self.toasts.error(
                            "A merge or rebase is already in progress — resolve or abort it first",
                            now,
                        );
                    } else if matches!(git.branch, Branch::Detached(_)) {
                        self.toasts
                            .error("HEAD is detached — check out a branch to rebase", now);
                    } else {
                        self.modal = Some(Modal::AiRebase(AiRebasePage::loading(
                            git.branch.label(),
                            &onto,
                        )));
                        git.worker.send(GitCommand::RebaseTodo { onto });
                    }
                    ctx.request_repaint();
                }
                // Rebase page outcomes: Start hands the plan to the sync runner
                // (one op at a time — busy ⇒ refusal toast, the page stays open
                // for a retry); Cancel/Esc/Close just drops the page, nothing
                // has run.
                if rebase_page_action.start {
                    if let (Some(page), Some(git)) = (self.rebase_page.as_ref(), self.git.as_mut())
                    {
                        let command = SyncCommand::InteractiveRebase {
                            current: page.current.clone(),
                            onto: page.onto.clone(),
                            steps: page.steps(),
                        };
                        if git.request_sync(command, &mut self.toasts, ctx.input(|i| i.time)) {
                            self.rebase_page = None;
                        }
                        ctx.request_repaint();
                    }
                } else if rebase_page_action.cancel {
                    self.rebase_page = None;
                    ctx.request_repaint();
                }
                // Conflict editor outcomes (conflicts.md §3): Close drops the editor;
                // a resolve maps to a worker command (Compose/Delete → ResolveFile,
                // Keep → Stage) and re-reads the rail so the resolved file drops out
                // (the editor closes when the last conflict is gone).
                if conflict_editor_action.close {
                    self.conflict_editor = None;
                    ctx.request_repaint();
                } else if let Some(request) = conflict_editor_action.resolve {
                    if let Some(git) = self.git.as_ref() {
                        let command = match request {
                            ResolveRequest::Compose { path, content } => GitCommand::ResolveFile {
                                path,
                                content: Some(content),
                            },
                            ResolveRequest::Delete { path } => GitCommand::ResolveFile {
                                path,
                                content: None,
                            },
                            ResolveRequest::Keep { path } => GitCommand::Stage(path),
                            ResolveRequest::UseSide { path, ours } => {
                                GitCommand::ResolveFileSide { path, ours }
                            }
                        };
                        git.worker.send(command);
                        git.worker.send(GitCommand::ReadConflicts);
                    }
                    if let Some(editor) = self.conflict_editor.as_mut() {
                        editor.reload();
                    }
                    ctx.request_repaint();
                }
                // Delete entries of the chips' context menu (git.md §9): nothing is sent
                // until the modal confirms.
                if let Some(target) = graph_action.delete {
                    self.modal = Some(Modal::DeleteBranch(target));
                }
                // Checkout entry of a tag's menu (git.md §9): detached checkout on
                // the tag's commit (auto-stash if the tree is dirty), then graph
                // reload — same pagination reset as a branch checkout so the new
                // (detached) HEAD's row stays in page.
                if let (Some(tag), Some(git)) = (graph_action.checkout_tag, self.git.as_mut()) {
                    git.graph_limit = graph::PAGE_SIZE;
                    git.graph_fresh = false;
                    git.send_then_reload_graph(GitCommand::CheckoutTag(tag));
                    ctx.request_repaint();
                }
                // Push tag entry (git.md §9): `git push origin <tag>` on the sync
                // runner — same execution rules as the toolbar network ops (one op
                // at a time, spinner + outcome toast via drain_sync).
                if let (Some(tag), Some(git)) = (graph_action.push_tag, self.git.as_mut()) {
                    git.request_sync(
                        SyncCommand::PushTag(tag),
                        &mut self.toasts,
                        ctx.input(|i| i.time),
                    );
                    ctx.request_repaint();
                }
                // Cherry-pick / Revert entries (git.md §9): replay or invert the
                // row's commit on the current branch — same execution rules as the
                // plain rebase (sync runner, one op at a time, spinner + toast); a
                // conflict leaves the op in progress for the banner to resolve.
                if let (Some(oid), Some(git)) = (graph_action.cherry_pick, self.git.as_mut()) {
                    git.request_sync(
                        SyncCommand::CherryPick(oid.to_string()),
                        &mut self.toasts,
                        ctx.input(|i| i.time),
                    );
                    ctx.request_repaint();
                }
                if let (Some(oid), Some(git)) = (graph_action.revert, self.git.as_mut()) {
                    git.request_sync(
                        SyncCommand::Revert(oid.to_string()),
                        &mut self.toasts,
                        ctx.input(|i| i.time),
                    );
                    ctx.request_repaint();
                }
                // Reset <current> to here (git.md §9): Soft/Mixed lose nothing, so
                // they run straight on the worker (graph reloaded behind); Hard is
                // destructive ⇒ gated behind a red modal naming the branch and the
                // target commit. The entry is absent on a detached HEAD, so a branch
                // is always checked out here.
                if let (Some((oid, mode)), Some(git)) = (graph_action.reset, self.git.as_ref()) {
                    if mode == git2::ResetType::Hard {
                        let short = oid.to_string();
                        self.modal = Some(Modal::ResetHard {
                            branch: git.branch.label().to_owned(),
                            target: oid,
                            short: short[..short.len().min(7)].to_owned(),
                        });
                    } else {
                        git.send_then_reload_graph(GitCommand::Reset { target: oid, mode });
                        ctx.request_repaint();
                    }
                }
                // Delete tag entry (git.md §9): nothing is sent until the modal
                // confirms (it carries the "Also delete on origin" choice).
                if let Some(tag) = graph_action.delete_tag {
                    self.modal = Some(Modal::DeleteTag {
                        tag,
                        also_remote: false,
                    });
                }
                // Apply stash: the stash stays and HEAD doesn't move, so only the
                // status is refreshed (worker reply) — no graph reload, unlike Pop.
                if let (Some(oid), Some(git)) = (graph_action.stash_apply, self.git.as_ref()) {
                    git.worker.send(GitCommand::StashApplyAt(oid));
                    ctx.request_repaint();
                }
                if let (Some(oid), Some(git)) = (graph_action.stash_pop, self.git.as_ref()) {
                    git.send_then_reload_graph(GitCommand::StashPopAt(oid));
                    ctx.request_repaint();
                }
                // Delete stash entry: nothing is sent until the modal confirms.
                if let Some(target) = graph_action.stash_drop {
                    self.modal = Some(Modal::DropStash(target));
                }
                if self.branch_editor.open
                    && !branch_editor_was_open
                    && self.branch_editor.target.is_none()
                {
                    // Branch editor opened this frame (toolbar click): scrolls to the
                    // HEAD row — it's the one that carries the field. A chip-targeted
                    // editor anchors on its own (already-visible) row, no scroll.
                    if let Some(git) = self.git.as_mut() {
                        git.scroll_to_head = true;
                        ctx.request_repaint();
                    }
                }
                if toolbar_action.stash {
                    if let Some(git) = self.git.as_ref() {
                        git.send_then_reload_graph(GitCommand::Stash);
                        ctx.request_repaint();
                    }
                }
                if toolbar_action.pop {
                    if let Some(git) = self.git.as_ref() {
                        git.send_then_reload_graph(GitCommand::StashPop);
                        ctx.request_repaint();
                    }
                }
                if let Some((oid, path)) = open_commit_file_request {
                    // Click on a file in the detail (M9-6) ⇒ opens its fullscreen diff
                    // (M9-7, read-only, vs first parent): we record the diff state and
                    // ask the worker to compute it; the result arrives via `drain`. The
                    // intent carries the oid of the commit whose list was displayed — not
                    // the current selection, which may have changed in the meantime.
                    if let Some(git) = &self.git {
                        git.worker.send(GitCommand::CommitFileDiff {
                            oid,
                            path: path.clone(),
                        });
                        DiffState::open(diff, DiffSource::Commit(oid), path);
                        ctx.request_repaint();
                    }
                }

                // A terminal holding keyboard focus reclaims the arrows (they go to the
                // PTY, keybindings §2): the sidebar's ↑/↓ file nav disarms, otherwise it
                // would reopen diffs from the terminal. Rearmed on the next click on a
                // row (git.md §3).
                if any_focused {
                    self.git_panel_state.file_nav_active = false;
                }
                // Only the working-tree overlay claims the DiffView zone: the commit
                // diff is read-only and never had the §3 staging shortcuts.
                let diff_open = diff.as_ref().is_some_and(|d| {
                    matches!(d.source, DiffSource::WorkingTree { .. }) && d.loaded.is_some()
                });
                let zone = focus_zone(diff_open, any_focused);
                let mut close_tab_requested = false;
                if let Some(active_layout) = self.workspace.active_layout_mut() {
                    if let Some(output) = output {
                        if let Some(id) = output.focus {
                            active_layout.set_focus(id);
                        }
                        if let Some(drag) = output.resize {
                            let (cell_w, cell_h) = cell_metrics(ctx, font_size);
                            active_layout.resize_split(
                                drag.first,
                                drag.second,
                                drag.delta,
                                rect(central_area),
                                cell_w,
                                cell_h,
                            );
                        }
                        if let Some(drop) = output.drop {
                            match drop.zone {
                                crate::ui::terminal_view::DropZone::Swap => {
                                    active_layout.swap_panes(drop.src, drop.target)
                                }
                                crate::ui::terminal_view::DropZone::Side(side) => {
                                    active_layout.move_pane(drop.src, drop.target, side)
                                }
                            }
                        }
                    }
                    if zone.terminal_shortcuts_active() {
                        route_layout_keys(
                            ctx,
                            &self.keymap,
                            active_layout,
                            central_area,
                            font_size,
                            &mut close_tab_requested,
                        );
                    }
                    let live: std::collections::HashSet<PaneId> =
                        active_layout.pane_ids().into_iter().collect();
                    if let Some(panes) = self.caches.panes.get_mut(&pane_key) {
                        panes.retain(|id, _| live.contains(id));
                    }
                }
                if zone.terminal_shortcuts_active() {
                    route_zoom_keys(ctx, &self.keymap, &mut self.font_zoom);
                }
                if close_tab_requested {
                    self.close_active_tab(active_tab);
                }
                if let Some(tab) = tab_action.select {
                    self.workspace.set_active_tab(tab);
                }
                if tab_action.new {
                    self.workspace.add_tab();
                }
                if let Some((from, anchor, after)) = tab_action.reorder {
                    self.workspace.reorder_tab(from, anchor, after);
                }
                // Before `close`: an edit commit and a close in the same frame must
                // rename the targeted tab before the reindexing.
                if let Some((tab, name)) = tab_action.rename {
                    self.workspace.rename_tab(tab, &name);
                }
                if let Some(tab) = tab_action.close {
                    self.close_active_tab(tab);
                }
            }
            None => {
                let panes_all = &mut self.caches.panes;
                root_layout(
                    ui,
                    &palette,
                    &items,
                    &child_flags,
                    &project_visibility,
                    None,
                    &branch_label,
                    status,
                    op_in_progress,
                    op,
                    git_state,
                    &mut intents,
                    show_workspace,
                    show_git,
                    false,
                    None,
                    None,
                    &mut open_commit_file_request,
                    None,
                    &mut file_menu,
                    self.git_file_view,
                    default_workspace_opener,
                    &installed_openers,
                    &mut open_workspace_request,
                    &mut toggle_preferences_request,
                    &mut open_feedback_request,
                    agents_badge,
                    agents_active,
                    &done_agents,
                    pr_to_review,
                    pr_active,
                    pr_rail_collapsed,
                    &mut pr_toggle_rail,
                    &mut sidebar,
                    left_sidebar_width,
                    right_sidebar_width,
                    keymap,
                    false,
                    true,
                    crate::ui::run_panel::HEADER_HEIGHT,
                    |_ui| {},
                    |ui| {
                        if agents_active {
                            let action = crate::ui::agents_view::agents_page(
                                ui,
                                &palette,
                                &agent_rows,
                                selected_index,
                                agents_view,
                                agents_column_width,
                                agents_terminal_height,
                                |idx, term_ui, view| match view {
                                    crate::ui::agents_view::TermView::Full => {
                                        if mirror_agent_terminal(
                                            term_ui,
                                            panes_all,
                                            &agent_keys,
                                            idx,
                                            selected_agent.as_ref(),
                                            font_size,
                                            &term_palette,
                                            &palette,
                                            clear_shortcut,
                                            &mut agents_terminal_focused,
                                            &mut open_link,
                                        ) {
                                            terminal_click = Some(idx);
                                        }
                                    }
                                    crate::ui::agents_view::TermView::Preview => {
                                        mirror_agent_preview(
                                            term_ui,
                                            panes_all,
                                            &agent_keys,
                                            idx,
                                            font_size,
                                            &term_palette,
                                        );
                                    }
                                },
                            );
                            agents_set_view = action.set_view;
                            agents_set_column_width = action.set_column_width;
                            agents_set_terminal_height = action.set_terminal_height;
                            agents_select = action.select.or(terminal_click);
                            agents_focus = action.jump;
                        } else if pr_active {
                            let mut review_view = pr_review_local
                                .as_mut()
                                .map(|r| pr_review_view(r, &pr_agent));
                            let hints = crate::ui::pull_requests_view::PrSourceHints {
                                github: pr_github_hint.as_deref(),
                                bitbucket: pr_bitbucket_hint.as_deref(),
                                no_repos: pr_no_repos,
                            };
                            let action = crate::ui::pull_requests_view::pull_requests_page(
                                ui,
                                &palette,
                                &pr_list,
                                pr_selected,
                                &hints,
                                review_view.as_mut(),
                                pr_detail_width,
                                pr_rail_collapsed,
                                self.git_file_view,
                            );
                            pr_select = action.select;
                            pr_open_url = action.open_url;
                            pr_checkout = pr_checkout || action.checkout;
                            pr_set_detail_width = action.set_detail_width;
                            set_file_view = action.set_file_view;
                            pr_back = pr_back || action.back;
                            pr_close_file = pr_close_file || action.close_file;
                            pr_select_file = pr_select_file.or(action.select_file);
                            if pr_select_commit.is_none() {
                                pr_select_commit = action.select_commit;
                            }
                            pr_open_inline = pr_open_inline.or(action.open_inline_comment);
                            pr_review_intents = action.review_intents;
                            pr_submit_review = pr_submit_review || action.submit_review;
                        } else {
                            ui.add_space(f32::from(TITLEBAR_HEIGHT));
                            open_dialog_requested = central_empty_state(ui, &palette, keymap);
                        }
                    },
                );
            }
        }

        if let Some(default) = pull_default_to_persist {
            self.persist(|prefs| Prefs {
                pull_default: default,
                ..prefs
            });
        }
        if let Some(intent) = run_intent {
            self.apply_run_intent(intent, ctx);
        }

        // Close/`Esc` on either kind: the fullscreen commit diff (M9-7) returns to
        // the graph without changing the central mode (still Graph).
        if close_diff {
            self.diff = None;
        }
        intents.append(&mut diff_intents);
        for intent in review_intents {
            self.apply_review_intent(intent, ctx);
        }

        let mut generate_requested = false;
        let mut continue_op_requested = false;
        if let Some(git) = &self.git {
            let mut sent = false;
            let mut reload_diff = false;
            for intent in intents {
                match intent {
                    GitIntent::GenerateMessage => generate_requested = true,
                    // Banner Abort (git.md §10): confirmation modal first —
                    // resolutions in progress are discarded by the abort.
                    GitIntent::AbortOp => self.modal = Some(Modal::AbortOp),
                    // Banner Continue (conflicts.md §2): finalize the op once every
                    // conflict is resolved — handled after the loop (needs `&mut`).
                    GitIntent::ContinueOp => continue_op_requested = true,
                    // Resolve button / conflicted row (conflicts.md §3): open the
                    // editor and read the conflict rail; an optional focus selects
                    // the clicked file once loaded.
                    GitIntent::OpenConflictEditor { focus } => {
                        git.worker.send(GitCommand::ReadConflicts);
                        self.conflict_editor = Some(ConflictEditorState::opening(focus));
                        sent = true;
                    }
                    // Discard hunk (git.md §4): destructive, so it arms a
                    // confirmation modal capturing the open file — never straight
                    // to the worker.
                    GitIntent::DiscardHunk(hunk) => {
                        if let Some(d) = self
                            .diff
                            .as_ref()
                            .filter(|d| matches!(d.source, DiffSource::WorkingTree { .. }))
                        {
                            self.modal = Some(Modal::DiscardHunk {
                                path: d.path.clone(),
                                hunk,
                            });
                        }
                    }
                    GitIntent::OpenDiff { path, staged } => {
                        git.worker.send(GitCommand::Diff {
                            path: path.clone(),
                            staged,
                        });
                        DiffState::open(&mut self.diff, DiffSource::WorkingTree { staged }, path);
                        sent = true;
                    }
                    // Shared flat/tree mode (M40): applied + persisted after the
                    // loop, once the `&self.git` borrow is released.
                    GitIntent::SetFileView(view) => set_file_view = Some(view),
                    other => {
                        if let Some(command) = overlay_or_command(other, self.diff.as_ref()) {
                            if matches!(
                                command,
                                GitCommand::StageHunk { .. }
                                    | GitCommand::UnstageHunk { .. }
                                    | GitCommand::StageLines { .. }
                                    | GitCommand::UnstageLines { .. }
                            ) {
                                reload_diff = true;
                            }
                            git.worker.send(command);
                            sent = true;
                        }
                    }
                }
            }
            // After granular staging, re-request the open file's diff so the overlay
            // reflects the current index state.
            if reload_diff {
                if let Some(DiffState {
                    source: DiffSource::WorkingTree { staged },
                    path,
                    view,
                    ..
                }) = &mut self.diff
                {
                    view.clear();
                    git.worker.send(GitCommand::Diff {
                        path: path.clone(),
                        staged: *staged,
                    });
                }
            }
            if sent {
                ctx.request_repaint();
            }
        }
        if let Some((root, source)) = create_worktree_request {
            let base = self.project_worktree_base(&root);
            let source = crate::git::worktree::CreateSource::Existing(source);
            self.request_create_worktree(root, source, None, base, ctx);
        }

        // Banner Continue (conflicts.md §2): run the op's `--continue` through the
        // sync runner (one op at a time) and close the editor; the resulting status
        // refresh clears the banner once the op ends.
        if continue_op_requested {
            if let Some(git) = self.git.as_mut() {
                let now = ctx.input(|i| i.time);
                git.request_sync(SyncCommand::ContinueOp, &mut self.toasts, now);
            }
            self.conflict_editor = None;
            ctx.request_repaint();
        }

        // AI generation (commit card): carried by the session's `AiRunner` — one
        // request at a time, the result comes back via `drain_ai`.
        if generate_requested {
            if let Some(git) = self.git.as_mut() {
                if git
                    .ai
                    .request(self.ai_provider, self.ai_instructions.clone())
                {
                    ctx.request_repaint();
                }
            }
        }

        if let Some(opener) = open_workspace_request {
            if let Some(repo) = self.workspace.active_repo() {
                // Both diff sources travel to the IDE: a commit preview opens the
                // file's current on-disk content, not the historical snapshot.
                let file = self.diff.as_ref().map(|diff| repo.path.join(&diff.path));
                let _ = launch_workspace(opener, &repo.path, file.as_deref());
            }
            if self.workspace_opener != opener {
                self.workspace_opener = opener;
                self.persist(move |prefs| Prefs {
                    workspace_opener: opener,
                    ..prefs
                });
            }
        }
        if let Some(index) = sidebar.select {
            self.workspace.set_active(index);
            // Picking a project leaves a cross-repo Helm mode for its terminal.
            if matches!(
                self.central_mode,
                CentralMode::Agents | CentralMode::PullRequests
            ) {
                self.central_mode = CentralMode::Terminal;
            }
        }
        // The sidebar's Agents entry opens the dashboard in the central area; `Esc`
        // (with no terminal focused — the dashboard owns the area) closes it.
        if sidebar.open_agents {
            self.central_mode = CentralMode::Agents;
            ctx.request_repaint();
        }
        // The Pull Requests entry opens the cockpit; `Esc` closes it (pull-requests.md §2).
        if sidebar.open_pull_requests {
            self.central_mode = CentralMode::PullRequests;
            ctx.request_repaint();
        }
        // A Done child row under the Agents entry was clicked: jump to that pane (the
        // focus then acknowledges its green, clearing the row).
        if let Some(index) = sidebar.focus_agent {
            self.focus_agent(index, ctx);
        }
        if let Some(index) = agents_select {
            if let Some(entry) = self.caches.agents.get(index) {
                self.selected_agent = Some((entry.repo_key.clone(), entry.tab_id, entry.pane_id));
            }
        }
        if let Some(index) = agents_focus {
            self.focus_agent(index, ctx);
        }
        if let Some(view) = agents_set_view {
            self.agents_view = view;
            self.persist(move |prefs| Prefs {
                agents_view: view,
                ..prefs
            });
        }
        if let Some(view) = set_file_view {
            self.git_file_view = view;
            self.persist(move |prefs| Prefs {
                git_file_view: view,
                ..prefs
            });
        }
        if let Some(width) = agents_set_column_width {
            self.agents_column_width = width;
            self.persist(move |prefs| Prefs {
                agents_column_width: width,
                ..prefs
            });
        }
        if let Some(height) = agents_set_terminal_height {
            self.agents_terminal_height = height;
            self.persist(move |prefs| Prefs {
                agents_terminal_height: height,
                ..prefs
            });
        }
        // Reinsert the active review surface (taken out for the frame) into the cache
        // before the actions that open / close / mutate it run.
        if let Some(review) = pr_review_local {
            self.pr_reviews.insert(review.key.clone(), review);
        }
        if !pr_review_intents.is_empty() {
            self.apply_pr_review_intents(pr_review_intents, ctx);
        }
        if pr_back {
            // Leave the cockpit's browse list; keep the surface cached so reopening
            // the PR is instant (drafts + loaded diff retained).
            self.pr_active = None;
        }
        if pr_close_file {
            self.close_pr_file();
        }
        if let Some(index) = pr_select {
            self.open_pr_review(index, ctx);
        }
        if let Some(idx) = pr_select_file {
            self.select_pr_file(idx, ctx);
        }
        if let Some(sel) = pr_select_commit {
            self.select_pr_commit(sel, ctx);
        }
        if let Some((idx, line)) = pr_open_inline {
            self.open_pr_inline_comment(idx, line, ctx);
        }
        if pr_submit_review {
            self.submit_pr_review(ctx);
        }
        if pr_checkout {
            if let Some(pr) = self.active_review().map(|review| review.pr.clone()) {
                self.request_pr_checkout(&pr, ctx);
            }
        }
        if let Some(url) = pr_open_url {
            let now = ctx.input(|i| i.time);
            match crate::terminal::links::open_url(&url) {
                Ok(()) => self.toasts.success("Opening pull request…", now),
                Err(err) => self.toasts.error(
                    format!("Couldn't open the pull request — {}", err.message()),
                    now,
                ),
            }
        }
        if let Some(width) = pr_set_detail_width {
            self.pr_detail_width = width;
            self.persist(move |prefs| Prefs {
                pr_detail_width: width,
                ..prefs
            });
        }
        if pr_toggle_rail {
            let collapsed = !self.pr_rail_collapsed;
            self.pr_rail_collapsed = collapsed;
            self.persist(move |prefs| Prefs {
                pr_rail_collapsed: collapsed,
                ..prefs
            });
        }
        // `Esc` leaves the dashboard — unless its mirrored terminal holds focus, in
        // which case `Esc` is the agent's interrupt and stays with the pane.
        if agents_active
            && !agents_terminal_focused
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.central_mode = CentralMode::Terminal;
            ctx.request_repaint();
        }
        // `Esc` leaves the PR cockpit (no mirrored terminal to compete for the key).
        if pr_active && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.central_mode = CentralMode::Terminal;
            ctx.request_repaint();
        }
        if let Some(index) = sidebar.reveal {
            reveal_in_finder(self.workspace.repo(index).map(|r| r.path.as_path()));
        }
        // File-row context menu (sidebar.md): reveal opens Finder on the file,
        // open-in-editor rides the same channel as a terminal link click.
        if let Some(path) = &file_menu.reveal {
            reveal_in_finder(Some(path.as_path()));
        }
        if let Some(path) = file_menu.open_in_editor {
            open_link.get_or_insert(LinkAction::File {
                path,
                line: None,
                column: None,
            });
        }
        if let Some(index) = sidebar.remove {
            remove_repo_or_group(&mut self.workspace, index);
            self.caches.sync(&self.workspace);
            let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
            self.persist(move |_| next);
            self.caches
                .set_branch_labels(workspace_branches(&self.workspace));
            self.caches
                .set_dirty_stats(workspace_dirty_stats(&self.workspace));
        }
        if let Some(index) = sidebar.delete_worktree {
            self.request_delete_worktree(index, ctx);
        }
        if let Some(index) = sidebar.create_worktree {
            self.open_create_worktree_modal(index, ctx);
        }
        if let Some(index) = sidebar.toggle_collapse {
            self.workspace.toggle_collapsed(index);
            let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
            self.persist(move |_| next);
        }
        if let Some(index) = sidebar.toggle_hidden {
            self.workspace.toggle_user_hidden(index);
            // Hiding the active project's group leaves the central area on the
            // dashboard — no orphaned terminal for a row no longer in the sidebar.
            if self
                .workspace
                .active()
                .is_some_and(|a| self.workspace.is_in_hidden_project(a))
            {
                self.central_mode = CentralMode::Agents;
            }
            let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
            self.persist(move |_| next);
        }
        if let Some(reorder) = sidebar.reorder {
            if self
                .workspace
                .reorder(reorder.from, reorder.anchor, reorder.after)
            {
                self.caches.sync(&self.workspace);
                // The active repo kept its identity; only its index shifted. Realign
                // the git session's stored index so `sync_git_session` does not read
                // the shift as a repo switch (which would kill the worker and close
                // the open diff / branch editor).
                if let (Some(git), Some(active)) = (self.git.as_mut(), self.workspace.active()) {
                    git.index = active;
                }
                let next = prefs_from_workspace(self.prefs.clone(), &self.workspace);
                self.persist(move |_| next);
            }
        }
        if open_feedback_request {
            self.modal = Some(Modal::Feedback(FeedbackPage::default()));
        }
        PageActions {
            open_folder: sidebar.open || open_dialog_requested,
            toggle_preferences: toggle_preferences_request,
            open_link,
        }
    }

    pub(super) fn render_modals(
        &mut self,
        ui: &mut egui::Ui,
        palette: theme::Palette,
        ctx: &egui::Context,
    ) {
        if let Some(Modal::CreateWorktree(pending)) = self.modal.as_mut() {
            let busy = self
                .worktree_create
                .as_ref()
                .is_some_and(|runner| runner.busy());
            let mut action = crate::ui::repo_sidebar::CreateWorktreeModalAction::default();
            crate::ui::repo_sidebar::create_worktree_modal(
                ui,
                &palette,
                &crate::ui::repo_sidebar::CreateWorktreePrompt {
                    root_label: &pending.root_label,
                    root: &pending.root,
                    base: pending.base.as_deref(),
                    sources: pending.sources.as_deref().unwrap_or(&[]),
                    selected: pending.selected,
                    base_branch: &pending.base_branch,
                    taken: &pending.taken,
                    error: pending.error.as_deref(),
                    loading: pending.sources.is_none() && pending.error.is_none(),
                    busy,
                },
                &mut pending.view,
                &mut action,
            );
            // Selection first: create can ride the same frame as a filter-driven
            // reselect (Enter right after typing).
            if let Some(selection) = action.select {
                pending.selected = Some(selection);
                pending.error = None;
            }
            let request = if action.create {
                create_request_from(pending)
            } else {
                None
            };
            if let Some((root, source, name, base)) = request {
                self.request_create_worktree(root, source, name, base, ctx);
                ctx.request_repaint();
            } else if action.dismiss {
                self.modal = None;
            }
            return;
        }

        // AI rebase recap (git.md §9): Start hands the request to the session's
        // runner — the modal closes while it runs (toolbar spinner + mutation
        // lock tell the busy state) and the report reopens a modal from the
        // drain. Busy ⇒ Start greyed out, same rule as the toolbar.
        if let Some(Modal::AiRebase(page)) = self.modal.as_mut() {
            let busy = self
                .git
                .as_ref()
                .is_some_and(|git| git.busy_action().is_some());
            let action = ai_rebase_modal(ui, &palette, page, self.ai_rebase_provider, busy);
            if action.start {
                let request = AiRebaseRequest {
                    current: page.current.clone(),
                    onto: page.onto.clone(),
                    instructions: page.instructions.clone(),
                    expected: page.expected(),
                };
                if let Some(git) = self.git.as_mut() {
                    let (current, onto) = (request.current.clone(), request.onto.clone());
                    let now = ctx.input(|i| i.time);
                    if git.ai_rebase.request(self.ai_rebase_provider, request) {
                        self.modal = None;
                        // The run takes minutes: confirm the start right away
                        // (the toolbar chip carries the live state from here).
                        self.toasts.success(
                            format!(
                                "AI rebase started — {} is rebasing '{current}' onto '{onto}'",
                                self.ai_rebase_provider.command()
                            ),
                            now,
                        );
                    } else {
                        // One mutating op at a time (git.md §10): the modal
                        // stays open for a retry.
                        self.toasts
                            .error("Another Git operation is in progress", now);
                    }
                    ctx.request_repaint();
                }
            } else if action.dismiss {
                self.modal = None;
            }
            return;
        }
        if let Some(Modal::AiRebaseReport(report)) = self.modal.as_ref() {
            if ai_rebase_report_modal(ui, &palette, report) {
                self.modal = None;
            }
            return;
        }
        if matches!(self.modal, Some(Modal::WhatsNew)) {
            if crate::ui::release_notes::modal(ui, &mut self.commonmark_cache) {
                self.modal = None;
            }
            return;
        }

        // Feedback (specs/feedback.md): Submit opens the GitHub "new issue" form
        // pre-filled in the browser (synchronous, instant) and closes the modal.
        if let Some(Modal::Feedback(page)) = self.modal.as_mut() {
            let action = feedback_modal(ui, &palette, page);
            if action.submit {
                let now = ctx.input(|i| i.time);
                match crate::feedback::open_issue(page.kind, &page.description) {
                    Ok(()) => self.toasts.success("Opening GitHub…", now),
                    Err(err) => self
                        .toasts
                        .error(format!("Feedback failed — {}", err.message()), now),
                }
                self.modal = None;
                ctx.request_repaint();
            } else if action.dismiss {
                self.modal = None;
            }
            return;
        }

        // Tag deletion (graph tag menu, git.md §9): the modal mutates its "Also
        // delete on origin" checkbox (`also_remote`), so it is handled here with a
        // mutable borrow rather than in the shared read-only match below. Checked,
        // the remote deletion runs first on the sync runner (busy ⇒ refusal toast,
        // nothing happens — never a silent half) and the local one is enqueued
        // behind its success (`sync_follow_up`); unchecked, the local deletion goes
        // straight to the worker, graph reloaded behind it.
        if let Some(Modal::DeleteTag { tag, also_remote }) = self.modal.as_mut() {
            let has_remote = self.git.as_ref().is_some_and(|git| git.has_remote);
            let mut modal_action = DeleteModalAction::default();
            delete_tag_modal(
                ui,
                &palette,
                tag,
                has_remote,
                also_remote,
                &mut modal_action,
            );
            if modal_action.confirm {
                let tag = tag.clone();
                let also_remote = *also_remote;
                self.modal = None;
                if let Some(git) = self.git.as_mut() {
                    let now = ctx.input(|i| i.time);
                    if also_remote {
                        git.request_sync(
                            SyncCommand::DeleteRemoteThenLocalTag(tag),
                            &mut self.toasts,
                            now,
                        );
                    } else {
                        git.send_then_reload_graph(GitCommand::DeleteTag(tag));
                    }
                    ctx.request_repaint();
                }
            } else if modal_action.dismiss {
                self.modal = None;
            }
            return;
        }

        if let Some(modal) = &self.modal {
            let mut modal_action = DeleteModalAction::default();
            match modal {
                Modal::DeleteWorktree(pending) => {
                    delete_worktree_modal(ui, &palette, &pending.prompt, &mut modal_action)
                }
                Modal::DeleteBranch(target) => {
                    delete_branch_modal(ui, &palette, target, &mut modal_action)
                }
                Modal::DropStash(target) => {
                    delete_stash_modal(ui, &palette, target, &mut modal_action)
                }
                Modal::ResetHard { branch, short, .. } => {
                    reset_hard_modal(ui, &palette, branch, short, &mut modal_action)
                }
                Modal::AbortOp => abort_op_modal(ui, &palette, &mut modal_action),
                Modal::ForcePush { branch, remote } => {
                    force_push_modal(ui, &palette, branch, remote, &mut modal_action)
                }
                Modal::DiscardHunk { .. } => discard_hunk_modal(ui, &palette, &mut modal_action),
                Modal::CreateWorktree(_)
                | Modal::DeleteTag { .. }
                | Modal::AiRebase(_)
                | Modal::AiRebaseReport(_)
                | Modal::Feedback(_)
                | Modal::WhatsNew => {
                    unreachable!("handled above")
                }
            }
            if modal_action.confirm {
                match self.modal.take() {
                    Some(Modal::DeleteWorktree(PendingDelete {
                        root, path, label, ..
                    })) => {
                        self.delete_runner(ctx).request(DeleteRequest {
                            root,
                            path,
                            label,
                            force: true,
                        });
                    }
                    // Branch deletion (graph context menu, git.md §9): local to the
                    // worker (graph reloaded behind, FIFO), remote to the SyncRunner
                    // (refresh status+graph via drain_sync).
                    Some(Modal::DeleteBranch(target)) => {
                        if let Some(git) = self.git.as_mut() {
                            // One network op at a time (git.md §10): never a silently
                            // lost deletion. `Both` goes remote first; the local branch
                            // is enqueued only after the remote deletion reply succeeds
                            // (`drain_sync_refresh`).
                            let network = match target {
                                DeleteBranchTarget::Local(name) => {
                                    git.send_then_reload_graph(GitCommand::DeleteBranch(name));
                                    None
                                }
                                DeleteBranchTarget::Remote(name) => {
                                    Some(SyncCommand::DeleteRemoteBranch(name))
                                }
                                DeleteBranchTarget::Both { local, remote } => {
                                    Some(SyncCommand::DeleteRemoteThenLocalBranch { remote, local })
                                }
                            };
                            if let Some(command) = network {
                                if !git.sync.request(command) {
                                    let now = ctx.input(|i| i.time);
                                    self.toasts
                                        .error("Another network operation is in progress", now);
                                }
                            }
                            ctx.request_repaint();
                        }
                    }
                    // Stash deletion (stash row context menu, git.md §9): to the worker,
                    // graph reloaded behind (FIFO) so the row disappears at once.
                    Some(Modal::DropStash(target)) => {
                        if let Some(git) = self.git.as_ref() {
                            git.send_then_reload_graph(GitCommand::StashDropAt(target.oid));
                            ctx.request_repaint();
                        }
                    }
                    // Hard reset confirmed (graph row menu, git.md §9): to the
                    // worker, graph reloaded behind (FIFO) so the moved branch and
                    // the reset working tree show at once.
                    Some(Modal::ResetHard { target, .. }) => {
                        if let Some(git) = self.git.as_ref() {
                            git.send_then_reload_graph(GitCommand::Reset {
                                target,
                                mode: git2::ResetType::Hard,
                            });
                            ctx.request_repaint();
                        }
                    }
                    // Abort confirmed (banner, git.md §10): to the sync runner —
                    // one op at a time, outcome toast + refresh via drain_sync.
                    Some(Modal::AbortOp) => {
                        if let Some(git) = self.git.as_mut() {
                            git.request_sync(
                                SyncCommand::AbortOp,
                                &mut self.toasts,
                                ctx.input(|i| i.time),
                            );
                            ctx.request_repaint();
                        }
                    }
                    // Force push confirmed (Push chevron, git.md §10): to the sync
                    // runner — `--force-with-lease`, outcome toast + refresh via
                    // drain_sync (a lease refusal surfaces as a persistent toast).
                    Some(Modal::ForcePush { .. }) => {
                        if let Some(git) = self.git.as_mut() {
                            git.request_sync(
                                SyncCommand::ForcePush,
                                &mut self.toasts,
                                ctx.input(|i| i.time),
                            );
                            ctx.request_repaint();
                        }
                    }
                    // Discard hunk confirmed (diff view, git.md §4): to the worker,
                    // then re-request the open diff so the overlay reflects the
                    // reverted working tree (as granular staging does).
                    Some(Modal::DiscardHunk { path, hunk }) => {
                        let reload = if let Some(DiffState {
                            source: DiffSource::WorkingTree { staged },
                            path: dpath,
                            view,
                            ..
                        }) = &mut self.diff
                        {
                            view.clear();
                            Some((dpath.clone(), *staged))
                        } else {
                            None
                        };
                        if let Some(git) = self.git.as_ref() {
                            git.worker.send(GitCommand::DiscardHunk { path, hunk });
                            if let Some((dpath, staged)) = reload {
                                git.worker.send(GitCommand::Diff {
                                    path: dpath,
                                    staged,
                                });
                            }
                            ctx.request_repaint();
                        }
                    }
                    Some(
                        Modal::CreateWorktree(_)
                        | Modal::DeleteTag { .. }
                        | Modal::AiRebase(_)
                        | Modal::AiRebaseReport(_)
                        | Modal::Feedback(_)
                        | Modal::WhatsNew,
                    )
                    | None => {}
                }
            } else if modal_action.dismiss {
                self.modal = None;
            }
        }
    }

    pub(super) fn flush_persistence(
        &mut self,
        ctx: &egui::Context,
        sidebars_were: SidebarVisibility,
    ) {
        self.persist_sidebar_widths_if_changed(ctx);
        self.persist_sidebar_visibility_if_changed(sidebars_were);
        self.flush_prefs_if_due(ctx);
    }
}

/// Compact caption for a finished agent on the dashboard (the green arms ~6 s
/// after the last output, so sub-minute reads "just now").
fn finished_ago(elapsed_ms: u64) -> String {
    let secs = elapsed_ms / 1_000;
    if secs < 60 {
        "Finished just now".to_owned()
    } else if secs < 3_600 {
        format!("Finished {}m ago", secs / 60)
    } else {
        format!("Finished {}h ago", secs / 3_600)
    }
}

/// Display name of a project root: its folder name, falling back to the full
/// path when there is none (e.g. a filesystem root).
fn project_root_label(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

/// Mirrors agent `idx`'s live pane into a dashboard card (list panel or column
/// terminal). `focused` is true for the single `selected` agent; clicking an
/// unfocused card returns `true` so the caller can promote it. Missing keys or
/// panes render nothing — agents come and go between watch ticks.
#[allow(clippy::too_many_arguments)]
fn mirror_agent_terminal(
    ui: &mut egui::Ui,
    panes: &mut HashMap<PaneKey, Panes>,
    keys: &[(RepoKey, TabId, PaneId)],
    idx: usize,
    selected: Option<&(RepoKey, TabId, PaneId)>,
    font_size: f32,
    term_palette: &TermPalette,
    palette: &theme::Palette,
    clear_shortcut: Option<Shortcut>,
    any_focused: &mut bool,
    open_link: &mut Option<LinkAction>,
) -> bool {
    let Some(key) = keys.get(idx) else {
        return false;
    };
    let focused = selected == Some(key);
    let (rk, tid, pid) = key;
    let Some(panes) = panes.get_mut(&(rk.clone(), *tid)) else {
        return false;
    };
    render_pane(
        ui,
        panes,
        *pid,
        focused,
        font_size,
        term_palette,
        palette,
        clear_shortcut,
        any_focused,
        open_link,
    )
}

/// Read-only progress preview of an agent's pane for a collapsed Columns card: its
/// last lines, scaled to fit the card (the pane is kept visible so its reader stays
/// live, but never resized — only the expanded card drives a PTY resize).
fn mirror_agent_preview(
    ui: &mut egui::Ui,
    panes: &mut HashMap<PaneKey, Panes>,
    keys: &[(RepoKey, TabId, PaneId)],
    idx: usize,
    font_size: f32,
    term_palette: &TermPalette,
) {
    let Some((rk, tid, pid)) = keys.get(idx) else {
        return;
    };
    let Some(panes) = panes.get_mut(&(rk.clone(), *tid)) else {
        return;
    };
    if let Some(TerminalState::Live(pane)) = panes.get_mut(pid) {
        pane.set_visible(true);
        pane.set_reply_palette(*term_palette);
        terminal_view_preview(
            ui,
            pane.grid(),
            term_palette,
            font_size,
            crate::ui::agents_view::AGENT_PREVIEW_LINES,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_pane(
    ui: &mut egui::Ui,
    panes: &mut HashMap<PaneId, TerminalState>,
    id: PaneId,
    focused: bool,
    font_size: f32,
    term_palette: &TermPalette,
    palette: &theme::Palette,
    clear_shortcut: Option<Shortcut>,
    any_focused: &mut bool,
    open_link: &mut Option<LinkAction>,
) -> bool {
    match panes.get_mut(&id) {
        Some(TerminalState::Live(pane)) => {
            // On screen this frame: let its reader pace the event loop (cleared for
            // all panes at frame start, so unpainted panes stay silent).
            pane.set_visible(true);
            pane.set_reply_palette(*term_palette);
            let exited = pane.has_exited();
            // Resolve the live cwd for link detection only while Cmd is held, so the
            // proc_pidinfo syscall stays off the per-frame path (terminal.md §12).
            let cmd_held = ui
                .ctx()
                .input(|i| i.modifiers.command || i.modifiers.mac_cmd);
            let link_cwd = (cmd_held && !exited).then(|| {
                pane.shell_pid()
                    .and_then(crate::terminal::cwd::live_cwd)
                    .unwrap_or_else(|| pane.spawn_cwd().to_path_buf())
            });
            let input = terminal_view(
                ui,
                pane.grid(),
                term_palette,
                font_size,
                focused,
                exited,
                clear_shortcut,
                link_cwd.as_deref(),
            );
            if let Some(link) = input.open_link {
                *open_link = Some(link);
            }
            let focus_id = ui.id().with("terminal_focus");
            if ui.ctx().memory(|m| m.has_focus(focus_id)) {
                *any_focused = true;
            }
            if exited {
                if input.relaunch {
                    let _ = pane.relaunch();
                }
            } else {
                if input.clear {
                    pane.clear();
                }
                if let Some(scroll) = input.scroll {
                    pane.scroll(scroll);
                }
                if !input.scroll_bytes.is_empty() {
                    let _ = pane.input(&input.scroll_bytes);
                }
                if !input.mouse_bytes.is_empty() {
                    let _ = pane.input(&input.mouse_bytes);
                }
                if !input.bytes.is_empty() {
                    let _ = pane.input(&input.bytes);
                }
                if let Some(text) = &input.paste {
                    let _ = pane.paste(text);
                }
                if input.size.rows != pane.rows() || input.size.cols != pane.cols() {
                    let _ = pane.resize(input.size.rows, input.size.cols);
                }
            }
            input.clicked
        }
        Some(TerminalState::Failed(err)) => {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(format!("Terminal unavailable: {err}"))
                        .color(palette.text_muted),
                );
            });
            false
        }
        None => false,
    }
}
