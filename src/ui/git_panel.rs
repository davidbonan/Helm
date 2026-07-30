use std::collections::HashSet;
use std::path::Path;

use crate::git::edit::EditRequest;
use crate::git::file_tree::{self, TreeRow};
use crate::git::status::{ChangeKind, FileEntry, OpSummary, RepoStatus};
use crate::keybindings::{Action, Keymap};
use crate::theme::{Palette, PILL_SIZE, RADIUS_PILL, SECTION_TITLE_SIZE, TITLE_SIZE};
use crate::ui::file_list::{
    self, file_menu_entries, file_row_fill, row_separator, FileMenuCtx, FileMenuOutput,
    FileViewMode, PATH_SIZE, ROW_HEIGHT,
};
use crate::ui::spinner::Spinner;
use crate::ui::SECTION_TOP_MARGIN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitIntent {
    Refresh,
    Stage(String),
    Unstage(String),
    StageAll,
    UnstageAll,
    Discard(String),
    DiscardAll,
    /// Stashes the given paths in a **single** stash — both staged and unstaged
    /// changes of each (WIP sidebar context menu, git.md §3). Confirmed by a modal
    /// on the panel side before it is emitted.
    StashFiles(Vec<String>),
    Commit(String),
    /// Amends **HEAD**'s message from the commit-detail reword editor (git.md §5):
    /// the composed message (subject + blank line + description). Handled app-side
    /// (worker amend + graph reload + HEAD re-select), never straight to the worker.
    AmendMessage(String),
    /// Asks the AI to fill the commit inputs (subject + description) — the
    /// generation is asynchronous and never commits.
    GenerateMessage,
    /// **Abort** button of the conflict panel footer (git.md §10): the caller
    /// confirms (modal) before anything runs.
    AbortOp,
    /// **Continue** button of the conflict panel footer (conflicts.md §2):
    /// finalises the operation on the sync runner — emitted only when no conflict
    /// stage remains.
    ContinueOp,
    /// Opens the in-app conflict editor (conflicts.md §2): a conflicted-row click
    /// (`focus` = that file); `focus` = `None` lands on the first file.
    OpenConflictEditor {
        focus: Option<String>,
    },
    /// Opens the file's overlay diff view (M6-3). `staged` indicates the original
    /// section: `false` ⇒ Unstaged (diff WT vs index), `true` ⇒ Staged (index vs HEAD).
    OpenDiff {
        path: String,
        staged: bool,
    },
    /// Granular staging emitted from the overlay diff view (M6-3); the open file's
    /// `path` is supplied by the caller when routing to the worker.
    StageHunk(usize),
    UnstageHunk(usize),
    StageLines {
        hunk: usize,
        lines: Vec<usize>,
    },
    UnstageLines {
        hunk: usize,
        lines: Vec<usize>,
    },
    /// **Discard hunk** emitted from the overlay diff view for an unstaged hunk
    /// (git.md §4): destructive, the app confirms it before reverting the working
    /// tree. The open file's `path` is supplied by the caller when routing.
    DiscardHunk(usize),
    /// Flat ⇄ Tree toggle of the file lists (M40): the app stores it in
    /// `Prefs.git_file_view` (shared, persisted) and re-renders both panels.
    SetFileView(FileViewMode),
    /// One inline-editor buffer to write back to the working tree (git.md §4):
    /// emitted 800 ms after the last keystroke, on the way out of the editor, and
    /// again with `force` when the user answers **Overwrite** to a divergence
    /// notice. Self-contained — the intent outlives the editor that produced it.
    FlushEdit(EditRequest),
    /// `Cmd+E` on a diff that cannot take a caret (git.md §4): the app names the
    /// reason in a toast carrying **Open in editor**, the external fallback.
    EditRefused {
        path: String,
    },
}

/// Target of a discard awaiting confirmation (git.md §3: destructive action).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscardTarget {
    File(String),
    /// Several files selected in the WIP sidebar (multi-select context menu).
    Files(Vec<String>),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileSelection {
    pub path: String,
    pub staged: bool,
}

/// View-state of the git sidebar (architecture §1, intent pattern): **domain
/// effects leave only via [`GitIntent`]** — no field here is a second copy of
/// domain state. Fields are grouped by concern; each label names its writers.
#[derive(Debug, Default)]
pub struct GitPanelState {
    // Commit draft — typed by the user, cleared here on commit click (the
    // message travels in `GitIntent::Commit`); filled by the app when an AI
    // suggestion lands (`drain_ai`).
    pub subject: String,
    pub description: String,
    // Section folding — toggled here by the section headers only.
    pub unstaged_collapsed: bool,
    pub staged_collapsed: bool,
    // Tree-view directory folding (M40) — session-only, keyed by directory full
    // path; toggled here by the directory rows. Empty (and unused) in Flat mode.
    pub unstaged_collapsed_dirs: HashSet<String>,
    pub staged_collapsed_dirs: HashSet<String>,
    // Discard confirmation modal — armed here by the Discard pills / context
    // menu, resolved here by the modal (confirm ⇒ `GitIntent::Discard*`).
    pub pending_discard: Option<DiscardTarget>,
    // Stash confirmation modal — armed here by the context-menu Stash entry,
    // resolved here by the modal (confirm ⇒ `GitIntent::StashFiles`). The whole
    // file (staged + unstaged) is stashed, never a partial stash (git.md §3).
    pub pending_stash: Option<Vec<String>>,
    // File selection & ↑/↓ nav — armed here (row click, arrow nav), disarmed
    // here (commit-input click, no-repo reset); the app also reads the pair to
    // route the arrows (sidebar nav vs graph) and disarms the nav when a
    // terminal takes keyboard focus.
    pub selected_file: Option<GitFileSelection>,
    pub file_nav_active: bool,
    // Multi-selection for the context-menu batch actions (Cmd/Shift+click). A
    // plain click resets it to the clicked file; the highlight follows it. The
    // anchor is the last plain/Cmd-clicked row, used by Shift+click ranges.
    pub marked_files: Vec<GitFileSelection>,
    pub selection_anchor: Option<GitFileSelection>,
    /// AI generation in progress — per-frame projection of `AiRunner::busy()`
    /// written by the app: spinner on the button, click ignored.
    pub ai_busy: bool,
    /// Commit being written — per-frame projection of
    /// `GitWorker::has_pending_commit()` written by the app: spinner on the
    /// commit button, click and shortcut ignored (no double commit).
    pub commit_busy: bool,
    /// First status snapshot not yet received (spawn, repo switch) — per-frame
    /// projection written by the app: the files card shows a loader instead of
    /// a misleading "Nothing to commit" / "0 files changed".
    pub status_loading: bool,
    /// Mutation awaiting its worker reply — per-frame projection of
    /// `GitWorker::pending_mutation()` written by the app: spinner in place of
    /// the Refresh icon (feedback for a slow stage-all / discard / checkout).
    pub mutation_busy: bool,
    /// An inline editor is open in the diff overlay — per-frame projection written
    /// by the app: it takes the text input, so the sidebar's ↑/↓ file navigation and
    /// `Cmd+Enter` are disarmed until it closes (keybindings.md §4).
    pub inline_editing: bool,
    /// A long op (sync, AI rebase — minutes) holds the repo's mutation lock —
    /// per-frame projection of `GitSession::lock_busy()` written by the app:
    /// the staging / discard / commit actions are greyed out, since the worker
    /// refuses every mutation meanwhile (git.md §9).
    pub lock_busy: bool,
}

impl GitPanelState {
    /// Drops everything aimed at the leaving repo's files when its session is
    /// dropped on a repo switch: an armed confirmation re-renders on the next
    /// frame and its intent would be routed to the **new** session.
    pub fn disarm_on_repo_switch(&mut self) {
        self.pending_discard = None;
        self.pending_stash = None;
        self.selected_file = None;
        self.marked_files.clear();
        self.selection_anchor = None;
    }
}

