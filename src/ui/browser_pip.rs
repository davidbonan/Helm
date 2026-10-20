//! The browser floating over the window (specs/browser.md §11): the shown tab's
//! page on a rimmed panel, a bar hanging above it while the pointer is near.

use lucide_icons::Icon;

use crate::browser::pip::{Grip, PipFrame, Side};
use crate::browser::PageState;
use crate::ui::browser_panel::{
    paint_backdrop, paint_host, pill_button, pressed_in, reload_button, viewport_rect, BarColors,
    BrowserChrome, BrowserIntent, BrowserPanelOutput, BrowserTabState,
};

const PIP_BAR_HEIGHT: f32 = 26.0;
const RIM: f32 = 4.0;
const RADIUS: u8 = 10;
/// How far past the rim its resize grips reach, sideways and below.
const GRIP_OUTSET: f32 = 4.0;
const CORNER_GRIP: f32 = 14.0;
const BAR_FADE: f32 = 0.12;
const BAR_PAD_X: f32 = 8.0;
const BAR_BUTTON_GAP: f32 = 24.0;

/// Where the panel floats, and where it may go: the window under the title
/// bar, less the bar's room.
#[derive(Clone, Copy)]
pub(crate) struct PipPlacement {
    pub frame: PipFrame,
    pub bounds: PipFrame,
}

impl PipPlacement {
    /// `frame`, else a first panel in the corner of `terminals`; within the
    /// window under the title bar, room kept above for the bar.
    pub(crate) fn in_window(
        ctx: &egui::Context,
        frame: Option<PipFrame>,
        terminals: egui::Rect,
    ) -> Self {
        let window = ctx.content_rect();
        let top = window.top() + f32::from(super::TITLEBAR_HEIGHT) + PIP_BAR_HEIGHT;
        let bounds = frame_of(egui::Rect::from_x_y_ranges(
            window.x_range(),
            top..=window.bottom(),
        ));
        let corner = if terminals.is_positive() {
            frame_of(terminals)
        } else {
            bounds
        };
        Self {
            frame: frame.unwrap_or_else(|| PipFrame::in_corner_of(corner)),
            bounds,
        }
    }
}

fn frame_of(rect: egui::Rect) -> PipFrame {
    PipFrame {
        x: rect.left(),
        y: rect.top(),
        width: rect.width(),
        height: rect.height(),
    }
}

/// What the floating panel draws: the shown tab, where it floats.
pub(crate) struct BrowserPipView<'a> {
    pub page: &'a PageState,
    pub tab: &'a mut BrowserTabState,
    pub placement: PipPlacement,
}

pub(crate) struct BrowserPipOutput {
    pub panel: BrowserPanelOutput,
    /// Where the panel now floats, moved or resized.
    pub frame: PipFrame,
}

