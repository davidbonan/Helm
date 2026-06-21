use crate::git::sync::{PullMode, SyncError, SyncOutcome};
use crate::git::worker::SyncCommand;
use crate::theme::{Palette, RADIUS_PILL};
use crate::ui::graph_view::BranchEditor;
use crate::ui::repo_sidebar::DeleteModalAction;
use crate::ui::spinner::Spinner;
use crate::ui::{clickable, paint_icon};

const TOOLBAR_HEIGHT: f32 = 36.0;
const TOOLBAR_PAD_X: f32 = 8.0;
const BUTTON_HEIGHT: f32 = 26.0;
const BUTTON_PAD_X: f32 = 9.0;
const BUTTON_GAP: f32 = 6.0;
const ICON_GLYPH: f32 = 14.0;
const ICON_GAP: f32 = 6.0;
const LABEL_SIZE: f32 = 13.0;
const CHEVRON_WIDTH: f32 = 20.0;
/// Vertical inset of the split-button's inner rule (1px between the main area
/// and the chevron, design-system §4).
const SPLIT_RULE_INSET: f32 = 5.0;
const MENU_TITLE_SIZE: f32 = 11.0;
const POPUP_GAP: f32 = 4.0;

const MENU_TITLE: &str =
    "Select a default pull/fetch operation to execute when clicking this button";

const GIT_MISSING: &str = "git binary not found";
const NO_REMOTE: &str = "No remote configured";
const DETACHED: &str = "HEAD is detached";
const UNBORN: &str = "Repository has no commits";
const BUSY: &str = "Operation in progress";
const CLEAN: &str = "Nothing to stash — the working tree is clean";
const NO_STASH: &str = "No stash to pop";
const FORCE_NO_UPSTREAM: &str = "No upstream to overwrite — push the branch first";

pub use crate::git::sync::PullDefault;

impl PullDefault {
    pub const ALL: [PullDefault; 4] = [
        PullDefault::FetchAll,
        PullDefault::Ff,
        PullDefault::FfOnly,
        PullDefault::Rebase,
    ];

    pub fn command(self) -> SyncCommand {
        match self {
            PullDefault::FetchAll => SyncCommand::FetchAll,
            PullDefault::Ff => SyncCommand::Pull(PullMode::Ff),
            PullDefault::FfOnly => SyncCommand::Pull(PullMode::FfOnly),
            PullDefault::Rebase => SyncCommand::Pull(PullMode::Rebase),
        }
    }

    pub fn button_label(self) -> &'static str {
        match self {
            PullDefault::FetchAll => "Fetch",
            _ => "Pull",
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            PullDefault::FetchAll => "Fetch All",
            PullDefault::Ff => "Pull (fast-forward if possible)",
            PullDefault::FfOnly => "Pull (fast-forward only)",
            PullDefault::Rebase => "Pull (rebase)",
        }
    }
}

/// Git command currently running, as seen by the toolbar: spinner on the button
/// that triggered the action and **all** other buttons greyed out
/// (D-2026-06-03-toolbar-loader-commandes-git).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyAction {
    Pull,
    Push,
    Branch,
    Stash,
    Pop,
    /// AI rebase run (git.md §9): minutes are normal, so the end-of-row loader
    /// names the operation, counts the time and offers **Cancel** — the only
    /// way out before the provider finishes.
    AiRebase {
        seconds: u64,
        /// Cancel already asked: the button turns inert ("Cancelling…") while
        /// the provider is killed and the branch restored.
        cancelling: bool,
    },
    /// Mutation triggered outside the toolbar (checkout from a chip, commit,
    /// staging…): no button spins, loader at the end of the row.
    Other,
}

/// Repo state as seen by the toolbar, computed by the caller (no git logic in
/// rendering): it drives labels, disablement + tooltips and busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarState {
    pub pull_default: PullDefault,
    /// Git command in progress (network op or worker mutation, git.md §10):
    /// loader + the whole toolbar greyed out while it runs.
    pub busy: Option<BusyAction>,
    pub has_remote: bool,
    /// Current branch has an upstream (git.md §10): drives the Push chevron's
    /// force-push entry — greyed out without one (the plain `-u` push covers the
    /// first publication).
    pub has_upstream: bool,
    pub detached: bool,
    /// Repo with no commit (unborn `HEAD`): Branch/Stash/Pop greyed out, only
    /// **Fetch All** stays runnable if a remote exists.
    pub unborn: bool,
    /// Dirty working tree: enables **Stash**.
    pub dirty: bool,
    pub stash_count: usize,
    /// `git` binary not found: network actions greyed out (detection M12-9).
    pub git_missing: bool,
}

