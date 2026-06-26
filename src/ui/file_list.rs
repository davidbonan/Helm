//! File list — shared style (design-system §4) used by both the git status
//! sidebar and the commit detail: full-width row, status icon, elided path,
//! `+N`/`−N` stats in fixed columns, dimmed separators.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::status::ChangeKind;
use crate::theme::Palette;
use crate::ui::with_alpha;

/// How the WIP and commit-detail file lists lay out their entries: **Flat** is a
/// list of full paths (the historical layout); **Tree** groups them by directory
/// (IDE-style, collapsible). One shared, persisted mode (`Prefs.git_file_view`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileViewMode {
    #[default]
    Flat,
    Tree,
}

/// Breathing room is baked into the row (no spacing between rows): the
/// hover/selection highlight fills the whole line, separators abut it.
pub(crate) const ROW_HEIGHT: f32 = 34.0;
pub(crate) const PATH_SIZE: f32 = 14.0;
/// Row inner margin — shared by the status sidebar and the commit detail: the
/// status icon lands at the same x in every list.
const ROW_PAD_X: f32 = 10.0;
const SELECTED_ROW_ACCENT_W: f32 = 3.0;
const ROW_SEPARATOR_ALPHA: u8 = 110;
const STATUS_ICON_SIZE: f32 = 15.0;
const STATUS_ICON_W: f32 = 13.0;
const STATUS_ICON_GAP: f32 = 7.0;
const STAT_SIZE: f32 = 13.0;
const STAT_ADD_COL_W: f32 = 34.0;
const STAT_DEL_COL_W: f32 = 30.0;
const STAT_GAP: f32 = 6.0;
/// Tree-view content offset per directory level (file leaves and the chevron of
/// nested directory rows). Depth 0 sits flush with the flat layout.
pub const TREE_INDENT_STEP: f32 = 14.0;
const VIEW_TOGGLE_HIT: f32 = 22.0;
const VIEW_TOGGLE_GLYPH: f32 = 15.0;
const DIR_GLYPH_SIZE: f32 = 14.0;
const DIR_ICON_GAP: f32 = 6.0;

/// Data for a file-list row.
pub(crate) struct FileRow<'a> {
    /// Displayed text: the full path in Flat mode, the bare filename in Tree
    /// mode (the caller keeps the full path for selection / accessibility).
    pub path: &'a str,
    pub kind: ChangeKind,
    pub additions: usize,
    pub deletions: usize,
    pub selected: bool,
    /// Stats hidden on hover (the sidebar overlays its action pills there).
    pub stats_hidden_on_hover: bool,
    /// Left offset of the row content (status icon + path) for tree indentation.
    /// The hover/selection fill and the accent bar stay full-width.
    pub indent: f32,
    /// Optional right-side space reserved by callers for their own badges. It
    /// sits before the stats columns when stats are present, so the path elides
    /// before both the caller badges and the line stats.
    pub trailing_reserved: f32,
}

/// Response + geometry handed back to the caller for its own interactions
/// (path click area, action overlay on hover).
pub(crate) struct FileRowOutput {
    pub response: egui::Response,
    pub rect: egui::Rect,
    pub hovered: bool,
    pub path_left: f32,
    pub content_right: f32,
    pub trailing_rect: egui::Rect,
}

/// OS side effects the file-row context menu defers to the app (clipboard copies
/// resolve inline). Drained by the caller after the frame.
#[derive(Default)]
pub struct FileMenuOutput {
    pub reveal: Option<PathBuf>,
    pub open_in_editor: Option<PathBuf>,
}

/// Repo working directory (to resolve absolute paths) + the frame's menu output,
/// threaded to every file row of the WIP and commit-detail panels.
pub(crate) struct FileMenuCtx<'a> {
    pub root: Option<&'a Path>,
    pub out: &'a mut FileMenuOutput,
}

/// Right-click menu shared by the WIP and commit-detail file rows: clipboard
/// copies resolved here, reveal / open-in-editor surfaced to the app. No
/// stage/discard — those stay on the row's hover pills.
pub(crate) fn file_context_menu(response: &egui::Response, rel_path: &str, ctx: &mut FileMenuCtx) {
    egui::Popup::context_menu(response)
        .style(crate::theme::menu_style)
        .show(|ui| file_menu_entries(ui, rel_path, ctx));
}

