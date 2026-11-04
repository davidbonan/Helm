//! The workflow strip (workflows.md §7): a pill at the right end of a run's tab
//! bar — step track, name, step, state, and the actions that apply. It rides in
//! the tab bar's own row, so the terminal keeps its height. Pure rendering.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use lucide_icons::Icon;

use crate::theme::{self, Palette};
use crate::ui::spinner::paint_pinwheel;
use crate::ui::{clickable, paint_icon};
use crate::workflow::run::RunState;

const PILL_HEIGHT: f32 = 24.0;
const PAD_LEFT: f32 = 9.0;
const PAD_RIGHT: f32 = 3.0;
const GAP: f32 = 7.0;
const TEXT_SIZE: f32 = 12.0;
const STATE_SIZE: f32 = 11.0;
const TITLE_MAX: f32 = 150.0;
const MESSAGE_MAX: f32 = 260.0;
const DOT_RADIUS: f32 = 2.5;
const DOT_PITCH: f32 = 8.0;
const MAX_DOTS: usize = 10;
const SPINNER_SIZE: f32 = 9.0;
const BUTTON: f32 = 20.0;
const BUTTON_GAP: f32 = 1.0;
const BUTTON_ICON: f32 = 11.0;
const TINT: f32 = 0.14;
const TINT_BORDER: f32 = 0.45;
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

fn icon(action: StripAction) -> Icon {
    match action {
        StripAction::Continue => Icon::Play,
        StripAction::NextStep => Icon::SkipForward,
        StripAction::Stop => Icon::Square,
        StripAction::Dismiss => Icon::X,
    }
}

/// The run's hue: accent while it works, the modified hue when it waits on
/// you, the added hue once done.
fn state_color(palette: &Palette, state: RunState) -> egui::Color32 {
    match state {
        RunState::Running => palette.accent,
        RunState::AwaitingReply | RunState::NeedsYou | RunState::Gate => palette.git_modified,
        RunState::Done => palette.git_added,
        RunState::Stopped => palette.text_muted,
    }
}

fn wants_you(state: RunState) -> bool {
    matches!(
        state,
        RunState::AwaitingReply | RunState::NeedsYou | RunState::Gate
    )
}

struct Texts {
    name: Arc<egui::Galley>,
    title: Option<Arc<egui::Galley>>,
    state: Arc<egui::Galley>,
    message: Option<Arc<egui::Galley>>,
}

impl Texts {
    fn lay_out(ui: &egui::Ui, view: &StripView) -> Self {
        let painter = ui.painter();
        let medium = egui::FontId::new(TEXT_SIZE, theme::medium_family(ui.ctx()));
        let regular = egui::FontId::proportional(TEXT_SIZE);
        let title = view.title.trim();
        Self {
            name: painter.layout_job(line(&view.name, medium, TITLE_MAX)),
            title: (!title.is_empty())
                .then(|| painter.layout_job(line(title, regular.clone(), TITLE_MAX))),
            state: painter.layout_job(line(
                state_label(view.state),
                egui::FontId::proportional(STATE_SIZE),
                f32::INFINITY,
            )),
            message: view
                .gate_message
                .as_deref()
                .map(|message| painter.layout_job(line(message, regular, MESSAGE_MAX))),
        }
    }

    fn width(&self) -> f32 {
        let optional = [&self.title, &self.message]
            .into_iter()
            .flatten()
            .map(|galley| galley.size().x + GAP)
            .sum::<f32>();
        self.name.size().x + GAP + optional + self.state.size().x
    }
}

fn line(text: &str, font: egui::FontId, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    job.wrap = TextWrapping::truncate_at_width(width);
    job
}

fn track_width(steps: usize) -> f32 {
    steps.clamp(1, MAX_DOTS) as f32 * DOT_PITCH
}

