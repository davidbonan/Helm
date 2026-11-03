//! Workflows section of Preferences (workflows.md §2): a structured form over the
//! global definitions — never raw text. Pure rendering: the caller persists when
//! [`workflows_section`] reports a change.

use crate::theme::Palette;
use crate::ui::preferences::settings_card;
use crate::workflow::definition::{Input, InputKind, Issue, Step, StepKind, Workflow};

const CARD_PAD: f32 = 16.0;
const CARD_GAP: f32 = 16.0;
const TEXT_SIZE: f32 = 13.0;
const HINT_SIZE: f32 = 12.0;
const KEY_WIDTH: f32 = 120.0;
const KIND_WIDTH: f32 = 120.0;
const TEXT_ROWS: usize = 4;
pub const NEW_WORKFLOW_LABEL: &str = "New workflow";
pub const ADD_INPUT_LABEL: &str = "Add input";
pub const ADD_STEP_LABEL: &str = "Add step";
pub const ADD_OPTION_LABEL: &str = "Add option";
pub const DUPLICATE_LABEL: &str = "Duplicate";
pub const DELETE_LABEL: &str = "Delete workflow";
pub const CONFIRM_DELETE_LABEL: &str = "Confirm delete";
pub const MOVE_UP_LABEL: &str = "Move up";
pub const MOVE_DOWN_LABEL: &str = "Move down";
pub const REMOVE_LABEL: &str = "Remove";
const UNTITLED: &str = "Untitled workflow";
const ABOUT: &str = "Launched from Cmd+P › Launch workflow, each step sent into one \
                     Claude Code session. Placeholders: {key} of an input, {branch}, \
                     {worktree}, {repo}.";

enum ListEdit {
    Up,
    Down,
    Remove,
}

fn apply<T>(items: &mut Vec<T>, index: usize, edit: ListEdit) {
    match edit {
        ListEdit::Up if index > 0 => items.swap(index, index - 1),
        ListEdit::Down if index + 1 < items.len() => items.swap(index, index + 1),
        ListEdit::Remove => {
            items.remove(index);
        }
        ListEdit::Up | ListEdit::Down => {}
    }
}

/// Returns whether a definition changed this frame.
pub fn workflows_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    workflows: &mut Vec<Workflow>,
) -> bool {
    let mut changed = false;
    ui.label(
        egui::RichText::new(ABOUT)
            .size(HINT_SIZE)
            .color(palette.text_muted),
    );
    ui.add_space(CARD_GAP);
    let mut duplicate = None;
    let mut delete = None;
    for (index, workflow) in workflows.iter_mut().enumerate() {
        settings_card(ui, palette, |ui| {
            egui::Frame::new().inner_margin(CARD_PAD).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.set_width(ui.available_width());
                let title = match workflow.name.trim() {
                    "" => UNTITLED,
                    name => name,
                };
                egui::CollapsingHeader::new(egui::RichText::new(title).size(TEXT_SIZE).strong())
                    .id_salt(("workflow", index))
                    .show(ui, |ui| {
                        changed |= workflow_form(ui, palette, workflow);
                        ui.add_space(8.0);
                        match footer(ui, ("workflow_delete", index)) {
                            Some(Footer::Duplicate) => duplicate = Some(index),
                            Some(Footer::Delete) => delete = Some(index),
                            None => {}
                        }
                    });
                issues(ui, palette, &workflow.issues());
            });
        });
        ui.add_space(CARD_GAP);
    }
    if let Some(index) = duplicate {
        let mut copy = workflows[index].clone();
        copy.name = format!("{} copy", copy.name);
        workflows.insert(index + 1, copy);
        changed = true;
    }
    if let Some(index) = delete {
        workflows.remove(index);
        changed = true;
    }
    if ui.button(format!("+ {NEW_WORKFLOW_LABEL}")).clicked() {
        workflows.push(Workflow {
            name: NEW_WORKFLOW_LABEL.to_owned(),
            inputs: Vec::new(),
            steps: vec![blank_prompt()],
        });
        changed = true;
    }
    changed
}

fn blank_prompt() -> Step {
    Step {
        title: String::new(),
        kind: StepKind::Prompt {
            text: String::new(),
        },
    }
}