/// The clipboard / reveal / open-in-editor entries, shared by the commit-detail
/// menu and the WIP sidebar menu (which prepends its own stage/discard/stash
/// actions). Added to an already-open menu `ui`, not its own popup.
pub(crate) fn file_menu_entries(ui: &mut egui::Ui, rel_path: &str, ctx: &mut FileMenuCtx) {
    let abs = ctx.root.map(|root| root.join(rel_path));
    if ui.button("Copy path").clicked() {
        let text = abs
            .as_ref()
            .map_or_else(|| rel_path.to_owned(), |p| p.to_string_lossy().into_owned());
        ui.ctx().copy_text(text);
        ui.close();
    }
    if ui.button("Copy relative path").clicked() {
        ui.ctx().copy_text(rel_path.to_owned());
        ui.close();
    }
    if let Some(abs) = abs {
        ui.separator();
        if ui.button("Reveal in Finder").clicked() {
            ctx.out.reveal = Some(abs.clone());
            ui.close();
        }
        if ui.button("Open in editor").clicked() {
            ctx.out.open_in_editor = Some(abs);
            ui.close();
        }
    }
}

/// Full-width row × [`ROW_HEIGHT`]: hover/selection background (+ accent bar on
/// the left), colored status icon, elided path (dimmed directory), `+N`/`−N`
/// stats in fixed columns on the right. `sense` is the caller's choice (whole
/// row clickable, or hover only with a dedicated click area on top).
pub(crate) fn file_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    sense: egui::Sense,
    row: &FileRow<'_>,
) -> FileRowOutput {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), sense);
    if sense.senses_click() {
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let hovered = rect_contains_pointer(ui, rect);
    if let Some(fill) = file_row_fill(palette, hovered, row.selected) {
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    if row.selected {
        let accent_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SELECTED_ROW_ACCENT_W, rect.bottom()),
        );
        ui.painter().rect_filled(accent_rect, 0.0, palette.accent);
    }
    let center_y = rect.center().y;
    let content_left = rect.left() + ROW_PAD_X + row.indent;
    let content_right = rect.right() - ROW_PAD_X;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(content_left + STATUS_ICON_W / 2.0, center_y),
        egui::vec2(STATUS_ICON_SIZE, STATUS_ICON_SIZE),
    );
    paint_status_icon(
        ui.painter(),
        icon_rect,
        status_icon(row.kind),
        status_color(palette, row.kind),
    );

    // The stats reserve tracks the data (not hover): the path does not "breathe"
    // when the sidebar hides the stats under its pills.
    let path_left = content_left + STATUS_ICON_W + STATUS_ICON_GAP;
    let has_stats = row.additions > 0 || row.deletions > 0;
    let stats_w = STAT_ADD_COL_W + STAT_GAP + STAT_DEL_COL_W;
    let trailing_reserved = row.trailing_reserved.max(0.0);
    let trailing_right = if has_stats {
        content_right - stats_w - STAT_GAP
    } else {
        content_right
    };
    let trailing_left = trailing_right - trailing_reserved;
    let trailing_gap = if trailing_reserved > 0.0 {
        STAT_GAP
    } else {
        0.0
    };
    let path_right = if trailing_reserved > 0.0 {
        trailing_left - trailing_gap
    } else if has_stats {
        content_right - stats_w - STAT_GAP
    } else {
        content_right
    };
    let path_max = (path_right - path_left).max(8.0);
    let galley = path_galley(ui, palette, row.path, path_max);
    ui.painter().galley(
        egui::pos2(path_left, center_y - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );

    // Child created unconditionally: `new_child` consumes one of the parent's
    // auto-ids, so creating it conditionally would shift the ids of the following
    // rows between frames.
    let stats_rect = egui::Rect::from_min_max(
        egui::pos2(content_right - stats_w, rect.top()),
        egui::pos2(content_right, rect.bottom()),
    );
    let mut stats_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(stats_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if has_stats && !(row.stats_hidden_on_hover && hovered) {
        stat_label(
            &mut stats_ui,
            palette.git_deleted,
            format!("−{}", row.deletions),
            STAT_DEL_COL_W,
        );
        stats_ui.add_space(STAT_GAP);
        stat_label(
            &mut stats_ui,
            palette.git_added,
            format!("+{}", row.additions),
            STAT_ADD_COL_W,
        );
    }

    FileRowOutput {
        response,
        rect,
        hovered,
        path_left,
        content_right,
        trailing_rect: egui::Rect::from_min_max(
            egui::pos2(trailing_left, rect.top()),
            egui::pos2(trailing_right, rect.bottom()),
        ),
    }
}

/// Icon-only Flat ⇄ Tree toggle for a file-list header (Git panel band,
/// commit-detail "Files changed" header). Shows the **target** mode's glyph
/// (target-mode affordance: `ListTree` while Flat, `List` while Tree) and
/// returns the mode to switch to when clicked.
pub fn view_toggle(
    ui: &mut egui::Ui,
    palette: &Palette,
    current: FileViewMode,
) -> Option<FileViewMode> {
    let target = match current {
        FileViewMode::Flat => FileViewMode::Tree,
        FileViewMode::Tree => FileViewMode::Flat,
    };
    let (icon, tooltip) = match target {
        FileViewMode::Tree => (lucide_icons::Icon::ListTree, "Tree view"),
        FileViewMode::Flat => (lucide_icons::Icon::List, "Flat view"),
    };
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(VIEW_TOGGLE_HIT, VIEW_TOGGLE_HIT), true);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::theme::RADIUS_PILL),
            palette.bg_surface_hover,
        );
    }
    let color = if hovered {
        palette.accent
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), VIEW_TOGGLE_GLYPH, icon, color);
    let response = response.on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    response.clicked().then_some(target)
}

