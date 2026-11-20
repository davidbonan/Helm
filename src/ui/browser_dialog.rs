//! The card a page's `alert`, `confirm` or `prompt` waits on (browser.md §12),
//! over the frozen page.

use crate::browser::dialog::{DialogAnswer, DialogKind, PageDialog};
use crate::theme::Palette;
use crate::ui::with_alpha;

const CARD_WIDTH: f32 = 420.0;
const CARD_TOP: f32 = 16.0;
const CARD_MARGIN: f32 = 12.0;
const MESSAGE_MAX_HEIGHT: f32 = 260.0;
const TITLE_SIZE: f32 = 13.0;
const TEXT_SIZE: f32 = 13.0;
const SCRIM_ALPHA: u8 = 50;
pub const OK_LABEL: &str = "OK";
pub const CANCEL_LABEL: &str = "Cancel";

/// The dialog over `area`, and the prompt's text, seeded with its default on
/// the first frame.
pub(crate) struct DialogCard<'a> {
    pub(crate) dialog: &'a PageDialog,
    pub(crate) field: &'a mut Option<String>,
    pub(crate) area: egui::Rect,
}

pub(crate) struct DialogOutcome {
    pub(crate) answer: Option<DialogAnswer>,
    /// The prompt's field holds the keyboard.
    pub(crate) field_focused: bool,
}

/// `Enter` answers OK, `Esc` Cancel.
pub(crate) fn page_dialog(
    ui: &mut egui::Ui,
    palette: &Palette,
    card: DialogCard<'_>,
) -> DialogOutcome {
    let DialogCard {
        dialog,
        field,
        area,
    } = card;
    ui.painter()
        .rect_filled(area, 0.0, with_alpha(egui::Color32::BLACK, SCRIM_ALPHA));
    // The page beneath takes no click while it waits.
    ui.interact(
        area,
        ui.id().with("page_dialog_scrim"),
        egui::Sense::click(),
    );
    let width = CARD_WIDTH.min(area.width() - 2.0 * CARD_MARGIN).max(0.0);
    let card_rect = egui::Rect::from_min_size(
        egui::pos2(area.center().x - width / 2.0, area.top() + CARD_TOP),
        egui::vec2(width, area.height() - CARD_TOP - CARD_MARGIN),
    );
    let seeded = field.is_none();
    let text = field.get_or_insert_with(|| match &dialog.kind {
        DialogKind::Prompt { default } => default.clone(),
        _ => String::new(),
    });
    let mut outcome = DialogOutcome {
        answer: keyboard_answer(ui, text),
        field_focused: false,
    };
    let mut card_ui = ui.new_child(egui::UiBuilder::new().max_rect(card_rect));
    crate::ui::modal_frame(card_ui.style()).show(&mut card_ui, |ui| {
        crate::ui::modal_controls_style(ui);
        ui.set_width(ui.available_width());
        ui.label(
            egui::RichText::new(title(dialog))
                .size(TITLE_SIZE)
                .color(palette.text_primary)
                .strong(),
        );
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(MESSAGE_MAX_HEIGHT)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&dialog.message)
                            .size(TEXT_SIZE)
                            .color(palette.text_secondary),
                    )
                    .wrap(),
                );
            });
        if matches!(dialog.kind, DialogKind::Prompt { .. }) {
            ui.add_space(10.0);
            let response = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
            if seeded {
                response.request_focus();
            }
            outcome.field_focused = response.has_focus();
        }
        ui.add_space(14.0);
        if let Some(answer) = footer(ui, palette, dialog, text) {
            outcome.answer = Some(answer);
        }
    });
    if outcome.answer.is_some() {
        *field = None;
    }
    outcome
}

fn title(dialog: &PageDialog) -> String {
    if dialog.site.is_empty() {
        "This page says".to_owned()
    } else {
        format!("{} says", dialog.site)
    }
}

fn keyboard_answer(ui: &egui::Ui, text: &str) -> Option<DialogAnswer> {
    let (enter, escape) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    if enter {
        return Some(DialogAnswer::Accept(text.to_owned()));
    }
    escape.then_some(DialogAnswer::Cancel)
}

fn footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    dialog: &PageDialog,
    text: &str,
) -> Option<DialogAnswer> {
    let mut answer = None;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = egui::Button::new(egui::RichText::new(OK_LABEL).color(egui::Color32::WHITE))
                .fill(palette.primary_button_fill())
                .min_size(egui::vec2(64.0, 0.0));
            if ui.add(ok).clicked() {
                answer = Some(DialogAnswer::Accept(text.to_owned()));
            }
            if dialog.kind != DialogKind::Alert && ui.button(CANCEL_LABEL).clicked() {
                answer = Some(DialogAnswer::Cancel);
            }
        });
    });
    answer
}