/// Intents emitted by the toolbar in a frame, consumed by `HelmApp`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolbarAction {
    /// Click on Pull's main area (default op) or on Push.
    pub sync: Option<SyncCommand>,
    /// Selection in the radio menu: sets the default **without running it**.
    pub set_default: Option<PullDefault>,
    /// **Push (force with lease)** one-shot entry of the Push chevron: opens the
    /// confirmation modal (it never executes directly, unlike `set_default`).
    pub force_push: bool,
    pub stash: bool,
    pub pop: bool,
    /// Cancel on the AI rebase chip: kill the provider and restore the branch.
    pub cancel_ai_rebase: bool,
}

/// Graph action toolbar (git.md §10, design-system §4): a
/// **Pull (split-button) · Push · Branch · Stash · Pop** row at the top of the
/// graph view, left-aligned, separated from the graph by a 1px rule. Pure
/// rendering: each click emits an intent (`ToolbarAction`), arbitrated by the
/// caller. **Branch** toggles the inline editor (`BranchEditor`), rendered by
/// `graph_view` on the HEAD row — where the new branch's chip will appear. Mouse
/// only (v1); Undo/Redo (deferred) will take their place on the left.
pub fn graph_toolbar(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &ToolbarState,
    editor: &mut BranchEditor,
) -> ToolbarAction {
    let mut action = ToolbarAction::default();
    let width = ui.available_width();
    let (strip, _) =
        ui.allocate_exact_size(egui::vec2(width, TOOLBAR_HEIGHT), egui::Sense::hover());
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip.shrink2(egui::vec2(TOOLBAR_PAD_X, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = BUTTON_GAP;

    pull_split_button(&mut row, palette, state, &mut action);
    push_split_button(&mut row, palette, state, &mut action);

    let branch_busy = state.busy == Some(BusyAction::Branch);
    let branch_blocker = branch_blocker(state);
    let response = toolbar_button(
        &mut row,
        palette,
        lucide_icons::Icon::GitBranch,
        "Branch",
        branch_blocker.is_none() && !branch_busy,
        branch_busy,
        egui::CornerRadius::same(RADIUS_PILL),
    );
    if let Some(reason) = branch_blocker {
        response.on_hover_text(reason);
    } else if !branch_busy && response.clicked() {
        *editor = if editor.open {
            BranchEditor::default()
        } else {
            BranchEditor {
                open: true,
                ..Default::default()
            }
        };
    }

    let stash_busy = state.busy == Some(BusyAction::Stash);
    let stash_blocker = stash_blocker(state);
    let response = toolbar_button(
        &mut row,
        palette,
        lucide_icons::Icon::Archive,
        "Stash",
        stash_blocker.is_none() && !stash_busy,
        stash_busy,
        egui::CornerRadius::same(RADIUS_PILL),
    );
    if let Some(reason) = stash_blocker {
        response.on_hover_text(reason);
    } else if !stash_busy && response.clicked() {
        action.stash = true;
    }

    let pop_busy = state.busy == Some(BusyAction::Pop);
    let pop_blocker = pop_blocker(state);
    let response = toolbar_button(
        &mut row,
        palette,
        lucide_icons::Icon::ArchiveRestore,
        "Pop",
        pop_blocker.is_none() && !pop_busy,
        pop_busy,
        egui::CornerRadius::same(RADIUS_PILL),
    );
    if let Some(reason) = pop_blocker {
        response.on_hover_text(reason);
    } else if !pop_busy && response.clicked() {
        action.pop = true;
    }

    match state.busy {
        Some(BusyAction::AiRebase {
            seconds,
            cancelling,
        }) => {
            row.add(
                Spinner::new()
                    .size(ICON_GLYPH)
                    .color(palette.text_secondary),
            );
            row.label(
                egui::RichText::new(ai_rebase_chip_label(seconds))
                    .size(LABEL_SIZE)
                    .color(palette.text_secondary),
            );
            let label = if cancelling {
                "Cancelling…"
            } else {
                "Cancel"
            };
            let response = toolbar_button(
                &mut row,
                palette,
                lucide_icons::Icon::X,
                label,
                !cancelling,
                false,
                egui::CornerRadius::same(RADIUS_PILL),
            );
            if cancelling {
                response.on_hover_text("Stopping the provider and restoring the branch");
            } else if response.clicked() {
                action.cancel_ai_rebase = true;
            }
        }
        Some(BusyAction::Other) => {
            row.add(
                Spinner::new()
                    .size(ICON_GLYPH)
                    .color(palette.text_secondary),
            );
        }
        _ => {}
    }

    ui.painter().line_segment(
        [
            egui::pos2(strip.left(), strip.bottom() - 1.0),
            egui::pos2(strip.right(), strip.bottom() - 1.0),
        ],
        egui::Stroke::new(1.0, palette.border_subtle),
    );
    action
}

/// "AI rebase · m:ss" — the elapsed time tells a long run is alive, not stuck.
fn ai_rebase_chip_label(seconds: u64) -> String {
    format!("AI rebase · {}:{:02}", seconds / 60, seconds % 60)
}

/// Useful message for a network op failure (git.md §10): typed variant ⇒ a short
/// sentence, `Other` ⇒ the summarized stderr as-is.
pub fn sync_failure_message(error: &SyncError) -> String {
    match error {
        SyncError::NoRemote => NO_REMOTE.to_owned(),
        SyncError::NoUpstream => "No upstream to overwrite".to_owned(),
        SyncError::FfOnlyRefused => "Pull refused — not fast-forwardable".to_owned(),
        // Conflicts name their op in `sync_error_message` (Pull, Rebase or Merge).
        SyncError::Conflicts => "stopped on conflicts — resolve from the terminal".to_owned(),
        SyncError::NonFastForward => "Push rejected — not fast-forward, never forced".to_owned(),
        SyncError::StaleInfo => "Force push rejected — the remote moved, fetch first".to_owned(),
        // Surfaced silently by `drain_sync` (no toast) — never formatted.
        SyncError::RemoteBranchGone => String::new(),
        SyncError::AuthFailed => "Authentication failed".to_owned(),
        SyncError::GitNotFound => GIT_MISSING.to_owned(),
        SyncError::TimedOut => "Git command timed out and was cancelled".to_owned(),
        SyncError::Other(stderr) => stderr.clone(),
    }
}

/// Failure toast for a network op (git.md §10): variants that already name their
/// op pass through as-is, the others are prefixed ("Push failed —
/// Authentication failed").
pub fn sync_error_message(command: SyncCommand, error: &SyncError) -> String {
    match error {
        SyncError::FfOnlyRefused | SyncError::NonFastForward | SyncError::StaleInfo => {
            sync_failure_message(error)
        }
        // A conflict can stop a Pull, a Rebase **or** a Merge: the op label tells which.
        SyncError::Conflicts => {
            format!("{} {}", sync_op_label(command), sync_failure_message(error))
        }
        _ => format!(
            "{} failed — {}",
            sync_op_label(command),
            sync_failure_message(error)
        ),
    }
}

/// Success toast for a network op: the op and its outcome (git.md §10) — explicit
/// feedback for async actions, auto-expired (`ui::toast`).
pub fn sync_success_message(command: SyncCommand, outcome: SyncOutcome) -> String {
    match (command, outcome) {
        (SyncCommand::FetchAll, SyncOutcome::UpToDate) => "Fetched — already up to date".to_owned(),
        (SyncCommand::FetchAll, SyncOutcome::Updated) => "Fetched — remote refs updated".to_owned(),
        (SyncCommand::Pull(_), SyncOutcome::UpToDate) => "Pulled — already up to date".to_owned(),
        (SyncCommand::Pull(_), SyncOutcome::Updated) => "Pulled — branch updated".to_owned(),
        (SyncCommand::Push, _) => "Pushed".to_owned(),
        (SyncCommand::ForcePush, _) => "Force-pushed".to_owned(),
        (SyncCommand::Rebase(_), SyncOutcome::UpToDate) => "Rebase — already up to date".to_owned(),
        (SyncCommand::Rebase(onto), SyncOutcome::Updated) => format!("Rebased onto {onto}"),
        (SyncCommand::InteractiveRebase { onto, .. }, _) => {
            format!("Interactively rebased onto {onto}")
        }
        (SyncCommand::Merge(_), SyncOutcome::UpToDate) => "Merge — already up to date".to_owned(),
        (SyncCommand::Merge(from), SyncOutcome::Updated) => format!("Merged {from}"),
        (SyncCommand::CherryPick(sha), _) => {
            format!("Cherry-picked {}", &sha[..sha.len().min(7)])
        }
        (SyncCommand::Revert(sha), _) => format!("Reverted {}", &sha[..sha.len().min(7)]),
        (SyncCommand::AbortOp, _) => "Merge/Rebase aborted — branch restored".to_owned(),
        (SyncCommand::ContinueOp, _) => "Continued — conflicts resolved".to_owned(),
        (SyncCommand::DeleteRemoteBranch(branch), _)
        | (SyncCommand::DeleteRemoteThenLocalBranch { remote: branch, .. }, _) => {
            format!("Deleted {branch} on the remote")
        }
        (SyncCommand::PushTag(tag), _) => format!("Pushed tag {tag} to origin"),
        (SyncCommand::DeleteRemoteThenLocalTag(tag), _) => {
            format!("Deleted tag {tag} on origin")
        }
    }
}

fn sync_op_label(command: SyncCommand) -> &'static str {
    match command {
        SyncCommand::FetchAll => "Fetch",
        SyncCommand::Pull(_) => "Pull",
        SyncCommand::Push => "Push",
        SyncCommand::ForcePush => "Force push",
        SyncCommand::Rebase(_) => "Rebase",
        SyncCommand::InteractiveRebase { .. } => "Interactive rebase",
        SyncCommand::Merge(_) => "Merge",
        SyncCommand::CherryPick(_) => "Cherry-pick",
        SyncCommand::Revert(_) => "Revert",
        SyncCommand::AbortOp => "Abort",
        SyncCommand::ContinueOp => "Continue",
        SyncCommand::DeleteRemoteBranch(_) | SyncCommand::DeleteRemoteThenLocalBranch { .. } => {
            "Delete remote branch"
        }
        SyncCommand::PushTag(_) => "Push tag",
        SyncCommand::DeleteRemoteThenLocalTag(_) => "Delete remote tag",
    }
}

/// Pull split-button: main area (default op, "Pull" / "Fetch" label) + a chevron
/// separated by a 1px rule that opens the default's radio menu (selection
/// **without running**).
fn pull_split_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &ToolbarState,
    action: &mut ToolbarAction,
) {
    let pull_busy = state.busy == Some(BusyAction::Pull);
    let blocker = pull_blocker(state);
    let main = toolbar_button(
        ui,
        palette,
        lucide_icons::Icon::ArrowDownToLine,
        state.pull_default.button_label(),
        blocker.is_none() && !pull_busy,
        pull_busy,
        egui::CornerRadius {
            nw: RADIUS_PILL,
            sw: RADIUS_PILL,
            ne: 0,
            se: 0,
        },
    );
    // The chevron abuts the main area (inner rule, not a separate button).
    let busy = state.busy.is_some();
    ui.spacing_mut().item_spacing.x = 0.0;
    let chevron = chevron_button(ui, palette, "Pull options", busy);
    ui.spacing_mut().item_spacing.x = BUTTON_GAP;
    ui.painter().line_segment(
        [
            egui::pos2(main.rect.right(), main.rect.top() + SPLIT_RULE_INSET),
            egui::pos2(main.rect.right(), main.rect.bottom() - SPLIT_RULE_INSET),
        ],
        egui::Stroke::new(1.0, palette.border_subtle),
    );
    match blocker {
        Some(reason) => {
            main.on_hover_text(reason);
        }
        None => {
            if !pull_busy && main.clicked() {
                action.sync = Some(state.pull_default.command());
            }
        }
    }
    if !busy {
        egui::Popup::menu(&chevron)
            .gap(POPUP_GAP)
            .style(crate::theme::menu_style)
            .show(|ui| {
                ui.label(
                    egui::RichText::new(MENU_TITLE)
                        .size(MENU_TITLE_SIZE)
                        .color(palette.text_muted),
                );
                for option in PullDefault::ALL {
                    if ui
                        .radio(state.pull_default == option, option.menu_label())
                        .clicked()
                    {
                        action.set_default = Some(option);
                    }
                }
            });
    }
}

