//! The Files tab's tree (specs/files.md §3, §5): one row per entry on show, keyboard
//! moves once a row was clicked.

use crate::files::tint::Tint;
use crate::files::tree::{key_move, EntryRow, RowKind, TreeKey, TreeMove, TreeRow};
use crate::theme::Palette;
use crate::ui::file_list;
use crate::ui::git_panel::GitIntent;

const ROW_HEIGHT: f32 = 26.0;
const ROW_PAD_X: f32 = 10.0;
const INDENT_STEP: f32 = 12.0;
const GLYPH_SIZE: f32 = 14.0;
const GLYPH_GAP: f32 = 4.0;
const NAME_GAP: f32 = 6.0;
const NAME_SIZE: f32 = 13.0;
const DOT_RADIUS: f32 = 3.0;
const DOT_RESERVE: f32 = 12.0;
const NO_FILES_LABEL: &str = "No files";

#[derive(Debug, Default)]
pub struct FileTreeState {
    /// Rows on show — per-frame projection written by the app; `None` until the
    /// worktree's root is read.
    pub rows: Option<Vec<TreeRow>>,
    /// Per-frame projection of the worktree's selected path.
    pub selected: Option<String>,
    /// `↑↓←→ Enter` — armed here by a row click, disarmed by the app when a
    /// terminal takes keyboard focus (files.md §5, git.md §3).
    pub nav_armed: bool,
}

/// The Files tab's body; returns what this frame's clicks and keys ask for.
pub fn file_tree(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut FileTreeState,
) -> Vec<GitIntent> {
    let mut intents = Vec::new();
    let Some(rows) = state.rows.as_deref() else {
        return intents;
    };
    if rows.is_empty() {
        let painter = RowPainter {
            palette,
            selected: None,
        };
        painter.muted(ui, 0, NO_FILES_LABEL);
        return intents;
    }
    let focused = hold_focus(ui);
    if let Some(moved) = focused.then(|| keyboard_move(ui, state)).flatten() {
        request_scroll(ui, &moved);
        intents.extend(move_intents(moved));
    }
    if let Some(row) = tree_rows(ui, palette, state) {
        ui.memory_mut(|memory| memory.request_focus(focus_id()));
        state.nav_armed = true;
        intents.extend(click_intents(row));
    }
    if !intents.is_empty() {
        ui.ctx().request_repaint();
    }
    intents
}

fn focus_id() -> egui::Id {
    egui::Id::new("file_tree_focus")
}

fn scroll_id() -> egui::Id {
    egui::Id::new("file_tree_scroll_to")
}

/// Keeps the tree's keyboard focus alive across frames — rows scroll out of
/// existence — and its arrows away from egui's focus moves. `true` while held.
fn hold_focus(ui: &mut egui::Ui) -> bool {
    let response = ui.interact(
        ui.available_rect_before_wrap(),
        focus_id(),
        egui::Sense::focusable_noninteractive(),
    );
    if !response.has_focus() {
        return false;
    }
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            focus_id(),
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
    true
}

fn keyboard_move(ui: &egui::Ui, state: &FileTreeState) -> Option<TreeMove> {
    if !state.nav_armed {
        return None;
    }
    let selected = state.selected.as_deref()?;
    let key = pressed_key(ui)?;
    key_move(state.rows.as_deref()?, selected, key)
}

fn pressed_key(ui: &egui::Ui) -> Option<TreeKey> {
    ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if modifiers.is_none() => match key {
                egui::Key::ArrowUp => Some(TreeKey::Up),
                egui::Key::ArrowDown => Some(TreeKey::Down),
                egui::Key::ArrowLeft => Some(TreeKey::Left),
                egui::Key::ArrowRight => Some(TreeKey::Right),
                egui::Key::Enter => Some(TreeKey::Enter),
                _ => None,
            },
            _ => None,
        })
    })
}

/// A move that lands on another row keeps it in view.
fn request_scroll(ui: &egui::Ui, moved: &TreeMove) {
    if let TreeMove::Select(path) | TreeMove::Open(path) = moved {
        file_list::request_row_scroll(ui, scroll_id(), path.clone());
    }
}

