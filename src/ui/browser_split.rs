//! Two tabs side by side in the page area (browser.md §16): two rounded cards on
//! the toolbar's tint, the active one ringed, a divider between them; and the
//! drop zone a tab dragged onto the page's right side lights.

use lucide_icons::Icon;

use crate::browser::split::{self, Side};
use crate::browser::PageState;
use crate::ui::browser_panel::{
    page_view_content, pressed_in, BrowserChrome, BrowserIntent, BrowserTabState, DraggedTab,
    PageArea, PageContent,
};
use crate::ui::with_alpha;

pub(super) const CARD_RADIUS: f32 = 10.0;
/// Between the cards, and around them.
const GAP: f32 = 6.0;
const RING_WIDTH: f32 = 2.0;
const RING_FADE: f32 = 0.15;
const GRIP_SIZE: egui::Vec2 = egui::vec2(4.0, 32.0);
const DROP_LABEL_SIZE: f32 = 12.5;
pub const OPEN_ON_RIGHT_LABEL: &str = "Open on the right";
pub const DIVIDER_LABEL: &str = "Resize split";

/// The shown tab's split partner: its page and state, and where it shows.
pub struct SplitPartner<'a> {
    pub page: &'a PageState,
    pub tab: &'a mut BrowserTabState,
    pub side: Side,
    /// The left half's share of the width.
    pub share: f32,
}

/// What the partner's half reported this frame.
#[derive(Debug, Default)]
pub struct PartnerOutput {
    /// Its start page's, failure page's or dialog's: for the partner's tab.
    pub intents: Vec<BrowserIntent>,
    pub page_rect: Option<egui::Rect>,
    pub blocked: bool,
    pub field_focused: bool,
}

/// Both halves keep their minimum width in `area`.
pub(super) fn fits(area: egui::Rect, share: f32) -> bool {
    split::widths(room(area), share).is_some()
}

fn room(area: egui::Rect) -> f32 {
    area.width() - 3.0 * GAP
}

/// The partner's half, the cards on `backing`.
pub(super) struct SplitArea<'a> {
    pub(super) partner: SplitPartner<'a>,
    pub(super) backing: egui::Color32,
}

/// The shown tab's half and its partner's in `ui`'s rect.
pub(super) struct Halves<'a> {
    pub(super) shown: PageArea<'a>,
    pub(super) split: SplitArea<'a>,
}

pub(super) fn split_halves(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    halves: Halves<'_>,
) -> PageContent {
    let Halves { shown, split } = halves;
    let SplitArea { partner, backing } = split;
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0.0, backing);
    let inner = area.shrink(GAP);
    let (left_width, _) = split::widths(room(area), partner.share).unwrap_or_default();
    let left = egui::Rect::from_min_size(inner.min, egui::vec2(left_width, inner.height()));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + GAP, inner.top()), inner.max);
    let (shown_card, partner_card) = match partner.side {
        Side::Right => (left, right),
        Side::Left => (right, left),
    };
    let palette = chrome.palette;
    let partner_fill = partner_fill(partner.page, partner.tab, palette);
    paint_card(ui, shown_card, shown.fill);
    paint_card(ui, partner_card, partner_fill);
    let shown_side = opposite(partner.side);
    let mut content = page_view_content(&mut half_ui(ui, shown_card, shown_side), chrome, shown);
    let partner_area = PageArea {
        page: partner.page,
        tab: partner.tab,
        fill: partner_fill,
        with_shelf: false,
        split: None,
    };
    let partner_content = page_view_content(
        &mut half_ui(ui, partner_card, partner.side),
        chrome,
        partner_area,
    );
    for card in [left, right] {
        crate::ui::sidebar_block::paint_corner_masks(ui.painter(), card, CARD_RADIUS, backing);
    }
    paint_rings(
        ui,
        palette.accent,
        [(left, Side::Left), (right, Side::Right)],
        shown_side,
    );
    if pressed_in(ui, partner_card) {
        content.intents.push(BrowserIntent::ShowPartner);
    }
    let divider = Divider {
        left,
        right,
        room: room(area),
    };
    content.intents.extend(divider_handle(ui, chrome, &divider));
    content.partner = Some(PartnerOutput {
        intents: partner_content.intents,
        page_rect: partner_content.page_rect,
        blocked: partner_content.blocked,
        field_focused: partner_content.field_focused,
    });
    content
}

fn opposite(side: Side) -> Side {
    match side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

/// Each half keeps its widgets' state by side, whichever tab is shown.
fn half_ui(ui: &mut egui::Ui, card: egui::Rect, side: Side) -> egui::Ui {
    let salt = match side {
        Side::Left => "split_left",
        Side::Right => "split_right",
    };
    ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(card))
}

/// The partner's page color: its tint while it shows its page, else the surface.
fn partner_fill(
    page: &PageState,
    tab: &BrowserTabState,
    palette: &crate::theme::Palette,
) -> egui::Color32 {
    let live = crate::ui::browser_panel::shows_live_page(page, tab);
    match page.tint.filter(|_| live) {
        Some([red, green, blue]) => egui::Color32::from_rgb(red, green, blue),
        None => palette.bg_surface,
    }
}

