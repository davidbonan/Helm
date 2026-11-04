//! Workflows section of Preferences (workflows.md §2): a structured form over the
//! global definitions — never raw text. Pure rendering: the caller persists when
//! [`workflows_section`] reports a change.

use lucide_icons::Icon;

use crate::theme::{self, Palette};
use crate::ui::picker::{Choice, PickOne, Trigger};
use crate::ui::preferences::{setting_divider, settings_card};
use crate::ui::{clickable, paint_icon};
use crate::workflow::definition::{Input, InputKind, Issue, Step, StepKind, Workflow};
use crate::workflow::template::{is_placeholder_name, BUILTINS};

const PAD: f32 = 16.0;
const CARD_GAP: f32 = 12.0;
const CARD_RADIUS: u8 = 12;
const HEADER_HEIGHT: f32 = 60.0;
const TILE: f32 = 32.0;
const TILE_RADIUS: u8 = 8;
const TILE_ICON: f32 = 16.0;
const TITLE_SIZE: f32 = 13.0;
const TEXT_SIZE: f32 = 13.0;
const DETAIL_SIZE: f32 = 12.0;
const SMALL_SIZE: f32 = 11.0;
const NAME_SIZE: f32 = 15.0;
const NAME_HEIGHT: f32 = 34.0;
const CONTROL_HEIGHT: f32 = 28.0;
const CONTROL_RADIUS: u8 = 6;
const CONTROL_PAD_X: f32 = 10.0;
const ICON_BUTTON: f32 = 24.0;
const ICON_SIZE: f32 = 13.0;
const CHIP_HEIGHT: f32 = 22.0;
const CHIP_PAD_X: f32 = 7.0;
const RAIL: f32 = 36.0;
const NODE_RADIUS: f32 = 11.0;
const STEP_PAD: f32 = 10.0;
const STEP_RADIUS: u8 = 10;
const STEP_GAP: f32 = 10.0;
const KEY_WIDTH: f32 = 112.0;
const KIND_WIDTH: f32 = 118.0;
const OPTION_MIN_WIDTH: f32 = 48.0;
const OPTION_MAX_WIDTH: f32 = 180.0;
const PROMPT_ROWS: usize = 4;
const GATE_ROWS: usize = 2;

pub const NEW_WORKFLOW_LABEL: &str = "New workflow";
pub const ADD_INPUT_LABEL: &str = "Add input";
pub const ADD_PROMPT_LABEL: &str = "Add prompt";
pub const ADD_GATE_LABEL: &str = "Add gate";
pub const ADD_OPTION_LABEL: &str = "Add option";
pub const DUPLICATE_LABEL: &str = "Duplicate workflow";
pub const DELETE_LABEL: &str = "Delete workflow";
pub const CONFIRM_DELETE_LABEL: &str = "Confirm delete";
pub const PROMPT_LABEL: &str = "Prompt";
pub const GATE_LABEL: &str = "Gate";
pub const READY_LABEL: &str = "Ready";
const UNTITLED: &str = "Untitled workflow";
const FIRST_OPTION: &str = "First option";
const ABOUT: &str = "Chained Claude Code prompts and pauses, launched from ⌘P › Launch \
                     workflow. Each step waits for the previous one to finish.";
const EMPTY_TEXT_WIDTH: f32 = 400.0;
const EMPTY_TITLE: &str = "No workflow yet";
const EMPTY_TEXT: &str = "Chain prompts into one Claude Code session: a dev → review \
                          cycle, a compact between two tasks, a pause to read the diff.";

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

enum CardAction {
    Toggle,
    Duplicate,
    Delete,
}

#[derive(PartialEq)]
enum Place {
    Name,
    Inputs,
    Steps,
    Step(usize),
}

fn place(issue: &Issue) -> Place {
    match issue {
        Issue::MissingName => Place::Name,
        Issue::NoStep => Place::Steps,
        Issue::BlankPrompt { step }
        | Issue::UnknownPlaceholder { step, .. }
        | Issue::TurnlessCommand { step, .. } => Place::Step(*step),
        Issue::InvalidKey { .. }
        | Issue::ReservedKey { .. }
        | Issue::DuplicateKey { .. }
        | Issue::SelectWithoutOption { .. }
        | Issue::DefaultNotAnOption { .. } => Place::Inputs,
    }
}

fn open_id() -> egui::Id {
    egui::Id::new("workflow-editor-open")
}

fn focus_id() -> egui::Id {
    egui::Id::new("workflow-editor-focus")
}

/// The field is drawn next frame: focusing an id absent from this one's
/// accessibility tree panics accesskit.
fn focus_next_frame(ui: &egui::Ui, field: egui::Id) {
    ui.data_mut(|d| d.insert_temp(focus_id(), field));
}

fn claim_focus(ui: &egui::Ui, response: &egui::Response) {
    if ui.data(|d| d.get_temp::<egui::Id>(focus_id())) == Some(response.id) {
        ui.data_mut(|d| d.remove::<egui::Id>(focus_id()));
        response.request_focus();
    }
}