fn move_intents(moved: TreeMove) -> Vec<GitIntent> {
    match moved {
        TreeMove::Select(path) => vec![GitIntent::SelectEntry(path)],
        TreeMove::Open(path) => vec![
            GitIntent::SelectEntry(path.clone()),
            GitIntent::OpenFile(path),
        ],
        TreeMove::Fold(folder) => vec![GitIntent::FoldFolder(folder)],
        TreeMove::Unfold(folder) => vec![GitIntent::UnfoldFolder(folder)],
    }
}

/// A click selects the row, toggles a folder, opens a file.
fn click_intents(row: EntryRow) -> Vec<GitIntent> {
    let select = GitIntent::SelectEntry(row.path.clone());
    let then = match row.kind {
        RowKind::Folder { unfolded: true } => GitIntent::FoldFolder(row.path),
        RowKind::Folder { unfolded: false } => GitIntent::UnfoldFolder(row.path),
        RowKind::File | RowKind::Symlink => GitIntent::OpenFile(row.path),
    };
    vec![select, then]
}

/// The scrollable rows, only those in view painted; returns the row clicked.
fn tree_rows(ui: &mut egui::Ui, palette: &Palette, state: &FileTreeState) -> Option<EntryRow> {
    let rows = state.rows.as_deref().unwrap_or_default();
    let painter = RowPainter {
        palette,
        selected: state.selected.as_deref(),
    };
    let height = ui.available_height();
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .id_salt("file_tree")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            scroll_to_requested(ui, rows, range.start);
            let mut clicked = None;
            for row in &rows[range] {
                clicked = clicked.or(painter.row(ui, row));
            }
            clicked
        })
        .inner
}

/// `ui` holds the rows from `first` on: the requested row is placed by its index,
/// painted or not.
fn scroll_to_requested(ui: &egui::Ui, rows: &[TreeRow], first: usize) {
    let Some(target) = ui.data_mut(|data| data.remove_temp::<String>(scroll_id())) else {
        return;
    };
    let Some(index) = rows
        .iter()
        .position(|row| row.entry().is_some_and(|entry| entry.path == target))
    else {
        return;
    };
    let area = ui.max_rect();
    let top = area.top() + (index as f32 - first as f32) * ROW_HEIGHT;
    let rect = egui::Rect::from_x_y_ranges(area.x_range(), top..=top + ROW_HEIGHT);
    ui.scroll_to_rect(rect, None);
}

fn indent(depth: usize) -> f32 {
    depth as f32 * INDENT_STEP
}

#[derive(Clone, Copy)]
enum Highlight {
    Selected,
    Hovered,
}

struct RowPainter<'a> {
    palette: &'a Palette,
    selected: Option<&'a str>,
}

