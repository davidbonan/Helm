use crate::theme::Palette;

use super::{clickable, paint_icon, shortcut_badge, workspace_sidebar_fill, SIDEBAR_PAD_X};

/// Phone access is on (specs/remote.md §2): who is connected, for the sidebar footer.
pub struct PhoneIndicator {
    pub devices: Vec<String>,
}

pub const PHONE_ACCESS_LABEL: &str = "Phone access";
pub(crate) const FOOTER_HEIGHT: f32 = 44.0;
const BUTTON_HEIGHT: f32 = 28.0;
const ICON_BUTTON_WIDTH: f32 = 28.0;
const BUTTON_RADIUS: u8 = 6;
const GLYPH: f32 = 15.0;
const PHONE_LABEL: &str = "Phone";
const PHONE_LABEL_SIZE: f32 = 12.5;
const PHONE_PAD_X: f32 = 8.0;
const PHONE_LABEL_GAP: f32 = 8.0;
const STATUS_DOT_RADIUS: f32 = 3.5;
const STATUS_DOT_RING: f32 = 1.5;
/// Opacity at rest: a hint, not a status light.
const STATUS_DOT_FADE: f32 = 0.45;

pub(crate) struct SidebarFooter<'a> {
    pub(crate) phone: Option<&'a PhoneIndicator>,
    pub(crate) open_phone: &'a mut bool,
    pub(crate) open_feedback: &'a mut bool,
    pub(crate) open_preferences: &'a mut bool,
    /// Painted under the gear while Cmd is held.
    pub(crate) preferences_badge: Option<String>,
}

/// Bottom row of the workspace sidebar: phone access on the left, feedback and
/// preferences on the right, under a rule spanning the sidebar edge to edge.
pub(crate) fn sidebar_footer(ui: &mut egui::Ui, palette: &Palette, footer: SidebarFooter<'_>) {
    let area = ui.max_rect();
    let pad = f32::from(SIDEBAR_PAD_X);
    ui.painter().hline(
        (area.left() - pad)..=(area.right() + pad),
        area.top(),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if let Some(phone) = footer.phone {
                if phone_button(ui, palette, phone).clicked() {
                    *footer.open_phone = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let prefs = preferences_button(ui, palette, footer.open_preferences);
                if let Some(badge) = footer.preferences_badge {
                    shortcut_badge(ui, palette, "shortcut_badge_prefs", &badge, prefs.rect);
                }
                if feedback_button(ui, palette).clicked() {
                    *footer.open_feedback = true;
                }
            });
        },
    );
}

fn phone_indicator_tooltip(devices: &[String]) -> String {
    match devices {
        [] => "Phone access on — no phone connected".to_owned(),
        names => format!("Phone access on — {} connected", names.join(", ")),
    }
}

/// Phone icon with a status dot — green while a phone is connected, muted while
/// access waits for one, faded until hovered. A click opens the pairing modal.
fn phone_button(ui: &mut egui::Ui, palette: &Palette, phone: &PhoneIndicator) -> egui::Response {
    let label = ui.painter().layout_no_wrap(
        PHONE_LABEL.to_owned(),
        egui::FontId::proportional(PHONE_LABEL_SIZE),
        palette.text_secondary,
    );
    let width = 2.0 * PHONE_PAD_X + GLYPH + PHONE_LABEL_GAP + label.size().x;
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, BUTTON_HEIGHT), true);
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(BUTTON_RADIUS),
            palette.bg_surface_hover,
        );
    }
    let icon_center = egui::pos2(rect.left() + PHONE_PAD_X + GLYPH / 2.0, rect.center().y);
    paint_icon(
        painter,
        icon_center,
        GLYPH,
        lucide_icons::Icon::Smartphone,
        palette.text_muted,
    );
    let status = if phone.devices.is_empty() {
        palette.text_muted
    } else {
        palette.git_added
    };
    let status = if hovered {
        status
    } else {
        status.gamma_multiply(STATUS_DOT_FADE)
    };
    painter.circle(
        icon_center + egui::vec2(GLYPH * 0.38, -GLYPH * 0.42),
        STATUS_DOT_RADIUS,
        status,
        egui::Stroke::new(STATUS_DOT_RING, workspace_sidebar_fill(palette)),
    );
    painter.galley(
        egui::pos2(
            icon_center.x + GLYPH / 2.0 + PHONE_LABEL_GAP,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        palette.text_secondary,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, PHONE_ACCESS_LABEL)
    });
    response.on_hover_text(phone_indicator_tooltip(&phone.devices))
}

/// Bug/suggestion report (specs/feedback.md): opens the feedback modal.
fn feedback_button(ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(ICON_BUTTON_WIDTH, BUTTON_HEIGHT), true);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Send feedback"));
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(BUTTON_RADIUS),
            palette.bg_surface_hover,
        );
    }
    paint_icon(
        painter,
        rect.center(),
        GLYPH,
        lucide_icons::Icon::Bug,
        palette.text_muted,
    );
    response
}

fn preferences_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    open_preferences: &mut bool,
) -> egui::Response {
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(ICON_BUTTON_WIDTH, BUTTON_HEIGHT), true);
    if response.clicked() {
        *open_preferences = !*open_preferences;
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open preferences")
    });
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(BUTTON_RADIUS),
            palette.bg_surface_hover,
        );
    }
    let color = if *open_preferences {
        palette.text_primary
    } else {
        palette.text_muted
    };
    paint_icon(
        painter,
        rect.center(),
        GLYPH,
        lucide_icons::Icon::Settings,
        color,
    );
    response
}
