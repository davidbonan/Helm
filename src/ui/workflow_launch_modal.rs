//! Inputs modal of a workflow launch (workflows.md §3): one field per input, in
//! order. Pure rendering — the caller owns the form and the launch.

use std::collections::BTreeMap;

use crate::theme::Palette;
use crate::ui::picker::{Choice, PickOne, Trigger};
use crate::workflow::definition::{InputKind, Workflow};

const MODAL_WIDTH: f32 = 460.0;
const TITLE_SIZE: f32 = 14.0;
const TEXT_SIZE: f32 = 13.0;
const LONG_TEXT_ROWS: usize = 4;
pub const LAUNCH_LABEL: &str = "Launch";
const CANCEL_LABEL: &str = "Cancel";

/// Modal state owned by `HelmApp`: the workflow and the values being typed.
pub struct LaunchForm {
    pub workflow: Workflow,
    pub values: BTreeMap<String, String>,
    focused: bool,
}

impl LaunchForm {
    pub fn new(workflow: Workflow) -> Self {
        let values = workflow.initial_values();
        Self {
            workflow,
            values,
            focused: false,
        }
    }
}

#[derive(Default)]
pub struct LaunchModalAction {
    pub launch: bool,
    /// Cancel, `Esc`, click outside.
    pub dismiss: bool,
}

/// `Enter` in a single-line field or `Cmd+Enter` anywhere launches once every input
/// is filled.
pub fn workflow_launch_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    form: &mut LaunchForm,
) -> LaunchModalAction {
    let mut action = LaunchModalAction::default();
    let modal = egui::Modal::new(egui::Id::new("workflow_launch_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(MODAL_WIDTH);
            ui.label(
                egui::RichText::new(&form.workflow.name)
                    .size(TITLE_SIZE)
                    .color(palette.text_primary)
                    .strong(),
            );
            ui.add_space(10.0);
            let submitted = fields(ui, palette, form);
            let complete = form.workflow.accepts(&form.values);
            let chord = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
            if complete && (submitted || chord) {
                action.launch = true;
            }
            ui.add_space(12.0);
            footer(ui, palette, complete, &mut action);
        });
    if modal.should_close() {
        action.dismiss = true;
    }
    action
}

/// Returns whether `Enter` was pressed in a single-line field; it also moves the
/// focus to the next field, so an unfinished form carries on from the keyboard.
fn fields(ui: &mut egui::Ui, palette: &Palette, form: &mut LaunchForm) -> bool {
    let mut submitted = false;
    let mut focus_next = false;
    let focus_first = !std::mem::replace(&mut form.focused, true);
    for (index, input) in form.workflow.inputs.iter().enumerate() {
        let label = if input.label.trim().is_empty() {
            &input.key
        } else {
            &input.label
        };
        ui.label(
            egui::RichText::new(label)
                .size(TEXT_SIZE)
                .color(palette.text_primary),
        );
        ui.add_space(2.0);
        let value = form.values.entry(input.key.clone()).or_default();
        let response = match &input.kind {
            InputKind::Text => {
                ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY))
            }
            InputKind::LongText => ui.add(
                egui::TextEdit::multiline(value)
                    .desired_rows(LONG_TEXT_ROWS)
                    .desired_width(f32::INFINITY),
            ),
            InputKind::Select { options } => {
                let response = Trigger::new(value)
                    .compact()
                    .width(ui.available_width())
                    .show(ui, palette);
                let choices: Vec<Choice> = options.iter().map(|o| Choice::new(o)).collect();
                let selected = options.iter().position(|o| o == value);
                if let Some(picked) = PickOne::new(&choices, selected).show(&response, palette) {
                    value.clone_from(&options[picked]);
                }
                response
            }
        };
        if (focus_first && index == 0) || std::mem::take(&mut focus_next) {
            response.request_focus();
        }
        if input.kind == InputKind::Text
            && response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            submitted = true;
            focus_next = true;
        }
        ui.add_space(8.0);
    }
    submitted
}

fn footer(ui: &mut egui::Ui, palette: &Palette, complete: bool, action: &mut LaunchModalAction) {
    ui.horizontal(|ui| {
        if ui.button(CANCEL_LABEL).clicked() {
            action.dismiss = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let launch = ui.add_enabled(
                complete,
                egui::Button::new(egui::RichText::new(LAUNCH_LABEL).color(egui::Color32::WHITE))
                    .fill(palette.primary_button_fill()),
            );
            if launch.clicked() {
                action.launch = true;
            }
        });
    });
}