/// Push split-button: main area (push the current branch to its upstream) +
/// a chevron whose menu holds a single **one-shot** entry, **Push (force with
/// lease)**. Unlike Pull's chevron it never sets a default — forcing stays a
/// deliberate act each time (git.md §10), routed through a confirmation modal.
/// The entry is greyed without an upstream (the plain `-u` push covers the
/// first publication).
fn push_split_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &ToolbarState,
    action: &mut ToolbarAction,
) {
    let push_busy = state.busy == Some(BusyAction::Push);
    let blocker = push_blocker(state);
    let main = toolbar_button(
        ui,
        palette,
        lucide_icons::Icon::ArrowUpFromLine,
        "Push",
        blocker.is_none() && !push_busy,
        push_busy,
        egui::CornerRadius {
            nw: RADIUS_PILL,
            sw: RADIUS_PILL,
            ne: 0,
            se: 0,
        },
    );
    // The chevron follows the main area's gating: the static blockers (no
    // remote, detached, unborn, missing git) and any running op disable it.
    let disabled = blocker.is_some() || state.busy.is_some();
    ui.spacing_mut().item_spacing.x = 0.0;
    let chevron = chevron_button(ui, palette, "Push options", disabled);
    ui.spacing_mut().item_spacing.x = BUTTON_GAP;
    ui.painter().line_segment(
        [
            egui::pos2(main.rect.right(), main.rect.top() + SPLIT_RULE_INSET),
            egui::pos2(main.rect.right(), main.rect.bottom() - SPLIT_RULE_INSET),
        ],
        egui::Stroke::new(1.0, palette.border_subtle),
    );
    match blocker {
        Some(reason) => {
            main.on_hover_text(reason);
        }
        None => {
            if !push_busy && main.clicked() {
                action.sync = Some(SyncCommand::Push);
            }
        }
    }
    if !disabled {
        egui::Popup::menu(&chevron)
            .gap(POPUP_GAP)
            .style(crate::theme::menu_style)
            .show(|ui| {
                let entry = ui.add_enabled(
                    state.has_upstream,
                    egui::Button::new("Push (force with lease)"),
                );
                if entry.clicked() {
                    action.force_push = true;
                } else if !state.has_upstream {
                    entry.on_hover_text(FORCE_NO_UPSTREAM);
                }
            });
    }
}