/// A card standing off the tint by a soft shadow, no border.
fn paint_card(ui: &egui::Ui, card: egui::Rect, fill: egui::Color32) {
    let radius = egui::CornerRadius::same(CARD_RADIUS as u8);
    let shadow = egui::Shadow {
        offset: [0, 1],
        blur: 8,
        spread: 0,
        color: with_alpha(egui::Color32::BLACK, 36),
    };
    ui.painter().add(shadow.as_shape(card, radius));
    ui.painter().rect_filled(card, radius, fill);
}

/// The accent ring of the active half, fading over to the other on a switch.
fn paint_rings(ui: &egui::Ui, accent: egui::Color32, cards: [(egui::Rect, Side); 2], shown: Side) {
    for (card, side) in cards {
        let id = ui.id().with(("split_ring", side == Side::Left));
        let lit = ui
            .ctx()
            .animate_bool_with_time(id, side == shown, RING_FADE);
        if lit <= 0.0 {
            continue;
        }
        let alpha = (f32::from(accent.a()) * lit).round() as u8;
        ui.painter().rect_stroke(
            card,
            egui::CornerRadius::same(CARD_RADIUS as u8),
            egui::Stroke::new(RING_WIDTH, with_alpha(accent, alpha)),
            egui::StrokeKind::Outside,
        );
    }
}

struct Divider {
    left: egui::Rect,
    right: egui::Rect,
    room: f32,
}

/// The gap between the cards: dragged, it shares the width anew; its grip shows
/// while the pointer is on it.
fn divider_handle(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    divider: &Divider,
) -> Option<BrowserIntent> {
    let gap = egui::Rect::from_x_y_ranges(
        divider.left.right()..=divider.right.left(),
        divider.left.y_range(),
    );
    let response = ui
        .interact(
            gap.expand2(egui::vec2(2.0, 0.0)),
            ui.id().with("split_divider"),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, DIVIDER_LABEL));
    let dragging = response.dragged();
    if dragging {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let shown = ui.ctx().animate_bool_with_time(
        response.id.with("grip"),
        response.hovered() || dragging,
        RING_FADE,
    );
    if shown > 0.0 {
        let palette = chrome.palette;
        let color = if dragging {
            palette.accent
        } else {
            palette.text_muted
        };
        let alpha = (f32::from(color.a()) * shown).round() as u8;
        ui.painter().rect_filled(
            egui::Rect::from_center_size(gap.center(), GRIP_SIZE),
            egui::CornerRadius::same((GRIP_SIZE.x / 2.0) as u8),
            with_alpha(color, alpha),
        );
    }
    if !dragging {
        return None;
    }
    let pointer = response.interact_pointer_pos()?;
    let left = pointer.x - divider.left.left() - GAP / 2.0;
    Some(BrowserIntent::ResizeSplit(split::share_at(
        left,
        divider.room,
    )))
}

/// A tab dragged over the right side of `area` lights it, *Open on the right*;
/// released there, the tab joins the shown one on that side. The shown tab
/// itself has nowhere to go.
pub(super) fn right_drop_zone(
    ui: &egui::Ui,
    chrome: &BrowserChrome<'_>,
    area: egui::Rect,
    shown: usize,
) -> Option<BrowserIntent> {
    let ctx = ui.ctx();
    let dragged = egui::DragAndDrop::payload::<DraggedTab>(ctx)?;
    if dragged.0 == shown {
        return None;
    }
    let pointer = ctx.pointer_hover_pos()?;
    let zone =
        egui::Rect::from_min_max(egui::pos2(area.center().x, area.top()), area.max).shrink(GAP);
    if !area.contains(pointer) || pointer.x < area.center().x {
        return None;
    }
    if ctx.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::clear_payload(ctx);
        return Some(BrowserIntent::SplitWith(dragged.0));
    }
    paint_drop_zone(ui, chrome, zone);
    None
}

fn paint_drop_zone(ui: &egui::Ui, chrome: &BrowserChrome<'_>, zone: egui::Rect) {
    let accent = chrome.palette.accent;
    let radius = egui::CornerRadius::same(CARD_RADIUS as u8);
    let painter = ui.painter();
    painter.rect_filled(zone, radius, with_alpha(accent, 34));
    painter.rect_stroke(
        zone,
        radius,
        egui::Stroke::new(1.5_f32, with_alpha(accent, 200)),
        egui::StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(
        OPEN_ON_RIGHT_LABEL.to_owned(),
        egui::FontId::proportional(DROP_LABEL_SIZE),
        egui::Color32::WHITE,
    );
    let icon = 14.0;
    let size = egui::vec2(icon + 8.0 + galley.size().x, 18.0) + egui::vec2(24.0, 12.0);
    let label = egui::Rect::from_center_size(zone.center(), size);
    painter.rect_filled(
        label,
        egui::CornerRadius::same((size.y / 2.0) as u8),
        accent,
    );
    let icon_center = egui::pos2(label.left() + 12.0 + icon / 2.0, label.center().y);
    crate::ui::paint_icon(
        painter,
        icon_center,
        icon,
        Icon::Columns,
        egui::Color32::WHITE,
    );
    let text_at = egui::pos2(
        icon_center.x + icon / 2.0 + 8.0,
        label.center().y - galley.size().y / 2.0,
    );
    painter.galley(text_at, galley, egui::Color32::WHITE);
}
