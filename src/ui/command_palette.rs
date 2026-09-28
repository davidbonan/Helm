//! Command palette window (keybindings.md §1): breadcrumb, fuzzy query and ranked
//! rows the app builds for the current screen. Navigation (↑/↓, `Esc` / `Backspace`
//! back, breadcrumb clicks) drives the palette state here; activating or removing
//! a row is reported to the app.

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use lucide_icons::Icon;

use crate::command_palette::{rank, CommandPalette, FuzzyMatch};
use crate::theme::Palette;

const WIDTH: f32 = 580.0;
const TOP_OFFSET: f32 = 96.0;
const PADDING: i8 = 8;
const QUERY_SIZE: f32 = 15.0;
const QUERY_ICON_SIZE: f32 = 15.0;
const CRUMB_SIZE: f32 = 12.0;
const TITLE_SIZE: f32 = 13.5;
const CONTEXT_SIZE: f32 = 12.5;
const DETAIL_SIZE: f32 = 11.5;
const BADGE_SIZE: f32 = 11.5;
const HINT_SIZE: f32 = 11.5;
const ROW_HEIGHT: f32 = 32.0;
const ROW_WITH_DETAIL_HEIGHT: f32 = 46.0;
const ROW_PAD_X: f32 = 10.0;
const LEADING_W: f32 = 24.0;
const LEADING_ICON_SIZE: f32 = 14.0;
const RUNNING_DOT_RADIUS: f32 = 4.0;
const CONTEXT_GAP: f32 = 10.0;
const CONTEXT_ICON_SIZE: f32 = 12.0;
const CONTEXT_ICON_GAP: f32 = 4.0;
const DETAIL_GAP: f32 = 3.0;
const CHEVRON_SIZE: f32 = 14.0;
const REMOVE_HIT: f32 = 24.0;
const REMOVE_GLYPH: f32 = 14.0;
const RIGHT_GAP: f32 = 8.0;
const BADGE_PAD: egui::Vec2 = egui::vec2(6.0, 2.0);
const LIST_MAX_HEIGHT: f32 = 360.0;
const ROW_RADIUS: u8 = 6;
const LOADING_TEXT: &str = "Loading…";
const RECENT_HEADING: &str = "Recent";
const OTHERS_HEADING: &str = "Other commands";
const HEADING_TOP_GAP: f32 = 6.0;
const HEADING_BOTTOM_GAP: f32 = 4.0;

pub enum Leading {
    Icon(Icon),
    /// Green status dot, the Run strip's running indicator.
    Running,
}

pub struct PaletteRow {
    pub leading: Leading,
    pub title: String,
    /// Muted qualifier after the title, with its glyph (a branch).
    pub context: Option<(Icon, String)>,
    /// Second line in monospace (a command).
    pub detail: Option<String>,
    pub badge: Option<String>,
    pub opens_screen: bool,
    /// What `Enter` does to the row, named in the footer.
    pub enter_label: &'static str,
    /// Tooltip of the trailing ✕ (also `Cmd+Backspace`); `None` = no ✕.
    pub remove_label: Option<&'static str>,
}

impl PaletteRow {
    pub fn command(icon: Icon, title: &str, opens_screen: bool) -> Self {
        Self {
            leading: Leading::Icon(icon),
            title: title.to_owned(),
            context: None,
            detail: None,
            badge: None,
            opens_screen,
            enter_label: if opens_screen { "Open" } else { "Run" },
            remove_label: None,
        }
    }

    fn searched_fields(&self) -> Vec<&str> {
        let context = self.context.as_ref().map_or("", |(_, text)| text.as_str());
        let detail = self.detail.as_deref().unwrap_or("");
        let badge = self.badge.as_deref().unwrap_or("");
        vec![&self.title, context, detail, badge]
    }
}

/// Indices refer to the `rows` given, not to their ranked order.
#[derive(Default)]
pub struct PaletteModalAction {
    pub activate: Option<usize>,
    pub remove: Option<usize>,
    pub dismiss: bool,
}

struct NavKeys {
    back: bool,
    up: bool,
    down: bool,
    enter: bool,
    remove: bool,
}

pub struct PaletteView<'a> {
    /// `None` while the screen's content is still loading.
    pub rows: Option<&'a [PaletteRow]>,
    /// What the screen's rows act on (the active worktree), shown once beside the query.
    pub scope: Option<(Icon, String)>,
    /// Leading rows the user ran lately, headed *Recent* while the query is blank.
    pub recent_rows: usize,
}