pub(crate) fn browser_pip(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    view: BrowserPipView<'_>,
) -> BrowserPipOutput {
    let BrowserPipView {
        page,
        tab,
        placement: PipPlacement { frame, bounds },
    } = view;
    let ctx = ui.ctx().clone();
    let id = egui::Id::new("browser_pip");
    let mut frame = frame.fitted(bounds);
    let panel = panel_rect(frame);
    let shows_page = !(tab.start_page || page.url.is_empty());
    let colors = BarColors::new(ui, chrome.palette, page.tint.filter(|_| shows_page));
    let bar = egui::Rect::from_min_max(
        panel.min - egui::vec2(0.0, PIP_BAR_HEIGHT),
        egui::pos2(panel.right(), panel.top()),
    );
    let bar_shown = id.with("bar_shown");
    let was_shown = ctx.data(|data| data.get_temp::<bool>(bar_shown).unwrap_or(false));
    let mut reach = panel.expand(GRIP_OUTSET);
    if was_shown {
        reach = reach.union(bar);
    }
    let wanted = page.pointer_over || pointer_within(&ctx, reach) || dragging_panel(&ctx, id);
    let bar_opacity = ctx.animate_bool_with_time(id.with("bar"), wanted, BAR_FADE);
    ctx.data_mut(|data| data.insert_temp(bar_shown, bar_opacity > 0.0));
    let mut intents = Vec::new();
    let mut page_rect = None;
    let mut grabbed = None;
    egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(panel.min - egui::vec2(GRIP_OUTSET, 0.0))
        .constrain(false)
        .show(&ctx, |ui| {
            ui.set_min_size(panel.size() + egui::vec2(2.0 * GRIP_OUTSET, GRIP_OUTSET));
            let card = if bar_opacity > 0.0 {
                panel.union(bar)
            } else {
                panel
            };
            paint_shadow(ui, card);
            let top = (f32::from(RADIUS) * (1.0 - bar_opacity)).round() as u8;
            let radius = egui::CornerRadius {
                nw: top,
                ne: top,
                sw: RADIUS,
                se: RADIUS,
            };
            ui.painter().rect_filled(panel, radius, colors.fill);
            let page_area = panel.shrink(RIM);
            let page_fill = if colors.tinted {
                colors.fill
            } else {
                chrome.palette.bg_surface
            };
            ui.painter().rect_filled(page_area, 0.0, page_fill);
            if shows_page {
                let viewport = viewport_rect(page_area, tab.page_view.viewport);
                paint_backdrop(ui, tab.backdrop.as_ref(), viewport);
                page_rect = Some(viewport);
            } else {
                intents.extend(crate::ui::browser_start_page::start_page(
                    ui,
                    chrome,
                    &mut tab.favorites,
                    page_area,
                ));
            }
            grabbed = resize_grips(ui, id, panel);
        });
    if let Some((grip, delta)) = grabbed {
        frame = frame.resized(grip, delta, bounds);
    }
    if bar_opacity > 0.0 {
        let pip_bar = PipBar {
            rect: bar,
            drag_id: id.with("bar_drag"),
            page,
            shows_page,
        };
        let output = egui::Area::new(id.with("bar_area"))
            .order(egui::Order::Middle)
            .fixed_pos(bar.min)
            .constrain(false)
            .show(&ctx, |ui| {
                ui.multiply_opacity(bar_opacity);
                bar_row(ui, &colors, &pip_bar)
            })
            .inner;
        intents.extend(output.intent);
        frame = frame.moved(output.moved.x, output.moved.y).fitted(bounds);
    }
    let clicked = pressed_in(ui, panel.expand(GRIP_OUTSET)) || pressed_in(ui, bar);
    BrowserPipOutput {
        panel: BrowserPanelOutput {
            intents,
            page_rect,
            page_bottom_radius: 0.0,
            page_overlaid: false,
            clicked,
            field_focused: false,
        },
        frame,
    }
}

pub(crate) fn panel_rect(frame: PipFrame) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(frame.x, frame.y),
        egui::vec2(frame.width, frame.height),
    )
}

/// Whether egui sees the pointer in `rect`: never over the page, whose view
/// takes its moves.
fn pointer_within(ctx: &egui::Context, rect: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.hover_pos())
        .is_some_and(|at| rect.contains(at))
}

/// The bar or a grip of the panel `id` is dragged.
fn dragging_panel(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.dragged_id().is_some_and(|dragged| {
        dragged == id.with("bar_drag") || GRIPS.iter().any(|grip| dragged == grip_id(id, grip.0))
    })
}

/// Under the panel, and the bar while it shows: one card.
fn paint_shadow(ui: &egui::Ui, card: egui::Rect) {
    let shadow = egui::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: crate::ui::with_alpha(egui::Color32::BLACK, 70),
    };
    ui.painter()
        .add(shadow.as_shape(card, egui::CornerRadius::same(RADIUS)));
}

struct PipBar<'a> {
    rect: egui::Rect,
    drag_id: egui::Id,
    page: &'a PageState,
    shows_page: bool,
}

struct BarOutput {
    intent: Option<BrowserIntent>,
    /// How far the bar was dragged this frame.
    moved: egui::Vec2,
}