fn workflow_form(ui: &mut egui::Ui, palette: &Palette, workflow: &mut Workflow) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        caption(ui, palette, "Name");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut workflow.name).desired_width(f32::INFINITY))
            .changed();
    });
    ui.add_space(8.0);
    caption(ui, palette, "Inputs");
    let mut edit = None;
    let count = workflow.inputs.len();
    for (index, input) in workflow.inputs.iter_mut().enumerate() {
        ui.push_id(("input", index), |ui| {
            changed |= input_row(ui, input);
            if let Some(e) = list_controls(ui, index, count) {
                edit = Some((index, e));
            }
        });
    }
    if let Some((index, e)) = edit {
        apply(&mut workflow.inputs, index, e);
        changed = true;
    }
    if ui.small_button(format!("+ {ADD_INPUT_LABEL}")).clicked() {
        workflow.inputs.push(Input {
            key: String::new(),
            label: String::new(),
            default: String::new(),
            kind: InputKind::Text,
        });
        changed = true;
    }
    ui.add_space(8.0);
    caption(ui, palette, "Steps");
    let mut edit = None;
    let count = workflow.steps.len();
    for (index, step) in workflow.steps.iter_mut().enumerate() {
        ui.push_id(("step", index), |ui| {
            changed |= step_row(ui, index, step);
            if let Some(e) = list_controls(ui, index, count) {
                edit = Some((index, e));
            }
        });
    }
    if let Some((index, e)) = edit {
        apply(&mut workflow.steps, index, e);
        changed = true;
    }
    if ui.small_button(format!("+ {ADD_STEP_LABEL}")).clicked() {
        workflow.steps.push(blank_prompt());
        changed = true;
    }
    changed
}

fn caption(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(TEXT_SIZE)
            .color(palette.text_secondary),
    );
}

fn input_row(ui: &mut egui::Ui, input: &mut Input) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut input.key)
                    .hint_text("key")
                    .desired_width(KEY_WIDTH),
            )
            .changed();
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut input.label)
                    .hint_text("Label")
                    .desired_width(ui.available_width() - KIND_WIDTH - 12.0),
            )
            .changed();
        changed |= input_kind(ui, &mut input.kind);
    });
    if let InputKind::Select { options } = &mut input.kind {
        changed |= options_editor(ui, options);
        changed |= select_default(ui, options, &mut input.default);
    } else {
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut input.default)
                    .hint_text("Default (optional)")
                    .desired_width(f32::INFINITY),
            )
            .changed();
    }
    changed
}

fn kind_label(kind: &InputKind) -> &'static str {
    match kind {
        InputKind::Text => "Text",
        InputKind::LongText => "Long text",
        InputKind::Select { .. } => "Choice list",
    }
}

fn input_kind(ui: &mut egui::Ui, kind: &mut InputKind) -> bool {
    let mut picked = None;
    egui::ComboBox::from_id_salt("kind")
        .width(KIND_WIDTH)
        .selected_text(kind_label(kind))
        .show_ui(ui, |ui| {
            let choices = [
                InputKind::Text,
                InputKind::LongText,
                InputKind::Select {
                    options: Vec::new(),
                },
            ];
            for choice in choices {
                let current = std::mem::discriminant(kind) == std::mem::discriminant(&choice);
                if ui.selectable_label(current, kind_label(&choice)).clicked() && !current {
                    picked = Some(choice);
                }
            }
        });
    let Some(choice) = picked else {
        return false;
    };
    *kind = choice;
    true
}

fn options_editor(ui: &mut egui::Ui, options: &mut Vec<String>) -> bool {
    let mut changed = false;
    let mut removed = None;
    for (index, option) in options.iter_mut().enumerate() {
        ui.push_id(("option", index), |ui| {
            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(option)
                            .hint_text("Option")
                            .desired_width(220.0),
                    )
                    .changed();
                if ui.small_button("✕").on_hover_text(REMOVE_LABEL).clicked() {
                    removed = Some(index);
                }
            });
        });
    }
    if let Some(index) = removed {
        options.remove(index);
        changed = true;
    }
    if ui.small_button(format!("+ {ADD_OPTION_LABEL}")).clicked() {
        options.push(String::new());
        changed = true;
    }
    changed
}

