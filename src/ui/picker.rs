use egui::text::{LayoutJob, TextWrapping};
use lucide_icons::Icon;

use crate::theme::{self, Palette, RADIUS_MENU_ITEM, RADIUS_PILL};
use crate::ui::{clickable, paint_icon};

const HEIGHT: f32 = 36.0;
const COMPACT_HEIGHT: f32 = 28.0;
const COMPACT_RADIUS: u8 = 6;
const PAD_X: f32 = 10.0;
const LABEL_SIZE: f32 = 13.0;
const ICON_SIZE: f32 = 13.0;
const ICON_GAP: f32 = 7.0;
const CHEVRON_SIZE: f32 = 12.0;
const MIN_WIDTH: f32 = 120.0;
const MAX_MENU_WIDTH: f32 = 360.0;
const POPUP_GAP: f32 = 4.0;
const ROW_HEIGHT: f32 = 28.0;
const ROW_PAD_X: f32 = 8.0;
const ROW_SPACING: f32 = 1.0;
const FILTER_FROM: usize = 8;
const FILTER_HINT: &str = "Filter…";
const NO_MATCH: &str = "No match";
const SEGMENT: egui::Vec2 = egui::vec2(88.0, 36.0);
const SEGMENT_ICON_SIZE: f32 = 15.0;
const SEGMENT_ICON_GAP: f32 = 6.0;

pub struct Choice<'a> {
    label: &'a str,
    icon: Option<Icon>,
}

impl<'a> Choice<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, icon: None }
    }

    pub fn icon(self, icon: Icon) -> Self {
        Self {
            icon: Some(icon),
            ..self
        }
    }
}

/// The one button every picker opens from: current value, its icon, and the
/// up-down chevron of a macOS pop-up button.
pub struct Trigger<'a> {
    label: &'a str,
    icon: Option<Icon>,
    compact: bool,
    width: Option<f32>,
}

impl<'a> Trigger<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            compact: false,
            width: None,
        }
    }

    pub fn icon(self, icon: Option<Icon>) -> Self {
        Self { icon, ..self }
    }

    pub fn compact(self) -> Self {
        Self {
            compact: true,
            ..self
        }
    }

    pub fn width(self, width: f32) -> Self {
        Self {
            width: Some(width),
            ..self
        }
    }

    pub fn show(self, ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
        let font = egui::FontId::proportional(LABEL_SIZE);
        let lead = if self.icon.is_some() {
            ICON_SIZE + ICON_GAP
        } else {
            0.0
        };
        let natural =
            PAD_X + lead + text_width(ui, self.label, &font) + ICON_GAP + CHEVRON_SIZE + PAD_X;
        let height = if self.compact { COMPACT_HEIGHT } else { HEIGHT };
        let size = egui::vec2(self.width.unwrap_or(natural.max(MIN_WIDTH)), height);
        let (rect, response, hovered) = clickable(ui, size, true);
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        let (fill, border) = if open {
            (palette.accent_subtle, palette.accent)
        } else if response.has_focus() {
            (ui.visuals().extreme_bg_color, palette.accent)
        } else if hovered {
            (palette.bg_surface_hover, palette.border_input)
        } else {
            (ui.visuals().extreme_bg_color, palette.border_subtle)
        };
        let radius = if self.compact {
            COMPACT_RADIUS
        } else {
            RADIUS_PILL
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            egui::CornerRadius::same(radius),
            fill,
            egui::Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );
        let accent_or = |rest| if open { palette.accent } else { rest };
        let mut left = rect.left() + PAD_X;
        if let Some(icon) = self.icon {
            paint_icon(
                painter,
                egui::pos2(left + ICON_SIZE / 2.0, rect.center().y),
                ICON_SIZE,
                icon,
                accent_or(palette.text_secondary),
            );
            left += lead;
        }
        let chevron_x = rect.right() - PAD_X - CHEVRON_SIZE / 2.0;
        let text = painter.layout_job(truncated(self.label, font, chevron_x - ICON_GAP - left));
        painter.galley(
            egui::pos2(left, rect.center().y - text.size().y / 2.0),
            text,
            palette.text_primary,
        );
        paint_icon(
            painter,
            egui::pos2(chevron_x, rect.center().y),
            CHEVRON_SIZE,
            Icon::ChevronsUpDown,
            accent_or(palette.text_muted),
        );
        let label = self.label.to_owned();
        response
            .widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        response
    }
}

