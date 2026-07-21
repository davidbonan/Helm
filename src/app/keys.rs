//! Keyboard routing (keybindings.md): maps shortcuts to layout / zoom / tab /
//! repo commands, and git intents to worker commands.

use super::*;

pub(crate) fn git_command(intent: GitIntent) -> Option<GitCommand> {
    match intent {
        GitIntent::Refresh => Some(GitCommand::Status),
        GitIntent::Stage(path) => Some(GitCommand::Stage(path)),
        GitIntent::Unstage(path) => Some(GitCommand::Unstage(path)),
        GitIntent::StageAll => Some(GitCommand::StageAll),
        GitIntent::UnstageAll => Some(GitCommand::UnstageAll),
        GitIntent::Discard(path) => Some(GitCommand::Discard(path)),
        GitIntent::DiscardAll => Some(GitCommand::DiscardAll),
        GitIntent::StashFiles(paths) => Some(GitCommand::StashFiles(paths)),
        GitIntent::Commit(message) => Some(GitCommand::Commit(message)),
        // Amend (commit-detail reword) is arbitrated app-side: it reloads the graph
        // and re-selects HEAD after the worker amend — never a bare worker command.
        GitIntent::AmendMessage(_) => None,
        // AI generation is routed to the `AiRunner` by the app (which carries
        // provider + instructions), not to the git worker.
        GitIntent::GenerateMessage => None,
        // Abort goes through the confirmation modal (app side), then the sync
        // runner; Continue / Resolve are arbitrated app-side too (sync runner /
        // editor open) — never straight to the worker.
        GitIntent::AbortOp | GitIntent::ContinueOp | GitIntent::OpenConflictEditor { .. } => None,
        // The overlay (opening + granular staging) is routed by `overlay_or_command`,
        // which has the open file's path. Discard hunk is intercepted earlier still
        // (it arms a confirmation modal), never reaching the worker directly.
        GitIntent::OpenDiff { .. }
        | GitIntent::StageHunk(_)
        | GitIntent::UnstageHunk(_)
        | GitIntent::StageLines { .. }
        | GitIntent::UnstageLines { .. }
        | GitIntent::DiscardHunk(_) => None,
        // Flat ⇄ Tree toggle (M40): a persisted preference, set + saved app-side
        // (render loop), never a worker command.
        GitIntent::SetFileView(_) => None,
    }
}

/// Maps an intent to a `GitCommand`. The granular staging intents
/// (`StageHunk`/`UnstageHunk`/`StageLines`/`UnstageLines`) carry only the hunk index:
/// the open overlay's file path is joined to them. Without an open working-tree
/// overlay they are ignored — a fullscreen commit diff is read-only (M9-7), and an
/// inherited (still-loading) overlay shows another file's hunks.
pub(crate) fn overlay_or_command(
    intent: GitIntent,
    open: Option<&DiffState>,
) -> Option<GitCommand> {
    let open = open.filter(|d| d.granular_writes_allowed());
    match intent {
        GitIntent::StageHunk(hunk) => open.map(|d| GitCommand::StageHunk {
            path: d.path.clone(),
            hunk,
        }),
        GitIntent::UnstageHunk(hunk) => open.map(|d| GitCommand::UnstageHunk {
            path: d.path.clone(),
            hunk,
        }),
        GitIntent::StageLines { hunk, lines } => open.map(|d| GitCommand::StageLines {
            path: d.path.clone(),
            hunk,
            lines,
        }),
        GitIntent::UnstageLines { hunk, lines } => open.map(|d| GitCommand::UnstageLines {
            path: d.path.clone(),
            hunk,
            lines,
        }),
        other => git_command(other),
    }
}

/// Active keyboard zone (keybindings §4). helm has only one zone that receives
/// the non-global shortcuts at a time; it decides which shortcuts apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusZone {
    /// A terminal pane has focus: the §2 shortcuts (split, zoom, navigation) apply.
    Terminal,
    /// The diff overlay view is open: only §3 and `Esc` apply (handled by `diff_view`).
    DiffView,
    /// Commit field, sidebar, or no zone: no terminal shortcut.
    Other,
}

impl FocusZone {
    /// The terminal shortcuts (§2: split/close/navigation/zoom) apply only when a
    /// terminal pane has focus — never in the diff view nor the commit field
    /// (keybindings §4).
    pub fn terminal_shortcuts_active(self) -> bool {
        matches!(self, FocusZone::Terminal)
    }
}