impl RowPainter<'_> {
    /// Returns the entry when its row is clicked.
    fn row(&self, ui: &mut egui::Ui, row: &TreeRow) -> Option<EntryRow> {
        match row {
            TreeRow::Entry(entry) => self.entry(ui, entry).then(|| entry.clone()),
            TreeRow::More { depth, hidden } => {
                self.muted(ui, *depth, &format!("{hidden} more not shown"));
                None
            }
        }
    }

    fn entry(&self, ui: &mut egui::Ui, entry: &EntryRow) -> bool {
        let (rect, response, hovered) =
            crate::ui::clickable(ui, egui::vec2(ui.available_width(), ROW_HEIGHT), true);
        let selected = self.selected == Some(entry.path.as_str());
        let highlight = if selected {
            Some(Highlight::Selected)
        } else {
            hovered.then_some(Highlight::Hovered)
        };
        if let Some(highlight) = highlight {
            self.paint_highlight(ui.painter(), rect, highlight);
        }
        self.paint_content(ui, entry, rect);
        let response = response.on_hover_text(&entry.path);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &entry.path)
        });
        response.clicked()
    }

    fn muted(&self, ui: &mut egui::Ui, depth: usize, text: &str) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::hover(),
        );
        let left = rect.left() + ROW_PAD_X + indent(depth) + GLYPH_SIZE + GLYPH_GAP;
        ui.painter().text(
            egui::pos2(left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(NAME_SIZE),
            self.palette.text_muted,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    }

    fn paint_highlight(&self, painter: &egui::Painter, rect: egui::Rect, highlight: Highlight) {
        match highlight {
            Highlight::Hovered => {
                painter.rect_filled(rect, 0.0, self.palette.bg_surface_hover);
            }
            Highlight::Selected => {
                painter.rect_filled(rect, 0.0, self.palette.accent_subtle);
                file_list::paint_selection_bar(painter, self.palette, rect);
            }
        }
    }

    /// Chevron (folders) · icon · name truncated at the end · a folder's change dot.
    fn paint_content(&self, ui: &egui::Ui, entry: &EntryRow, rect: egui::Rect) {
        let name_left = self.paint_glyphs(ui.painter(), rect, entry);
        let dot = entry
            .tint
            .filter(|_| matches!(entry.kind, RowKind::Folder { .. }));
        let name_right = rect.right() - ROW_PAD_X - dot.map_or(0.0, |_| DOT_RESERVE);
        let name_rect = egui::Rect::from_x_y_ranges(name_left..=name_right, rect.y_range());
        self.paint_name(ui, entry, name_rect);
        if let Some(tint) = dot {
            let center = egui::pos2(rect.right() - ROW_PAD_X - DOT_RADIUS, rect.center().y);
            ui.painter()
                .circle_filled(center, DOT_RADIUS, tint_color(self.palette, tint));
        }
    }

    /// Returns where the name starts.
    fn paint_glyphs(&self, painter: &egui::Painter, rect: egui::Rect, entry: &EntryRow) -> f32 {
        let y = rect.center().y;
        let chevron_x = rect.left() + ROW_PAD_X + indent(entry.depth);
        let icon_x = chevron_x + GLYPH_SIZE + GLYPH_GAP;
        let (icon, chevron) = match entry.kind {
            RowKind::Folder { unfolded: true } => (
                lucide_icons::Icon::Folder,
                Some(lucide_icons::Icon::ChevronDown),
            ),
            RowKind::Folder { unfolded: false } => (
                lucide_icons::Icon::Folder,
                Some(lucide_icons::Icon::ChevronRight),
            ),
            RowKind::File => (lucide_icons::Icon::File, None),
            RowKind::Symlink => (lucide_icons::Icon::FileSymlink, None),
        };
        if let Some(chevron) = chevron {
            let center = egui::pos2(chevron_x + GLYPH_SIZE / 2.0, y);
            crate::ui::paint_icon(
                painter,
                center,
                GLYPH_SIZE,
                chevron,
                self.palette.text_muted,
            );
        }
        let icon_color = if entry.ignored {
            self.palette.text_muted
        } else {
            self.palette.text_secondary
        };
        let center = egui::pos2(icon_x + GLYPH_SIZE / 2.0, y);
        crate::ui::paint_icon(painter, center, GLYPH_SIZE, icon, icon_color);
        icon_x + GLYPH_SIZE + NAME_GAP
    }

    fn paint_name(&self, ui: &egui::Ui, entry: &EntryRow, rect: egui::Rect) {
        let color = self.name_color(entry);
        let mut job = egui::text::LayoutJob::single_section(
            entry.name().to_owned(),
            egui::text::TextFormat::simple(egui::FontId::proportional(NAME_SIZE), color),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width().max(8.0));
        let galley = ui.painter().layout_job(job);
        let top = rect.center().y - galley.size().y / 2.0;
        ui.painter()
            .galley(egui::pos2(rect.left(), top), galley, color);
    }

    /// A changed file in its `git.*` color, an ignored entry muted (files.md §3).
    fn name_color(&self, entry: &EntryRow) -> egui::Color32 {
        let file_tint = entry
            .tint
            .filter(|_| !matches!(entry.kind, RowKind::Folder { .. }));
        match file_tint {
            Some(tint) => tint_color(self.palette, tint),
            None if entry.ignored => self.palette.text_muted,
            None => self.palette.text_primary,
        }
    }
}

fn tint_color(palette: &Palette, tint: Tint) -> egui::Color32 {
    match tint {
        Tint::Added => palette.git_added,
        Tint::Modified => palette.git_modified,
        Tint::Conflicted => palette.git_conflict,
    }
}