/// Menu of exclusive choices under a trigger: the chosen row is tinted,
/// arrows and Enter drive it, a long list grows a filter. Returns the
/// index picked, only when it differs from `selected`.
pub struct PickOne<'a> {
    choices: &'a [Choice<'a>],
    selected: Option<usize>,
    empty_note: &'a str,
}

impl<'a> PickOne<'a> {
    pub fn new(choices: &'a [Choice<'a>], selected: Option<usize>) -> Self {
        Self {
            choices,
            selected,
            empty_note: "",
        }
    }

    pub fn empty_note(self, empty_note: &'a str) -> Self {
        Self { empty_note, ..self }
    }

    pub fn show(self, trigger: &egui::Response, palette: &Palette) -> Option<usize> {
        let rows: Vec<Row> = self
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| Row {
                label: choice.label,
                icon: choice.icon,
                marked: Some(index) == self.selected,
            })
            .collect();
        let mark = if self.selected.is_some() {
            Mark::Tint
        } else {
            Mark::None
        };
        let menu = Menu {
            rows: &rows,
            mark,
            empty_note: self.empty_note,
        };
        let picked = menu.show(trigger, palette)?;
        (Some(picked) != self.selected).then_some(picked)
    }
}

pub struct Toggle<'a> {
    pub label: &'a str,
    pub on: bool,
}

/// Menu of independent on/off rows, an eye per row; it stays open while rows
/// flip. Returns the index flipped this frame.
pub struct PickMany<'a> {
    toggles: &'a [Toggle<'a>],
}

impl<'a> PickMany<'a> {
    pub fn new(toggles: &'a [Toggle<'a>]) -> Self {
        Self { toggles }
    }

    pub fn show(self, trigger: &egui::Response, palette: &Palette) -> Option<usize> {
        let rows: Vec<Row> = self
            .toggles
            .iter()
            .map(|toggle| Row {
                label: toggle.label,
                icon: None,
                marked: toggle.on,
            })
            .collect();
        let menu = Menu {
            rows: &rows,
            mark: Mark::Eye,
            empty_note: "",
        };
        menu.show(trigger, palette)
    }
}

/// Trigger + `PickOne` over a fixed set of values named by `label`. Returns
/// `true` when `current` changed.
pub struct Dropdown<'a, T> {
    options: &'a [T],
    current: &'a mut T,
    label: fn(T) -> &'static str,
    compact: bool,
    width: Option<f32>,
}

impl<'a, T: Copy + PartialEq> Dropdown<'a, T> {
    pub fn new(options: &'a [T], current: &'a mut T, label: fn(T) -> &'static str) -> Self {
        Self {
            options,
            current,
            label,
            compact: false,
            width: None,
        }
    }

    pub fn compact(self) -> Self {
        Self {
            compact: true,
            ..self
        }
    }

    pub fn width(self, width: f32) -> Self {
        Self {
            width: Some(width),
            ..self
        }
    }

    pub fn show(self, ui: &mut egui::Ui, palette: &Palette) -> bool {
        let mut trigger = Trigger::new((self.label)(*self.current));
        if self.compact {
            trigger = trigger.compact();
        }
        if let Some(width) = self.width {
            trigger = trigger.width(width);
        }
        let response = trigger.show(ui, palette);
        let choices: Vec<Choice> = self
            .options
            .iter()
            .map(|option| Choice::new((self.label)(*option)))
            .collect();
        let selected = self.options.iter().position(|o| o == self.current);
        let Some(index) = PickOne::new(&choices, selected).show(&response, palette) else {
            return false;
        };
        *self.current = self.options[index];
        true
    }
}