fn select_default(ui: &mut egui::Ui, options: &[String], default: &mut String) -> bool {
    let before = default.clone();
    let shown = if default.is_empty() {
        "First option"
    } else {
        default.as_str()
    };
    egui::ComboBox::from_id_salt("default")
        .selected_text(format!("Default: {shown}"))
        .show_ui(ui, |ui| {
            ui.selectable_value(default, String::new(), "First option");
            for option in options.iter().filter(|o| !o.trim().is_empty()) {
                ui.selectable_value(default, option.clone(), option);
            }
        });
    *default != before
}

fn step_row(ui: &mut egui::Ui, index: usize, step: &mut Step) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{}.", index + 1)).size(TEXT_SIZE));
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut step.title)
                    .hint_text("Title")
                    .desired_width(ui.available_width() - KIND_WIDTH - 12.0),
            )
            .changed();
        changed |= step_kind(ui, &mut step.kind);
    });
    let (text, hint) = match &mut step.kind {
        StepKind::Prompt { text } => (text, "Prompt sent to Claude"),
        StepKind::Gate { message } => (message, "Shown while the run waits for Continue"),
    };
    changed |= ui
        .add(
            egui::TextEdit::multiline(text)
                .hint_text(hint)
                .desired_rows(TEXT_ROWS)
                .desired_width(f32::INFINITY),
        )
        .changed();
    changed
}

fn step_kind(ui: &mut egui::Ui, kind: &mut StepKind) -> bool {
    let is_gate = matches!(kind, StepKind::Gate { .. });
    let mut want_gate = is_gate;
    egui::ComboBox::from_id_salt("step_kind")
        .width(KIND_WIDTH)
        .selected_text(if is_gate { "Gate" } else { "Prompt" })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut want_gate, false, "Prompt");
            ui.selectable_value(&mut want_gate, true, "Gate");
        });
    if want_gate == is_gate {
        return false;
    }
    let text = match kind {
        StepKind::Prompt { text } => std::mem::take(text),
        StepKind::Gate { message } => std::mem::take(message),
    };
    *kind = if want_gate {
        StepKind::Gate { message: text }
    } else {
        StepKind::Prompt { text }
    };
    true
}

fn list_controls(ui: &mut egui::Ui, index: usize, count: usize) -> Option<ListEdit> {
    let mut edit = None;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(index > 0, egui::Button::new("↑").small())
            .on_hover_text(MOVE_UP_LABEL)
            .clicked()
        {
            edit = Some(ListEdit::Up);
        }
        if ui
            .add_enabled(index + 1 < count, egui::Button::new("↓").small())
            .on_hover_text(MOVE_DOWN_LABEL)
            .clicked()
        {
            edit = Some(ListEdit::Down);
        }
        if ui.small_button(REMOVE_LABEL).clicked() {
            edit = Some(ListEdit::Remove);
        }
    });
    ui.add_space(4.0);
    edit
}

enum Footer {
    Duplicate,
    Delete,
}

/// Delete asks a second click, armed in egui memory (workflows.md §2).
fn footer(ui: &mut egui::Ui, id: impl std::hash::Hash) -> Option<Footer> {
    let armed_id = egui::Id::new(id);
    let armed = ui.data(|d| d.get_temp::<bool>(armed_id).unwrap_or(false));
    let mut outcome = None;
    ui.horizontal(|ui| {
        if ui.button(DUPLICATE_LABEL).clicked() {
            outcome = Some(Footer::Duplicate);
        }
        let label = if armed {
            CONFIRM_DELETE_LABEL
        } else {
            DELETE_LABEL
        };
        if ui.button(label).clicked() {
            if armed {
                outcome = Some(Footer::Delete);
            }
            ui.data_mut(|d| d.insert_temp(armed_id, !armed));
        }
    });
    outcome
}

fn issues(ui: &mut egui::Ui, palette: &Palette, issues: &[Issue]) {
    for issue in issues {
        let color = if issue.is_error() {
            palette.git_deleted
        } else {
            palette.git_modified
        };
        ui.label(
            egui::RichText::new(issue.message())
                .size(HINT_SIZE)
                .color(color),
        );
    }
}
