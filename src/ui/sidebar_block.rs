use std::f32::consts::{FRAC_PI_2, PI};

use crate::theme::Palette;

/// Space between a right sidebar block and the window edges, and between blocks.
pub(crate) const BLOCK_GAP: i8 = 8;
pub(crate) const BLOCK_RADIUS: u8 = 12;
const CORNER_STEPS: u32 = 8;

/// Frame of a right sidebar panel: the ground around its block, the gap on every
/// side but the left, where the terminals or the next block already leave it.
pub(crate) fn block_panel_frame(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.bg_canvas)
        .inner_margin(egui::Margin {
            left: 0,
            right: BLOCK_GAP,
            top: BLOCK_GAP,
            bottom: BLOCK_GAP,
        })
}

/// A rounded, outlined block filling the rest of `ui`; `contents` lays out inside
/// `padding`. Whatever `contents` paints square to the edges is rounded off.
pub(crate) fn sidebar_block<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    padding: egui::Margin,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let rect = ui.available_rect_before_wrap();
    let radius = egui::CornerRadius::same(BLOCK_RADIUS);
    let painter = ui.painter().clone();
    painter.add(block_shadow(palette).as_shape(rect, radius));
    painter.rect_filled(rect, radius, palette.bg_canvas);
    let mut block = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect - padding)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    block.set_clip_rect(rect.intersect(ui.clip_rect()));
    let inner = contents(&mut block);
    paint_corner_masks(&painter, rect, f32::from(BLOCK_RADIUS), palette.bg_canvas);
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.advance_cursor_after_rect(rect);
    inner
}

fn block_shadow(palette: &Palette) -> egui::Shadow {
    let alpha = if palette.dark { 80 } else { 18 };
    egui::Shadow {
        offset: [0, 2],
        blur: 12,
        spread: 0,
        color: crate::ui::with_alpha(egui::Color32::BLACK, alpha),
    }
}

/// Paints `ground` over each corner of `rect` outside its rounding.
fn paint_corner_masks(
    painter: &egui::Painter,
    rect: egui::Rect,
    radius: f32,
    ground: egui::Color32,
) {
    let corners = [
        (rect.left_top(), egui::vec2(radius, radius), PI),
        (rect.right_top(), egui::vec2(-radius, radius), 1.5 * PI),
        (rect.right_bottom(), egui::vec2(-radius, -radius), 0.0),
        (rect.left_bottom(), egui::vec2(radius, -radius), FRAC_PI_2),
    ];
    let mut mesh = egui::Mesh::default();
    for (corner, to_center, start) in corners {
        let center = corner + to_center;
        let hub = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, ground);
        for step in 0..=CORNER_STEPS {
            let angle = start + FRAC_PI_2 * step as f32 / CORNER_STEPS as f32;
            mesh.colored_vertex(
                center + radius * egui::vec2(angle.cos(), angle.sin()),
                ground,
            );
        }
        for step in 0..CORNER_STEPS {
            mesh.add_triangle(hub, hub + 1 + step, hub + 2 + step);
        }
    }
    painter.add(mesh);
}