fn name_field_id(index: usize) -> egui::Id {
    egui::Id::new(("workflow-name", index))
}

/// Returns whether a definition changed this frame.
pub fn workflows_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    workflows: &mut Vec<Workflow>,
) -> bool {
    ui.label(
        egui::RichText::new(ABOUT)
            .size(DETAIL_SIZE)
            .color(palette.text_muted),
    );
    ui.add_space(PAD);
    let mut open = ui
        .data(|d| d.get_temp::<Option<usize>>(open_id()))
        .flatten();
    let mut changed = false;
    let mut action = None;
    for (index, workflow) in workflows.iter_mut().enumerate() {
        ui.push_id(("workflow", index), |ui| {
            let card = workflow_card(ui, palette, index, workflow, open == Some(index));
            changed |= card.edited;
            if let Some(a) = card.action {
                action = Some((index, a));
            }
        });
        ui.add_space(CARD_GAP);
    }
    match action {
        Some((index, CardAction::Toggle)) => open = (open != Some(index)).then_some(index),
        Some((index, CardAction::Duplicate)) => {
            let mut copy = workflows[index].clone();
            copy.name = format!("{} copy", copy.name);
            workflows.insert(index + 1, copy);
            open = Some(index + 1);
            changed = true;
        }
        Some((index, CardAction::Delete)) => {
            workflows.remove(index);
            open = None;
            changed = true;
        }
        None => {}
    }
    let added = if workflows.is_empty() {
        empty_state(ui, palette)
    } else {
        ghost_button(ui, palette, NEW_WORKFLOW_LABEL)
    };
    if added {
        workflows.push(Workflow {
            name: NEW_WORKFLOW_LABEL.to_owned(),
            inputs: Vec::new(),
            steps: vec![blank_step(false)],
        });
        open = Some(workflows.len() - 1);
        focus_next_frame(ui, name_field_id(workflows.len() - 1));
        changed = true;
    }
    ui.data_mut(|d| d.insert_temp(open_id(), open));
    changed
}

fn blank_step(gate: bool) -> Step {
    let kind = if gate {
        StepKind::Gate {
            message: String::new(),
        }
    } else {
        StepKind::Prompt {
            text: String::new(),
        }
    };
    Step {
        title: String::new(),
        kind,
    }
}

fn empty_state(ui: &mut egui::Ui, palette: &Palette) -> bool {
    let mut clicked = false;
    settings_card(ui, palette, |ui| {
        egui::Frame::new().inner_margin(28.0).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let (tile, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(44.0), egui::Sense::hover());
                icon_tile(ui.painter(), palette, tile, 22.0);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(EMPTY_TITLE)
                        .size(14.0)
                        .family(theme::medium_family(ui.ctx()))
                        .color(palette.text_primary),
                );
                ui.scope(|ui| {
                    ui.set_max_width(EMPTY_TEXT_WIDTH);
                    ui.label(
                        egui::RichText::new(EMPTY_TEXT)
                            .size(DETAIL_SIZE)
                            .color(palette.text_muted),
                    );
                });
                ui.add_space(6.0);
                clicked = primary_button(ui, palette, NEW_WORKFLOW_LABEL);
            });
        });
    });
    clicked
}

fn icon_tile(painter: &egui::Painter, palette: &Palette, rect: egui::Rect, icon: f32) {
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(TILE_RADIUS),
        palette.accent_subtle,
    );
    paint_icon(painter, rect.center(), icon, Icon::Workflow, palette.accent);
}

struct CardOutcome {
    edited: bool,
    action: Option<CardAction>,
}

fn workflow_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    index: usize,
    workflow: &mut Workflow,
    open: bool,
) -> CardOutcome {
    let issues = workflow.issues();
    let mut outcome = CardOutcome {
        edited: false,
        action: None,
    };
    settings_card(ui, palette, |ui| {
        outcome.action = header(ui, palette, workflow, &issues, open);
        if !open {
            return;
        }
        setting_divider(ui, palette);
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: PAD as i8,
                right: PAD as i8,
                top: 14,
                bottom: PAD as i8,
            })
            .show(ui, |ui| {
                outcome.edited = workflow_body(ui, palette, index, workflow, &issues);
            });
    });
    outcome
}

fn display_name(workflow: &Workflow) -> &str {
    match workflow.name.trim() {
        "" => UNTITLED,
        name => name,
    }
}