struct Listing<'a> {
    rows: &'a [PaletteRow],
    hits: &'a [(usize, FuzzyMatch)],
    /// Leading hits headed *Recent*, the others after them; `0` = no heading.
    recent: usize,
}

impl Listing<'_> {
    fn heading_before(&self, position: usize) -> Option<&'static str> {
        match position {
            _ if self.recent == 0 => None,
            0 => Some(RECENT_HEADING),
            position if position == self.recent => Some(OTHERS_HEADING),
            _ => None,
        }
    }
}

pub fn command_palette_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut CommandPalette,
    view: &PaletteView<'_>,
) -> PaletteModalAction {
    let loading = view.rows.is_none();
    let rows = view.rows.unwrap_or_default();
    let mut action = PaletteModalAction::default();
    let id = egui::Id::new("command_palette");
    let area =
        egui::Modal::default_area(id).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, TOP_OFFSET));
    let modal = egui::Modal::new(id)
        .area(area)
        .frame(crate::ui::modal_frame(ui.style()).inner_margin(PADDING))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(WIDTH);
            let screen = state.screen();
            if state.trail().count() > 1 {
                breadcrumb(ui, palette, state);
            }

            let fields: Vec<Vec<&str>> = rows.iter().map(PaletteRow::searched_fields).collect();
            let hits = rank(&state.query, &fields);
            let keys = nav_keys(ui, state.query.is_empty());
            if keys.back && !state.back() {
                action.dismiss = true;
            }
            if keys.up {
                state.select_previous(hits.len());
            }
            if keys.down {
                state.select_next(hits.len());
            }
            let selected = state.selected(hits.len());
            let selected_row = selected.map(|position| hits[position].0);
            if keys.enter {
                action.activate = selected_row;
            }
            if keys.remove {
                action.remove = selected_row.filter(|&index| rows[index].remove_label.is_some());
            }

            query_field(ui, palette, state, view.scope.as_ref());
            ui.separator();
            if state.screen() != screen {
                ui.ctx().request_repaint();
                return;
            }
            if loading {
                placeholder(ui, palette, LOADING_TEXT);
            } else if rows.is_empty() {
                placeholder(ui, palette, screen.empty_text());
            } else if hits.is_empty() {
                placeholder(
                    ui,
                    palette,
                    &format!("No match for “{}”.", state.query.trim()),
                );
            } else {
                let listed_screen = id.with("listed_screen");
                let newly_listed = ui.data(|d| d.get_temp(listed_screen)) != Some(screen);
                let scroll_selected = keys.up || keys.down || newly_listed;
                let listing = Listing {
                    rows,
                    hits: &hits,
                    recent: if state.query.trim().is_empty() {
                        view.recent_rows
                    } else {
                        0
                    },
                };
                list(ui, palette, state, &listing, scroll_selected, &mut action);
                ui.data_mut(|d| d.insert_temp(listed_screen, screen));
            }
            ui.separator();
            footer(
                ui,
                palette,
                selected_row.map(|index| &rows[index]),
                state.trail().count() > 1,
            );
        });
    if modal.backdrop_response.clicked() {
        action.dismiss = true;
    }
    action
}

/// Read and consumed before the query field sees them: `Esc` would blur it, and a
/// `Backspace` on an empty query or a `Cmd+Backspace` on a removable row is not an edit.
fn nav_keys(ui: &mut egui::Ui, query_empty: bool) -> NavKeys {
    ui.input_mut(|input| NavKeys {
        back: input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            || (query_empty && input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)),
        up: input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
        down: input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
        enter: input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        remove: input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace),
    })
}

fn breadcrumb(ui: &mut egui::Ui, palette: &Palette, state: &mut CommandPalette) {
    let trail: Vec<_> = state.trail().collect();
    let last = trail.len() - 1;
    let mut back_to = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(4.0);
        for (depth, screen) in trail.iter().enumerate() {
            let text = egui::RichText::new(screen.title()).size(CRUMB_SIZE);
            if depth == last {
                ui.label(text.color(palette.text_primary).strong());
                continue;
            }
            let crumb = ui
                .add(egui::Label::new(text.color(palette.text_muted)).sense(egui::Sense::click()))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if crumb.clicked() {
                back_to = Some(depth);
            }
            ui.label(
                egui::RichText::new(Icon::ChevronRight.unicode().to_string())
                    .size(CRUMB_SIZE)
                    .color(palette.text_muted),
            );
        }
    });
    if let Some(depth) = back_to {
        state.back_to(depth);
    }
    ui.add_space(2.0);
}

