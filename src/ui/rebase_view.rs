//! Interactive-rebase page (git.md §9): replaces the graph in the central area
//! while the plan is being prepared. One row per commit to replay — newest on
//! top like the graph — with the target state (Pick / Reword / Squash / Fixup /
//! Drop) and the Reword message editor. Pure rendering: the caller owns the
//! [`RebasePage`] state and arbitrates the emitted [`RebasePageAction`].

use crate::git::rebase::{plan_error, RebaseAction, RebaseChoice, RebaseCommit, RebaseStep};
use crate::theme::Palette;
use crate::ui::spinner::Spinner;

const TITLE_SIZE: f32 = 16.0;
const HINT_SIZE: f32 = 12.0;
const ROW_TEXT_SIZE: f32 = 13.0;
const SHA_SIZE: f32 = 12.0;
const FOOTER_TEXT_SIZE: f32 = 12.0;
const ACTION_COMBO_W: f32 = 92.0;
const REWORD_ROWS: usize = 3;
const PAD: f32 = 12.0;
const LOADING_LABEL: &str = "Loading commits";
const EMPTY_LABEL: &str = "No commits to replay";
const START_LABEL: &str = "Start rebase";
const CANCEL_LABEL: &str = "Cancel";
const CLOSE_LABEL: &str = "Close";
const BUSY_TOOLTIP: &str = "Operation in progress";
const HINT: &str = "Newest commit on top — Squash and Fixup meld into the commit below.";

/// State of the page, owned by `HelmApp`: created in `loading` on the menu
/// click, filled (or failed) by the worker's `RebaseTodo` reply.
pub struct RebasePage {
    /// Checked-out branch at opening (title; the executed plan re-validates).
    pub current: String,
    /// Rebase target (the clicked ref).
    pub onto: String,
    /// `true` until the worker reply lands: the page shows a loader.
    pub loading: bool,
    /// Plan load failure (unknown ref, capped range…): clean error state.
    pub error: Option<String>,
    /// **Newest first** (graph order); the todo sent to the execution is
    /// rebuilt oldest-first by [`RebasePage::steps`].
    pub entries: Vec<RebaseEntry>,
}

/// One commit row: the loaded commit plus the user's choice.
pub struct RebaseEntry {
    pub commit: RebaseCommit,
    pub choice: RebaseChoice,
    /// Reword buffer, prefilled with the original message — kept when the
    /// choice changes so a round-trip through Drop loses nothing.
    pub message: String,
}

impl RebasePage {
    pub fn loading(current: impl Into<String>, onto: impl Into<String>) -> Self {
        Self {
            current: current.into(),
            onto: onto.into(),
            loading: true,
            error: None,
            entries: Vec::new(),
        }
    }

    /// Adopts the worker's commit list (received oldest-first, displayed
    /// newest-first).
    pub fn adopt(&mut self, commits: Vec<RebaseCommit>) {
        self.loading = false;
        self.error = None;
        self.entries = commits
            .into_iter()
            .rev()
            .map(|commit| RebaseEntry {
                choice: RebaseChoice::Pick,
                message: commit.message.clone(),
                commit,
            })
            .collect();
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.loading = false;
        self.error = Some(message.into());
    }

    /// Plan validity — entries handed to the domain **oldest first**.
    pub fn plan_error(&self) -> Option<String> {
        let shape: Vec<(RebaseChoice, bool)> = self
            .entries
            .iter()
            .rev()
            .map(|entry| (entry.choice, entry.message.trim().is_empty()))
            .collect();
        plan_error(&shape)
    }

    /// Steps for the execution, **oldest first** (git todo order).
    pub fn steps(&self) -> Vec<RebaseStep> {
        self.entries
            .iter()
            .rev()
            .map(|entry| RebaseStep {
                oid: entry.commit.oid,
                action: match entry.choice {
                    RebaseChoice::Pick => RebaseAction::Pick,
                    RebaseChoice::Reword => RebaseAction::Reword(entry.message.clone()),
                    RebaseChoice::Squash => RebaseAction::Squash,
                    RebaseChoice::Fixup => RebaseAction::Fixup,
                    RebaseChoice::Drop => RebaseAction::Drop,
                },
            })
            .collect()
    }

    fn all_dropped(&self) -> bool {
        !self.entries.is_empty()
            && self
                .entries
                .iter()
                .all(|entry| entry.choice == RebaseChoice::Drop)
    }
}

/// Signals emitted by the page within a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct RebasePageAction {
    /// Run the plan (validated — the button is disabled otherwise).
    pub start: bool,
    /// Close without running (Cancel, Close on error/empty, `Esc`).
    pub cancel: bool,
}

pub fn choice_label(choice: RebaseChoice) -> &'static str {
    match choice {
        RebaseChoice::Pick => "Pick",
        RebaseChoice::Reword => "Reword",
        RebaseChoice::Squash => "Squash",
        RebaseChoice::Fixup => "Fixup",
        RebaseChoice::Drop => "Drop",
    }
}

const CHOICES: [RebaseChoice; 5] = [
    RebaseChoice::Pick,
    RebaseChoice::Reword,
    RebaseChoice::Squash,
    RebaseChoice::Fixup,
    RebaseChoice::Drop,
];