/// Segmented control: one bordered group, the chosen segment detached on top in
/// the accent. Returns the index clicked, only when it differs from `selected`.
pub struct Segments<'a> {
    choices: &'a [Choice<'a>],
    selected: usize,
    segment: egui::Vec2,
}

impl<'a> Segments<'a> {
    pub fn new(choices: &'a [Choice<'a>], selected: usize) -> Self {
        Self {
            choices,
            selected,
            segment: SEGMENT,
        }
    }

    pub fn segment_size(self, segment: egui::Vec2) -> Self {
        Self { segment, ..self }
    }

    /// Allocated as one block then laid out left-to-right: `ui.horizontal` would
    /// inherit a right-to-left parent and reverse the order.
    pub fn show(self, ui: &mut egui::Ui, palette: &Palette) -> Option<usize> {
        let count = self.choices.len();
        let group = egui::vec2(self.segment.x * count as f32, self.segment.y);
        let (rect, _) = ui.allocate_exact_size(group, egui::Sense::hover());
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            ui.visuals().extreme_bg_color,
            egui::Stroke::new(1.0_f32, palette.border_subtle),
            egui::StrokeKind::Inside,
        );
        for index in 1..count {
            ui.painter().vline(
                rect.left() + self.segment.x * index as f32,
                egui::Rangef::new(rect.top() + 1.0, rect.bottom() - 1.0),
                egui::Stroke::new(1.0_f32, palette.border_subtle),
            );
        }
        let mut segments = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        segments.spacing_mut().item_spacing.x = 0.0;
        let mut picked = None;
        for (index, choice) in self.choices.iter().enumerate() {
            let segment = Segment {
                choice,
                size: self.segment,
                selected: index == self.selected,
                position: SegmentPosition::at(index, count),
            };
            if segment.show(&mut segments, palette).clicked() && index != self.selected {
                picked = Some(index);
            }
        }
        picked
    }
}

struct Segment<'a> {
    choice: &'a Choice<'a>,
    size: egui::Vec2,
    selected: bool,
    position: SegmentPosition,
}

impl Segment<'_> {
    /// The selected segment covers the adjacent separators; hover fills the
    /// inside without covering the group's border.
    fn show(&self, ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
        let (rect, response, hovered) = clickable(ui, self.size, true);
        if self.selected {
            ui.painter().rect(
                rect,
                egui::CornerRadius::same(RADIUS_PILL),
                palette.accent_subtle,
                egui::Stroke::new(1.0_f32, palette.accent),
                egui::StrokeKind::Inside,
            );
        } else if hovered {
            ui.painter().rect_filled(
                rect.shrink(1.0),
                self.position.radius(),
                palette.bg_surface_hover,
            );
        }
        let color = if self.selected {
            palette.accent
        } else {
            palette.text_secondary
        };
        let font = egui::FontId::proportional(LABEL_SIZE);
        let label = self.choice.label;
        let lead = if self.choice.icon.is_some() {
            SEGMENT_ICON_SIZE + SEGMENT_ICON_GAP
        } else {
            0.0
        };
        let left = rect.center().x - (lead + text_width(ui, label, &font)) / 2.0;
        if let Some(icon) = self.choice.icon {
            paint_icon(
                ui.painter(),
                egui::pos2(left + SEGMENT_ICON_SIZE / 2.0, rect.center().y),
                SEGMENT_ICON_SIZE,
                icon,
                color,
            );
        }
        ui.painter().text(
            egui::pos2(left + lead, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            color,
        );
        let selected = self.selected;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
        });
        response
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentPosition {
    First,
    Middle,
    Last,
}