fn query_field(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut CommandPalette,
    scope: Option<&(Icon, String)>,
) {
    let before = state.query.clone();
    let hint = state.screen().query_hint();
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(Icon::Search.unicode().to_string())
                .size(QUERY_ICON_SIZE)
                .color(palette.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some((icon, label)) = scope {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("{} {label}", icon.unicode()))
                        .size(CONTEXT_SIZE)
                        .color(palette.text_secondary),
                );
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .id(egui::Id::new("command_palette_query"))
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::proportional(QUERY_SIZE))
                    .hint_text(egui::RichText::new(hint).color(palette.text_muted))
                    .desired_width(f32::INFINITY),
            );
            response.request_focus();
        });
    });
    if state.query != before {
        state.select(0);
    }
}

fn placeholder(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD_X);
        ui.label(egui::RichText::new(text).color(palette.text_muted));
    });
    ui.add_space(10.0);
}

fn list(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut CommandPalette,
    listing: &Listing<'_>,
    scroll_selected: bool,
    action: &mut PaletteModalAction,
) {
    let selected = state.selected(listing.hits.len());
    let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    egui::ScrollArea::vertical()
        .id_salt("command_palette_rows")
        .max_height(LIST_MAX_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (position, (index, hit)) in listing.hits.iter().enumerate() {
                if let Some(heading) = listing.heading_before(position) {
                    if position > 0 {
                        ui.add_space(HEADING_TOP_GAP);
                    }
                    list_heading(ui, palette, heading);
                }
                let is_selected = selected == Some(position);
                let out = row(ui, palette, &listing.rows[*index], hit, is_selected);
                if is_selected && scroll_selected {
                    out.response.scroll_to_me(None);
                }
                if out.response.hovered() && pointer_moved {
                    state.select(position);
                }
                if out.removed {
                    action.remove = Some(*index);
                } else if out.response.clicked() {
                    action.activate = Some(*index);
                }
            }
        });
}

fn list_heading(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD_X);
        ui.label(crate::ui::section_label(palette, text));
    });
    ui.add_space(HEADING_BOTTOM_GAP);
}

struct RowOutput {
    response: egui::Response,
    removed: bool,
}

fn row(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &PaletteRow,
    hit: &FuzzyMatch,
    selected: bool,
) -> RowOutput {
    let height = if row.detail.is_some() {
        ROW_WITH_DETAIL_HEIGHT
    } else {
        ROW_HEIGHT
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &row.title));
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(ROW_RADIUS),
            palette.bg_surface_hover,
        );
    }

    let mut right = rect.right() - ROW_PAD_X;
    let removed = row
        .remove_label
        .is_some_and(|label| remove_button(ui, palette, &response, &mut right, label));
    if row.opens_screen {
        crate::ui::paint_icon(
            ui.painter(),
            egui::pos2(right - CHEVRON_SIZE / 2.0, rect.center().y),
            CHEVRON_SIZE,
            Icon::ChevronRight,
            palette.text_muted,
        );
        right -= CHEVRON_SIZE + RIGHT_GAP;
    }
    if let Some(badge) = &row.badge {
        right = paint_badge(ui, palette, badge, egui::pos2(right, rect.center().y)) - RIGHT_GAP;
    }

    let left = rect.left() + ROW_PAD_X;
    let first_line_y = if row.detail.is_some() {
        rect.top() + height * 0.34
    } else {
        rect.center().y
    };
    paint_leading(
        ui,
        palette,
        &row.leading,
        egui::pos2(left + LEADING_ICON_SIZE / 2.0, first_line_y),
    );

    let text_left = left + LEADING_W;
    let text_width = (right - text_left).max(0.0);
    let title = highlighted(
        &row.title,
        &hit.positions[0],
        egui::FontId::proportional(TITLE_SIZE),
        palette.text_primary,
        palette.accent,
        text_width,
    );
    let title = ui.painter().layout_job(title);
    let title_size = title.size();
    ui.painter().galley(
        egui::pos2(text_left, first_line_y - title_size.y / 2.0),
        title,
        palette.text_primary,
    );

    if let Some((icon, context)) = &row.context {
        let icon_x = text_left + title_size.x + CONTEXT_GAP;
        let context_left = icon_x + CONTEXT_ICON_SIZE + CONTEXT_ICON_GAP;
        if context_left < right {
            crate::ui::paint_icon(
                ui.painter(),
                egui::pos2(icon_x + CONTEXT_ICON_SIZE / 2.0, first_line_y),
                CONTEXT_ICON_SIZE,
                *icon,
                palette.text_muted,
            );
            let job = highlighted(
                context,
                &hit.positions[1],
                egui::FontId::proportional(CONTEXT_SIZE),
                palette.text_secondary,
                palette.accent,
                right - context_left,
            );
            let galley = ui.painter().layout_job(job);
            let y = first_line_y - galley.size().y / 2.0;
            ui.painter()
                .galley(egui::pos2(context_left, y), galley, palette.text_secondary);
        }
    }

    if let Some(detail) = &row.detail {
        let job = highlighted(
            detail,
            &hit.positions[2],
            egui::FontId::monospace(DETAIL_SIZE),
            palette.text_muted,
            palette.accent,
            text_width,
        );
        let galley = ui.painter().layout_job(job);
        let y = first_line_y + title_size.y / 2.0 + DETAIL_GAP;
        ui.painter()
            .galley(egui::pos2(text_left, y), galley, palette.text_muted);
    }

    RowOutput { response, removed }
}