/// Renders the page; `busy` greys the Start button out (a git command is
/// already running — same rule as the toolbar).
pub fn rebase_view(
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &mut RebasePage,
    busy: bool,
) -> RebasePageAction {
    let mut action = RebasePageAction::default();
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        action.cancel = true;
    }

    ui.add_space(PAD);
    header(ui, palette, page);
    ui.add_space(PAD);

    if page.loading {
        centered_loader(ui, palette);
        return action;
    }
    if let Some(error) = &page.error {
        action.cancel |= centered_notice(ui, error, palette.git_deleted);
        return action;
    }
    if page.entries.is_empty() {
        let notice = format!(
            "{EMPTY_LABEL} — {} is already contained in {}",
            page.current, page.onto
        );
        action.cancel |= centered_notice(ui, &notice, palette.text_secondary);
        return action;
    }

    let footer_height = 52.0;
    let list_height = (ui.available_height() - footer_height).max(0.0);
    egui::ScrollArea::vertical()
        .max_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, entry) in page.entries.iter_mut().enumerate() {
                entry_row(ui, palette, entry, index);
            }
        });

    footer(ui, palette, page, busy, &mut action);
    action
}

fn header(ui: &mut egui::Ui, palette: &Palette, page: &RebasePage) {
    ui.horizontal(|ui| {
        ui.add_space(PAD);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Interactive rebase — {} onto {}",
                    page.current, page.onto
                ))
                .size(TITLE_SIZE)
                .color(palette.text_primary)
                .strong(),
            );
            ui.label(
                egui::RichText::new(HINT)
                    .size(HINT_SIZE)
                    .color(palette.text_muted),
            );
        });
    });
}

fn centered_loader(ui: &mut egui::Ui, palette: &Palette) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 24.0).max(0.0));
        ui.add(Spinner::new().size(22.0).color(palette.text_muted));
        ui.label(egui::RichText::new(LOADING_LABEL).color(palette.text_muted));
    });
}

/// Error / empty state: message + Close. Returns `true` on Close.
fn centered_notice(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> bool {
    let mut close = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 32.0).max(0.0));
        ui.label(egui::RichText::new(text).size(ROW_TEXT_SIZE).color(color));
        ui.add_space(8.0);
        close = ui.button(CLOSE_LABEL).clicked();
    });
    close
}

/// One commit row; `index` only salts the action combo's id.
fn entry_row(ui: &mut egui::Ui, palette: &Palette, entry: &mut RebaseEntry, index: usize) {
    let dropped = entry.choice == RebaseChoice::Drop;
    ui.horizontal(|ui| {
        ui.add_space(PAD);
        egui::ComboBox::from_id_salt(("rebase-action", index))
            .width(ACTION_COMBO_W)
            .selected_text(choice_label(entry.choice))
            .show_ui(ui, |ui| {
                for choice in CHOICES {
                    ui.selectable_value(&mut entry.choice, choice, choice_label(choice));
                }
            });
        ui.label(
            egui::RichText::new(&entry.commit.short_id)
                .monospace()
                .size(SHA_SIZE)
                .color(palette.text_muted),
        );
        let mut summary = egui::RichText::new(&entry.commit.summary)
            .size(ROW_TEXT_SIZE)
            .color(if dropped {
                palette.text_muted
            } else {
                palette.text_primary
            });
        if dropped {
            summary = summary.strikethrough();
        }
        ui.label(summary);
        ui.label(
            egui::RichText::new(&entry.commit.author)
                .size(SHA_SIZE)
                .color(palette.text_muted),
        );
    });
    if entry.choice == RebaseChoice::Reword {
        ui.horizontal(|ui| {
            ui.add_space(PAD + ACTION_COMBO_W);
            ui.add(
                egui::TextEdit::multiline(&mut entry.message)
                    .desired_rows(REWORD_ROWS)
                    .desired_width((ui.available_width() - PAD).max(0.0))
                    .hint_text("New commit message"),
            );
        });
    }
    ui.add_space(4.0);
}

fn footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &RebasePage,
    busy: bool,
    action: &mut RebasePageAction,
) {
    let plan_issue = page.plan_error();
    ui.separator();
    ui.horizontal(|ui| {
        ui.add_space(PAD);
        if let Some(issue) = &plan_issue {
            ui.label(
                egui::RichText::new(issue.as_str())
                    .size(FOOTER_TEXT_SIZE)
                    .color(palette.git_deleted),
            );
        } else if page.all_dropped() {
            ui.label(
                egui::RichText::new(format!(
                    "All commits dropped — {} will be reset to {}",
                    page.current, page.onto
                ))
                .size(FOOTER_TEXT_SIZE)
                .color(palette.git_modified),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(PAD);
            let start = ui.add_enabled(
                plan_issue.is_none() && !busy,
                egui::Button::new(egui::RichText::new(START_LABEL).color(egui::Color32::WHITE))
                    .fill(palette.primary_button_fill()),
            );
            if busy {
                start.on_hover_text(BUSY_TOOLTIP);
            } else if start.clicked() {
                action.start = true;
            }
            if ui.button(CANCEL_LABEL).clicked() {
                action.cancel = true;
            }
        });
    });
}