const BRANCH_SIZE: f32 = 13.0;
const PILL_PAD_X: f32 = 11.0;
const PILL_PAD_Y: f32 = 5.0;
// Radius deliberately tighter than RADIUS_PILL: reads as a "button", not a label.
const PILL_RADIUS: u8 = 3;
const ACTION_GAP: f32 = 4.0;
const ICON_HIT: f32 = 22.0;
const ICON_GLYPH: f32 = 15.0;
// Sidebar cards (M13 mockup): no border (the sidebar edges form the frame),
// inner bands separated by full-width rules. Content aligned on the sidebar
// margin, like the commit detail.
const CARD_GAP: f32 = 10.0;
const HEADER_BAND_H: f32 = 36.0;
const SUMMARY_BAND_H: f32 = 34.0;
const SUMMARY_SIZE: f32 = 13.0;
const SECTION_HEADER_H: f32 = 30.0;
const SECTION_GAP: f32 = 8.0;
const RATIO_BAR_W: f32 = 56.0;
const RATIO_BAR_H: f32 = 6.0;
const RATIO_BAR_GAP: f32 = 2.0;
const RATIO_BAR_MIN: f32 = 6.0;
const DESCRIPTION_ROWS: usize = 4;
const SUBJECT_SOFT_LIMIT: usize = 72;
const DESCRIPTION_SOFT_LIMIT: usize = 1000;
const LABEL_SIZE: f32 = 12.5;
// Typed text of the commit subject + description, a notch above egui's 12.5 Body
// so the message you compose reads larger than its labels.
const COMMIT_INPUT_SIZE: f32 = 14.0;
const COUNTER_SIZE: f32 = 10.5;
const COUNTER_RESERVE: f32 = 50.0;
const INPUT_PAD_X: i8 = 8;
const INPUT_PAD_Y: i8 = 6;
// Subtle radii taken from the mockup: inputs slightly rounded, commit button
// nearly square (≈ 0.06 × its height).
const INPUT_RADIUS: u8 = 6;
const COMMIT_BUTTON_RADIUS: u8 = 4;
const COMMIT_BUTTON_ICON: f32 = 13.0;
const COMMIT_BUTTON_H: f32 = 34.0;
const COMMIT_CARD_H: f32 = 252.0;
const TINT_ALPHA: u8 = 28;
// Footer of the conflict panel: the Continue / Abort action buttons side by side,
// sized so they keep the same bottom margin as the standalone commit button.
const CONFLICT_FOOTER_H: f32 = 82.0;
const NO_REPO_LABEL: &str = "No repository open";
const CLEAN_LABEL: &str = "Nothing to commit";
// Same size as the graph loader; the a11y label is the only handle for tests.
const STATUS_SPINNER_SIZE: f32 = 22.0;
const STATUS_LOADING_LABEL: &str = "Loading status";
const SUBJECT_HINT: &str = "Write a short, clear summary";
const DESCRIPTION_HINT: &str = "Explain the change (why and what)";

#[derive(Clone, Copy)]
enum RowSection {
    Unstaged,
    Staged,
}

#[derive(Clone, Copy)]
enum FileNav {
    Previous,
    Next,
}

#[allow(clippy::too_many_arguments)]
pub fn git_panel(
    ui: &mut egui::Ui,
    palette: &Palette,
    branch: &str,
    status: &RepoStatus,
    op_in_progress: bool,
    op: Option<&OpSummary>,
    state: &mut GitPanelState,
    keymap: &Keymap,
    intents: &mut Vec<GitIntent>,
    repo_root: Option<&Path>,
    file_menu: &mut FileMenuOutput,
    view: FileViewMode,
) {
    ui.add_space(4.0);
    // A merge / rebase / cherry-pick / revert in progress takes over the panel
    // with the dedicated conflict view (conflicts.md §2); the normal status +
    // commit layout returns once the op ends. The first snapshot is awaited first
    // (the default status would read as "no conflicts" before anything is scanned).
    if op_in_progress && !state.status_loading {
        conflict_panel(
            ui, palette, op, status, state, intents, repo_root, file_menu,
        );
        return;
    }

    let total_h = remaining_height(ui);
    let files_card_h = (total_h - COMMIT_CARD_H - CARD_GAP).max(0.0);
    card(ui, files_card_h, |ui| {
        header_band(ui, palette, branch, status, state, intents, view);
        card_divider(ui, palette);
        if state.status_loading {
            status_loading_placeholder(ui, palette);
            return;
        }
        summary_band(ui, palette, status);
        card_divider(ui, palette);
        let mut menu = FileMenuCtx {
            root: repo_root,
            out: file_menu,
        };
        file_sections(ui, palette, status, state, intents, &mut menu, view);
    });
    ui.add_space(CARD_GAP);
    card(ui, COMMIT_CARD_H, |ui| {
        commit_card(ui, palette, status, op_in_progress, state, keymap, intents)
    });

    discard_confirm(ui, palette, state, intents);
    stash_confirm(ui, palette, state, intents);
    if let Some(nav) = file_nav_pressed(ui, state) {
        navigate_selected_file(ui, status, state, intents, nav, view);
    }
}

/// Conflict panel (M33-4, conflicts.md §2), shown while an op is
/// in progress: a "<verb> conflicts detected" header + the source/target chips,
/// the **Conflicted Files** group (each row opens the editor) above the
/// **Resolved Files** group, then **Continue** / **Abort** in the footer.
#[allow(clippy::too_many_arguments)]
fn conflict_panel(
    ui: &mut egui::Ui,
    palette: &Palette,
    op: Option<&OpSummary>,
    status: &RepoStatus,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    repo_root: Option<&Path>,
    file_menu: &mut FileMenuOutput,
) {
    let conflicted: Vec<&FileEntry> = status
        .unstaged
        .iter()
        .filter(|e| e.kind == ChangeKind::Conflicted)
        .collect();
    let verb = op.map_or("Merge", |o| o.verb);

    let total_h = remaining_height(ui);
    let files_card_h = (total_h - CONFLICT_FOOTER_H - CARD_GAP).max(0.0);
    card(ui, files_card_h, |ui| {
        conflict_header(ui, palette, verb, op, !conflicted.is_empty());
        card_divider(ui, palette);
        let mut menu = FileMenuCtx {
            root: repo_root,
            out: file_menu,
        };
        conflict_sections(
            ui,
            palette,
            status,
            &conflicted,
            &status.staged,
            state,
            intents,
            &mut menu,
        );
    });
    ui.add_space(CARD_GAP);
    card(ui, CONFLICT_FOOTER_H, |ui| {
        conflict_footer(ui, palette, verb, conflicted.len(), intents);
    });
}

/// Header band: an alert when conflicts remain ("<verb> conflicts detected"),
/// otherwise the all-clear ("Ready to continue"); below it the "<verb> `source`
/// into `target`" sub-line when both branch names resolved (conflicts.md §2).
fn conflict_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    verb: &str,
    op: Option<&OpSummary>,
    has_conflicts: bool,
) {
    let (icon, tint, text) = if has_conflicts {
        (
            lucide_icons::Icon::AlertTriangle,
            palette.git_conflict,
            format!("{} conflicts detected", op_noun(verb)),
        )
    } else {
        (
            lucide_icons::Icon::CheckCircle,
            palette.git_added,
            "Ready to continue".to_owned(),
        )
    };
    band(ui, HEADER_BAND_H, |ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        crate::ui::paint_icon(ui.painter(), icon_rect.center(), ICON_GLYPH, icon, tint);
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(text)
                .size(TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
    });
    if let (Some(source), Some(target)) =
        op.map_or((None, None), |o| (o.source.as_deref(), o.target.as_deref()))
    {
        conflict_subline(ui, palette, verb, source, target);
    }
}

/// "<verb> `source` into `target`" — the verb in prose, the branches as chips.
fn conflict_subline(ui: &mut egui::Ui, palette: &Palette, verb: &str, source: &str, target: &str) {
    band(ui, SUMMARY_BAND_H, |ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(
            egui::RichText::new(verb)
                .size(PATH_SIZE)
                .color(palette.text_secondary),
        );
        branch_chip(ui, palette, source, (ui.available_width() * 0.42).max(36.0));
        ui.label(
            egui::RichText::new("into")
                .size(PATH_SIZE)
                .color(palette.text_secondary),
        );
        branch_chip(ui, palette, target, ui.available_width().max(36.0));
    });
}

/// The two stacked groups: **Conflicted Files** (rows open the editor; a master
/// **Mark All Resolved** stages them as-is) over **Resolved Files** (read-only,
/// the staged set — a resolved conflict is a normal staged file, conflicts.md §5).
#[allow(clippy::too_many_arguments)]
fn conflict_sections(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    conflicted: &[&FileEntry],
    resolved: &[FileEntry],
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(2.0);
    let half = ((remaining_height(ui) - SECTION_GAP) / 2.0).max(0.0);
    card(ui, half, |ui| {
        conflicted_section(ui, palette, status, conflicted, state, intents, menu);
    });
    ui.add_space(SECTION_GAP);
    card(ui, half, |ui| {
        resolved_section(ui, palette, resolved);
    });
}