/// Directory grouping row of the tree view: collapse chevron + folder glyph +
/// name, indented by `indent`. The hover fill spans the whole line and the
/// entire row is the click target (toggles the directory's collapsed state —
/// the caller owns the collapsed set). Returns the row response.
pub fn dir_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    name: &str,
    indent: f32,
    collapsed: bool,
) -> egui::Response {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(ui.available_width(), ROW_HEIGHT), true);
    if hovered {
        ui.painter()
            .rect_filled(rect, 0.0, palette.bg_surface_hover);
    }
    let center_y = rect.center().y;
    let content_left = rect.left() + ROW_PAD_X + indent;
    let chevron = if collapsed {
        lucide_icons::Icon::ChevronRight
    } else {
        lucide_icons::Icon::ChevronDown
    };
    let chevron_color = if hovered {
        palette.text_secondary
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(content_left + DIR_GLYPH_SIZE / 2.0, center_y),
        DIR_GLYPH_SIZE,
        chevron,
        chevron_color,
    );
    let folder_x = content_left + DIR_GLYPH_SIZE + DIR_ICON_GAP;
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(folder_x + DIR_GLYPH_SIZE / 2.0, center_y),
        DIR_GLYPH_SIZE,
        lucide_icons::Icon::Folder,
        palette.text_muted,
    );
    let name_left = folder_x + DIR_GLYPH_SIZE + DIR_ICON_GAP;
    let name_max = (rect.right() - ROW_PAD_X - name_left).max(8.0);
    let mut job = egui::text::LayoutJob::single_section(
        name.to_owned(),
        egui::text::TextFormat::simple(
            egui::FontId::proportional(PATH_SIZE),
            palette.text_secondary,
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(name_max);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(name_left, center_y - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response
}

/// Keyboard navigation ↑/↓: requests scrolling to row `target`, consumed by
/// [`consume_row_scroll`] when it renders. A click never requests a scroll — a
/// clicked row is already visible.
pub(crate) fn request_row_scroll<T: Clone + PartialEq + Send + Sync + 'static>(
    ui: &egui::Ui,
    id: egui::Id,
    target: T,
) {
    ui.data_mut(|data| data.insert_temp(id, target));
}

/// Scrolls the row into the viewport — shortest path, no centering — if it is
/// the target requested by [`request_row_scroll`], then consumes the request.
pub(crate) fn consume_row_scroll<T: Clone + PartialEq + Send + Sync + 'static>(
    ui: &egui::Ui,
    response: &egui::Response,
    id: egui::Id,
    target: &T,
) {
    let requested = ui
        .data(|data| data.get_temp::<T>(id))
        .is_some_and(|requested| requested == *target);
    if requested {
        response.scroll_to_me(None);
        ui.data_mut(|data| data.remove::<T>(id));
    }
}

/// Dimmed 1px rule between two rows.
pub(crate) fn row_separator(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, with_alpha(palette.border_subtle, ROW_SEPARATOR_ALPHA)),
    );
}

pub(crate) fn file_row_fill(
    palette: &Palette,
    hovered: bool,
    selected: bool,
) -> Option<egui::Color32> {
    if selected {
        Some(selected_file_row_fill(palette))
    } else if hovered {
        Some(palette.bg_surface_hover)
    } else {
        None
    }
}

fn selected_file_row_fill(palette: &Palette) -> egui::Color32 {
    if is_dark_color(palette.bg_canvas) {
        palette.accent_subtle
    } else {
        palette.bg_surface
    }
}

fn is_dark_color(color: egui::Color32) -> bool {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    u16::from(r) + u16::from(g) + u16::from(b) < 384
}

fn rect_contains_pointer(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|pos| rect.contains(pos))
    })
}

