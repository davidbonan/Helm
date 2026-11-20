//! The browser over the whole window (browser.md §15): grown out of where it sat,
//! under a thin title strip in the toolbar's tint.

use lucide_icons::Icon;

use crate::ui::browser_panel::{
    browser_panel, child_at, BarColors, BrowserChrome, BrowserIntent, BrowserPanelOutput,
    BrowserPanelView,
};
use crate::ui::with_alpha;

const STRIP_HEIGHT: f32 = crate::ui::TITLEBAR_HEIGHT as f32;
const HOME_RADIUS: f32 = crate::ui::sidebar_block::BLOCK_RADIUS as f32;
/// The window behind, dimmed at most this much.
const SCRIM_ALPHA: f32 = 70.0;
/// The lift's shadow, strongest halfway.
const LIFT_ALPHA: f32 = 120.0;
const BADGE_SIZE: f32 = 20.0;
const TEXT_SIZE: f32 = 12.5;
const CHIP_HEIGHT: f32 = 22.0;
const CHIP_PAD_X: f32 = 8.0;
const EXIT_HEIGHT: f32 = 26.0;
const EXIT_PAD_X: f32 = 10.0;
const KEYCAP_SIZE: f32 = 11.0;
const STRIP_PAD_RIGHT: f32 = 10.0;
pub const EXIT_LABEL: &str = "Exit full window";

/// How far the browser has grown, and what the strip names.
pub(crate) struct FullWindowStage<'a> {
    /// 0 where it sat, 1 over the whole window.
    pub(crate) progress: f32,
    /// Where it sat: the sidebar block, the floating panel, or the window's edge.
    pub(crate) home: egui::Rect,
    pub(crate) project: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) exit_shortcut: Option<&'a str>,
}

/// The shown tab's panel, and how far it has grown.
pub(crate) struct FullWindowView<'a> {
    pub(crate) panel: BrowserPanelView<'a>,
    pub(crate) stage: FullWindowStage<'a>,
}

pub(crate) struct FullWindowOutput {
    pub(crate) panel: BrowserPanelOutput,
    /// Where the browser is drawn this frame.
    pub(crate) frame: egui::Rect,
}

pub(crate) fn browser_full_window(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: FullWindowView<'_>,
) -> FullWindowOutput {
    let FullWindowView { panel: view, stage } = view;
    let stage = &stage;
    let ctx = ui.ctx().clone();
    let window = ctx.content_rect();
    let progress = stage.progress.clamp(0.0, 1.0);
    let frame = lerp_rect(stage.home, window, progress);
    let shows_page = crate::ui::browser_panel::shows_live_page(view.page, view.tab);
    let tint = view.page.tint.filter(|_| shows_page);
    let id = egui::Id::new("browser_full_window");
    // Over the title row's controls; the popups it opens once grown come above it.
    if progress < 1.0 {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Foreground, id));
    }
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .constrain(false)
        .show(&ctx, |ui| {
            ui.set_min_size(window.size());
            // The window beneath takes no click while the browser covers it.
            ui.interact(window, ui.id().with("beneath"), egui::Sense::click());
            let scrim = (SCRIM_ALPHA * progress).round() as u8;
            ui.painter()
                .rect_filled(window, 0.0, with_alpha(egui::Color32::BLACK, scrim));
            let radius = egui::CornerRadius::same((HOME_RADIUS * (1.0 - progress)).round() as u8);
            paint_lift(ui, frame, radius, progress);
            let colors = BarColors::new(ui, chrome.palette, tint);
            ui.painter().rect_filled(frame, radius, colors.fill);
            let (strip, below) = frame.split_top_bottom_at_y(frame.top() + STRIP_HEIGHT * progress);
            let mut exit = false;
            if strip.height() >= 1.0 {
                let mut strip_ui = child_at(ui, strip);
                strip_ui.set_clip_rect(strip);
                strip_ui.multiply_opacity(progress);
                exit = title_strip(&mut strip_ui, &colors, stage);
            }
            let mut panel_ui = child_at(ui, below);
            panel_ui.set_clip_rect(below.intersect(window));
            let mut panel = browser_panel(&mut panel_ui, chrome, view);
            if !panel.page_corners.rounds_top {
                panel.page_corners.radius *= 1.0 - progress;
            }
            panel.page_overlaid |= progress < 1.0;
            if exit {
                panel.intents.push(BrowserIntent::ToggleFullWindow);
            }
            FullWindowOutput { panel, frame }
        })
        .inner
}

fn lerp_rect(from: egui::Rect, to: egui::Rect, t: f32) -> egui::Rect {
    egui::Rect::from_min_max(from.min.lerp(to.min, t), from.max.lerp(to.max, t))
}

/// A soft shadow lifting the browser while it moves, gone at both ends.
fn paint_lift(ui: &egui::Ui, frame: egui::Rect, radius: egui::CornerRadius, progress: f32) {
    let lift = (std::f32::consts::PI * progress).sin();
    if lift <= 0.01 {
        return;
    }
    let shadow = egui::Shadow {
        offset: [0, 10],
        blur: 48,
        spread: 2,
        color: with_alpha(egui::Color32::BLACK, (LIFT_ALPHA * lift).round() as u8),
    };
    ui.painter().add(shadow.as_shape(frame, radius));
}