fn conflicted_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    conflicted: &[&FileEntry],
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
) {
    let count = conflicted.len();
    let mut mark_all = false;
    band(ui, SECTION_HEADER_H, |ui| {
        ui.label(conflict_section_title(palette, "Conflicted Files", count));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            mark_all = intent_pill(
                ui,
                palette,
                "Mark All Resolved",
                palette.git_added,
                count > 0 && !state.lock_busy,
            );
        });
    });
    if count == 0 {
        band(ui, ROW_HEIGHT, |ui| {
            ui.label(
                egui::RichText::new("All conflicts resolved")
                    .size(PATH_SIZE)
                    .color(palette.text_muted),
            );
        });
    } else {
        file_scroll(ui, "git_conflicted_files", |ui| {
            for entry in conflicted {
                row_separator(ui, palette);
                file_row(
                    ui,
                    palette,
                    status,
                    entry,
                    RowSection::Unstaged,
                    0.0,
                    &entry.path,
                    state,
                    intents,
                    menu,
                    FileViewMode::Flat,
                );
            }
        });
    }
    if mark_all {
        for entry in conflicted {
            intents.push(GitIntent::Stage(entry.path.clone()));
        }
    }
}

fn resolved_section(ui: &mut egui::Ui, palette: &Palette, resolved: &[FileEntry]) {
    let count = resolved.len();
    band(ui, SECTION_HEADER_H, |ui| {
        ui.label(conflict_section_title(palette, "Resolved Files", count));
    });
    if count == 0 {
        band(ui, ROW_HEIGHT, |ui| {
            ui.label(
                egui::RichText::new("Nothing resolved yet")
                    .size(PATH_SIZE)
                    .color(palette.text_muted),
            );
        });
    } else {
        file_scroll(ui, "git_resolved_files", |ui| {
            for entry in resolved {
                row_separator(ui, palette);
                resolved_row(ui, palette, entry);
            }
        });
    }
}

/// Read-only resolved row: the status row painter with no interaction, just an
/// accessible label so the count and paths are visible to tests.
fn resolved_row(ui: &mut egui::Ui, palette: &Palette, entry: &FileEntry) {
    let row = file_list::file_row(
        ui,
        palette,
        egui::Sense::hover(),
        &file_list::FileRow {
            path: &entry.path,
            kind: entry.kind,
            additions: entry.additions,
            deletions: entry.deletions,
            selected: false,
            stats_hidden_on_hover: false,
            indent: 0.0,
            trailing_reserved: 0.0,
        },
    );
    row.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &entry.path));
}

fn conflict_section_title(palette: &Palette, title: &str, count: usize) -> egui::RichText {
    egui::RichText::new(format!("{title} ({count})"))
        .size(SECTION_TITLE_SIZE)
        .color(palette.text_primary)
}

/// Footer: **Continue** (accent, enabled only once no conflict remains) and
/// **Abort** (danger) side by side, each taking half the width. Both carry the
/// op's noun (conflicts.md §2). Abort opens a confirmation modal via its intent;
/// Continue runs the sequencer continuation.
fn conflict_footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    verb: &str,
    conflicts_left: usize,
    intents: &mut Vec<GitIntent>,
) {
    let noun = op_noun(verb);
    ui.add_space(12.0);
    let gap = 8.0;
    let button_w = ((ui.available_width() - gap) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if footer_button(
            ui,
            palette,
            &format!("Continue {noun}"),
            button_w,
            conflicts_left == 0,
            palette.primary_button_fill(),
        ) {
            intents.push(GitIntent::ContinueOp);
        }
        if footer_button(
            ui,
            palette,
            &format!("Abort {noun}"),
            button_w,
            true,
            palette.git_deleted,
        ) {
            intents.push(GitIntent::AbortOp);
        }
    });
}

/// Filled action button of the conflict footer; disabled ⇒ greyed and inert. The
/// accessibility label is the full text ("Continue Merge" / "Abort Merge").
fn footer_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    width: f32,
    enabled: bool,
    fill: egui::Color32,
) -> bool {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(width, COMMIT_BUTTON_H), enabled);
    let (fill, text) = if !enabled {
        (palette.state_disabled, palette.text_muted)
    } else if hovered {
        (crate::ui::with_alpha(fill, 220), egui::Color32::WHITE)
    } else {
        (fill, egui::Color32::WHITE)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(COMMIT_BUTTON_RADIUS), fill);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(PILL_SIZE + 1.0),
        text,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    enabled && response.clicked()
}

/// Noun naming the op for the footer labels, from the [`OpSummary`] verb.
fn op_noun(verb: &str) -> &'static str {
    match verb {
        "Rebasing" => "Rebase",
        "Cherry-picking" => "Cherry-pick",
        "Reverting" => "Revert",
        _ => "Merge",
    }
}

/// Confirmation modal for the conflict panel's **Abort** (git.md §10): the merge or
/// rebase stops and the branch returns to its pre-op state — conflict
/// resolutions in progress are discarded. Same outcome contract as the Delete
/// modals (red button ⇒ confirm, Cancel/Esc ⇒ dismiss), arbitrated by the
/// caller.
pub fn abort_op_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    out: &mut crate::ui::repo_sidebar::DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("abort_op_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            ui.label(egui::RichText::new("Abort the merge/rebase in progress?").strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "The branch returns to its previous state; conflict resolutions \
                 in progress are discarded.",
                )
                .color(palette.text_secondary),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(crate::ui::danger_button(palette, "Abort")).clicked() {
                        out.confirm = true;
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                out.confirm = true;
            }
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Confirmation before discarding a single unstaged hunk from the diff view
/// (git.md §4): reverting the working tree cannot be undone, so it follows the
/// Delete/Abort modal contract (red button ⇒ confirm, Cancel/Esc ⇒ dismiss).
pub fn discard_hunk_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    out: &mut crate::ui::repo_sidebar::DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("discard_hunk_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            ui.label(egui::RichText::new("Discard this hunk?").strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "The hunk's working-tree changes are reverted to the index. \
                 This cannot be undone.",
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
                        .add(crate::ui::danger_button(palette, "Discard"))
                        .clicked()
                    {
                        out.confirm = true;
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                out.confirm = true;
            }
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Loader while the first status snapshot has not arrived (spawn, repo
/// switch): a centered spinner — the default `RepoStatus` would otherwise read
/// as a clean tree ("Nothing to commit") before anything was scanned.
fn status_loading_placeholder(ui: &mut egui::Ui, palette: &Palette) {
    ui.vertical_centered(|ui| {
        ui.add_space((remaining_height(ui) / 2.0 - ROW_HEIGHT).max(0.0));
        ui.add(
            Spinner::new()
                .size(STATUS_SPINNER_SIZE)
                .color(palette.text_muted),
        )
        .widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ProgressIndicator,
                true,
                STATUS_LOADING_LABEL,
            )
        });
    });
}

/// No repository at all (first launch, last repo removed).
pub(crate) fn no_repo(ui: &mut egui::Ui, palette: &Palette, state: &mut GitPanelState) {
    state.selected_file = None;
    state.file_nav_active = false;
    state.marked_files.clear();
    state.selection_anchor = None;
    state.pending_stash = None;
    ui.add_space(SECTION_TOP_MARGIN);
    ui.label(
        egui::RichText::new(NO_REPO_LABEL)
            .size(PATH_SIZE)
            .color(palette.text_muted),
    );
}

/// Fixed-height zone, borderless (the sidebar edges form the frame).
fn card(ui: &mut egui::Ui, height: f32, contents: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(ui.available_width(), height.max(0.0));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect);
    contents(&mut child);
}

/// Full-width horizontal band, content vertically centered.
fn band(ui: &mut egui::Ui, height: f32, contents: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    contents(&mut child);
}