/// What the folded card reads: the chain of its step titles.
fn step_chain(steps: &[Step]) -> String {
    if steps.is_empty() {
        return "No step yet".to_owned();
    }
    steps
        .iter()
        .enumerate()
        .map(|(index, step)| match (step.title.trim(), &step.kind) {
            ("", StepKind::Gate { .. }) => GATE_LABEL.to_owned(),
            ("", StepKind::Prompt { .. }) => format!("Step {}", index + 1),
            (title, _) => title.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("  →  ")
}

fn header(
    ui: &mut egui::Ui,
    palette: &Palette,
    workflow: &Workflow,
    issues: &[Issue],
    open: bool,
) -> Option<CardAction> {
    let size = egui::vec2(ui.available_width(), HEADER_HEIGHT);
    let (rect, response, hovered) = clickable(ui, size, true);
    let title = display_name(workflow).to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
    if hovered {
        let bottom = if open { 0 } else { CARD_RADIUS };
        let radius = egui::CornerRadius {
            nw: CARD_RADIUS,
            ne: CARD_RADIUS,
            sw: bottom,
            se: bottom,
        };
        ui.painter()
            .rect_filled(rect.shrink(1.0), radius, palette.bg_surface_hover);
    }
    let tile = egui::Rect::from_center_size(
        egui::pos2(rect.left() + PAD + TILE / 2.0, rect.center().y),
        egui::Vec2::splat(TILE),
    );
    icon_tile(ui.painter(), palette, tile, TILE_ICON);

    let mut slot = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(PAD, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let (chevron, _) = slot.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
    let chevron_icon = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    paint_icon(
        slot.painter(),
        chevron.center(),
        14.0,
        chevron_icon,
        palette.text_secondary,
    );
    slot.add_space(6.0);
    status_chip(&mut slot, palette, issues);
    let mut action = response.clicked().then_some(CardAction::Toggle);
    if open {
        slot.add_space(8.0);
        let armed_id = slot.id().with("delete-armed");
        let armed = slot.data(|d| d.get_temp::<bool>(armed_id).unwrap_or(false));
        if armed {
            if danger_button(&mut slot, palette, CONFIRM_DELETE_LABEL) {
                action = Some(CardAction::Delete);
            }
        } else if icon_button(&mut slot, palette, Icon::Trash2, DELETE_LABEL, true) {
            slot.data_mut(|d| d.insert_temp(armed_id, true));
        }
        if icon_button(&mut slot, palette, Icon::Copy, DUPLICATE_LABEL, true) {
            action = Some(CardAction::Duplicate);
        }
        if response.clicked() {
            slot.data_mut(|d| d.remove::<bool>(armed_id));
        }
    }
    let text_left = tile.right() + 12.0;
    let text_right = slot.min_rect().left() - 12.0;
    let painter = ui.painter();
    let name = single_line(
        painter,
        &title,
        egui::FontId::new(TITLE_SIZE, theme::medium_family(ui.ctx())),
        palette.text_primary,
        text_right - text_left,
    );
    let chain = single_line(
        painter,
        &step_chain(&workflow.steps),
        egui::FontId::proportional(DETAIL_SIZE),
        palette.text_muted,
        text_right - text_left,
    );
    let block = name.size().y + 3.0 + chain.size().y;
    let top = rect.center().y - block / 2.0;
    painter.galley(
        egui::pos2(text_left, top),
        name.clone(),
        palette.text_primary,
    );
    painter.galley(
        egui::pos2(text_left, top + name.size().y + 3.0),
        chain,
        palette.text_muted,
    );
    action
}

fn single_line(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::text::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(8.0));
    painter.layout_job(job)
}

/// Ready, or what keeps it from launching; hover lists every issue.
fn status_chip(ui: &mut egui::Ui, palette: &Palette, issues: &[Issue]) {
    let errors = issues.iter().filter(|i| i.is_error()).count();
    let warnings = issues.len() - errors;
    let (text, color) = match (errors, warnings) {
        (0, 0) => (READY_LABEL.to_owned(), palette.git_added),
        (0, 1) => ("1 warning".to_owned(), palette.git_modified),
        (0, n) => (format!("{n} warnings"), palette.git_modified),
        (1, _) => ("1 issue".to_owned(), palette.git_deleted),
        (n, _) => (format!("{n} issues"), palette.git_deleted),
    };
    let font = egui::FontId::proportional(SMALL_SIZE);
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font, palette.text_secondary);
    let size = egui::vec2(8.0 + 6.0 + 6.0 + galley.size().x + 9.0, CHIP_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same((CHIP_HEIGHT / 2.0) as u8),
        palette.bg_surface,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    painter.circle_filled(
        egui::pos2(rect.left() + 8.0 + 3.0, rect.center().y),
        3.0,
        color,
    );
    painter.galley(
        egui::pos2(
            rect.left() + 8.0 + 6.0 + 6.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.text_secondary,
    );
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
    if !issues.is_empty() {
        response.on_hover_ui(|ui| {
            for issue in issues {
                ui.label(issue.message());
            }
        });
    }
}

fn workflow_body(
    ui: &mut egui::Ui,
    palette: &Palette,
    index: usize,
    workflow: &mut Workflow,
    issues: &[Issue],
) -> bool {
    ui.spacing_mut().item_spacing.y = 8.0;
    let name = ui.add_sized(
        [ui.available_width(), NAME_HEIGHT],
        egui::TextEdit::singleline(&mut workflow.name)
            .id(name_field_id(index))
            .font(egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())))
            .vertical_align(egui::Align::Center)
            .margin(egui::Margin::symmetric(CONTROL_PAD_X as i8, 0))
            .hint_text(egui::RichText::new("Workflow name").color(palette.text_muted)),
    );
    claim_focus(ui, &name);
    let mut changed = name.changed();
    issue_lines(ui, palette, issues, &Place::Name);

    ui.add_space(10.0);
    section_label(
        ui,
        palette,
        "Inputs",
        "Asked at launch — each one a {key} for the prompts",
    );
    changed |= inputs_editor(ui, palette, &mut workflow.inputs);
    issue_lines(ui, palette, issues, &Place::Inputs);

    ui.add_space(14.0);
    section_label(
        ui,
        palette,
        "Steps",
        "Sent one by one into the same Claude Code session",
    );
    let keys: Vec<String> = workflow
        .inputs
        .iter()
        .map(|input| input.key.clone())
        .filter(|key| is_placeholder_name(key) && !BUILTINS.contains(&key.as_str()))
        .collect();
    changed |= steps_editor(ui, palette, &mut workflow.steps, &keys, issues);
    issue_lines(ui, palette, issues, &Place::Steps);
    changed
}

fn section_label(ui: &mut egui::Ui, palette: &Palette, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .size(SMALL_SIZE)
                .family(theme::medium_family(ui.ctx()))
                .extra_letter_spacing(0.6)
                .color(palette.text_muted),
        );
        ui.label(
            egui::RichText::new(hint)
                .size(DETAIL_SIZE)
                .color(palette.text_muted),
        );
    });
}