/// Confirmation modal for **Push (force with lease)** (git.md §10): names the
/// branch and the remote it overwrites, red **Force push** to confirm. Forcing
/// stays a deliberate act — the modal is the gate before the lease push runs on
/// the sync runner. Same outcome contract as the delete modals
/// ([`DeleteModalAction`]).
pub fn force_push_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    branch: &str,
    remote: &str,
    out: &mut DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("force_push_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            ui.label(egui::RichText::new(format!("Force-push “{branch}” to {remote}?")).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Overwrites the upstream branch with your local history, \
                     with a lease so git refuses if the remote moved.",
                )
                .color(palette.text_secondary),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(crate::ui::danger_button(palette, "Force push"))
                        .clicked()
                    {
                        out.confirm = true;
                    }
                });
            });
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Confirmation modal for **Hard reset** (graph row menu, git.md §9): names the
/// branch and the target commit, red **Reset** to confirm. A hard reset discards
/// the index and working tree, so it sits behind this gate before running on the
/// worker. Same outcome contract as the delete modals ([`DeleteModalAction`]).
pub fn reset_hard_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    branch: &str,
    short: &str,
    out: &mut DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("reset_hard_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            ui.label(egui::RichText::new(format!("Hard-reset “{branch}” to {short}?")).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Discards staged and unstaged changes to match the target commit. \
                     Untracked files are left in place.",
                )
                .color(palette.text_secondary),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::ui::danger_button(palette, "Reset")).clicked() {
                        out.confirm = true;
                    }
                });
            });
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