impl SegmentPosition {
    fn at(index: usize, count: usize) -> Self {
        if index == 0 {
            SegmentPosition::First
        } else if index + 1 == count {
            SegmentPosition::Last
        } else {
            SegmentPosition::Middle
        }
    }

    fn radius(self) -> egui::CornerRadius {
        match self {
            SegmentPosition::First => egui::CornerRadius {
                nw: RADIUS_PILL,
                ne: 0,
                sw: RADIUS_PILL,
                se: 0,
            },
            SegmentPosition::Middle => egui::CornerRadius::ZERO,
            SegmentPosition::Last => egui::CornerRadius {
                nw: 0,
                ne: RADIUS_PILL,
                sw: 0,
                se: RADIUS_PILL,
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Tint,
    Eye,
}

struct Row<'a> {
    label: &'a str,
    icon: Option<Icon>,
    marked: bool,
}

struct Menu<'a> {
    rows: &'a [Row<'a>],
    mark: Mark,
    empty_note: &'a str,
}

#[derive(Clone, Default)]
struct MenuState {
    filter: String,
}

impl Menu<'_> {
    fn show(&self, trigger: &egui::Response, palette: &Palette) -> Option<usize> {
        let id = egui::Popup::default_response_id(trigger);
        let shown = egui::Popup::menu(trigger)
            .gap(POPUP_GAP)
            .style(theme::menu_style)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let width = self.natural_width(ui).max(trigger.rect.width());
                ui.set_width(width.min(MAX_MENU_WIDTH.max(trigger.rect.width())));
                self.body(ui, palette, id)
            });
        match shown {
            Some(inner) => inner.inner,
            None => {
                trigger.ctx.data_mut(|data| data.remove::<MenuState>(id));
                None
            }
        }
    }

    fn body(&self, ui: &mut egui::Ui, palette: &Palette, id: egui::Id) -> Option<usize> {
        ui.spacing_mut().item_spacing.y = ROW_SPACING;
        let stored = ui.data(|data| data.get_temp::<MenuState>(id));
        let opening = stored.is_none();
        let mut state = stored.unwrap_or_default();
        let filtering = self.rows.len() >= FILTER_FROM;
        let mut picked = None;
        let mut first_shown = None;
        let mut submitted = false;
        if filtering {
            let field = filter_field(ui, palette, &mut state.filter);
            if opening {
                field.request_focus();
            }
            submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        }
        let needle = state.filter.to_lowercase();
        let focus_on_open = self.rows.iter().position(|row| row.marked);
        for (index, row) in self.rows.iter().enumerate() {
            if !row.label.to_lowercase().contains(&needle) {
                continue;
            }
            first_shown.get_or_insert(index);
            let response = row_widget(
                ui,
                palette,
                RowView {
                    row,
                    mark: self.mark,
                    id: id.with(index),
                },
            );
            if opening && !filtering && self.mark == Mark::Tint && focus_on_open == Some(index) {
                response.request_focus();
            }
            if response.clicked() {
                picked = Some(index);
            }
        }
        if first_shown.is_none() {
            let note = if self.rows.is_empty() {
                self.empty_note
            } else {
                NO_MATCH
            };
            note_row(ui, palette, note);
        }
        if submitted {
            picked = first_shown;
        }
        if picked.is_some() && self.mark != Mark::Eye {
            ui.close();
        }
        ui.data_mut(|data| data.insert_temp(id, state));
        picked
    }

    fn natural_width(&self, ui: &egui::Ui) -> f32 {
        let font = egui::FontId::new(LABEL_SIZE, theme::medium_family(ui.ctx()));
        let lead = if self.mark == Mark::Eye || self.rows.iter().any(|row| row.icon.is_some()) {
            ICON_SIZE + ICON_GAP
        } else {
            0.0
        };
        let widest = self
            .rows
            .iter()
            .map(|row| text_width(ui, row.label, &font))
            .fold(0.0, f32::max);
        ROW_PAD_X + lead + widest + ROW_PAD_X
    }
}