fn issue_lines(ui: &mut egui::Ui, palette: &Palette, issues: &[Issue], at: &Place) {
    for issue in issues.iter().filter(|i| place(i) == *at) {
        let (icon, color) = if issue.is_error() {
            (Icon::AlertCircle, palette.git_deleted)
        } else {
            (Icon::AlertTriangle, palette.git_modified)
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
            paint_icon(ui.painter(), rect.center(), 12.0, icon, color);
            ui.label(
                egui::RichText::new(issue.message())
                    .size(DETAIL_SIZE)
                    .color(color),
            );
        });
    }
}

fn inputs_editor(ui: &mut egui::Ui, palette: &Palette, inputs: &mut Vec<Input>) -> bool {
    let list = ui.id().with("inputs");
    let mut changed = false;
    let mut edit = None;
    let count = inputs.len();
    for (index, input) in inputs.iter_mut().enumerate() {
        ui.push_id(("input", index), |ui| {
            let key_id = list.with(("key", index));
            let row = input_row(ui, palette, index, count, input, key_id);
            changed |= row.edited;
            if let Some(e) = row.edit {
                edit = Some((index, e));
            }
        });
    }
    if let Some((index, e)) = edit {
        apply(inputs, index, e);
        changed = true;
    }
    if ghost_button(ui, palette, ADD_INPUT_LABEL) {
        inputs.push(Input {
            key: String::new(),
            label: String::new(),
            default: String::new(),
            kind: InputKind::Text,
        });
        focus_next_frame(ui, list.with(("key", inputs.len() - 1)));
        changed = true;
    }
    changed
}

struct RowOutcome {
    edited: bool,
    edit: Option<ListEdit>,
}

fn inset_card(ui: &mut egui::Ui, palette: &Palette, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(STEP_RADIUS))
        .inner_margin(STEP_PAD)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.visuals_mut().extreme_bg_color = palette.bg_canvas;
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 8.0);
            contents(ui);
        });
}

fn input_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    index: usize,
    count: usize,
    input: &mut Input,
    key_id: egui::Id,
) -> RowOutcome {
    let mut outcome = RowOutcome {
        edited: false,
        edit: None,
    };
    inset_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            row_text(ui, "{", key_font(), palette.text_muted);
            let key = ui.add_sized(
                [KEY_WIDTH, CONTROL_HEIGHT],
                field(&mut input.key, palette, "key")
                    .id(key_id)
                    .font(key_font()),
            );
            claim_focus(ui, &key);
            outcome.edited |= key.changed();
            row_text(ui, "}", key_font(), palette.text_muted);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space(4.0);
            let label_width = ui.available_width() - KIND_WIDTH - 3.0 * (ICON_BUTTON + 6.0) - 6.0;
            outcome.edited |= ui
                .add_sized(
                    [label_width, CONTROL_HEIGHT],
                    field(&mut input.label, palette, "Label asked at launch"),
                )
                .changed();
            outcome.edited |= input_kind(ui, palette, &mut input.kind);
            outcome.edit = list_controls(ui, palette, "input", index, count);
        });
        if let InputKind::Select { options } = &mut input.kind {
            outcome.edited |= options_editor(ui, palette, options);
            outcome.edited |= select_default(ui, palette, options, &mut input.default);
        } else {
            outcome.edited |= ui
                .add_sized(
                    [ui.available_width(), CONTROL_HEIGHT],
                    field(&mut input.default, palette, "Default value (optional)"),
                )
                .changed();
        }
    });
    outcome
}