fn chevron_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &'static str,
    disabled: bool,
) -> egui::Response {
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(CHEVRON_WIDTH, BUTTON_HEIGHT), !disabled);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius {
                nw: 0,
                sw: 0,
                ne: RADIUS_PILL,
                se: RADIUS_PILL,
            },
            palette.bg_surface_hover,
        );
    }
    let ink = if disabled {
        palette.state_disabled
    } else {
        palette.text_secondary
    };
    paint_icon(
        ui.painter(),
        rect.center(),
        ICON_GLYPH,
        lucide_icons::Icon::ChevronDown,
        ink,
    );
    response
        .widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, !disabled, label));
    response
}

/// Toolbar button: Lucide icon + `text.secondary` label, hover
/// `bg.surface.hover`. Disabled ⇒ `state.disabled` (the tooltip is set by the
/// caller); busy ⇒ spinner in place of the icon, click ignored.
fn toolbar_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: lucide_icons::Icon,
    label: &str,
    enabled: bool,
    busy: bool,
    corners: egui::CornerRadius,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(LABEL_SIZE),
        egui::Color32::PLACEHOLDER,
    );
    let width = BUTTON_PAD_X * 2.0 + ICON_GLYPH + ICON_GAP + galley.size().x;
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, BUTTON_HEIGHT), enabled);
    if hovered {
        ui.painter()
            .rect_filled(rect, corners, palette.bg_surface_hover);
    }
    let ink = if enabled || busy {
        palette.text_secondary
    } else {
        palette.state_disabled
    };
    let icon_center = egui::pos2(
        rect.left() + BUTTON_PAD_X + ICON_GLYPH / 2.0,
        rect.center().y,
    );
    if busy {
        // `paint_at`, never `put`: `put` advances the layout cursor past the
        // spinner rect — placed earlier, it pulled the cursor back and the next
        // button painted over this one.
        Spinner::new().size(ICON_GLYPH).color(ink).paint_at(
            ui,
            egui::Rect::from_center_size(icon_center, egui::vec2(ICON_GLYPH, ICON_GLYPH)),
        );
    } else {
        paint_icon(ui.painter(), icon_center, ICON_GLYPH, icon, ink);
    }
    ui.painter().galley(
        egui::pos2(
            rect.left() + BUTTON_PAD_X + ICON_GLYPH + ICON_GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    let label = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled && !busy, &label)
    });
    response
}