struct RowView<'a> {
    row: &'a Row<'a>,
    mark: Mark,
    id: egui::Id,
}

fn row_widget(ui: &mut egui::Ui, palette: &Palette, view: RowView<'_>) -> egui::Response {
    let RowView { row, mark, id } = view;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let chosen = mark == Mark::Tint && row.marked;
    let fill = if chosen {
        Some(palette.accent_subtle)
    } else if response.hovered() || response.has_focus() {
        Some(palette.bg_surface_hover)
    } else {
        None
    };
    let painter = ui.painter();
    if let Some(fill) = fill {
        painter.rect_filled(rect, egui::CornerRadius::same(RADIUS_MENU_ITEM), fill);
    }
    let hidden = mark == Mark::Eye && !row.marked;
    let ink = if chosen {
        palette.accent
    } else if hidden {
        palette.text_muted
    } else {
        palette.text_primary
    };
    let lead_icon = match mark {
        Mark::Eye if row.marked => Some((Icon::Eye, palette.accent)),
        Mark::Eye => Some((Icon::EyeOff, palette.text_muted)),
        _ => row
            .icon
            .map(|icon| (icon, if chosen { ink } else { palette.text_secondary })),
    };
    let mut left = rect.left() + ROW_PAD_X;
    if let Some((icon, color)) = lead_icon {
        paint_icon(
            painter,
            egui::pos2(left + ICON_SIZE / 2.0, rect.center().y),
            ICON_SIZE,
            icon,
            color,
        );
        left += ICON_SIZE + ICON_GAP;
    }
    let family = if chosen {
        theme::medium_family(ui.ctx())
    } else {
        egui::FontFamily::Proportional
    };
    let text = painter.layout_job(truncated(
        row.label,
        egui::FontId::new(LABEL_SIZE, family),
        rect.right() - ROW_PAD_X - left,
    ));
    painter.galley(
        egui::pos2(left, rect.center().y - text.size().y / 2.0),
        text,
        ink,
    );
    let label = row.label.to_owned();
    let marked = row.marked;
    response.widget_info(move || match mark {
        Mark::None => egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label),
        Mark::Tint => {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, marked, &label)
        }
        Mark::Eye => egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, marked, &label),
    });
    response
}

fn filter_field(ui: &mut egui::Ui, palette: &Palette, filter: &mut String) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover(),
    );
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + ROW_PAD_X + ICON_SIZE / 2.0, rect.center().y),
        ICON_SIZE,
        Icon::Search,
        palette.text_muted,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(rect.left() + ROW_PAD_X + ICON_SIZE + ICON_GAP, rect.top()),
        rect.max,
    );
    let response = ui.put(
        field,
        egui::TextEdit::singleline(filter)
            .frame(egui::Frame::NONE)
            .vertical_align(egui::Align::Center)
            .font(egui::FontId::proportional(LABEL_SIZE))
            .hint_text(egui::RichText::new(FILTER_HINT).color(palette.text_muted))
            .desired_width(f32::INFINITY),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() + ROW_SPACING,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    ui.add_space(ROW_SPACING * 2.0);
    response
}

fn note_row(ui: &mut egui::Ui, palette: &Palette, note: &str) {
    if note.is_empty() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + ROW_PAD_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        note,
        egui::FontId::proportional(LABEL_SIZE),
        palette.text_muted,
    );
}

fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
        .size()
        .x
}

fn truncated(text: &str, font: egui::FontId, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    job.wrap = TextWrapping::truncate_at_width(width.max(0.0));
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_position_rounds_only_the_outer_corners() {
        assert_eq!(SegmentPosition::at(0, 3), SegmentPosition::First);
        assert_eq!(SegmentPosition::at(1, 3), SegmentPosition::Middle);
        assert_eq!(SegmentPosition::at(2, 3), SegmentPosition::Last);
    }
}