fn card_divider(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

fn header_band(
    ui: &mut egui::Ui,
    palette: &Palette,
    branch: &str,
    status: &RepoStatus,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    view: FileViewMode,
) {
    let can_discard = status
        .unstaged
        .iter()
        .any(|e| e.kind != ChangeKind::Conflicted);
    band(ui, HEADER_BAND_H, |ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        crate::ui::paint_icon(
            ui.painter(),
            icon_rect.center(),
            ICON_GLYPH,
            lucide_icons::Icon::GitBranch,
            palette.text_primary,
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Git")
                .size(TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(4.0);
        // Reserve the right-side actions (view toggle + refresh + discard, plus
        // the implicit item_spacing between them) so a long branch name truncates
        // instead of covering them.
        let actions_w = 3.0 * ICON_HIT + 4.0 + ui.spacing().item_spacing.x + ACTION_GAP;
        branch_chip(ui, palette, branch, ui.available_width() - actions_w);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.mutation_busy {
                // Same footprint as the icon button so the band doesn't shift;
                // refreshing during a mutation would be a no-op anyway.
                refresh_spinner(ui, palette);
            } else if icon_button(
                ui,
                palette,
                palette.accent,
                true,
                "Refresh",
                lucide_icons::Icon::RefreshCw,
            ) {
                intents.push(GitIntent::Refresh);
            }
            ui.add_space(2.0);
            if icon_button(
                ui,
                palette,
                palette.git_deleted,
                can_discard && !state.lock_busy,
                "Discard all",
                lucide_icons::Icon::Trash2,
            ) && state.pending_discard.is_none()
            {
                state.pending_discard = Some(DiscardTarget::All);
            }
            ui.add_space(2.0);
            if let Some(target) = file_list::view_toggle(ui, palette, view) {
                intents.push(GitIntent::SetFileView(target));
            }
        });
    });
}

fn summary_band(ui: &mut egui::Ui, palette: &Palette, status: &RepoStatus) {
    let files = status.changed_file_count();
    let (additions, deletions) = status.total_line_stats();
    band(ui, SUMMARY_BAND_H, |ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        crate::ui::paint_icon(
            ui.painter(),
            icon_rect.center(),
            14.0,
            lucide_icons::Icon::FileDiff,
            palette.text_secondary,
        );
        ui.add_space(3.0);
        ui.label(
            egui::RichText::new(files_changed_label(files))
                .size(SUMMARY_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ratio_bar(ui, palette, additions, deletions);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("−{deletions}"))
                    .size(SUMMARY_SIZE)
                    .color(palette.git_deleted),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("+{additions}"))
                    .size(SUMMARY_SIZE)
                    .color(palette.git_added),
            );
        });
    });
}

pub fn files_changed_label(count: usize) -> String {
    if count == 1 {
        "1 file changed".to_owned()
    } else {
        format!("{count} files changed")
    }
}

/// Widths (green, red) of the additions/deletions ratio bar. Each non-zero side
/// keeps a readable minimum width; a single side ⇒ full bar.
pub fn ratio_bar_widths(
    additions: usize,
    deletions: usize,
    total_w: f32,
    gap: f32,
    min_w: f32,
) -> (f32, f32) {
    if additions == 0 && deletions == 0 {
        return (0.0, 0.0);
    }
    if deletions == 0 {
        return (total_w, 0.0);
    }
    if additions == 0 {
        return (0.0, total_w);
    }
    let usable = total_w - gap;
    let green = usable * additions as f32 / (additions + deletions) as f32;
    let green = green.clamp(min_w, usable - min_w);
    (green, usable - green)
}

pub(crate) fn ratio_bar(ui: &mut egui::Ui, palette: &Palette, additions: usize, deletions: usize) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(RATIO_BAR_W, RATIO_BAR_H), egui::Sense::hover());
    let radius = egui::CornerRadius::same((RATIO_BAR_H / 2.0) as u8);
    let (green_w, red_w) = ratio_bar_widths(
        additions,
        deletions,
        RATIO_BAR_W,
        RATIO_BAR_GAP,
        RATIO_BAR_MIN,
    );
    let painter = ui.painter();
    if green_w <= 0.0 && red_w <= 0.0 {
        painter.rect_filled(rect, radius, palette.bg_surface);
    } else {
        if green_w > 0.0 {
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(green_w, RATIO_BAR_H)),
                radius,
                palette.git_added,
            );
        }
        if red_w > 0.0 {
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(rect.right() - red_w, rect.top()), rect.max),
                radius,
                palette.git_deleted,
            );
        }
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "diff ratio"));
}

/// Two **fixed-height** blocks (same height even with 0 entries — user
/// feedback), each with its own internal scroll if the list overflows.
fn file_sections(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
    view: FileViewMode,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(2.0);
    let half = ((remaining_height(ui) - SECTION_GAP) / 2.0).max(0.0);
    card(ui, half, |ui| {
        unstaged_section(ui, palette, status, state, intents, menu, view);
    });
    ui.add_space(SECTION_GAP);
    card(ui, half, |ui| {
        staged_section(ui, palette, status, state, intents, menu, view);
    });
}

/// Scrollable row list bounded to the block's remaining height.
fn file_scroll(ui: &mut egui::Ui, id: &'static str, contents: impl FnOnce(&mut egui::Ui)) {
    let body_h = remaining_height(ui);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(body_h)
        .min_scrolled_height(body_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Rows flush together (the breathing room lives in ROW_HEIGHT): the
            // hover highlight fills the whole line, with no dead band at the separators.
            ui.spacing_mut().item_spacing.y = 0.0;
            contents(ui);
        });
}

fn unstaged_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
    view: FileViewMode,
) {
    let count = status.unstaged.len();
    let mut stage_all = false;
    let mut toggled = false;
    band(ui, SECTION_HEADER_H, |ui| {
        toggled = section_toggle(ui, palette, "Unstaged", count, state.unstaged_collapsed);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            stage_all = intent_pill(
                ui,
                palette,
                "Stage All",
                palette.git_added,
                count > 0 && !state.lock_busy,
            );
        });
    });
    if toggled {
        state.unstaged_collapsed = !state.unstaged_collapsed;
    }
    if !state.unstaged_collapsed {
        if status.unstaged.is_empty() && status.staged.is_empty() {
            band(ui, ROW_HEIGHT, |ui| {
                ui.label(
                    egui::RichText::new(CLEAN_LABEL)
                        .size(PATH_SIZE)
                        .color(palette.text_muted),
                );
            });
        } else {
            file_scroll(ui, "git_unstaged_files", |ui| {
                render_files(
                    ui,
                    palette,
                    status,
                    &status.unstaged,
                    RowSection::Unstaged,
                    state,
                    intents,
                    menu,
                    view,
                );
            });
        }
    }
    if stage_all {
        intents.push(GitIntent::StageAll);
    }
}

fn staged_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
    view: FileViewMode,
) {
    let count = status.staged.len();
    let mut unstage_all = false;
    let mut toggled = false;
    band(ui, SECTION_HEADER_H, |ui| {
        toggled = section_toggle(ui, palette, "Staged", count, state.staged_collapsed);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            unstage_all = intent_pill(
                ui,
                palette,
                "Unstage All",
                palette.git_deleted,
                count > 0 && !state.lock_busy,
            );
        });
    });
    if toggled {
        state.staged_collapsed = !state.staged_collapsed;
    }
    if !state.staged_collapsed && !status.staged.is_empty() {
        file_scroll(ui, "git_staged_files", |ui| {
            render_files(
                ui,
                palette,
                status,
                &status.staged,
                RowSection::Staged,
                state,
                intents,
                menu,
                view,
            );
        });
    }
    if unstage_all {
        intents.push(GitIntent::UnstageAll);
    }
}

/// Renders a section's rows in the requested view (M40). Flat keeps the
/// historical per-row separators; Tree groups by directory — collapsible
/// [`file_list::dir_row`]s and indented leaves — dropping the separators (the
/// indentation carries the grouping). The directory rows toggle the section's
/// collapsed set; the leaves display the bare filename while keeping the full
/// path for selection / accessibility.
#[allow(clippy::too_many_arguments)]
fn render_files(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    entries: &[FileEntry],
    section: RowSection,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
    view: FileViewMode,
) {
    match view {
        FileViewMode::Flat => {
            for entry in entries {
                row_separator(ui, palette);
                file_row(
                    ui,
                    palette,
                    status,
                    entry,
                    section,
                    0.0,
                    &entry.path,
                    state,
                    intents,
                    menu,
                    view,
                );
            }
        }
        FileViewMode::Tree => {
            let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
            let rows = {
                let collapsed = section_collapsed_dirs(state, section);
                file_tree::tree_rows(&paths, collapsed)
            };
            let mut toggle: Option<String> = None;
            for row in rows {
                match row {
                    TreeRow::Dir {
                        name,
                        full_path,
                        depth,
                        collapsed,
                    } => {
                        let indent = depth as f32 * file_list::TREE_INDENT_STEP;
                        if file_list::dir_row(ui, palette, &name, indent, collapsed).clicked() {
                            toggle = Some(full_path);
                        }
                    }
                    TreeRow::File { index, depth } => {
                        let entry = &entries[index];
                        let indent = depth as f32 * file_list::TREE_INDENT_STEP;
                        file_row(
                            ui,
                            palette,
                            status,
                            entry,
                            section,
                            indent,
                            leaf_name(&entry.path),
                            state,
                            intents,
                            menu,
                            view,
                        );
                    }
                }
            }
            if let Some(dir) = toggle {
                let set = section_collapsed_dirs_mut(state, section);
                if !set.remove(&dir) {
                    set.insert(dir);
                }
            }
        }
    }
}