/// Any git command in progress greys out the other buttons (the button that
/// triggered the action spins instead of being greyed out).
fn busy_blocker(state: &ToolbarState, own: BusyAction) -> Option<&'static str> {
    state.busy.is_some_and(|busy| busy != own).then_some(BUSY)
}

fn pull_blocker(state: &ToolbarState) -> Option<&'static str> {
    if state.git_missing {
        Some(GIT_MISSING)
    } else if !state.has_remote {
        Some(NO_REMOTE)
    } else if state.detached {
        Some(DETACHED)
    } else if state.unborn && state.pull_default != PullDefault::FetchAll {
        Some(UNBORN)
    } else {
        busy_blocker(state, BusyAction::Pull)
    }
}

fn push_blocker(state: &ToolbarState) -> Option<&'static str> {
    if state.git_missing {
        Some(GIT_MISSING)
    } else if !state.has_remote {
        Some(NO_REMOTE)
    } else if state.detached {
        Some(DETACHED)
    } else if state.unborn {
        Some(UNBORN)
    } else {
        busy_blocker(state, BusyAction::Push)
    }
}

fn branch_blocker(state: &ToolbarState) -> Option<&'static str> {
    if state.unborn {
        Some(UNBORN)
    } else {
        busy_blocker(state, BusyAction::Branch)
    }
}

fn stash_blocker(state: &ToolbarState) -> Option<&'static str> {
    if state.unborn {
        Some(UNBORN)
    } else if !state.dirty {
        Some(CLEAN)
    } else {
        busy_blocker(state, BusyAction::Stash)
    }
}