/// Text centered on a control row, where a label would sit on its top.
fn row_text(ui: &mut egui::Ui, text: &str, font: egui::FontId, color: egui::Color32) {
    let width = text_width(ui, text, &font);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CONTROL_HEIGHT), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
}

fn key_font() -> egui::FontId {
    egui::FontId::monospace(DETAIL_SIZE)
}

fn field<'t>(text: &'t mut String, palette: &Palette, hint: &str) -> egui::TextEdit<'t> {
    egui::TextEdit::singleline(text)
        .background_color(palette.bg_canvas)
        .vertical_align(egui::Align::Center)
        .margin(egui::Margin::symmetric(8, 0))
        .hint_text(egui::RichText::new(hint).color(palette.text_muted))
}

fn kind_presentation(kind: &InputKind) -> (Icon, &'static str) {
    match kind {
        InputKind::Text => (Icon::Type, "Text"),
        InputKind::LongText => (Icon::AlignLeft, "Long text"),
        InputKind::Select { .. } => (Icon::List, "Choice list"),
    }
}

fn input_kind(ui: &mut egui::Ui, palette: &Palette, kind: &mut InputKind) -> bool {
    let kinds = [
        InputKind::Text,
        InputKind::LongText,
        InputKind::Select {
            options: Vec::new(),
        },
    ];
    let (icon, label) = kind_presentation(kind);
    let response = Trigger::new(label)
        .icon(Some(icon))
        .compact()
        .width(KIND_WIDTH)
        .show(ui, palette);
    let choices = kinds.each_ref().map(|choice| {
        let (icon, label) = kind_presentation(choice);
        Choice::new(label).icon(icon)
    });
    let selected = kinds
        .iter()
        .position(|choice| std::mem::discriminant(kind) == std::mem::discriminant(choice));
    let Some(index) = PickOne::new(&choices, selected).show(&response, palette) else {
        return false;
    };
    *kind = kinds[index].clone();
    true
}

fn options_editor(ui: &mut egui::Ui, palette: &Palette, options: &mut Vec<String>) -> bool {
    let list = ui.id().with("options");
    let mut changed = false;
    let mut removed = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        small_caption(ui, palette, "Options");
        for (index, option) in options.iter_mut().enumerate() {
            ui.push_id(("option", index), |ui| {
                let chip = option_chip(ui, palette, option, list.with(("option", index)));
                changed |= chip.edited;
                if chip.edit.is_some() {
                    removed = Some(index);
                }
            });
        }
        if ghost_button(ui, palette, ADD_OPTION_LABEL) {
            options.push(String::new());
            focus_next_frame(ui, list.with(("option", options.len() - 1)));
            changed = true;
        }
    });
    if let Some(index) = removed {
        options.remove(index);
        changed = true;
    }
    changed
}

fn option_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    option: &mut String,
    id: egui::Id,
) -> RowOutcome {
    let mut outcome = RowOutcome {
        edited: false,
        edit: None,
    };
    egui::Frame::new()
        .fill(palette.bg_canvas)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(egui::Margin {
            left: 8,
            right: 4,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let font = egui::FontId::proportional(DETAIL_SIZE);
            let width = ui
                .painter()
                .layout_no_wrap(option.clone(), font.clone(), palette.text_primary)
                .size()
                .x
                .clamp(OPTION_MIN_WIDTH, OPTION_MAX_WIDTH);
            let response = ui.add(
                egui::TextEdit::singleline(option)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .min_size(egui::vec2(0.0, CONTROL_HEIGHT - 2.0))
                    .vertical_align(egui::Align::Center)
                    .font(font)
                    .desired_width(width + 4.0)
                    .hint_text(egui::RichText::new("Option").color(palette.text_muted)),
            );
            claim_focus(ui, &response);
            outcome.edited = response.changed();
            let label = format!("Remove option {}", option.trim());
            if small_icon_button(ui, palette, Icon::X, &label) {
                outcome.edit = Some(ListEdit::Remove);
            }
        });
    outcome
}

fn select_default(
    ui: &mut egui::Ui,
    palette: &Palette,
    options: &[String],
    default: &mut String,
) -> bool {
    let before = default.clone();
    ui.horizontal(|ui| {
        small_caption(ui, palette, "Default");
        let shown = if default.is_empty() {
            FIRST_OPTION
        } else {
            default.as_str()
        };
        let response = Trigger::new(shown)
            .compact()
            .width(KIND_WIDTH)
            .show(ui, palette);
        let named: Vec<&String> = options.iter().filter(|o| !o.trim().is_empty()).collect();
        let choices: Vec<Choice> = std::iter::once(FIRST_OPTION)
            .chain(named.iter().map(|o| o.as_str()))
            .map(Choice::new)
            .collect();
        let selected = if default.is_empty() {
            Some(0)
        } else {
            named.iter().position(|o| *o == default).map(|i| i + 1)
        };
        match PickOne::new(&choices, selected).show(&response, palette) {
            Some(0) => default.clear(),
            Some(index) => default.clone_from(named[index - 1]),
            None => {}
        }
    });
    *default != before
}