fn section_collapsed_dirs(state: &GitPanelState, section: RowSection) -> &HashSet<String> {
    match section {
        RowSection::Unstaged => &state.unstaged_collapsed_dirs,
        RowSection::Staged => &state.staged_collapsed_dirs,
    }
}

fn section_collapsed_dirs_mut(
    state: &mut GitPanelState,
    section: RowSection,
) -> &mut HashSet<String> {
    match section {
        RowSection::Unstaged => &mut state.unstaged_collapsed_dirs,
        RowSection::Staged => &mut state.staged_collapsed_dirs,
    }
}

fn leaf_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn remaining_height(ui: &egui::Ui) -> f32 {
    let bottom = ui
        .clip_rect()
        .bottom()
        .min(ui.ctx().content_rect().bottom());
    (bottom - ui.next_widget_position().y).max(0.0)
}

#[allow(clippy::too_many_arguments)]
fn commit_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    op_in_progress: bool,
    state: &mut GitPanelState,
    keymap: &Keymap,
    intents: &mut Vec<GitIntent>,
) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(
            egui::RichText::new("Commit message")
                .size(LABEL_SIZE)
                .color(palette.text_primary),
        );
        ui.label(
            egui::RichText::new("*")
                .size(LABEL_SIZE)
                .color(palette.git_deleted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Same condition as commit: the prompt only analyzes the staged changes.
            let can_generate = !status.staged.is_empty();
            if ai_button(ui, palette, can_generate, state.ai_busy) {
                intents.push(GitIntent::GenerateMessage);
            }
        });
    });

    let subject_len = state.subject.chars().count();
    let subject_counter = counter_text(palette, subject_len, SUBJECT_SOFT_LIMIT);
    let mut subject_clicked = false;
    input_frame(palette).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let edit_w = (ui.available_width() - COUNTER_RESERVE).max(8.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.subject)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(edit_w)
                    .font(egui::FontId::proportional(COMMIT_INPUT_SIZE))
                    .hint_text(
                        egui::RichText::new(SUBJECT_HINT)
                            .size(COMMIT_INPUT_SIZE)
                            .color(palette.text_muted),
                    ),
            );
            subject_clicked = response.clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(subject_counter);
            });
        });
    });

    ui.label(
        egui::RichText::new("Description (optional)")
            .size(LABEL_SIZE)
            .color(palette.text_primary),
    );
    let description_len = state.description.chars().count();
    let description_counter = counter_text(palette, description_len, DESCRIPTION_SOFT_LIMIT);
    let mut description_clicked = false;
    input_frame(palette).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // A multiline TextEdit grows with its content; left unbounded, extra
        // lines push the commit button past the card's fixed-height clip. Cap
        // the editor at DESCRIPTION_ROWS and scroll beyond that. `min_scrolled_height`
        // (64px by default) would otherwise keep the viewport ~4 rows tall once it
        // overflows, so pin it to the same height as the cap.
        let input_font = egui::FontId::proportional(COMMIT_INPUT_SIZE);
        let row_h = ui
            .painter()
            .layout_no_wrap(
                "A".to_owned(),
                input_font.clone(),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .y;
        let view_h = row_h * DESCRIPTION_ROWS as f32;
        let mut response = None;
        egui::ScrollArea::vertical()
            .id_salt("commit_description")
            .max_height(view_h)
            .min_scrolled_height(view_h)
            .show(ui, |ui| {
                response = Some(
                    ui.add(
                        egui::TextEdit::multiline(&mut state.description)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .desired_rows(DESCRIPTION_ROWS)
                            .desired_width(f32::INFINITY)
                            .font(input_font.clone())
                            .hint_text(
                                egui::RichText::new(DESCRIPTION_HINT)
                                    .size(COMMIT_INPUT_SIZE)
                                    .color(palette.text_muted),
                            ),
                    ),
                );
            });
        description_clicked = response.is_some_and(|r| r.clicked());
        // Bounded row: a bare `with_layout` would spread over the card's whole
        // remaining height and push the button out of the clip.
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), COUNTER_SIZE + 3.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(description_counter);
            },
        );
    });
    if subject_clicked || description_clicked {
        state.file_nav_active = false;
    }

    ui.add_space(2.0);
    let can_commit = !op_in_progress
        && !state.commit_busy
        && !state.lock_busy
        && commit_enabled(status, &state.subject);
    let clicked = commit_button(
        ui,
        palette,
        &commit_button_label(status.staged.len()),
        can_commit,
        state.commit_busy,
    );
    let shortcut = can_commit && !state.inline_editing && commit_shortcut_pressed(ui, keymap);
    if clicked || shortcut {
        let message = commit_message(&state.subject, &state.description);
        intents.push(GitIntent::Commit(message));
    }
}

pub fn commit_button_label(staged_count: usize) -> String {
    match staged_count {
        0 => "Commit".to_owned(),
        1 => "Commit 1 file".to_owned(),
        n => format!("Commit {n} files"),
    }
}

/// Full-width primary button of the commit card, painted to the mockup
/// (nearly-square corners, git-branch icon + centered label); the accessibility
/// label stays the label alone. Busy ⇒ spinner in place of the icon, click
/// ignored (a commit is being written).
fn commit_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    enabled: bool,
    busy: bool,
) -> bool {
    let (rect, response, hovered) = crate::ui::clickable(
        ui,
        egui::vec2(ui.available_width(), COMMIT_BUTTON_H),
        enabled,
    );
    let fill = if !enabled {
        palette.state_disabled
    } else if hovered {
        palette.primary_button_hover()
    } else {
        palette.primary_button_fill()
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(PILL_SIZE + 1.0),
        egui::Color32::WHITE,
    );
    let gap = 6.0;
    let content_w = COMMIT_BUTTON_ICON + gap + galley.size().x;
    let left = rect.center().x - content_w / 2.0;
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(COMMIT_BUTTON_RADIUS), fill);
    let icon_center = egui::pos2(left + COMMIT_BUTTON_ICON / 2.0, rect.center().y);
    if busy {
        Spinner::new()
            .size(COMMIT_BUTTON_ICON)
            .color(egui::Color32::WHITE)
            .paint_at(
                ui,
                egui::Rect::from_center_size(
                    icon_center,
                    egui::vec2(COMMIT_BUTTON_ICON, COMMIT_BUTTON_ICON),
                ),
            );
    } else {
        crate::ui::paint_icon(
            ui.painter(),
            icon_center,
            COMMIT_BUTTON_ICON,
            lucide_icons::Icon::GitBranch,
            egui::Color32::WHITE,
        );
    }
    let painter = ui.painter();
    painter.galley(
        egui::pos2(
            left + COMMIT_BUTTON_ICON + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        egui::Color32::WHITE,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    enabled && response.clicked()
}

const AI_BUTTON_HIT: f32 = 18.0;
const AI_ICON: f32 = 13.0;
const AI_BUTTON_LABEL: &str = "Generate commit message";

/// AI button of the commit card (sparkles icon): fills the inputs without ever
/// committing. Busy ⇒ spinner, click ignored; compact (18pt) so it doesn't raise
/// the label's row.
fn ai_button(ui: &mut egui::Ui, palette: &Palette, enabled: bool, busy: bool) -> bool {
    let clickable = enabled && !busy;
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(AI_BUTTON_HIT, AI_BUTTON_HIT), clickable);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.bg_surface_hover,
        );
    }
    let color = if busy {
        palette.text_muted
    } else if !enabled {
        palette.state_disabled
    } else if hovered {
        palette.accent
    } else {
        palette.text_muted
    };
    if busy {
        Spinner::new().size(AI_ICON).color(color).paint_at(
            ui,
            egui::Rect::from_center_size(rect.center(), egui::vec2(AI_ICON, AI_ICON)),
        );
    } else {
        crate::ui::paint_icon(
            ui.painter(),
            rect.center(),
            AI_ICON,
            lucide_icons::Icon::Sparkles,
            color,
        );
    }
    let response = response.on_hover_text(AI_BUTTON_LABEL);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, clickable, AI_BUTTON_LABEL)
    });
    clickable && response.clicked()
}

