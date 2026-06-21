//! Rendering of the bundled release notes (update.md §9.4): the markdown body is
//! shared by the boot What's new modal and the Preferences › Updates block, so
//! both surfaces use the same widget. The notes themselves live in
//! [`crate::release_notes`] (domain).

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

const MODAL_WIDTH: f32 = 520.0;
const MODAL_MAX_HEIGHT: f32 = 460.0;

/// Renders the bundled notes as markdown into the current `Ui`. Links open in the
/// browser (egui hyperlink). The caller owns the shared [`CommonMarkCache`] so the
/// layout is memoised across frames and surfaces.
pub fn body(ui: &mut egui::Ui, cache: &mut CommonMarkCache) {
    CommonMarkViewer::new().show(ui, cache, crate::release_notes::RELEASE_NOTES);
}

/// The boot "What's new" modal (update.md §9.4): centered, scrollable notes with a
/// Close button. Returns `true` on Close (button, `Esc`, or click outside).
pub fn modal(ui: &mut egui::Ui, cache: &mut CommonMarkCache) -> bool {
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("whats_new_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(MODAL_WIDTH);
            ui.heading("What's new");
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(MODAL_MAX_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, |ui| body(ui, cache));
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
    close || modal.should_close()
}