fn small_caption(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    let font = egui::FontId::proportional(DETAIL_SIZE);
    row_text(ui, text, font, palette.text_muted);
    ui.add_space(4.0);
}

#[derive(Clone, Copy)]
enum NodeKind {
    Prompt,
    Gate,
    Add,
}

struct Node {
    y: f32,
    kind: NodeKind,
    number: usize,
    faulty: bool,
}

fn steps_editor(
    ui: &mut egui::Ui,
    palette: &Palette,
    steps: &mut Vec<Step>,
    keys: &[String],
    issues: &[Issue],
) -> bool {
    let list = ui.id().with("steps");
    let rail_x = ui.cursor().left() + RAIL / 2.0;
    let mut nodes = Vec::new();
    let mut changed = false;
    let mut edit = None;
    let count = steps.len();
    for (index, step) in steps.iter_mut().enumerate() {
        ui.push_id(("step", index), |ui| {
            let mut outcome = None;
            let frame = beside_rail(ui, |ui| {
                outcome = Some(step_card(
                    ui,
                    palette,
                    &StepContext {
                        index,
                        count,
                        keys,
                        issues,
                        title_id: list.with(("title", index)),
                    },
                    step,
                ));
            });
            let kind = match step.kind {
                StepKind::Prompt { .. } => NodeKind::Prompt,
                StepKind::Gate { .. } => NodeKind::Gate,
            };
            nodes.push(Node {
                y: frame.top() + STEP_PAD + CONTROL_HEIGHT / 2.0,
                kind,
                number: index + 1,
                faulty: issues
                    .iter()
                    .any(|i| i.is_error() && place(i) == Place::Step(index)),
            });
            if let Some(outcome) = outcome {
                changed |= outcome.edited;
                if let Some(e) = outcome.edit {
                    edit = Some((index, e));
                }
            }
        });
        ui.add_space(STEP_GAP - ui.spacing().item_spacing.y);
    }
    let mut added = None;
    let add_row = beside_rail(ui, |ui| {
        ui.horizontal(|ui| {
            if ghost_button(ui, palette, ADD_PROMPT_LABEL) {
                added = Some(false);
            }
            if ghost_button(ui, palette, ADD_GATE_LABEL) {
                added = Some(true);
            }
        });
    });
    nodes.push(Node {
        y: add_row.center().y,
        kind: NodeKind::Add,
        number: 0,
        faulty: false,
    });
    paint_rail(ui.painter(), palette, rail_x, &nodes);
    if let Some((index, e)) = edit {
        apply(steps, index, e);
        changed = true;
    }
    if let Some(gate) = added {
        steps.push(blank_step(gate));
        focus_next_frame(ui, list.with(("title", steps.len() - 1)));
        changed = true;
    }
    changed
}

fn beside_rail(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) -> egui::Rect {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: RAIL as i8,
            ..Default::default()
        })
        .show(ui, contents)
        .response
        .rect
}

fn gate_tint(palette: &Palette) -> egui::Color32 {
    palette.git_modified.gamma_multiply(0.18)
}

fn paint_rail(painter: &egui::Painter, palette: &Palette, x: f32, nodes: &[Node]) {
    let line = egui::Stroke::new(1.5_f32, palette.border_input);
    for pair in nodes.windows(2) {
        painter.line_segment(
            [
                egui::pos2(x, pair[0].y + NODE_RADIUS),
                egui::pos2(x, pair[1].y - NODE_RADIUS),
            ],
            line,
        );
    }
    for node in nodes {
        let center = egui::pos2(x, node.y);
        let (fill, ring) = match node.kind {
            NodeKind::Prompt => (palette.accent_subtle, palette.accent),
            NodeKind::Gate => (gate_tint(palette), palette.git_modified),
            NodeKind::Add => (palette.bg_canvas, palette.border_input),
        };
        let ring = if node.faulty {
            palette.git_deleted
        } else {
            ring
        };
        painter.circle(center, NODE_RADIUS, fill, egui::Stroke::new(1.0_f32, ring));
        match node.kind {
            NodeKind::Prompt => {
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    node.number.to_string(),
                    egui::FontId::proportional(SMALL_SIZE),
                    ring,
                );
            }
            NodeKind::Gate => paint_icon(painter, center, 12.0, Icon::Hand, ring),
            NodeKind::Add => paint_icon(painter, center, 12.0, Icon::Plus, palette.text_muted),
        }
    }
}

struct StepContext<'a> {
    index: usize,
    count: usize,
    keys: &'a [String],
    issues: &'a [Issue],
    title_id: egui::Id,
}