/// The trailing ✕, laid over the row so its click wins over the row's; shifts
/// `right` past it.
fn remove_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &egui::Response,
    right: &mut f32,
    label: &'static str,
) -> bool {
    let rect = egui::Rect::from_center_size(
        egui::pos2(*right - REMOVE_HIT / 2.0, row.rect.center().y),
        egui::vec2(REMOVE_HIT, REMOVE_HIT),
    );
    *right -= REMOVE_HIT + RIGHT_GAP;
    let response = ui
        .interact(rect, row.id.with("remove"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let color = if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(ROW_RADIUS),
            palette.bg_surface,
        );
        palette.git_deleted
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), REMOVE_GLYPH, Icon::X, color);
    response.clicked()
}

/// Right-aligned chip ending at `right_center`; returns its left edge.
fn paint_badge(ui: &egui::Ui, palette: &Palette, text: &str, right_center: egui::Pos2) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(BADGE_SIZE),
        palette.text_secondary,
    );
    let size = galley.size() + BADGE_PAD * 2.0;
    let rect = egui::Rect::from_min_size(
        egui::pos2(right_center.x - size.x, right_center.y - size.y / 2.0),
        size,
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_BUTTON),
        palette.bg_surface,
    );
    ui.painter()
        .galley(rect.min + BADGE_PAD, galley, palette.text_secondary);
    rect.left()
}

fn paint_leading(ui: &egui::Ui, palette: &Palette, leading: &Leading, center: egui::Pos2) {
    match leading {
        Leading::Icon(icon) => crate::ui::paint_icon(
            ui.painter(),
            center,
            LEADING_ICON_SIZE,
            *icon,
            palette.text_secondary,
        ),
        Leading::Running => {
            ui.painter()
                .circle_filled(center, RUNNING_DOT_RADIUS, palette.git_added);
        }
    }
}

/// `text` on one line truncated to `max_width`, the chars at `positions` in `highlight`.
fn highlighted(
    text: &str,
    positions: &[usize],
    font: egui::FontId,
    color: egui::Color32,
    highlight: egui::Color32,
    max_width: f32,
) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: TextWrapping::truncate_at_width(max_width),
        ..Default::default()
    };
    let mut run = String::new();
    let mut run_highlighted = false;
    for (index, c) in text.chars().enumerate() {
        let is_highlighted = positions.contains(&index);
        if is_highlighted != run_highlighted && !run.is_empty() {
            let run_color = if run_highlighted { highlight } else { color };
            job.append(&run, 0.0, TextFormat::simple(font.clone(), run_color));
            run.clear();
        }
        run_highlighted = is_highlighted;
        run.push(c);
    }
    let run_color = if run_highlighted { highlight } else { color };
    job.append(&run, 0.0, TextFormat::simple(font, run_color));
    job
}

fn footer(ui: &mut egui::Ui, palette: &Palette, selected: Option<&PaletteRow>, nested: bool) {
    let mut hints = vec![("↑↓", "Navigate")];
    if let Some(row) = selected {
        hints.push(("↵", row.enter_label));
        if let Some(label) = row.remove_label {
            hints.push(("⌘⌫", label));
        }
    }
    hints.push(("esc", if nested { "Back" } else { "Close" }));
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        for (key, label) in hints {
            ui.label(
                egui::RichText::new(key)
                    .size(HINT_SIZE)
                    .color(palette.text_secondary),
            );
            ui.label(
                egui::RichText::new(label)
                    .size(HINT_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(8.0);
        }
    });
}