fn input_frame(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.bg_canvas)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(INPUT_RADIUS))
        .inner_margin(egui::Margin::symmetric(INPUT_PAD_X, INPUT_PAD_Y))
}

/// `n / limit` counter embedded in the field; turns the conflict color beyond
/// the limit (indicative, never blocking).
fn counter_text(palette: &Palette, len: usize, limit: usize) -> egui::RichText {
    egui::RichText::new(format!("{len} / {limit}"))
        .size(COUNTER_SIZE)
        .color(if len > limit {
            palette.git_conflict
        } else {
            palette.text_muted
        })
}

fn branch_chip(ui: &mut egui::Ui, palette: &Palette, branch: &str, max_width: f32) {
    if branch.is_empty() {
        return;
    }
    let font = egui::FontId::monospace(BRANCH_SIZE - 1.0);
    let mut job = egui::text::LayoutJob::single_section(
        branch.to_owned(),
        egui::text::TextFormat::simple(font, egui::Color32::PLACEHOLDER),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_width - 12.0).max(0.0));
    let galley = ui.painter().layout_job(job);
    let size = galley.size() + egui::vec2(12.0, 5.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        palette.bg_surface,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, branch));
}

fn discard_confirm(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
) {
    let Some(target) = state.pending_discard.clone() else {
        return;
    };
    let body = match &target {
        DiscardTarget::All => "Discard all unstaged changes? This cannot be undone.".to_owned(),
        DiscardTarget::File(path) => {
            format!("Discard changes to “{path}”? This cannot be undone.")
        }
        DiscardTarget::Files(paths) => format!(
            "Discard changes to {} files? This cannot be undone.",
            paths.len()
        ),
    };

    let mut decided: Option<bool> = None;
    let modal = egui::Modal::new(egui::Id::new("git_discard_confirm"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(260.0);
            ui.label(egui::RichText::new("Discard changes?").strong());
            ui.add_space(4.0);
            ui.label(egui::RichText::new(body).color(palette.text_secondary));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    decided = Some(false);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(crate::ui::danger_button(palette, "Discard"))
                        .clicked()
                    {
                        decided = Some(true);
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                decided = Some(true);
            }
        });
    if modal.should_close() {
        decided = decided.or(Some(false));
    }

    match decided {
        Some(true) => {
            match target {
                DiscardTarget::All => intents.push(GitIntent::DiscardAll),
                DiscardTarget::File(path) => intents.push(GitIntent::Discard(path)),
                DiscardTarget::Files(paths) => {
                    for path in paths {
                        intents.push(GitIntent::Discard(path));
                    }
                }
            }
            state.pending_discard = None;
        }
        Some(false) => state.pending_discard = None,
        None => {}
    }
}

/// Confirmation for the context-menu **Stash** entry: the whole file is stashed
/// (staged **and** unstaged), never a partial stash (git.md §3) — the body spells
/// that out so the action is not mistaken for an index-only stash.
fn stash_confirm(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
) {
    let Some(paths) = state.pending_stash.clone() else {
        return;
    };
    let body = if paths.len() == 1 {
        format!(
            "Stash all changes to “{}”? Staged and unstaged changes are stashed together — not a partial stash.",
            paths[0]
        )
    } else {
        format!(
            "Stash all changes to {} files? Staged and unstaged changes are stashed together — not a partial stash.",
            paths.len()
        )
    };

    let mut decided: Option<bool> = None;
    let modal = egui::Modal::new(egui::Id::new("git_stash_confirm"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(260.0);
            ui.label(egui::RichText::new("Stash changes?").strong());
            ui.add_space(4.0);
            ui.label(egui::RichText::new(body).color(palette.text_secondary));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    decided = Some(false);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Stash").clicked() {
                        decided = Some(true);
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                decided = Some(true);
            }
        });
    if modal.should_close() {
        decided = decided.or(Some(false));
    }

    match decided {
        Some(true) => {
            intents.push(GitIntent::StashFiles(paths));
            state.pending_stash = None;
        }
        Some(false) => state.pending_stash = None,
        None => {}
    }
}

/// Right-click menu of a WIP file row. One selected file shows the contextual
/// stage/unstage + discard + stash actions over the shared clipboard entries;
/// a multi-selection shows only the batch actions (git.md §3). `staged` is the
/// clicked row's section.
fn wip_file_context_menu(
    response: &egui::Response,
    rel_path: &str,
    staged: bool,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
) {
    let marked = state.marked_files.clone();
    egui::Popup::context_menu(response)
        .style(crate::theme::menu_style)
        .show(|ui| {
            if marked.len() >= 2 {
                let unstaged: Vec<String> = marked
                    .iter()
                    .filter(|f| !f.staged)
                    .map(|f| f.path.clone())
                    .collect();
                let to_unstage: Vec<String> = marked
                    .iter()
                    .filter(|f| f.staged)
                    .map(|f| f.path.clone())
                    .collect();
                let mut all: Vec<String> = marked.iter().map(|f| f.path.clone()).collect();
                all.sort();
                all.dedup();
                if !unstaged.is_empty() && mutation_entry(ui, state, "Stage").clicked() {
                    for path in &unstaged {
                        intents.push(GitIntent::Stage(path.clone()));
                    }
                    ui.close();
                }
                if !to_unstage.is_empty() && mutation_entry(ui, state, "Unstage").clicked() {
                    for path in &to_unstage {
                        intents.push(GitIntent::Unstage(path.clone()));
                    }
                    ui.close();
                }
                if !unstaged.is_empty() && mutation_entry(ui, state, "Discard").clicked() {
                    state.pending_discard = Some(DiscardTarget::Files(unstaged.clone()));
                    ui.close();
                }
                if mutation_entry(ui, state, "Stash").clicked() {
                    state.pending_stash = Some(all.clone());
                    ui.close();
                }
            } else {
                if staged {
                    if mutation_entry(ui, state, "Unstage").clicked() {
                        intents.push(GitIntent::Unstage(rel_path.to_owned()));
                        ui.close();
                    }
                } else {
                    if mutation_entry(ui, state, "Stage").clicked() {
                        intents.push(GitIntent::Stage(rel_path.to_owned()));
                        ui.close();
                    }
                    if mutation_entry(ui, state, "Discard").clicked() {
                        state.pending_discard = Some(DiscardTarget::File(rel_path.to_owned()));
                        ui.close();
                    }
                }
                if mutation_entry(ui, state, "Stash").clicked() {
                    state.pending_stash = Some(vec![rel_path.to_owned()]);
                    ui.close();
                }
                ui.separator();
                file_menu_entries(ui, rel_path, menu);
            }
        });
}

/// A context-menu entry that writes to the repository. It follows the same
/// `lock_busy` gate as the row's inline pills: while a long operation holds the
/// mutation lock the worker refuses the write anyway, so offering it would only
/// arm a confirmation for a command that cannot run.
fn mutation_entry(ui: &mut egui::Ui, state: &GitPanelState, label: &str) -> egui::Response {
    ui.add_enabled(!state.lock_busy, egui::Button::new(label))
}

/// The Commit binding (keybindings §3, `Cmd+Enter` by default) — equivalent to
/// the button.
fn commit_shortcut_pressed(ui: &egui::Ui, keymap: &Keymap) -> bool {
    ui.input(|i| {
        i.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if keymap.matches(Action::Commit, *key, *modifiers)
            )
        })
    })
}

fn file_nav_pressed(ui: &egui::Ui, state: &GitPanelState) -> Option<FileNav> {
    if state.selected_file.is_none() || !state.file_nav_active || state.inline_editing {
        return None;
    }
    ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if no_modifiers(*modifiers) => match key {
                egui::Key::ArrowUp => Some(FileNav::Previous),
                egui::Key::ArrowDown => Some(FileNav::Next),
                _ => None,
            },
            _ => None,
        })
    })
}

fn no_modifiers(modifiers: egui::Modifiers) -> bool {
    !modifiers.command
        && !modifiers.mac_cmd
        && !modifiers.alt
        && !modifiers.ctrl
        && !modifiers.shift
}