/// Allocated at the cursor as one block, so a right-to-left parent pins it to
/// the bar's right end.
pub fn workflow_strip(
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &StripView,
) -> Option<StripAction> {
    let texts = Texts::lay_out(ui, view);
    let shown = actions(view.state);
    let buttons = shown.len() as f32 * (BUTTON + BUTTON_GAP);
    let width =
        PAD_LEFT + track_width(view.steps) + GAP + texts.width() + GAP + buttons + PAD_RIGHT;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, PILL_HEIGHT), egui::Sense::hover());
    paint_pill(ui, palette, view.state, rect);
    let mut x = rect.left() + PAD_LEFT;
    paint_track(ui, palette, view, egui::pos2(x, rect.center().y));
    x += track_width(view.steps) + GAP;
    let center_y = rect.center().y;
    let mut text = |ui: &egui::Ui, galley: Arc<egui::Galley>, color: egui::Color32| {
        let advance = galley.size().x + GAP;
        let top = center_y - galley.size().y / 2.0;
        ui.painter().galley(egui::pos2(x, top), galley, color);
        x += advance;
    };
    text(ui, texts.name, palette.text_primary);
    if let Some(title) = texts.title {
        text(ui, title, palette.text_secondary);
    }
    let state_ink = if view.state == RunState::Running {
        palette.text_muted
    } else {
        state_color(palette, view.state)
    };
    text(ui, texts.state, state_ink);
    if let Some(message) = texts.message {
        text(ui, message, palette.text_primary);
    }
    let labels = strip_labels(view);
    let info = ui.interact(rect, ui.id().with("workflow-strip"), egui::Sense::hover());
    let info = match &view.gate_message {
        Some(message) => info.on_hover_text(message),
        None => info,
    };
    info.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &labels));
    let buttons_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - PAD_RIGHT - buttons, rect.top()),
        rect.max,
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(buttons_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = BUTTON_GAP;
    let mut clicked = None;
    for &action in shown {
        if action_button(&mut row, palette, action).clicked() {
            clicked = Some(action);
        }
    }
    clicked
}

/// What the pill reads as to assistive tech: the run, its step, its state and
/// the gate's message.
fn strip_labels(view: &StripView) -> String {
    let step = match view.title.trim() {
        "" => format!("step {}/{}", view.step, view.steps),
        title => format!("step {}/{} — {title}", view.step, view.steps),
    };
    let mut label = format!("{}, {step}, {}", view.name, state_label(view.state));
    if let Some(message) = &view.gate_message {
        label.push_str(": ");
        label.push_str(message);
    }
    label
}

fn paint_pill(ui: &egui::Ui, palette: &Palette, state: RunState, rect: egui::Rect) {
    let tinted = wants_you(state) || state == RunState::Done;
    let (fill, border) = if tinted {
        let hue = state_color(palette, state);
        (hue.gamma_multiply(TINT), hue.gamma_multiply(TINT_BORDER))
    } else {
        (palette.bg_surface, palette.border_subtle)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same((PILL_HEIGHT / 2.0) as u8),
        fill,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
}

/// One dot per step: done steps filled, upcoming ones hollow, the current one
/// in the run's hue — a pinwheel while Claude works on it.
fn paint_track(ui: &egui::Ui, palette: &Palette, view: &StripView, left_center: egui::Pos2) {
    let count = view.steps.clamp(1, MAX_DOTS);
    let current = (view.step.max(1) - 1).min(count - 1);
    let hue = state_color(palette, view.state);
    let painter = ui.painter();
    for index in 0..count {
        let center = egui::pos2(
            left_center.x + DOT_PITCH * (index as f32 + 0.5),
            left_center.y,
        );
        let finished = view.state == RunState::Done;
        if index == current && view.state == RunState::Running {
            paint_pinwheel(ui, center, SPINNER_SIZE, 2, &palette.lane_colors, None);
        } else if index < current || finished {
            painter.circle_filled(
                center,
                DOT_RADIUS,
                if finished {
                    hue
                } else {
                    palette.text_secondary
                },
            );
        } else if index == current {
            painter.circle_filled(center, DOT_RADIUS + 0.5, hue);
        } else {
            painter.circle_stroke(
                center,
                DOT_RADIUS - 0.5,
                egui::Stroke::new(1.0_f32, palette.border_input),
            );
        }
    }
}

fn action_button(ui: &mut egui::Ui, palette: &Palette, action: StripAction) -> egui::Response {
    let (rect, response, hovered) = clickable(ui, egui::vec2(BUTTON, BUTTON), true);
    let primary = action == StripAction::Continue;
    let fill = if primary {
        Some(palette.accent)
    } else if hovered {
        Some(palette.bg_surface_hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .circle_filled(rect.center(), BUTTON / 2.0, fill);
    }
    let ink = match (primary, action, hovered) {
        (true, _, _) => palette.lane_node_text,
        (_, StripAction::Stop, true) => palette.git_deleted,
        (_, _, true) => palette.text_primary,
        _ => palette.text_secondary,
    };
    paint_icon(ui.painter(), rect.center(), BUTTON_ICON, icon(action), ink);
    let name = label(action);
    let response = response.on_hover_text(name);
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response
}
