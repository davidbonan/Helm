use crate::keybindings::{Action, Keymap};
use crate::theme::{Palette, RADIUS_PILL, SHORTCUT_BADGE_SIZE};
use crate::ui::MAX_SHORTCUT;

/// Signals emitted by the tab bar within a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct TabBarAction {
    pub select: Option<usize>,
    pub close: Option<usize>,
    pub new: bool,
    pub rename: Option<(usize, String)>,
    /// Drag-and-drop reorder of the active repo's tabs: move tab `from` to land
    /// before (or `after`) tab `anchor`. `Workspace::reorder_tab` arbitrates.
    pub reorder: Option<(usize, usize, bool)>,
}

/// Drag payload carried while a tab chip is dragged: its index in the tab bar.
#[derive(Clone, Copy)]
struct DragTab(usize);

/// In-progress edit of a tab name (double-click or "Rename" context menu).
/// Held by `HelmApp` across frames; commits on Enter or focus loss, abandons on
/// Escape. Trimming and reverting to the default title are arbitrated by
/// `Workspace::rename_tab`.
pub struct TabRename {
    index: usize,
    buffer: String,
    started: bool,
}

impl TabRename {
    pub fn new(index: usize, current: &str) -> Self {
        Self {
            index,
            buffer: current.to_owned(),
            started: false,
        }
    }
}

const BAR_HEIGHT: f32 = 34.0;
const TAB_PAD_X: f32 = 12.0;
const TAB_GAP: f32 = 2.0;
const TAB_MIN_W: f32 = 52.0;
const TAB_MAX_W: f32 = 220.0;
const LABEL_SIZE: f32 = 13.0;
const CLOSE_SIZE: f32 = 14.0;
const CLOSE_GLYPH: f32 = 12.0;
/// Gap between the title and the trailing close/badge slot.
const TITLE_TRAIL_GAP: f32 = 8.0;
/// Trailing slot reserved for the close cross or the `⌘N` badge. Constant so
/// revealing the badge (Cmd held) never reflows the content-fit chips.
const TRAIL_W: f32 = 24.0;
/// Right inset of the close cross center, kept tight so the ✕ hugs the tab edge.
const CLOSE_PAD_RIGHT: f32 = 6.0;
/// Vertical separator between chips (and before the `+`): footprint and line height.
const DIVIDER_W: f32 = 13.0;
const DIVIDER_H: f32 = 16.0;
/// Per-tab bottom border, drawn over the baseline hairline: opaque accent on the
/// active tab, near-transparent accent on the inactive ones.
const UNDERLINE_H: f32 = 2.0;
const INACTIVE_UNDERLINE_ALPHA: u8 = 100;
const PLUS_HIT: f32 = 26.0;
const PLUS_SIZE: f32 = 18.0;