fn navigate_selected_file(
    ui: &egui::Ui,
    status: &RepoStatus,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    nav: FileNav,
    view: FileViewMode,
) {
    let files = visible_files(status, state, view);
    if files.is_empty() {
        return;
    }
    let current = state
        .selected_file
        .as_ref()
        .and_then(|selected| files.iter().position(|file| file == selected));
    let target = match (current, nav) {
        (Some(0), FileNav::Previous) | (None, FileNav::Previous) => files.len() - 1,
        (Some(index), FileNav::Previous) => index - 1,
        (Some(index), FileNav::Next) => (index + 1) % files.len(),
        (None, FileNav::Next) => 0,
    };
    let selected = files[target].clone();
    ui.memory_mut(|memory| {
        memory.request_focus(file_selection_id(selected.staged, &selected.path))
    });
    // Only keyboard navigation follows the selection into the viewport — never
    // a click (a clicked row is already visible).
    file_list::request_row_scroll(ui, file_scroll_id(), selected.clone());
    state.selected_file = Some(selected.clone());
    state.file_nav_active = true;
    state.marked_files = vec![selected.clone()];
    state.selection_anchor = Some(selected.clone());
    intents.push(GitIntent::OpenDiff {
        path: selected.path,
        staged: selected.staged,
    });
}

/// Cmd+click: add/remove the file from the multi-selection.
fn toggle_marked(state: &mut GitPanelState, sel: &GitFileSelection) {
    if let Some(pos) = state.marked_files.iter().position(|f| f == sel) {
        state.marked_files.remove(pos);
    } else {
        state.marked_files.push(sel.clone());
    }
}

/// Shift+click: select every visible row between the anchor (last plain/Cmd
/// click, or the open-diff cursor) and the target, inclusive.
fn range_select(
    status: &RepoStatus,
    state: &mut GitPanelState,
    view: FileViewMode,
    target: &GitFileSelection,
) {
    let files = visible_files(status, state, view);
    let anchor = state
        .selection_anchor
        .as_ref()
        .or(state.selected_file.as_ref())
        .and_then(|a| files.iter().position(|f| f == a));
    let target_index = files.iter().position(|f| f == target);
    match (anchor, target_index) {
        (Some(a), Some(t)) => {
            let (lo, hi) = if a <= t { (a, t) } else { (t, a) };
            state.marked_files = files[lo..=hi].to_vec();
        }
        _ => state.marked_files = vec![target.clone()],
    }
}

/// ↑/↓ navigation order: the openable rows in **display order**. Tree mode
/// follows the visible tree (collapsed directories hide their files) so the
/// arrows match what the eye sees.
fn visible_files(
    status: &RepoStatus,
    state: &GitPanelState,
    view: FileViewMode,
) -> Vec<GitFileSelection> {
    let mut files = Vec::with_capacity(status.unstaged.len() + status.staged.len());
    if !state.unstaged_collapsed {
        section_open_files(
            &status.unstaged,
            false,
            view,
            &state.unstaged_collapsed_dirs,
            &mut files,
        );
    }
    if !state.staged_collapsed {
        section_open_files(
            &status.staged,
            true,
            view,
            &state.staged_collapsed_dirs,
            &mut files,
        );
    }
    files
}

fn section_open_files(
    entries: &[FileEntry],
    staged: bool,
    view: FileViewMode,
    collapsed_dirs: &HashSet<String>,
    out: &mut Vec<GitFileSelection>,
) {
    match view {
        FileViewMode::Flat => out.extend(openable_files(entries, staged)),
        FileViewMode::Tree => {
            let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
            for row in file_tree::tree_rows(&paths, collapsed_dirs) {
                if let TreeRow::File { index, .. } = row {
                    let entry = &entries[index];
                    if entry.kind != ChangeKind::Conflicted {
                        out.push(GitFileSelection {
                            path: entry.path.clone(),
                            staged,
                        });
                    }
                }
            }
        }
    }
}

fn openable_files(
    entries: &[FileEntry],
    staged: bool,
) -> impl Iterator<Item = GitFileSelection> + '_ {
    entries
        .iter()
        .filter(|entry| entry.kind != ChangeKind::Conflicted)
        .map(move |entry| GitFileSelection {
            path: entry.path.clone(),
            staged,
        })
}

fn file_selection_id(staged: bool, path: &str) -> egui::Id {
    egui::Id::new(("git_file", staged, path))
}

fn file_scroll_id() -> egui::Id {
    egui::Id::new("git_selected_file_scroll")
}

pub fn commit_enabled(status: &RepoStatus, subject: &str) -> bool {
    !subject.trim().is_empty() && !status.staged.is_empty()
}

/// Composes the git message: subject + body separated by a blank line (git convention).
pub fn commit_message(subject: &str, description: &str) -> String {
    let subject = subject.trim();
    let description = description.trim();
    if description.is_empty() {
        subject.to_owned()
    } else {
        format!("{subject}\n\n{description}")
    }
}

fn section_toggle(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    count: usize,
    collapsed: bool,
) -> bool {
    let chevron = if collapsed {
        lucide_icons::Icon::ChevronRight
    } else {
        lucide_icons::Icon::ChevronDown
    };
    let text = egui::RichText::new(format!("{}  {title} ({count})", chevron.unicode()))
        .size(SECTION_TITLE_SIZE)
        .color(palette.text_primary);
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

#[allow(clippy::too_many_arguments)]
fn file_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RepoStatus,
    entry: &FileEntry,
    section: RowSection,
    indent: f32,
    display: &str,
    state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    menu: &mut FileMenuCtx,
    view: FileViewMode,
) {
    let staged = matches!(section, RowSection::Staged);
    let sel = GitFileSelection {
        path: entry.path.clone(),
        staged,
    };
    // Highlighted when in the batch multi-selection or the open-diff cursor.
    let selected = state.marked_files.contains(&sel) || state.selected_file.as_ref() == Some(&sel);
    // Stats hidden on hover: the action pills take their place
    // (D-2026-06-03-git-sidebar-redesign).
    let row = file_list::file_row(
        ui,
        palette,
        egui::Sense::hover(),
        &file_list::FileRow {
            path: display,
            kind: entry.kind,
            additions: entry.additions,
            deletions: entry.deletions,
            selected,
            stats_hidden_on_hover: true,
            indent,
            trailing_reserved: 0.0,
        },
    );
    let (rect, hovered) = (row.rect, row.hovered);

    // The path (outside the action zone) opens the file's overlay diff (git.md §4);
    // a conflict opens the in-app conflict editor on that file (conflicts.md §3).
    let is_conflict = entry.kind == ChangeKind::Conflicted;
    if !is_conflict {
        let path_rect = egui::Rect::from_min_max(
            egui::pos2(row.path_left, rect.top()),
            egui::pos2(row.content_right, rect.bottom()),
        );
        let path_response = ui
            .interact(
                path_rect,
                file_selection_id(staged, &entry.path),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let modifiers = ui.input(|i| i.modifiers);
        if path_response.clicked() {
            if modifiers.command || modifiers.mac_cmd {
                toggle_marked(state, &sel);
                state.selection_anchor = Some(sel.clone());
            } else if modifiers.shift {
                range_select(status, state, view, &sel);
            } else {
                state.marked_files = vec![sel.clone()];
                state.selected_file = Some(sel.clone());
                state.file_nav_active = true;
                state.selection_anchor = Some(sel.clone());
                path_response.request_focus();
                intents.push(GitIntent::OpenDiff {
                    path: entry.path.clone(),
                    staged,
                });
            }
        }
        // Right-click on a row outside the current selection makes it the lone
        // target before the menu opens (Finder behaviour).
        if path_response.secondary_clicked() && !state.marked_files.contains(&sel) {
            state.marked_files = vec![sel.clone()];
            // The row the menu acts on is also where a following shift-click
            // ranges from — a stale anchor would select from another row.
            state.selection_anchor = Some(sel.clone());
        }
        if selected {
            file_list::consume_row_scroll(ui, &path_response, file_scroll_id(), &sel);
        }
        wip_file_context_menu(&path_response, &entry.path, staged, state, intents, menu);
        path_response
            .on_hover_text(&entry.path)
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &entry.path));
    } else {
        let path_rect = egui::Rect::from_min_max(
            egui::pos2(row.path_left, rect.top()),
            egui::pos2(row.content_right, rect.bottom()),
        );
        let conflict_response = ui
            .interact(
                path_rect,
                file_selection_id(staged, &entry.path),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Conflict — click to resolve");
        if conflict_response.clicked() {
            intents.push(GitIntent::OpenConflictEditor {
                focus: Some(entry.path.clone()),
            });
        }
        conflict_response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &entry.path));
    }

    if is_conflict {
        return;
    }

    // The actions overlay the path on hover (the path takes the full width at
    // rest); an opaque background, reserved before the pills then fitted to their
    // actual extent, masks the text beneath. The child is created even without
    // hover: `new_child` consumes an auto-id from the parent, so creating it
    // conditionally would shift the ids of the following rows between frames
    // (egui warning "widget changed id between passes", red rect in debug).
    let backdrop = ui.painter().add(egui::Shape::Noop);
    let action_rect = egui::Rect::from_min_max(
        egui::pos2(row.path_left, rect.top()),
        egui::pos2(row.content_right, rect.bottom()),
    );
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(action_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if !hovered {
        return;
    }
    match section {
        RowSection::Unstaged => {
            if intent_pill(
                &mut col,
                palette,
                "Stage",
                palette.git_added,
                !state.lock_busy,
            ) {
                intents.push(GitIntent::Stage(entry.path.clone()));
            }
            col.add_space(ACTION_GAP);
            if intent_pill(
                &mut col,
                palette,
                "Discard",
                palette.git_deleted,
                !state.lock_busy,
            ) && state.pending_discard.is_none()
            {
                state.pending_discard = Some(DiscardTarget::File(entry.path.clone()));
            }
        }
        RowSection::Staged => {
            if intent_pill(
                &mut col,
                palette,
                "Unstage",
                palette.git_deleted,
                !state.lock_busy,
            ) {
                intents.push(GitIntent::Unstage(entry.path.clone()));
            }
        }
    }
    let backdrop_rect = egui::Rect::from_min_max(
        egui::pos2(col.min_rect().left() - ACTION_GAP, rect.top()),
        rect.max,
    );
    let backdrop_fill = file_row_fill(palette, true, selected).unwrap_or(palette.bg_surface_hover);
    ui.painter().set(
        backdrop,
        egui::Shape::rect_filled(backdrop_rect, 0.0, backdrop_fill),
    );
}