/// Resolves the active zone from facts observable at routing time: the diff overlay
/// view wins (it hides the pane tree); otherwise a terminal pane holding egui focus
/// places the zone in `Terminal`. Everything else (commit field, sidebars, no focus)
/// falls into `Other`.
pub fn focus_zone(diff_open: bool, terminal_focused: bool) -> FocusZone {
    if diff_open {
        FocusZone::DiffView
    } else if terminal_focused {
        FocusZone::Terminal
    } else {
        FocusZone::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutCommand {
    Split(Orient),
    Close,
    Focus(Dir),
    Resize(Dir),
}

pub(crate) fn layout_command(
    keymap: &Keymap,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<LayoutCommand> {
    let bindings = [
        (Action::SplitRight, LayoutCommand::Split(Orient::Vertical)),
        (Action::SplitDown, LayoutCommand::Split(Orient::Horizontal)),
        (Action::ClosePane, LayoutCommand::Close),
        (Action::FocusLeft, LayoutCommand::Focus(Dir::Left)),
        (Action::FocusRight, LayoutCommand::Focus(Dir::Right)),
        (Action::FocusUp, LayoutCommand::Focus(Dir::Up)),
        (Action::FocusDown, LayoutCommand::Focus(Dir::Down)),
        (Action::ResizeLeft, LayoutCommand::Resize(Dir::Left)),
        (Action::ResizeRight, LayoutCommand::Resize(Dir::Right)),
        (Action::ResizeUp, LayoutCommand::Resize(Dir::Up)),
        (Action::ResizeDown, LayoutCommand::Resize(Dir::Down)),
    ];
    bindings
        .into_iter()
        .find(|(action, _)| keymap.matches(*action, key, modifiers))
        .map(|(_, command)| command)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZoomCommand {
    In,
    Out,
    Reset,
}

pub(crate) fn zoom_command(
    keymap: &Keymap,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<ZoomCommand> {
    // A logical `+` is the shifted `=` on most layouts: fold it onto `Equals`
    // (shift dropped) so the spec's `Cmd+=` (keybindings §2) keeps firing when
    // the chord arrives as ⌘⇧=. The raw event is tried first — a custom binding
    // on `plus` still wins.
    let folded = (key == egui::Key::Plus).then_some((
        egui::Key::Equals,
        egui::Modifiers {
            shift: false,
            ..modifiers
        },
    ));
    std::iter::once((key, modifiers))
        .chain(folded)
        .find_map(|(key, modifiers)| {
            if keymap.matches(Action::ZoomIn, key, modifiers) {
                Some(ZoomCommand::In)
            } else if keymap.matches(Action::ZoomOut, key, modifiers) {
                Some(ZoomCommand::Out)
            } else if keymap.matches(Action::ZoomReset, key, modifiers) {
                Some(ZoomCommand::Reset)
            } else {
                None
            }
        })
}

pub fn route_zoom_keys(ctx: &egui::Context, keymap: &Keymap, zoom: &mut FontZoom) {
    ctx.input(|input| {
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                match zoom_command(keymap, *key, *modifiers) {
                    Some(ZoomCommand::In) => zoom.zoom_in(),
                    Some(ZoomCommand::Out) => zoom.zoom_out(),
                    Some(ZoomCommand::Reset) => zoom.reset(),
                    None => {}
                }
            }
        }
    });
}

/// `action`'s current binding pressed this frame (one-shot global shortcuts).
pub(crate) fn action_pressed(ctx: &egui::Context, keymap: &Keymap, action: Action) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { key, pressed: true, modifiers, .. }
                    if keymap.matches(action, *key, *modifiers)
            )
        })
    })
}

/// Number-row shortcuts are positional (keybindings §1: "sidebar order"). Prefer
/// the physical key so a layout whose Nth top-row key emits punctuation —
/// AZERTY-FR types `'` (logical `Key::Quote`) on the physical `Num4` — still
/// selects N. Falls back to the logical key when no digit sits in the physical
/// slot (other keys, or synthetic events that carry no physical key).
pub(crate) fn positional_key(logical: egui::Key, physical: Option<egui::Key>) -> egui::Key {
    physical
        .filter(|p| digit_index(*p).is_some())
        .unwrap_or(logical)
}

fn digit_index(key: egui::Key) -> Option<usize> {
    match key {
        egui::Key::Num1 => Some(0),
        egui::Key::Num2 => Some(1),
        egui::Key::Num3 => Some(2),
        egui::Key::Num4 => Some(3),
        egui::Key::Num5 => Some(4),
        egui::Key::Num6 => Some(5),
        egui::Key::Num7 => Some(6),
        egui::Key::Num8 => Some(7),
        egui::Key::Num9 => Some(8),
        _ => None,
    }
}

pub(crate) fn select_tab_command(key: egui::Key, modifiers: egui::Modifiers) -> Option<usize> {
    if !modifiers.command || modifiers.shift || modifiers.alt || modifiers.ctrl {
        return None;
    }
    digit_index(key)
}