/// The bar above the panel: the page's host, reload, Dock.
fn bar_row(ui: &mut egui::Ui, colors: &BarColors, bar: &PipBar<'_>) -> BarOutput {
    let rect = bar.rect;
    ui.set_min_size(rect.size());
    let drag = ui
        .interact(rect, bar.drag_id, egui::Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Grab);
    let radius = egui::CornerRadius {
        nw: RADIUS,
        ne: RADIUS,
        sw: 0,
        se: 0,
    };
    ui.painter().rect_filled(rect, radius, colors.fill);
    if bar.shows_page {
        paint_host(ui, colors, rect, &bar.page.url);
    }
    let dock_at = egui::pos2(
        rect.right() - BAR_PAD_X - BAR_BUTTON_GAP / 2.0,
        rect.center().y,
    );
    let mut intent = pill_button(ui, colors, dock_at, Icon::PanelRightOpen, "Dock")
        .clicked()
        .then_some(BrowserIntent::Dock);
    if bar.shows_page {
        let reload_at = dock_at - egui::vec2(BAR_BUTTON_GAP, 0.0);
        if let Some(navigation) = reload_button(ui, colors, reload_at, bar.page.loading) {
            intent = Some(BrowserIntent::Navigate(navigation));
        }
    }
    if drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    BarOutput {
        intent,
        moved: drag.drag_delta(),
    }
}

/// Each grip, its name, its cursor.
const GRIPS: [(&str, Grip, egui::CursorIcon); 8] = [
    (
        "left",
        grip(Some(Side::Start), None),
        egui::CursorIcon::ResizeHorizontal,
    ),
    (
        "right",
        grip(Some(Side::End), None),
        egui::CursorIcon::ResizeHorizontal,
    ),
    (
        "top",
        grip(None, Some(Side::Start)),
        egui::CursorIcon::ResizeVertical,
    ),
    (
        "bottom",
        grip(None, Some(Side::End)),
        egui::CursorIcon::ResizeVertical,
    ),
    (
        "top_left",
        grip(Some(Side::Start), Some(Side::Start)),
        egui::CursorIcon::ResizeNorthWest,
    ),
    (
        "top_right",
        grip(Some(Side::End), Some(Side::Start)),
        egui::CursorIcon::ResizeNorthEast,
    ),
    (
        "bottom_left",
        grip(Some(Side::Start), Some(Side::End)),
        egui::CursorIcon::ResizeSouthWest,
    ),
    (
        "bottom_right",
        grip(Some(Side::End), Some(Side::End)),
        egui::CursorIcon::ResizeSouthEast,
    ),
];

const fn grip(horizontal: Option<Side>, vertical: Option<Side>) -> Grip {
    Grip {
        horizontal,
        vertical,
    }
}

fn grip_id(id: egui::Id, name: &str) -> egui::Id {
    id.with("grip").with(name)
}

/// Where a grip catches the pointer: across the rim, reaching past it sideways
/// and below; never above, where the bar hangs. Corners reach further along
/// both edges they join.
fn grip_rect(panel: egui::Rect, grip: Grip) -> egui::Rect {
    let depth = if grip.horizontal.is_some() && grip.vertical.is_some() {
        CORNER_GRIP
    } else {
        RIM
    };
    let x = match grip.horizontal {
        Some(Side::Start) => (panel.left() - GRIP_OUTSET)..=(panel.left() + depth),
        Some(Side::End) => (panel.right() - depth)..=(panel.right() + GRIP_OUTSET),
        None => (panel.left() + CORNER_GRIP)..=(panel.right() - CORNER_GRIP),
    };
    let y = match grip.vertical {
        Some(Side::Start) => panel.top()..=(panel.top() + depth),
        Some(Side::End) => (panel.bottom() - depth)..=(panel.bottom() + GRIP_OUTSET),
        None => (panel.top() + CORNER_GRIP)..=(panel.bottom() - CORNER_GRIP),
    };
    egui::Rect::from_x_y_ranges(x, y)
}

/// The grip dragged this frame, and by how much.
fn resize_grips(ui: &egui::Ui, id: egui::Id, panel: egui::Rect) -> Option<(Grip, (f32, f32))> {
    let mut grabbed = None;
    for (name, grip, cursor) in GRIPS {
        let response = ui
            .interact(
                grip_rect(panel, grip),
                grip_id(id, name),
                egui::Sense::drag(),
            )
            .on_hover_cursor(cursor);
        if response.dragged() {
            let delta = response.drag_delta();
            grabbed = Some((grip, (delta.x, delta.y)));
        }
    }
    grabbed
}