/// The traffic lights' room, the project and the branch, then Exit. `true` when
/// Exit was clicked.
fn title_strip(ui: &mut egui::Ui, colors: &BarColors, stage: &FullWindowStage<'_>) -> bool {
    let strip = ui.max_rect();
    let line = strip.top() + crate::ui::TITLE_LINE_Y;
    let mut x = strip.left() + crate::ui::TRAFFIC_LIGHTS_RESERVE;
    let painter = ui.painter().clone();
    let badge = egui::Rect::from_center_size(
        egui::pos2(x + BADGE_SIZE / 2.0, line),
        egui::Vec2::splat(BADGE_SIZE),
    );
    painter.rect_filled(
        badge,
        egui::CornerRadius::same(6),
        with_alpha(colors.ink, 18),
    );
    crate::ui::paint_icon(
        &painter,
        badge.center(),
        12.0,
        Icon::Globe,
        colors.secondary,
    );
    x = badge.right() + 8.0;
    x = paint_text(
        &painter,
        egui::pos2(x, line),
        stage.project,
        colors.secondary,
    ) + 6.0;
    x = paint_text(&painter, egui::pos2(x, line), "/", colors.muted) + 6.0;
    branch_chip(&painter, colors, egui::pos2(x, line), stage.branch);
    exit_button(ui, colors, line, stage.exit_shortcut)
}

/// Text from `at` (vertically centered there); where it ends.
fn paint_text(painter: &egui::Painter, at: egui::Pos2, text: &str, color: egui::Color32) -> f32 {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(TEXT_SIZE),
        color,
    );
    let width = galley.size().x;
    painter.galley(at - egui::vec2(0.0, galley.size().y / 2.0), galley, color);
    at.x + width
}

fn branch_chip(painter: &egui::Painter, colors: &BarColors, at: egui::Pos2, branch: &str) {
    if branch.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(
        branch.to_owned(),
        egui::FontId::proportional(TEXT_SIZE),
        colors.ink,
    );
    let icon = 12.0;
    let width = CHIP_PAD_X * 2.0 + icon + 5.0 + galley.size().x;
    let chip = egui::Rect::from_min_size(
        egui::pos2(at.x, at.y - CHIP_HEIGHT / 2.0),
        egui::vec2(width, CHIP_HEIGHT),
    );
    painter.rect_filled(
        chip,
        egui::CornerRadius::same((CHIP_HEIGHT / 2.0) as u8),
        with_alpha(colors.ink, 14),
    );
    let icon_center = egui::pos2(chip.left() + CHIP_PAD_X + icon / 2.0, at.y);
    crate::ui::paint_icon(
        painter,
        icon_center,
        icon,
        Icon::GitBranch,
        colors.secondary,
    );
    let text_at = egui::pos2(
        icon_center.x + icon / 2.0 + 5.0,
        at.y - galley.size().y / 2.0,
    );
    painter.galley(text_at, galley, colors.ink);
}

/// A ghost capsule at the strip's right end: icon, label, the shortcut's keycap.
fn exit_button(ui: &mut egui::Ui, colors: &BarColors, line: f32, shortcut: Option<&str>) -> bool {
    let painter = ui.painter().clone();
    let font = egui::FontId::proportional(TEXT_SIZE);
    let label = painter.layout_no_wrap(EXIT_LABEL.to_owned(), font, colors.ink);
    let keycap = shortcut.map(|keys| {
        painter.layout_no_wrap(
            keys.to_owned(),
            egui::FontId::proportional(KEYCAP_SIZE),
            colors.muted,
        )
    });
    let icon = 13.0;
    let keycap_width = keycap.as_ref().map_or(0.0, |galley| galley.size().x + 12.0);
    let width = EXIT_PAD_X * 2.0 + icon + 6.0 + label.size().x + keycap_width;
    let right = ui.max_rect().right() - STRIP_PAD_RIGHT;
    let rect = egui::Rect::from_min_max(
        egui::pos2(right - width, line - EXIT_HEIGHT / 2.0),
        egui::pos2(right, line + EXIT_HEIGHT / 2.0),
    );
    let response = ui
        .interact(rect, ui.id().with("exit_full_window"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, EXIT_LABEL));
    let pill = egui::CornerRadius::same((EXIT_HEIGHT / 2.0) as u8);
    let hovered = response.hovered();
    let fill = if hovered {
        colors.button_hover
    } else {
        with_alpha(colors.ink, 10)
    };
    let pressed = response.is_pointer_button_down_on();
    let drawn = if pressed {
        rect.shrink2(rect.size() * 0.02)
    } else {
        rect
    };
    painter.rect_filled(drawn, pill, fill);
    let icon_center = egui::pos2(drawn.left() + EXIT_PAD_X + icon / 2.0, line);
    crate::ui::paint_icon(&painter, icon_center, icon, Icon::Minimize2, colors.ink);
    let label_left = icon_center.x + icon / 2.0 + 6.0;
    let label_width = label.size().x;
    painter.galley(
        egui::pos2(label_left, line - label.size().y / 2.0),
        label,
        colors.ink,
    );
    if let Some(keycap) = keycap {
        let cap = egui::Rect::from_min_size(
            egui::pos2(label_left + label_width + 6.0, line - 9.0),
            egui::vec2(keycap.size().x + 8.0, 18.0),
        );
        painter.rect_stroke(
            cap,
            egui::CornerRadius::same(5),
            egui::Stroke::new(1.0_f32, with_alpha(colors.ink, 40)),
            egui::StrokeKind::Inside,
        );
        painter.galley(cap.center() - keycap.size() / 2.0, keycap, colors.muted);
    }
    response.clicked()
}