pub(crate) fn select_repo_command(key: egui::Key, modifiers: egui::Modifiers) -> Option<usize> {
    if !modifiers.command || !modifiers.ctrl || modifiers.shift || modifiers.alt {
        return None;
    }
    digit_index(key)
}

/// `Cmd+Ctrl+0` opens the Agents dashboard — slot 0 of the positional repo
/// family (keybindings §1): fixed, not rebindable, like `Cmd+Ctrl+1..9`.
pub(crate) fn open_agents_command(key: egui::Key, modifiers: egui::Modifiers) -> bool {
    modifiers.command
        && modifiers.ctrl
        && !modifiers.shift
        && !modifiers.alt
        && key == egui::Key::Num0
}

/// `Cmd+Ctrl+0` pressed this frame. The physical `0` slot is honored too, so the
/// chord stays layout-independent like the repo selectors (an AZERTY slot that
/// emits punctuation without `Shift` still resolves to `Num0`).
pub(crate) fn open_agents_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|input| {
        input.events.iter().any(|event| match event {
            egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } => {
                let key = physical_key
                    .filter(|p| *p == egui::Key::Num0)
                    .unwrap_or(*key);
                open_agents_command(key, *modifiers)
            }
            _ => false,
        })
    })
}

pub fn route_select_repo_keys(ctx: &egui::Context, workspace: &mut Workspace) {
    let index = ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } => select_repo_command(positional_key(*key, *physical_key), *modifiers),
            _ => None,
        })
    });
    // The digit is a visible-order slot (worktrees.md §7): map it past any
    // worktree hidden under a folded root before selecting.
    if let Some(index) = index.and_then(|n| workspace.nth_visible(n)) {
        workspace.set_active(index);
    }
}

/// Cycle the active repo/worktree (keybindings §1, global): `Ctrl+Tab` /
/// `Ctrl+Shift+Tab` by default — layout-independent, no number row. Rebindable.
pub fn route_cycle_repo_keys(ctx: &egui::Context, keymap: &Keymap, workspace: &mut Workspace) {
    if action_pressed(ctx, keymap, Action::NextRepo) {
        workspace.cycle_active(true);
    }
    if action_pressed(ctx, keymap, Action::PrevRepo) {
        workspace.cycle_active(false);
    }
}

/// Tabs of the active repo (keybindings §1, global): `Cmd+T` opens a fresh tab,
/// `Cmd+1..9` selects tab 1..9. Without an active repo, `add_tab`/`set_active_tab`
/// are no-ops (terminal.md §10).
pub fn route_tab_keys(ctx: &egui::Context, keymap: &Keymap, workspace: &mut Workspace) {
    let actions: Vec<TabAction> = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } => tab_action(keymap, *key, *physical_key, *modifiers),
                _ => None,
            })
            .collect()
    });
    for action in actions {
        match action {
            TabAction::New => {
                workspace.add_tab();
            }
            TabAction::Select(tab) => {
                workspace.set_active_tab(tab);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabAction {
    New,
    Select(usize),
}

/// New-tab matches on the **logical** key (rebindable); the positional fallback
/// only feeds the reserved `Cmd+1..9` selector.
pub(crate) fn tab_action(
    keymap: &Keymap,
    key: egui::Key,
    physical: Option<egui::Key>,
    modifiers: egui::Modifiers,
) -> Option<TabAction> {
    if keymap.matches(Action::NewTab, key, modifiers) {
        return Some(TabAction::New);
    }
    select_tab_command(positional_key(key, physical), modifiers).map(TabAction::Select)
}

/// Routes the terminal shortcuts (§2) to the active tab's tree. `close_tab` surfaces
/// the "`Cmd+W` on the **last pane** of a tab" case: we don't create a fresh leaf in
/// the tree (which `Layout::close` would do), we signal the caller to close the **tab**
/// (terminal.md §11) — killing the PTY set + reindexing lives on the `app` side
/// (`close_active_tab`).
pub fn route_layout_keys(
    ctx: &egui::Context,
    keymap: &Keymap,
    layout: &mut Layout,
    area: egui::Rect,
    font_size: f32,
    close_tab: &mut bool,
) {
    let commands: Vec<LayoutCommand> = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => layout_command(keymap, *key, *modifiers),
                _ => None,
            })
            .collect()
    });
    if commands.is_empty() {
        return;
    }
    let (cell_w, cell_h) = cell_metrics(ctx, font_size);
    let area = rect(area);
    for command in commands {
        match command {
            LayoutCommand::Split(orient) => {
                layout.split(orient);
            }
            LayoutCommand::Close => {
                if layout.pane_ids().len() == 1 {
                    *close_tab = true;
                } else {
                    layout.close();
                }
            }
            LayoutCommand::Focus(dir) => layout.focus_neighbor(dir, area),
            LayoutCommand::Resize(dir) => layout.resize(dir, area, cell_w, cell_h),
        }
    }
}
