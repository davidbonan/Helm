//! AI rebase modals (git.md §9): the **recap** modal shows the not-yet-started
//! rebase (current → target, commits to replay) with an extra-instructions box
//! for the provider (e.g. squash everything into one commit) — Start hands the
//! request to the AI rebase runner, nothing runs before it. The **report**
//! modal shows the provider's account once the run completed, under an outcome
//! headline verified on the repo. Pure rendering: the caller owns the states
//! and arbitrates the emitted actions.

use crate::ai::AiProvider;
use crate::git::ai_rebase::{AiRebaseOutcome, AiRebaseReport};
use crate::git::rebase::RebaseCommit;
use crate::theme::Palette;
use crate::ui::spinner::Spinner;

const MODAL_WIDTH: f32 = 460.0;
const TITLE_SIZE: f32 = 14.0;
const TEXT_SIZE: f32 = 13.0;
const SHA_SIZE: f32 = 11.0;
const COMMITS_MAX_HEIGHT: f32 = 180.0;
const REPORT_MAX_HEIGHT: f32 = 260.0;
const INSTRUCTIONS_ROWS: usize = 3;
const INSTRUCTIONS_HINT: &str = "e.g. Squash everything into a single commit";
pub const START_LABEL: &str = "Start AI rebase";
const CANCEL_LABEL: &str = "Cancel";
const CLOSE_LABEL: &str = "Close";
const COPY_LABEL: &str = "Copy report";
const LOADING_LABEL: &str = "Loading commits";
const BUSY_TOOLTIP: &str = "Operation in progress";

/// State of the recap modal, owned by `HelmApp`: created in `loading` on the
/// menu click, filled (or failed) by the worker's `RebaseTodo` reply.
pub struct AiRebasePage {
    /// Checked-out branch at opening (title; the execution re-validates).
    pub current: String,
    /// Rebase target (the clicked ref).
    pub onto: String,
    /// `true` until the worker reply lands: the modal shows a loader.
    pub loading: bool,
    /// Recap load failure (unknown ref, capped range…): clean error state.
    pub error: Option<String>,
    /// **Newest first** (graph order); [`AiRebasePage::expected`] rebuilds the
    /// oldest-first order the execution re-checks.
    pub commits: Vec<RebaseCommit>,
    /// Extra instructions box, handed verbatim to the provider's prompt.
    pub instructions: String,
}

impl AiRebasePage {
    pub fn loading(current: impl Into<String>, onto: impl Into<String>) -> Self {
        Self {
            current: current.into(),
            onto: onto.into(),
            loading: true,
            error: None,
            commits: Vec::new(),
            instructions: String::new(),
        }
    }

    /// Adopts the worker's commit list (received oldest-first, displayed
    /// newest-first).
    pub fn adopt(&mut self, commits: Vec<RebaseCommit>) {
        self.loading = false;
        self.error = None;
        self.commits = commits.into_iter().rev().collect();
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.loading = false;
        self.error = Some(message.into());
    }

    /// Oids the user approved, **oldest first** — re-derived and compared by
    /// the execution (stale recap refused).
    pub fn expected(&self) -> Vec<git2::Oid> {
        self.commits.iter().rev().map(|c| c.oid).collect()
    }

    fn startable(&self) -> bool {
        !self.loading && self.error.is_none() && !self.commits.is_empty()
    }
}

/// Signals emitted by the recap modal within a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct AiRebaseModalAction {
    /// Hand the request to the runner (the button is disabled otherwise).
    pub start: bool,
    /// Close without running (Cancel, Close on error/empty, `Esc`).
    pub dismiss: bool,
}

/// Renders the recap modal; `busy` greys Start out (a git command is already
/// running — same rule as the toolbar).
pub fn ai_rebase_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &mut AiRebasePage,
    provider: AiProvider,
    busy: bool,
) -> AiRebaseModalAction {
    let mut action = AiRebaseModalAction::default();
    let modal = egui::Modal::new(egui::Id::new("ai_rebase_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(MODAL_WIDTH);
            ui.label(
                egui::RichText::new(format!("AI rebase — {} onto {}", page.current, page.onto))
                    .size(TITLE_SIZE)
                    .color(palette.text_primary)
                    .strong(),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} runs the rebase in the repository and resolves conflicts itself — \
                 it never pushes.",
                    provider.command()
                ))
                .size(TEXT_SIZE)
                .color(palette.text_muted),
            );
            ui.add_space(10.0);

            if page.loading {
                ui.horizontal(|ui| {
                    ui.add(Spinner::new().size(14.0).color(palette.text_muted));
                    ui.label(
                        egui::RichText::new(LOADING_LABEL)
                            .size(TEXT_SIZE)
                            .color(palette.text_muted),
                    );
                });
            } else if let Some(error) = &page.error {
                ui.label(
                    egui::RichText::new(error.as_str())
                        .size(TEXT_SIZE)
                        .color(palette.git_deleted),
                );
            } else if page.commits.is_empty() {
                ui.label(
                    egui::RichText::new(format!(
                        "No commits to replay — {} is already contained in {}",
                        page.current, page.onto
                    ))
                    .size(TEXT_SIZE)
                    .color(palette.text_secondary),
                );
            } else {
                recap_body(ui, palette, page);
            }

            ui.add_space(12.0);
            footer(ui, palette, page, busy, &mut action);
        });
    if modal.should_close() {
        action.dismiss = true;
    }
    action
}