fn step_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    context: &StepContext<'_>,
    step: &mut Step,
) -> RowOutcome {
    let mut outcome = RowOutcome {
        edited: false,
        edit: None,
    };
    inset_card(ui, palette, |ui| {
        ui.horizontal(|ui| {
            outcome.edited |= kind_toggle(ui, palette, &mut step.kind);
            ui.add_space(4.0);
            let width = ui.available_width() - 3.0 * (ICON_BUTTON + 6.0);
            let title = ui.add_sized(
                [width, CONTROL_HEIGHT],
                egui::TextEdit::singleline(&mut step.title)
                    .id(context.title_id)
                    .frame(egui::Frame::NONE)
                    .vertical_align(egui::Align::Center)
                    .font(egui::FontId::new(TEXT_SIZE, theme::medium_family(ui.ctx())))
                    .hint_text(egui::RichText::new("Step title").color(palette.text_muted)),
            );
            claim_focus(ui, &title);
            outcome.edited |= title.changed();
            outcome.edit = list_controls(ui, palette, "step", context.index, context.count);
        });
        let body_id = ui.id().with("body");
        let (text, hint, rows) = match &mut step.kind {
            StepKind::Prompt { text } => (text, "Prompt sent to Claude", PROMPT_ROWS),
            StepKind::Gate { message } => (
                message,
                "Shown while the run pauses — Continue resumes it",
                GATE_ROWS,
            ),
        };
        outcome.edited |= ui
            .add(
                egui::TextEdit::multiline(text)
                    .id(body_id)
                    .background_color(palette.bg_canvas)
                    .desired_rows(rows)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 6))
                    .hint_text(egui::RichText::new(hint).color(palette.text_muted)),
            )
            .changed();
        outcome.edited |= placeholder_chips(ui, palette, context.keys, text, body_id);
        issue_lines(ui, palette, context.issues, &Place::Step(context.index));
    });
    outcome
}

/// Inputs first (accent), then the built-ins; a click inserts at the caret.
fn placeholder_chips(
    ui: &mut egui::Ui,
    palette: &Palette,
    keys: &[String],
    text: &mut String,
    field: egui::Id,
) -> bool {
    let mut inserted = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let tokens = keys
            .iter()
            .map(|key| (key.as_str(), Tone::Input))
            .chain(BUILTINS.iter().map(|key| (*key, Tone::Builtin)));
        for (key, tone) in tokens {
            let token = format!("{{{key}}}");
            if chip_button(ui, palette, &token, tone) {
                insert_at_caret(ui.ctx(), field, text, &token);
                inserted = true;
            }
        }
    });
    inserted
}

fn insert_at_caret(ctx: &egui::Context, field: egui::Id, text: &mut String, token: &str) {
    let mut state = egui::TextEdit::load_state(ctx, field).unwrap_or_default();
    let end = text.chars().count();
    let [from, to] = state.cursor.char_range().map_or([end, end], |range| {
        range.sorted_cursors().map(|c| c.index.min(end))
    });
    let byte = |at: usize| text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
    let (start, stop) = (byte(from), byte(to));
    text.replace_range(start..stop, token);
    let caret = egui::text::CCursor::new(from + token.chars().count());
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(caret)));
    state.store(ctx, field);
    ctx.memory_mut(|m| m.request_focus(field));
}