/// Tab bar heading the central area (design-system §4, terminal.md §4). Lists the
/// active repo's tabs (`titles`); the active tab carries `accent` text and an
/// accent underline, chips are separated by thin dividers. Clicking a tab switches
/// to it; hovering reveals a per-tab close cross; the `+` button on the right opens
/// a New Tab. Double-click or "Rename" context menu edits the name in place (the
/// `rename` state is held by the caller). Pure rendering: actions are reported in
/// `out` and executed by the caller (same path as `Cmd+T`/`Cmd+1..9`/`Cmd+W`).
pub fn tab_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    titles: &[String],
    active: usize,
    rename: &mut Option<TabRename>,
    keymap: &Keymap,
    out: &mut TabBarAction,
) {
    // Tab close/reindex: an edit state pointing past the list is abandoned.
    if rename.as_ref().is_some_and(|r| r.index >= titles.len()) {
        *rename = None;
    }
    // Tab selection is ⌘1..9 (lone Cmd): the badge hides as soon as Ctrl joins,
    // since ⌃⌘1..9 then selects a repo (keybindings §1).
    let cmd_held = ui.input(|i| {
        let m = i.modifiers;
        m.command && !m.shift && !m.alt && !m.ctrl
    });
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), BAR_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TAB_GAP, 0.0);
            // The drop side (before/after the hovered chip) follows the live pointer;
            // `dnd_*_payload` only fire on the chip under it, so this is `Some` then.
            let pointer = ui.input(|i| i.pointer.interact_pos());
            // Chips in render order — feeds the drop insertion line, drawn after the
            // loop once every rect is known.
            let mut placed: Vec<(usize, egui::Rect)> = Vec::new();
            let mut hovering: Option<(usize, usize, bool)> = None;
            let mut released: Option<(usize, usize, bool)> = None;
            for (index, title) in titles.iter().enumerate() {
                // Stable id per chip: the badge's `new_child` (conditional on Cmd)
                // would otherwise shift the auto-ids of the following chips (egui red
                // frames + relayout, same pitfall as the repos sidebar).
                let response = ui
                    .push_id(index, |ui| {
                        tab_chip(
                            ui,
                            palette,
                            index,
                            title,
                            index == active,
                            cmd_held,
                            rename,
                            out,
                        )
                    })
                    .inner;
                let after = pointer.is_some_and(|p| p.x > response.rect.center().x);
                if let Some(drag) = response.dnd_hover_payload::<DragTab>() {
                    hovering = Some((drag.0, index, after));
                }
                if let Some(drag) = response.dnd_release_payload::<DragTab>() {
                    released = Some((drag.0, index, after));
                }
                placed.push((index, response.rect));
                divider(ui, palette);
            }
            plus_button(ui, palette, out);
            if let Some(shortcut) = keymap.shortcut_for(Action::NewTab).filter(|_| cmd_held) {
                ui.label(
                    egui::RichText::new(shortcut.display())
                        .size(SHORTCUT_BADGE_SIZE)
                        .color(palette.text_muted),
                );
            }
            if let Some((from, anchor, after)) = released {
                if tab_insert_at(from, anchor, after, titles.len()).is_some() {
                    out.reorder = Some((from, anchor, after));
                }
            } else if let Some((from, anchor, after)) = hovering {
                if let Some(insert_at) = tab_insert_at(from, anchor, after, titles.len()) {
                    draw_tab_drop_line(ui, palette, &placed, insert_at);
                }
            }
            // Bottom borders are painted on the bar's own layer: drawing them after
            // this closure (parent painter) lands them past the bar — the terminal
            // below overdraws them. The baseline hairline spans the whole bar; each
            // chip then gets an opaque accent border when active, near-transparent
            // otherwise.
            let bottom = ui.max_rect().bottom();
            ui.painter().hline(
                ui.max_rect().x_range(),
                bottom - 0.5,
                egui::Stroke::new(1.0_f32, palette.border_subtle),
            );
            for (index, rect) in &placed {
                let color = if *index == active {
                    palette.accent
                } else {
                    crate::ui::with_alpha(palette.accent, INACTIVE_UNDERLINE_ALPHA)
                };
                ui.painter().hline(
                    rect.x_range(),
                    bottom - UNDERLINE_H / 2.0,
                    egui::Stroke::new(UNDERLINE_H, color),
                );
            }
        },
    );
}

/// Thin vertical separator drawn between chips and before the `+` (design mock).
fn divider(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(DIVIDER_W, BAR_HEIGHT), egui::Sense::hover());
    let half = DIVIDER_H / 2.0;
    ui.painter().vline(
        rect.center().x,
        (rect.center().y - half)..=(rect.center().y + half),
        egui::Stroke::new(1.0_f32, palette.border_input),
    );
}

/// Insertion slot (0..=len, in the pre-removal indexing) for dropping tab `from`
/// against `anchor`, or `None` for a no-op drop onto its own edge. Used both to
/// gate the emitted reorder and to position the preview line.
fn tab_insert_at(from: usize, anchor: usize, after: bool, len: usize) -> Option<usize> {
    if from >= len || anchor >= len {
        return None;
    }
    let insert_at = if after { anchor + 1 } else { anchor };
    (insert_at != from && insert_at != from + 1).then_some(insert_at)
}

/// Vertical insertion indicator drawn during a tab drag: an accent line in the gap
/// before the chip at `insert_at`, or after the last chip when the tab lands at the end.
fn draw_tab_drop_line(
    ui: &egui::Ui,
    palette: &Palette,
    placed: &[(usize, egui::Rect)],
    insert_at: usize,
) {
    let Some((_, first)) = placed.first() else {
        return;
    };
    let x = placed
        .iter()
        .find(|(index, _)| *index == insert_at)
        .map(|(_, rect)| rect.left() - TAB_GAP / 2.0)
        .or_else(|| placed.last().map(|(_, rect)| rect.right() + TAB_GAP / 2.0))
        .unwrap_or(first.left());
    ui.painter().vline(
        x,
        first.top()..=first.bottom(),
        egui::Stroke::new(2.0_f32, palette.accent),
    );
}