/// Commit recap (newest on top, like the graph) + the instructions box.
fn recap_body(ui: &mut egui::Ui, palette: &Palette, page: &mut AiRebasePage) {
    let plural = if page.commits.len() > 1 { "s" } else { "" };
    ui.label(
        egui::RichText::new(format!("{} commit{plural} to replay", page.commits.len()))
            .size(TEXT_SIZE)
            .color(palette.text_secondary),
    );
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(COMMITS_MAX_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for commit in &page.commits {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&commit.short_id)
                            .monospace()
                            .size(SHA_SIZE)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(&commit.summary)
                            .size(TEXT_SIZE)
                            .color(palette.text_primary),
                    );
                });
            }
        });
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("AI instructions (optional)")
            .size(TEXT_SIZE)
            .color(palette.text_primary),
    );
    ui.add_space(2.0);
    ui.add(
        egui::TextEdit::multiline(&mut page.instructions)
            .desired_rows(INSTRUCTIONS_ROWS)
            .desired_width(f32::INFINITY)
            .hint_text(egui::RichText::new(INSTRUCTIONS_HINT).color(palette.text_muted)),
    );
}

fn footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &AiRebasePage,
    busy: bool,
    action: &mut AiRebaseModalAction,
) {
    ui.horizontal(|ui| {
        if ui.button(CANCEL_LABEL).clicked() {
            action.dismiss = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let start = ui.add_enabled(
                page.startable() && !busy,
                egui::Button::new(egui::RichText::new(START_LABEL).color(egui::Color32::WHITE))
                    .fill(palette.primary_button_fill()),
            );
            if busy {
                start.on_hover_text(BUSY_TOOLTIP);
            } else if start.clicked() {
                action.start = true;
            }
        });
    });
}

/// Outcome headline of the report modal — the verified outcome, never the
/// provider's words.
pub fn outcome_label(outcome: AiRebaseOutcome) -> &'static str {
    match outcome {
        AiRebaseOutcome::Completed => "Rebase completed",
        AiRebaseOutcome::Unchanged => "Branch unchanged",
        AiRebaseOutcome::LeftInProgress => "Rebase left in progress",
    }
}

/// Renders the report modal: outcome headline + the provider's account.
/// Returns `true` on Close (button, `Esc` or click outside).
pub fn ai_rebase_report_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    report: &AiRebaseReport,
) -> bool {
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("ai_rebase_report_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(MODAL_WIDTH);
            let color = match report.outcome {
                AiRebaseOutcome::Completed => palette.git_added,
                AiRebaseOutcome::Unchanged => palette.text_secondary,
                AiRebaseOutcome::LeftInProgress => palette.git_conflict,
            };
            ui.label(
                egui::RichText::new(format!("AI rebase — {}", outcome_label(report.outcome)))
                    .size(TITLE_SIZE)
                    .color(color)
                    .strong(),
            );
            if report.outcome == AiRebaseOutcome::LeftInProgress {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "Conflicts are unresolved — resolve them in the terminal or Abort \
                     from the sidebar banner.",
                    )
                    .size(TEXT_SIZE)
                    .color(palette.text_secondary),
                );
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(REPORT_MAX_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&report.summary)
                            .size(TEXT_SIZE)
                            .color(palette.text_primary),
                    );
                });
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(CLOSE_LABEL).clicked() {
                    close = true;
                }
                // The account (conflicts and how they were resolved) is worth
                // keeping: one click to the clipboard before closing.
                if ui.button(COPY_LABEL).clicked() {
                    ui.ctx().copy_text(report.summary.clone());
                }
            });
        });
    close || modal.should_close()
}
