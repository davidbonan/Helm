//! Shared leaf primitives for the right-hand detail panels — the commit-detail
//! sidebar (git.md §9) and the PR review rail (pull-requests.md §11) — so both
//! render the author avatar and the count chip in one visual language.

use crate::theme::{Palette, RADIUS_PILL};
use crate::ui::graph_view::initials;

const AVATAR_SIZE: f32 = 30.0;
const AVATAR_INITIALS_SIZE: f32 = 11.0;
const COUNT_CHIP_SIZE: f32 = 11.0;

/// Avatar dot: the author's initials (same rules as the graph bubble) on a
/// stable color derived from the name — drawn from the lane palette, whose
/// `lane_node_text` ink is already designed for this background.
pub fn author_avatar(ui: &mut egui::Ui, palette: &Palette, author: &str) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(AVATAR_SIZE, AVATAR_SIZE), egui::Sense::hover());
    let hash = author.bytes().fold(0usize, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    ui.painter()
        .circle_filled(rect.center(), AVATAR_SIZE / 2.0, palette.lane_color(hash));
    let text = initials(author);
    if !text.is_empty() {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &text,
            egui::FontId::proportional(AVATAR_INITIALS_SIZE),
            palette.lane_node_text,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
}

/// Bordered count chip beside a section title (e.g. the "Files changed" band).
pub fn count_chip(ui: &mut egui::Ui, palette: &Palette, count: usize) {
    let text = count.to_string();
    let galley = ui.painter().layout_no_wrap(
        text.clone(),
        egui::FontId::proportional(COUNT_CHIP_SIZE),
        egui::Color32::PLACEHOLDER,
    );
    let size = galley.size() + egui::vec2(10.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        palette.bg_surface,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
}