/// `+N` / `−N` number in a fixed right-aligned column (digits aligned across
/// rows); widened if the number overflows the column.
pub(crate) fn stat_label(ui: &mut egui::Ui, color: egui::Color32, text: String, column_w: f32) {
    let galley =
        ui.painter()
            .layout_no_wrap(text.clone(), egui::FontId::proportional(STAT_SIZE), color);
    let width = column_w.max(galley.size().x);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, ui.available_height()),
        egui::Sense::hover(),
    );
    ui.painter().galley(
        egui::pos2(
            rect.right() - galley.size().x,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
}

/// Elided path: dimmed directory, emphasized file.
pub(crate) fn path_galley(
    ui: &egui::Ui,
    palette: &Palette,
    path: &str,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let (dir, file) = match path.rfind('/') {
        Some(idx) => (&path[..=idx], &path[idx + 1..]),
        None => ("", path),
    };
    let mut job = egui::text::LayoutJob::default();
    if !dir.is_empty() {
        job.append(
            dir,
            0.0,
            egui::text::TextFormat::simple(
                egui::FontId::proportional(PATH_SIZE),
                palette.text_muted,
            ),
        );
    }
    job.append(
        file,
        0.0,
        egui::text::TextFormat::simple(egui::FontId::proportional(PATH_SIZE), palette.text_primary),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    ui.painter().layout_job(job)
}

pub(crate) fn status_icon(kind: ChangeKind) -> lucide_icons::Icon {
    match kind {
        ChangeKind::Untracked | ChangeKind::Added => lucide_icons::Icon::Plus,
        ChangeKind::Modified => lucide_icons::Icon::Pencil,
        ChangeKind::Deleted => lucide_icons::Icon::Minus,
        ChangeKind::Renamed => lucide_icons::Icon::ArrowRight,
        ChangeKind::Conflicted => lucide_icons::Icon::AlertTriangle,
    }
}

pub(crate) fn paint_status_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: lucide_icons::Icon,
    color: egui::Color32,
) {
    crate::ui::paint_icon(painter, rect.center(), rect.height(), icon, color);
}

pub(crate) fn status_color(palette: &Palette, kind: ChangeKind) -> egui::Color32 {
    match kind {
        ChangeKind::Untracked | ChangeKind::Added => palette.git_added,
        ChangeKind::Modified => palette.git_modified,
        ChangeKind::Deleted => palette.git_deleted,
        ChangeKind::Renamed => palette.git_renamed,
        ChangeKind::Conflicted => palette.git_conflict,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use egui_kittest::kittest::Queryable;

    use super::*;

    #[test]
    fn file_view_mode_default_and_snake_case() {
        assert_eq!(FileViewMode::default(), FileViewMode::Flat);
        assert_eq!(
            serde_json::to_string(&FileViewMode::Tree).unwrap(),
            "\"tree\""
        );
        assert_eq!(
            serde_json::to_string(&FileViewMode::Flat).unwrap(),
            "\"flat\""
        );
        assert_eq!(
            serde_json::from_str::<FileViewMode>("\"tree\"").unwrap(),
            FileViewMode::Tree
        );
    }

    #[test]
    fn view_toggle_advertises_the_target_mode_and_emits_it() {
        let palette = Palette::light();
        let picked: Rc<Cell<Option<FileViewMode>>> = Rc::new(Cell::new(None));
        let sink = picked.clone();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(120.0, 60.0))
            .build_ui(move |ui| {
                if let Some(mode) = view_toggle(ui, &palette, FileViewMode::Flat) {
                    sink.set(Some(mode));
                }
            });
        harness.step();
        // Flat mode advertises the Tree target; clicking emits the switch.
        harness.get_by_label("Tree view").click();
        harness.step();
        assert_eq!(picked.get(), Some(FileViewMode::Tree));
    }

    #[test]
    fn view_toggle_in_tree_mode_advertises_flat() {
        let palette = Palette::light();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(120.0, 60.0))
            .build_ui(move |ui| {
                view_toggle(ui, &palette, FileViewMode::Tree);
            });
        harness.step();
        harness.get_by_label("Flat view");
    }

    #[test]
    fn dir_row_renders_name_and_is_clickable() {
        let palette = Palette::light();
        let clicked = Rc::new(Cell::new(false));
        let sink = clicked.clone();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(240.0, 60.0))
            .build_ui(move |ui| {
                if dir_row(ui, &palette, "src", TREE_INDENT_STEP, false).clicked() {
                    sink.set(true);
                }
            });
        harness.step();
        harness.get_by_label("src").click();
        harness.step();
        assert!(clicked.get(), "a directory row click toggles its collapse");
    }

    #[test]
    fn file_row_indent_offsets_content_not_the_row() {
        let palette = Palette::light();
        let captured: Rc<RefCell<Vec<(f32, f32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = captured.clone();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(320.0, 120.0))
            .build_ui(move |ui| {
                sink.borrow_mut().clear();
                for indent in [0.0_f32, 2.0 * TREE_INDENT_STEP] {
                    let out = file_row(
                        ui,
                        &palette,
                        egui::Sense::click(),
                        &FileRow {
                            path: "file_tree.rs",
                            kind: ChangeKind::Modified,
                            additions: 0,
                            deletions: 0,
                            selected: false,
                            stats_hidden_on_hover: false,
                            indent,
                            trailing_reserved: 0.0,
                        },
                    );
                    sink.borrow_mut()
                        .push((out.rect.left(), out.rect.width(), out.path_left));
                }
            });
        harness.step();
        let rows = captured.borrow();
        assert_eq!(rows.len(), 2);
        let (flat_left, flat_w, flat_path) = rows[0];
        let (tree_left, tree_w, tree_path) = rows[1];
        // Fill + accent span: the row rect is identical at both indents.
        assert!(
            (flat_left - tree_left).abs() < 0.5,
            "row left must not shift"
        );
        assert!((flat_w - tree_w).abs() < 0.5, "row width must not shrink");
        // Only the content shifts right, by exactly the indent.
        assert!(
            (tree_path - flat_path - 2.0 * TREE_INDENT_STEP).abs() < 0.5,
            "content must shift by the indent"
        );
    }

    #[test]
    fn status_icons_cover_every_change_kind() {
        let glyph = |kind| status_icon(kind).unicode();
        assert_eq!(
            glyph(ChangeKind::Untracked),
            lucide_icons::Icon::Plus.unicode()
        );
        assert_eq!(glyph(ChangeKind::Added), lucide_icons::Icon::Plus.unicode());
        assert_eq!(
            glyph(ChangeKind::Modified),
            lucide_icons::Icon::Pencil.unicode()
        );
        assert_eq!(
            glyph(ChangeKind::Deleted),
            lucide_icons::Icon::Minus.unicode()
        );
        assert_eq!(
            glyph(ChangeKind::Renamed),
            lucide_icons::Icon::ArrowRight.unicode()
        );
        assert_eq!(
            glyph(ChangeKind::Conflicted),
            lucide_icons::Icon::AlertTriangle.unicode()
        );
    }

    #[test]
    fn status_color_distinguishes_each_kind() {
        let p = Palette::light();
        assert_eq!(status_color(&p, ChangeKind::Added), p.git_added);
        assert_eq!(status_color(&p, ChangeKind::Untracked), p.git_added);
        assert_eq!(status_color(&p, ChangeKind::Modified), p.git_modified);
        assert_eq!(status_color(&p, ChangeKind::Deleted), p.git_deleted);
        assert_eq!(status_color(&p, ChangeKind::Renamed), p.git_renamed);
        assert_eq!(status_color(&p, ChangeKind::Conflicted), p.git_conflict);
        assert_ne!(
            status_color(&p, ChangeKind::Modified),
            status_color(&p, ChangeKind::Deleted),
            "modified and deleted must read differently"
        );
    }

    #[test]
    fn selected_file_row_fill_stays_visible_in_dark_mode() {
        let p = Palette::dark();
        let old_delta = u16::from(max_channel_delta(p.bg_surface, p.bg_canvas));
        let selected = selected_file_row_fill(&p);
        let selected_delta = u16::from(max_channel_delta(selected, p.bg_canvas));

        assert!(
            selected_delta >= old_delta * 3,
            "dark selected row must contrast more than the old surface fill"
        );
        assert_eq!(file_row_fill(&p, false, true), Some(selected));
        assert_eq!(
            file_row_fill(&p, true, true),
            Some(selected),
            "the opened file stays highlighted while hovered"
        );
    }

    #[test]
    fn selected_file_row_fill_preserves_light_mode_surface() {
        let p = Palette::light();
        assert_eq!(selected_file_row_fill(&p), p.bg_surface);
        assert_eq!(file_row_fill(&p, true, false), Some(p.bg_surface_hover));
    }

    fn max_channel_delta(a: egui::Color32, b: egui::Color32) -> u8 {
        let [ar, ag, ab, _] = a.to_srgba_unmultiplied();
        let [br, bg, bb, _] = b.to_srgba_unmultiplied();
        ar.abs_diff(br).max(ag.abs_diff(bg)).max(ab.abs_diff(bb))
    }
}
