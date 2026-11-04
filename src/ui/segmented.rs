//! The compact segmented control (design-system segmented control): one bordered
//! group of pills, the live one filled `accent.subtle` with `accent` ink, so the
//! choice reads as a selected segment rather than a faint colour shift.

use crate::theme::{Palette, RADIUS_BUTTON};
use crate::ui::clickable;

const LABEL_SIZE: f32 = 12.5;
const SEGMENT_PAD: egui::Vec2 = egui::vec2(18.0, 8.0);

/// Draws `segments` left to right whatever the layout it lands in; returns the
/// segment clicked this frame when it is not `selected` already.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    palette: &Palette,
    segments: &[(T, &str)],
    selected: T,
) -> Option<T> {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON + 1))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let right_to_left = ui.layout().prefer_right_to_left();
            let ordered: Vec<&(T, &str)> = if right_to_left {
                segments.iter().rev().collect()
            } else {
                segments.iter().collect()
            };
            let mut picked = None;
            for (value, label) in ordered {
                if segment(ui, palette, label, *value == selected) && *value != selected {
                    picked = Some(*value);
                }
            }
            picked
        })
        .inner
}

fn segment(ui: &mut egui::Ui, palette: &Palette, label: &str, active: bool) -> bool {
    let font = egui::FontId::proportional(LABEL_SIZE);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let size = galley.size() + SEGMENT_PAD;
    let (rect, response, hovered) = clickable(ui, size, true);
    let fill = if active {
        palette.accent_subtle
    } else if hovered {
        palette.bg_surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    }
    let color = if active {
        palette.accent
    } else {
        palette.text_muted
    };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label));
    response.clicked()
}
