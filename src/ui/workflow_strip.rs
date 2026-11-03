//! The workflow strip (workflows.md §7): a thin bar on top of a run's tab — name,
//! step, state, and the actions that apply. Pure rendering.

use crate::theme::Palette;
use crate::workflow::run::RunState;

const STRIP_HEIGHT: f32 = 28.0;
const TEXT_SIZE: f32 = 12.0;
const DOT_RADIUS: f32 = 3.5;
pub const CONTINUE_LABEL: &str = "Continue";
pub const NEXT_STEP_LABEL: &str = "Next step";
pub const STOP_LABEL: &str = "Stop";
pub const DISMISS_LABEL: &str = "Dismiss";

pub struct StripView {
    pub name: String,
    /// 1-based.
    pub step: usize,
    pub steps: usize,
    pub title: String,
    pub state: RunState,
    pub gate_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripAction {
    Continue,
    NextStep,
    Stop,
    Dismiss,
}

pub fn state_label(state: RunState) -> &'static str {
    match state {
        RunState::Running => "Running",
        RunState::AwaitingReply => "Awaiting your reply",
        RunState::NeedsYou => "Needs you",
        RunState::Gate => "Paused",
        RunState::Done => "Done",
        RunState::Stopped => "Stopped",
    }
}

fn actions(state: RunState) -> &'static [StripAction] {
    match state {
        RunState::Running => &[StripAction::Stop],
        RunState::AwaitingReply | RunState::NeedsYou => &[StripAction::NextStep, StripAction::Stop],
        RunState::Gate => &[StripAction::Continue, StripAction::Stop],
        RunState::Done | RunState::Stopped => &[StripAction::Dismiss],
    }
}

fn label(action: StripAction) -> &'static str {
    match action {
        StripAction::Continue => CONTINUE_LABEL,
        StripAction::NextStep => NEXT_STEP_LABEL,
        StripAction::Stop => STOP_LABEL,
        StripAction::Dismiss => DISMISS_LABEL,
    }
}

fn dot_color(palette: &Palette, state: RunState) -> egui::Color32 {
    match state {
        RunState::Running => palette.accent,
        RunState::AwaitingReply | RunState::NeedsYou | RunState::Gate => palette.git_modified,
        RunState::Done => palette.git_added,
        RunState::Stopped => palette.text_muted,
    }
}

pub fn workflow_strip(
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &StripView,
) -> Option<StripAction> {
    let mut clicked = None;
    egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .inner_margin(egui::Margin::symmetric(10, 0))
        .show(ui, |ui| {
            ui.set_height(STRIP_HEIGHT);
            ui.horizontal_centered(|ui| {
                let (dot, _) = ui.allocate_exact_size(
                    egui::vec2(DOT_RADIUS * 2.0, DOT_RADIUS * 2.0),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(
                    dot.center(),
                    DOT_RADIUS,
                    dot_color(palette, view.state),
                );
                ui.label(
                    egui::RichText::new(&view.name)
                        .size(TEXT_SIZE)
                        .color(palette.text_primary)
                        .strong(),
                );
                let step = match view.title.trim() {
                    "" => format!("step {}/{}", view.step, view.steps),
                    title => format!("step {}/{} — {title}", view.step, view.steps),
                };
                ui.label(
                    egui::RichText::new(step)
                        .size(TEXT_SIZE)
                        .color(palette.text_secondary),
                );
                ui.label(
                    egui::RichText::new(state_label(view.state))
                        .size(TEXT_SIZE)
                        .color(palette.text_muted),
                );
                if let Some(message) = &view.gate_message {
                    ui.label(
                        egui::RichText::new(message)
                            .size(TEXT_SIZE)
                            .color(palette.text_primary),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for &action in actions(view.state).iter().rev() {
                        if ui.small_button(label(action)).clicked() {
                            clicked = Some(action);
                        }
                    }
                });
            });
        });
    clicked
}