fn pop_blocker(state: &ToolbarState) -> Option<&'static str> {
    if state.unborn {
        Some(UNBORN)
    } else if state.stash_count == 0 {
        Some(NO_STASH)
    } else {
        busy_blocker(state, BusyAction::Pop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ToolbarState {
        ToolbarState {
            pull_default: PullDefault::Ff,
            busy: None,
            has_remote: true,
            has_upstream: true,
            detached: false,
            unborn: false,
            dirty: true,
            stash_count: 1,
            git_missing: false,
        }
    }

    #[test]
    fn pull_default_maps_to_commands_and_labels() {
        assert_eq!(PullDefault::FetchAll.command(), SyncCommand::FetchAll);
        assert_eq!(PullDefault::Ff.command(), SyncCommand::Pull(PullMode::Ff));
        assert_eq!(
            PullDefault::FfOnly.command(),
            SyncCommand::Pull(PullMode::FfOnly)
        );
        assert_eq!(
            PullDefault::Rebase.command(),
            SyncCommand::Pull(PullMode::Rebase)
        );
        assert_eq!(PullDefault::FetchAll.button_label(), "Fetch");
        assert_eq!(PullDefault::Ff.button_label(), "Pull");
        assert_eq!(PullDefault::default(), PullDefault::Ff);
    }

    #[test]
    fn sync_failures_map_to_useful_messages() {
        assert_eq!(
            sync_failure_message(&SyncError::NoRemote),
            "No remote configured"
        );
        assert_eq!(
            sync_failure_message(&SyncError::FfOnlyRefused),
            "Pull refused — not fast-forwardable"
        );
        assert_eq!(
            sync_failure_message(&SyncError::NonFastForward),
            "Push rejected — not fast-forward, never forced"
        );
        assert_eq!(
            sync_failure_message(&SyncError::AuthFailed),
            "Authentication failed"
        );
        assert_eq!(
            sync_failure_message(&SyncError::GitNotFound),
            "git binary not found"
        );
        assert_eq!(
            sync_failure_message(&SyncError::Other("fatal: oops".to_owned())),
            "fatal: oops",
            "the summarized stderr passes through as-is"
        );
    }

    #[test]
    fn sync_error_toasts_name_the_operation_when_the_message_does_not() {
        assert_eq!(
            sync_error_message(SyncCommand::Push, &SyncError::AuthFailed),
            "Push failed — Authentication failed"
        );
        assert_eq!(
            sync_error_message(SyncCommand::FetchAll, &SyncError::GitNotFound),
            "Fetch failed — git binary not found"
        );
        assert_eq!(
            sync_error_message(
                SyncCommand::Pull(PullMode::Ff),
                &SyncError::Other("fatal: oops".to_owned())
            ),
            "Pull failed — fatal: oops"
        );
        // Self-describing: no double prefix.
        assert_eq!(
            sync_error_message(
                SyncCommand::Pull(PullMode::FfOnly),
                &SyncError::FfOnlyRefused
            ),
            "Pull refused — not fast-forwardable"
        );
        assert_eq!(
            sync_error_message(SyncCommand::Push, &SyncError::NonFastForward),
            "Push rejected — not fast-forward, never forced"
        );
        assert_eq!(
            sync_error_message(SyncCommand::Pull(PullMode::Ff), &SyncError::Conflicts),
            "Pull stopped on conflicts — resolve from the terminal"
        );
        assert_eq!(
            sync_error_message(SyncCommand::Rebase("main".into()), &SyncError::Conflicts),
            "Rebase stopped on conflicts — resolve from the terminal"
        );
        assert_eq!(
            sync_error_message(SyncCommand::Merge("feat".into()), &SyncError::Conflicts),
            "Merge stopped on conflicts — resolve from the terminal"
        );
        assert_eq!(
            sync_error_message(
                SyncCommand::Rebase("main".into()),
                &SyncError::Other("fatal: oops".to_owned())
            ),
            "Rebase failed — fatal: oops"
        );
    }

    #[test]
    fn sync_success_toasts_describe_the_outcome() {
        assert_eq!(
            sync_success_message(SyncCommand::FetchAll, SyncOutcome::UpToDate),
            "Fetched — already up to date"
        );
        assert_eq!(
            sync_success_message(SyncCommand::FetchAll, SyncOutcome::Updated),
            "Fetched — remote refs updated"
        );
        assert_eq!(
            sync_success_message(SyncCommand::Pull(PullMode::Rebase), SyncOutcome::UpToDate),
            "Pulled — already up to date"
        );
        assert_eq!(
            sync_success_message(SyncCommand::Pull(PullMode::Ff), SyncOutcome::Updated),
            "Pulled — branch updated"
        );
        assert_eq!(
            sync_success_message(SyncCommand::Push, SyncOutcome::Updated),
            "Pushed"
        );
        assert_eq!(
            sync_success_message(
                SyncCommand::Rebase("origin/main".into()),
                SyncOutcome::Updated
            ),
            "Rebased onto origin/main"
        );
        assert_eq!(
            sync_success_message(SyncCommand::Rebase("main".into()), SyncOutcome::UpToDate),
            "Rebase — already up to date"
        );
        assert_eq!(
            sync_success_message(SyncCommand::Merge("feat".into()), SyncOutcome::Updated),
            "Merged feat"
        );
        assert_eq!(
            sync_success_message(SyncCommand::Merge("feat".into()), SyncOutcome::UpToDate),
            "Merge — already up to date"
        );
    }

    #[test]
    fn force_push_toasts_describe_the_outcome_and_the_lease_refusal() {
        assert_eq!(
            sync_success_message(SyncCommand::ForcePush, SyncOutcome::Updated),
            "Force-pushed"
        );
        // Self-describing — no "Force push failed —" double prefix.
        assert_eq!(
            sync_error_message(SyncCommand::ForcePush, &SyncError::StaleInfo),
            "Force push rejected — the remote moved, fetch first"
        );
        assert_eq!(
            sync_error_message(SyncCommand::ForcePush, &SyncError::NoUpstream),
            "Force push failed — No upstream to overwrite"
        );
    }

    #[test]
    fn delete_remote_branch_toasts_name_the_branch_and_the_op() {
        assert_eq!(
            sync_success_message(
                SyncCommand::DeleteRemoteBranch("origin/feat".to_owned()),
                SyncOutcome::Updated
            ),
            "Deleted origin/feat on the remote"
        );
        assert_eq!(
            sync_error_message(
                SyncCommand::DeleteRemoteBranch("origin/feat".to_owned()),
                &SyncError::AuthFailed
            ),
            "Delete remote branch failed — Authentication failed"
        );
    }

    #[test]
    fn everything_enabled_on_a_ready_repo() {
        let state = ready();
        assert_eq!(pull_blocker(&state), None);
        assert_eq!(push_blocker(&state), None);
        assert_eq!(branch_blocker(&state), None);
        assert_eq!(stash_blocker(&state), None);
        assert_eq!(pop_blocker(&state), None);
    }

    #[test]
    fn no_remote_and_detached_block_network_actions() {
        let state = ToolbarState {
            has_remote: false,
            ..ready()
        };
        assert_eq!(pull_blocker(&state), Some(NO_REMOTE));
        assert_eq!(push_blocker(&state), Some(NO_REMOTE));

        let state = ToolbarState {
            detached: true,
            ..ready()
        };
        assert_eq!(pull_blocker(&state), Some(DETACHED));
        assert_eq!(push_blocker(&state), Some(DETACHED));
        assert_eq!(branch_blocker(&state), None, "Branch stays local");
    }

    #[test]
    fn unborn_repo_only_keeps_fetch_all() {
        let state = ToolbarState {
            unborn: true,
            ..ready()
        };
        assert_eq!(
            pull_blocker(&state),
            Some(UNBORN),
            "pull default ⇒ greyed out"
        );
        let fetch = ToolbarState {
            pull_default: PullDefault::FetchAll,
            ..state
        };
        assert_eq!(pull_blocker(&fetch), None, "Fetch All stays runnable");
        assert_eq!(push_blocker(&state), Some(UNBORN));
        assert_eq!(branch_blocker(&state), Some(UNBORN));
        assert_eq!(stash_blocker(&state), Some(UNBORN));
        assert_eq!(pop_blocker(&state), Some(UNBORN));
    }

    #[test]
    fn a_running_command_grays_every_other_button() {
        let pulling = ToolbarState {
            busy: Some(BusyAction::Pull),
            ..ready()
        };
        assert_eq!(
            pull_blocker(&pulling),
            None,
            "the busy button spins, not greyed out"
        );
        assert_eq!(push_blocker(&pulling), Some(BUSY));
        assert_eq!(branch_blocker(&pulling), Some(BUSY));
        assert_eq!(stash_blocker(&pulling), Some(BUSY));
        assert_eq!(pop_blocker(&pulling), Some(BUSY));

        let stashing = ToolbarState {
            busy: Some(BusyAction::Stash),
            ..ready()
        };
        assert_eq!(stash_blocker(&stashing), None);
        assert_eq!(pull_blocker(&stashing), Some(BUSY));
        assert_eq!(push_blocker(&stashing), Some(BUSY));
        assert_eq!(branch_blocker(&stashing), Some(BUSY));
        assert_eq!(pop_blocker(&stashing), Some(BUSY));
    }

    #[test]
    fn a_mutation_outside_the_toolbar_grays_all_buttons() {
        let busy = ToolbarState {
            busy: Some(BusyAction::Other),
            ..ready()
        };
        assert_eq!(pull_blocker(&busy), Some(BUSY));
        assert_eq!(push_blocker(&busy), Some(BUSY));
        assert_eq!(branch_blocker(&busy), Some(BUSY));
        assert_eq!(stash_blocker(&busy), Some(BUSY));
        assert_eq!(pop_blocker(&busy), Some(BUSY));
    }

    #[test]
    fn missing_git_blocks_network_but_not_local_actions() {
        let state = ToolbarState {
            git_missing: true,
            ..ready()
        };
        assert_eq!(pull_blocker(&state), Some(GIT_MISSING));
        assert_eq!(push_blocker(&state), Some(GIT_MISSING));
        assert_eq!(branch_blocker(&state), None);
        assert_eq!(stash_blocker(&state), None);
        assert_eq!(pop_blocker(&state), None);
    }

    #[test]
    fn clean_tree_blocks_stash_and_empty_stash_blocks_pop() {
        let state = ToolbarState {
            dirty: false,
            stash_count: 0,
            ..ready()
        };
        assert_eq!(stash_blocker(&state), Some(CLEAN));
        assert_eq!(pop_blocker(&state), Some(NO_STASH));
    }
}