/// Neutral button at rest, **tinted to the intent on hover**.
pub(crate) fn intent_pill(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    intent: egui::Color32,
    enabled: bool,
) -> bool {
    let font = egui::FontId::proportional(PILL_SIZE);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let size = galley.size() + egui::vec2(PILL_PAD_X * 2.0, PILL_PAD_Y * 2.0);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, enabled);
    let (fill, stroke, text) = if !enabled {
        (
            palette.bg_surface,
            palette.border_subtle,
            palette.state_disabled,
        )
    } else if hovered {
        (tint(intent), intent, intent)
    } else {
        (
            palette.bg_surface,
            palette.border_subtle,
            palette.text_secondary,
        )
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(PILL_RADIUS),
        fill,
        egui::Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    enabled && response.clicked()
}

/// Lucide icon button, tinted to the intent on hover. `tooltip` also serves as
/// the accessibility label (testable via `get_by_label`).
/// The Refresh slot while a mutation awaits its worker reply (stage-all,
/// discard, checkout…): a spinner where the icon was.
fn refresh_spinner(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ICON_HIT, ICON_HIT), egui::Sense::hover());
    Spinner::new()
        .size(ICON_GLYPH)
        .color(palette.text_muted)
        .paint_at(
            ui,
            egui::Rect::from_center_size(rect.center(), egui::vec2(ICON_GLYPH, ICON_GLYPH)),
        );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, "Working")
    });
}

fn icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    intent: egui::Color32,
    enabled: bool,
    tooltip: &str,
    icon: lucide_icons::Icon,
) -> bool {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(ICON_HIT, ICON_HIT), enabled);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.bg_surface_hover,
        );
    }
    let color = if !enabled {
        palette.state_disabled
    } else if hovered {
        intent
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), ICON_GLYPH, icon, color);
    let response = response.on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tooltip));
    enabled && response.clicked()
}

fn tint(color: egui::Color32) -> egui::Color32 {
    crate::ui::with_alpha(color, TINT_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::status::{ChangeKind, FileEntry};

    fn entry(path: &str, kind: ChangeKind) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            kind,
            additions: 0,
            deletions: 0,
        }
    }

    #[test]
    fn commit_disabled_without_message_or_staged() {
        let mut status = RepoStatus::default();
        assert!(
            !commit_enabled(&status, ""),
            "no message and nothing staged"
        );
        assert!(
            !commit_enabled(&status, "fix"),
            "a message but nothing staged"
        );
        status.staged.push(entry("a.txt", ChangeKind::Added));
        assert!(
            !commit_enabled(&status, "   "),
            "staged but a blank message"
        );
        assert!(
            commit_enabled(&status, "fix"),
            "a message and a staged file enables commit"
        );
    }

    #[test]
    fn commit_message_joins_subject_and_description() {
        assert_eq!(commit_message("  subj  ", ""), "subj");
        assert_eq!(commit_message("subj", "  "), "subj");
        assert_eq!(commit_message("subj", "body"), "subj\n\nbody");
    }

    #[test]
    fn commit_button_label_counts_staged_files() {
        assert_eq!(commit_button_label(0), "Commit");
        assert_eq!(commit_button_label(1), "Commit 1 file");
        assert_eq!(commit_button_label(15), "Commit 15 files");
    }

    #[test]
    fn files_changed_label_handles_singular_and_plural() {
        assert_eq!(files_changed_label(0), "0 files changed");
        assert_eq!(files_changed_label(1), "1 file changed");
        assert_eq!(files_changed_label(16), "16 files changed");
    }

    #[test]
    fn ratio_bar_widths_are_proportional_with_a_readable_minimum() {
        let (g, r) = ratio_bar_widths(0, 0, 56.0, 2.0, 6.0);
        assert_eq!((g, r), (0.0, 0.0), "no change renders an empty track");

        let (g, r) = ratio_bar_widths(10, 0, 56.0, 2.0, 6.0);
        assert_eq!((g, r), (56.0, 0.0), "additions only fill the whole bar");

        let (g, r) = ratio_bar_widths(0, 10, 56.0, 2.0, 6.0);
        assert_eq!((g, r), (0.0, 56.0), "deletions only fill the whole bar");

        let (g, r) = ratio_bar_widths(231, 48, 56.0, 2.0, 6.0);
        assert!(g > r, "more additions show a longer green segment");
        assert!(
            (g + r - 54.0).abs() < 0.01,
            "segments split total minus gap"
        );
        assert!(r >= 6.0, "the smaller side keeps a readable minimum");

        let (g, r) = ratio_bar_widths(10_000, 1, 56.0, 2.0, 6.0);
        assert_eq!(r, 6.0, "a tiny side is clamped to the minimum, not erased");
        assert!((g + r - 54.0).abs() < 0.01);
    }

    fn unstaged(paths: &[&str]) -> RepoStatus {
        RepoStatus {
            unstaged: paths
                .iter()
                .map(|p| entry(p, ChangeKind::Modified))
                .collect(),
            staged: vec![],
        }
    }

    fn sel(path: &str, staged: bool) -> GitFileSelection {
        GitFileSelection {
            path: path.to_owned(),
            staged,
        }
    }

    #[test]
    fn toggle_marked_adds_then_removes() {
        let mut state = GitPanelState::default();
        toggle_marked(&mut state, &sel("a.txt", false));
        assert_eq!(state.marked_files, vec![sel("a.txt", false)]);
        toggle_marked(&mut state, &sel("a.txt", false));
        assert!(state.marked_files.is_empty());
    }

    #[test]
    fn range_select_spans_anchor_to_target_inclusive() {
        let status = unstaged(&["a.txt", "b.txt", "c.txt"]);
        let mut state = GitPanelState {
            selection_anchor: Some(sel("a.txt", false)),
            ..Default::default()
        };
        range_select(
            &status,
            &mut state,
            FileViewMode::Flat,
            &sel("c.txt", false),
        );
        let paths: Vec<&str> = state.marked_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt", "c.txt"]);
    }

    #[test]
    fn range_select_is_order_independent() {
        let status = unstaged(&["a.txt", "b.txt", "c.txt"]);
        let mut state = GitPanelState {
            selection_anchor: Some(sel("c.txt", false)),
            ..Default::default()
        };
        range_select(
            &status,
            &mut state,
            FileViewMode::Flat,
            &sel("a.txt", false),
        );
        let paths: Vec<&str> = state.marked_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt", "c.txt"], "anchor below target");
    }

    #[test]
    fn range_select_without_anchor_falls_back_to_the_target() {
        let status = unstaged(&["a.txt", "b.txt"]);
        let mut state = GitPanelState::default();
        range_select(
            &status,
            &mut state,
            FileViewMode::Flat,
            &sel("b.txt", false),
        );
        assert_eq!(state.marked_files, vec![sel("b.txt", false)]);
    }
}