fn kind_toggle(ui: &mut egui::Ui, palette: &Palette, kind: &mut StepKind) -> bool {
    let is_gate = matches!(kind, StepKind::Gate { .. });
    let segments = [
        (false, Icon::MessageSquare, PROMPT_LABEL),
        (true, Icon::Hand, GATE_LABEL),
    ];
    let (selected_ink, selected_fill) = if is_gate {
        (palette.git_modified, gate_tint(palette))
    } else {
        (palette.accent, palette.accent_subtle)
    };
    let font = egui::FontId::proportional(DETAIL_SIZE);
    let widths: Vec<f32> = segments
        .iter()
        .map(|(_, _, label)| {
            let text = ui
                .painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), palette.text_primary)
                .size()
                .x;
            8.0 + 12.0 + 5.0 + text + 9.0
        })
        .collect();
    let group = egui::vec2(widths.iter().sum(), CONTROL_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(group, egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(CONTROL_RADIUS + 1),
        palette.bg_canvas,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let mut want_gate = is_gate;
    let mut left = rect.left();
    for ((gate, icon, label), width) in segments.into_iter().zip(widths) {
        let segment = egui::Rect::from_min_size(
            egui::pos2(left, rect.top()),
            egui::vec2(width, rect.height()),
        );
        left += width;
        let selected = gate == is_gate;
        let response = ui
            .interact(segment, ui.id().with(("kind", label)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
        });
        if response.clicked() {
            want_gate = gate;
        }
        let ink = if selected {
            selected_ink
        } else if response.hovered() {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        if selected {
            ui.painter().rect_filled(
                segment.shrink(2.0),
                egui::CornerRadius::same(CONTROL_RADIUS - 1),
                selected_fill,
            );
        }
        paint_icon(
            ui.painter(),
            egui::pos2(segment.left() + 8.0 + 6.0, segment.center().y),
            12.0,
            icon,
            ink,
        );
        ui.painter().text(
            egui::pos2(segment.left() + 8.0 + 12.0 + 5.0, segment.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            ink,
        );
    }
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

fn list_controls(
    ui: &mut egui::Ui,
    palette: &Palette,
    what: &str,
    index: usize,
    count: usize,
) -> Option<ListEdit> {
    let n = index + 1;
    let mut edit = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let remove = format!("Remove {what} {n}");
        if icon_button(ui, palette, Icon::Trash2, &remove, true) {
            edit = Some(ListEdit::Remove);
        }
        let down = format!("Move {what} {n} down");
        if icon_button(ui, palette, Icon::ArrowDown, &down, index + 1 < count) {
            edit = Some(ListEdit::Down);
        }
        let up = format!("Move {what} {n} up");
        if icon_button(ui, palette, Icon::ArrowUp, &up, index > 0) {
            edit = Some(ListEdit::Up);
        }
    });
    edit
}

fn icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    enabled: bool,
) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::Vec2::splat(ICON_BUTTON), enabled);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(CONTROL_RADIUS),
            palette.border_subtle,
        );
    }
    let ink = match (enabled, hovered) {
        (false, _) => palette.state_disabled,
        (true, true) => palette.text_primary,
        (true, false) => palette.text_secondary,
    };
    paint_icon(ui.painter(), rect.center(), ICON_SIZE, icon, ink);
    let label = label.to_owned();
    response
        .widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    response.clicked()
}

fn small_icon_button(ui: &mut egui::Ui, palette: &Palette, icon: Icon, label: &str) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::Vec2::splat(18.0), true);
    let ink = if hovered {
        palette.git_deleted
    } else {
        palette.text_muted
    };
    paint_icon(ui.painter(), rect.center(), 11.0, icon, ink);
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
        .size()
        .x
}

/// Dashed-feel add button: a plus and its label, outlined, filled on hover.
fn ghost_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> bool {
    let font = egui::FontId::proportional(DETAIL_SIZE);
    let width = CONTROL_PAD_X + 12.0 + 6.0 + text_width(ui, label, &font) + CONTROL_PAD_X;
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, CONTROL_HEIGHT), true);
    let (fill, ink) = if hovered {
        (palette.bg_surface_hover, palette.text_primary)
    } else {
        (egui::Color32::TRANSPARENT, palette.text_secondary)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(CONTROL_RADIUS),
        fill,
        egui::Stroke::new(1.0_f32, palette.border_input),
        egui::StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + CONTROL_PAD_X + 6.0, rect.center().y),
        12.0,
        Icon::Plus,
        ink,
    );
    ui.painter().text(
        egui::pos2(rect.left() + CONTROL_PAD_X + 18.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        ink,
    );
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

fn primary_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> bool {
    let font = egui::FontId::new(TEXT_SIZE, theme::medium_family(ui.ctx()));
    let width = 14.0 + 12.0 + 6.0 + text_width(ui, label, &font) + 14.0;
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, 32.0), true);
    let fill = if hovered {
        palette.primary_button_hover()
    } else {
        palette.primary_button_fill()
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(theme::RADIUS_PILL), fill);
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 14.0 + 6.0, rect.center().y),
        13.0,
        Icon::Plus,
        egui::Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 14.0 + 18.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        egui::Color32::WHITE,
    );
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

fn danger_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> bool {
    let font = egui::FontId::proportional(DETAIL_SIZE);
    let width = CONTROL_PAD_X * 2.0 + text_width(ui, label, &font);
    let (rect, response, _) = clickable(ui, egui::vec2(width, ICON_BUTTON), true);
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(CONTROL_RADIUS),
        palette.git_deleted,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        egui::Color32::WHITE,
    );
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

#[derive(Clone, Copy)]
enum Tone {
    Input,
    Builtin,
}

/// Placeholder token: an input's stands out, a built-in stays quiet.
fn chip_button(ui: &mut egui::Ui, palette: &Palette, label: &str, tone: Tone) -> bool {
    let font = egui::FontId::monospace(SMALL_SIZE);
    let width = CHIP_PAD_X * 2.0 + text_width(ui, label, &font);
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, CHIP_HEIGHT), true);
    let (fill, ink) = match (tone, hovered) {
        (Tone::Input, false) => (palette.accent_subtle, palette.accent),
        (Tone::Input, true) => (palette.accent_subtle, palette.accent_hover),
        (Tone::Builtin, false) => (egui::Color32::TRANSPARENT, palette.text_muted),
        (Tone::Builtin, true) => (palette.border_subtle, palette.text_primary),
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(CONTROL_RADIUS), fill);
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, font, ink);
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}
