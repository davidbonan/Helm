//! Feedback modal (specs/feedback.md): a Suggestion/Bug dropdown + a
//! description box, filed as a GitHub issue on the helm repo. Pure rendering —
//! the caller owns the page state and the submit; the modal only reports the
//! user's intent.

use crate::feedback::FeedbackKind;
use crate::theme::Palette;

const MODAL_WIDTH: f32 = 420.0;
const TITLE_SIZE: f32 = 14.0;
const TEXT_SIZE: f32 = 13.0;
const KIND_COMBO_W: f32 = 150.0;
const DESCRIPTION_ROWS: usize = 5;
const DESCRIPTION_HINT: &str = "What happened, or what would you like to see?";
const TITLE: &str = "Send feedback";
pub const SEND_LABEL: &str = "Send";
const CANCEL_LABEL: &str = "Cancel";

/// Modal state owned by `HelmApp`: the chosen kind (defaults to Bug) and the
/// description being typed.
pub struct FeedbackPage {
    pub kind: FeedbackKind,
    pub description: String,
}

impl Default for FeedbackPage {
    fn default() -> Self {
        Self {
            kind: FeedbackKind::Bug,
            description: String::new(),
        }
    }
}

impl FeedbackPage {
    fn sendable(&self) -> bool {
        !self.description.trim().is_empty()
    }
}

/// Signals emitted by the modal within a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct FeedbackModalAction {
    /// Open the GitHub issue form (the button is disabled otherwise).
    pub submit: bool,
    /// Close without sending (Cancel, `Esc`, click outside).
    pub dismiss: bool,
}

/// Renders the modal; Send is disabled until the description is non-blank.
pub fn feedback_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &mut FeedbackPage,
) -> FeedbackModalAction {
    let mut action = FeedbackModalAction::default();
    let modal = egui::Modal::new(egui::Id::new("feedback_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(MODAL_WIDTH);
            ui.label(
                egui::RichText::new(TITLE)
                    .size(TITLE_SIZE)
                    .color(palette.text_primary)
                    .strong(),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Opens a pre-filled GitHub issue you submit.")
                    .size(TEXT_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Type")
                        .size(TEXT_SIZE)
                        .color(palette.text_primary),
                );
                egui::ComboBox::from_id_salt("feedback_kind")
                    .width(KIND_COMBO_W)
                    .selected_text(page.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in FeedbackKind::ALL {
                            ui.selectable_value(&mut page.kind, kind, kind.label());
                        }
                    });
            });
            ui.add_space(10.0);

            ui.label(
                egui::RichText::new("Description")
                    .size(TEXT_SIZE)
                    .color(palette.text_primary),
            );
            ui.add_space(2.0);
            ui.add(
                egui::TextEdit::multiline(&mut page.description)
                    .desired_rows(DESCRIPTION_ROWS)
                    .desired_width(f32::INFINITY)
                    .hint_text(egui::RichText::new(DESCRIPTION_HINT).color(palette.text_muted)),
            );

            ui.add_space(12.0);
            footer(ui, palette, page, &mut action);
        });
    if modal.should_close() {
        action.dismiss = true;
    }
    action
}

fn footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &FeedbackPage,
    action: &mut FeedbackModalAction,
) {
    ui.horizontal(|ui| {
        if ui.button(CANCEL_LABEL).clicked() {
            action.dismiss = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let send = ui.add_enabled(
                page.sendable(),
                egui::Button::new(egui::RichText::new(SEND_LABEL).color(egui::Color32::WHITE))
                    .fill(palette.primary_button_fill()),
            );
            if send.clicked() {
                action.submit = true;
            }
        });
    });
}