#[allow(clippy::too_many_arguments)]
fn tab_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    index: usize,
    title: &str,
    is_active: bool,
    cmd_held: bool,
    rename: &mut Option<TabRename>,
    out: &mut TabBarAction,
) -> egui::Response {
    // Measure the title (truncated to the cap) so the chip fits its content; the
    // trailing slot is a constant width, never the badge's, so Cmd never reflows.
    let max_title_w = TAB_MAX_W - 2.0 * TAB_PAD_X - TITLE_TRAIL_GAP - TRAIL_W;
    let mut job = egui::text::LayoutJob::single_section(
        title.to_owned(),
        egui::text::TextFormat::simple(
            egui::FontId::proportional(LABEL_SIZE),
            tab_text_color(palette, is_active),
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_title_w);
    let galley = ui.painter().layout_job(job);
    let galley_size = galley.size();
    let chip_w =
        (2.0 * TAB_PAD_X + galley_size.x + TITLE_TRAIL_GAP + TRAIL_W).clamp(TAB_MIN_W, TAB_MAX_W);

    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(chip_w, BAR_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.dnd_set_drag_payload(DragTab(index));
    let hovered = response.hovered();
    let dragged = response.dragged();
    if dragged {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.bg_surface_hover,
        );
    }

    if rename.as_ref().is_some_and(|r| r.index == index) {
        rename_edit(ui, rect, index, rename, out);
        return response;
    }

    ui.painter().galley(
        egui::pos2(
            rect.left() + TAB_PAD_X,
            rect.center().y - galley_size.y / 2.0,
        ),
        galley,
        tab_text_color(palette, is_active),
    );

    let trailing_left = rect.right() - TAB_PAD_X - TRAIL_W;
    // While Cmd is held: the ⌘N badge replaces the close cross (inert hit).
    let badge_shown = cmd_held && index < MAX_SHORTCUT;
    if badge_shown {
        let mut badge = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(trailing_left, rect.top()),
                    egui::pos2(rect.right() - TAB_PAD_X, rect.bottom()),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        badge.label(
            egui::RichText::new(format!("⌘{}", index + 1))
                .size(SHORTCUT_BADGE_SIZE)
                .color(palette.text_muted),
        );
    }

    let close_clicked = if badge_shown {
        false
    } else {
        let close_rect = egui::Rect::from_center_size(
            egui::pos2(
                rect.right() - CLOSE_PAD_RIGHT - CLOSE_SIZE / 2.0,
                rect.center().y,
            ),
            egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
        );
        let close = ui
            .interact(
                close_rect,
                ui.id().with(("tab_close", index)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if hovered {
            paint_close_glyph(ui.painter(), close_rect, palette, close.hovered());
        }
        close.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Close {title}"))
        });
        close.clicked()
    };

    if response.double_clicked() {
        *rename = Some(TabRename::new(index, title));
    } else if response.clicked() && !close_clicked {
        out.select = Some(index);
    }
    if close_clicked {
        out.close = Some(index);
    }

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, is_active, title)
    });
    egui::Popup::context_menu(&response)
        .style(crate::theme::menu_style)
        .show(|ui| {
            if ui.button("Rename").clicked() {
                *rename = Some(TabRename::new(index, title));
                ui.close();
            }
        });
    response
}

/// In-place name editor inside the chip: focus + select-all on open, Enter or focus
/// loss commits (reported in `out.rename`), Escape abandons.
fn rename_edit(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    index: usize,
    rename: &mut Option<TabRename>,
    out: &mut TabBarAction,
) {
    let state = rename.as_mut().expect("checked by caller");
    let mut edit_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(TAB_PAD_X / 2.0, 1.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let output = egui::TextEdit::singleline(&mut state.buffer)
        .font(egui::FontId::proportional(LABEL_SIZE))
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(4, 1))
        .show(&mut edit_ui);

    if !state.started {
        state.started = true;
        output.response.request_focus();
        let mut edit_state = output.state;
        edit_state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(state.buffer.chars().count()),
            )));
        edit_state.store(ui.ctx(), output.response.id);
        return;
    }

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        *rename = None;
    } else if output.response.lost_focus() {
        out.rename = Some((index, state.buffer.clone()));
        *rename = None;
    }
}

fn plus_button(ui: &mut egui::Ui, palette: &Palette, out: &mut TabBarAction) {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(PLUS_HIT, BAR_HEIGHT), true);
    if response.clicked() {
        out.new = true;
    }
    if hovered {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(0.0, 5.0)),
            egui::CornerRadius::same(RADIUS_PILL),
            palette.bg_surface_hover,
        );
    }
    let color = if hovered {
        palette.text_primary
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(
        ui.painter(),
        rect.center(),
        PLUS_SIZE,
        lucide_icons::Icon::Plus,
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "New tab"));
}

pub fn tab_text_color(palette: &Palette, is_active: bool) -> egui::Color32 {
    if is_active {
        palette.accent
    } else {
        palette.text_secondary
    }
}

fn paint_close_glyph(painter: &egui::Painter, rect: egui::Rect, palette: &Palette, hovered: bool) {
    let color = if hovered {
        palette.text_primary
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(
        painter,
        rect.center(),
        CLOSE_GLYPH,
        lucide_icons::Icon::X,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_tab_uses_accent_text() {
        let p = Palette::light();
        assert_eq!(tab_text_color(&p, true), p.accent);
        assert_eq!(tab_text_color(&p, false), p.text_secondary);
    }
}
