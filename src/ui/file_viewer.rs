//! The read-only file viewer over the center zone (specs/files.md §4): the diff
//! view's card, highlighter and image preview over a worktree file.

use crate::files::content::{Content, FileSnapshot, Stamp};
use crate::git::status::ChangeKind;
use crate::theme::{Palette, PILL_SIZE, RADIUS_PILL, TITLE_SIZE};
use crate::ui::diff_view::{
    close_button, header_file_icon, header_icon_of, image_preview, overlay_card,
    paint_line_content, ImagePreview, CONTENT_TRAILING_PAD, HIGHLIGHT_BUDGET, LINE_HEIGHT,
    LINE_PAD_X, LINE_SIZE, NUM_PAD_X, NUM_SIZE,
};
use crate::ui::file_list::{paint_status_icon, status_color, status_icon, status_label};
use crate::ui::syntax_highlight::HighlightedFileCache;
use crate::ui::text_selection::{
    clicked_selection, copy_requested, dragged_selection, paint_text_selection,
    text_click_position, TextRow, TextSelection,
};
use crate::ui::with_alpha;

const MIN_NUMBER_DIGITS: usize = 3;
const CHIP_ALPHA: u8 = 30;

/// What the viewer derived from the file on screen, and its selection.
#[derive(Debug, Default)]
pub struct FileViewerState {
    /// Path and stamp of the content everything below was derived from.
    shown: Option<(String, Option<Stamp>)>,
    /// Keyed by the syntax theme; `None` inside for a file with no known syntax.
    highlight: Option<(&'static str, Option<HighlightedFileCache>)>,
    widest_line: usize,
    selection: Option<TextSelection>,
    image: Option<ImagePreview>,
}

impl FileViewerState {
    /// Re-derives what the content on screen feeds once another read of it lands.
    /// A new read of the same file keeps its selection where the lines still reach;
    /// another file starts with none.
    fn follow(&mut self, snapshot: &FileSnapshot) {
        let current = matches!(
            &self.shown,
            Some((path, stamp)) if *path == snapshot.path && *stamp == snapshot.stamp
        );
        if current {
            return;
        }
        let same_file = matches!(&self.shown, Some((path, _)) if *path == snapshot.path);
        let lines = text_lines(snapshot);
        self.highlight = None;
        self.widest_line = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        self.selection = self
            .selection
            .filter(|_| same_file)
            .and_then(|selection| selection.clamped_to(&lines));
        self.shown = Some((snapshot.path.clone(), snapshot.stamp));
    }

    fn selected_text(&self, snapshot: &FileSnapshot) -> Option<String> {
        self.selection?.text_of(&text_lines(snapshot))
    }

    fn show_text(&mut self, ui: &mut egui::Ui, file: &ViewedFile<'_>, lines: &[String]) {
        self.fill_highlight(ui, file, lines);
        let char_w = ui.ctx().fonts_mut(|fonts| {
            fonts
                .glyph_width(&egui::FontId::monospace(LINE_SIZE), ' ')
                .max(1.0)
        });
        let gutter = Gutter::for_lines(lines.len(), char_w);
        let content_width =
            gutter.content_left(0.0) + self.widest_line as f32 * char_w + CONTENT_TRAILING_PAD;
        let highlight = self
            .highlight
            .as_ref()
            .and_then(|(_, cache)| cache.as_ref());
        let mut text_rows = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut edit = egui::ScrollArea::both()
            .id_salt(("file_viewer", file.snapshot.path.as_str()))
            .auto_shrink([false, false])
            .show_rows(ui, LINE_HEIGHT, lines.len(), |ui, range| {
                let rows = Rows {
                    palette: file.palette,
                    gutter,
                    char_w,
                    width: content_width.max(ui.available_width()),
                    highlight,
                    selection: self.selection,
                };
                let mut edit = None;
                for index in range {
                    edit = rows.line(ui, index, &lines[index], &mut text_rows).or(edit);
                }
                edit
            })
            .inner;
        if let Some(selection) = dragged_selection(ui, &text_rows) {
            edit = Some(SelectionEdit::Select(selection));
        }
        match edit {
            Some(SelectionEdit::Select(selection)) if self.selection != Some(selection) => {
                self.selection = Some(selection);
                ui.ctx().request_repaint();
            }
            Some(SelectionEdit::Clear) => self.selection = None,
            _ => {}
        }
    }

    /// Opens the spans on the current theme, then fills one frame's budget of them.
    fn fill_highlight(&mut self, ui: &egui::Ui, file: &ViewedFile<'_>, lines: &[String]) {
        let theme = file.palette.syntax;
        if !matches!(&self.highlight, Some((shown, _)) if *shown == theme) {
            self.highlight = Some((theme, HighlightedFileCache::new(&file.snapshot.path, theme)));
        }
        let incomplete = self
            .highlight
            .as_mut()
            .and_then(|(_, cache)| cache.as_mut())
            .is_some_and(|cache| cache.extend(lines, HIGHLIGHT_BUDGET));
        if incomplete {
            ui.ctx().request_repaint();
        }
    }
}

/// The file as last read, and its change in the polled status (`None` if clean).
pub struct ViewedFile<'a> {
    pub palette: &'a Palette,
    pub snapshot: &'a FileSnapshot,
    pub change: Option<ChangeKind>,
}

/// Renders the viewer; `true` when closing is asked (Close button or `Esc`).
pub fn file_viewer(ui: &mut egui::Ui, file: &ViewedFile<'_>, state: &mut FileViewerState) -> bool {
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    state.follow(file.snapshot);
    if copy_requested(ui) {
        if let Some(text) = state.selected_text(file.snapshot) {
            ui.ctx().copy_text(text);
        }
    }
    let closed = overlay_card(file.palette)
        .show(ui, |ui| {
            let closed = header(ui, file);
            ui.add_space(8.0);
            body(ui, file, state);
            closed
        })
        .inner;
    escape || closed
}

/// Path, size and git change; `true` when Close was clicked.
fn header(ui: &mut egui::Ui, file: &ViewedFile<'_>) -> bool {
    let palette = file.palette;
    let snapshot = file.snapshot;
    ui.horizontal(|ui| {
        header_file_icon(
            ui,
            header_icon_of(palette, &snapshot.path),
            Some(palette.bg_surface),
        );
        ui.label(path_job(palette, &snapshot.path));
        if !matches!(snapshot.content, Content::Missing) {
            ui.label(
                egui::RichText::new(size_label(snapshot.size))
                    .size(PILL_SIZE)
                    .color(palette.text_muted),
            );
        }
        if let Some(kind) = file.change {
            change_chip(ui, palette, kind);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close_button(ui, palette)
        })
        .inner
    })
    .inner
}

/// The folder dimmed, the name standing out.
fn path_job(palette: &Palette, path: &str) -> egui::text::LayoutJob {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat::simple(egui::FontId::proportional(TITLE_SIZE), color);
    if !folder.is_empty() {
        job.append(&format!("{folder}/"), 0.0, format(palette.text_muted));
    }
    job.append(name, 0.0, format(palette.text_primary));
    job
}

fn change_chip(ui: &mut egui::Ui, palette: &Palette, kind: ChangeKind) {
    let color = status_color(palette, kind);
    egui::Frame::new()
        .fill(with_alpha(color, CHIP_ALPHA))
        .corner_radius(egui::CornerRadius::same(RADIUS_PILL))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (icon, _) =
                ui.allocate_exact_size(egui::vec2(PILL_SIZE, PILL_SIZE), egui::Sense::hover());
            paint_status_icon(ui.painter(), icon, status_icon(kind), color);
            ui.label(
                egui::RichText::new(status_label(kind))
                    .size(PILL_SIZE)
                    .color(color),
            );
        });
}

fn body(ui: &mut egui::Ui, file: &ViewedFile<'_>, state: &mut FileViewerState) {
    let palette = file.palette;
    let size = size_label(file.snapshot.size);
    match &file.snapshot.content {
        Content::Text(lines) if lines.is_empty() => placeholder(ui, palette, "Empty file"),
        Content::Text(lines) => state.show_text(ui, file, lines),
        Content::Image(blob) => image_preview(ui, palette, blob, &mut state.image),
        Content::Symlink(target) => placeholder(ui, palette, &format!("Symlink to {target}")),
        Content::Binary => placeholder(ui, palette, &format!("Binary file · {size}")),
        Content::TooLarge => {
            placeholder(ui, palette, &format!("File too large to display · {size}"))
        }
        Content::Unreadable => placeholder(ui, palette, "File can't be read"),
        Content::Missing => placeholder(ui, palette, "File no longer exists"),
    }
}

fn placeholder(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(LINE_SIZE)
            .color(palette.text_muted),
    );
}

enum SelectionEdit {
    Select(TextSelection),
    Clear,
}

/// The line-number column: as wide as the last number, three digits at least.
#[derive(Clone, Copy)]
struct Gutter {
    width: f32,
}

impl Gutter {
    fn for_lines(count: usize, char_w: f32) -> Self {
        let digits = count.to_string().len().max(MIN_NUMBER_DIGITS);
        Self {
            width: digits as f32 * char_w + NUM_PAD_X * 2.0,
        }
    }

    fn number_right(self, left: f32) -> f32 {
        left + self.width - NUM_PAD_X
    }

    fn content_left(self, left: f32) -> f32 {
        left + self.width + LINE_PAD_X
    }
}

struct Rows<'a> {
    palette: &'a Palette,
    gutter: Gutter,
    char_w: f32,
    width: f32,
    highlight: Option<&'a HighlightedFileCache>,
    selection: Option<TextSelection>,
}

impl Rows<'_> {
    /// Paints line `index`; returns the selection its click asks for.
    fn line(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        text: &str,
        text_rows: &mut Vec<TextRow>,
    ) -> Option<SelectionEdit> {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(self.width, LINE_HEIGHT),
            egui::Sense::click_and_drag(),
        );
        let content_left = self.gutter.content_left(rect.left());
        let text_len = text.chars().count();
        text_rows.push(TextRow {
            row: index,
            rect,
            content_left,
            char_w: self.char_w,
            text_len,
        });
        if let Some((from, to)) = self.selection.and_then(|s| s.range_for_row(index, text)) {
            paint_text_selection(ui, self.palette, rect, content_left, self.char_w, from, to);
        }
        let number = (index + 1).to_string();
        ui.painter().text(
            egui::pos2(self.gutter.number_right(rect.left()), rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &number,
            egui::FontId::monospace(NUM_SIZE),
            self.palette.text_muted,
        );
        let spans = self.highlight.and_then(|cache| cache.line(index));
        paint_line_content(
            ui,
            content_left,
            rect,
            text,
            spans,
            self.palette.text_secondary,
        );
        if response
            .hover_pos()
            .is_some_and(|pos| pos.x >= content_left)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        let label = format!("{number} {text}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));

        let at = text_click_position(&response, content_left, self.char_w, index, text_len);
        if response.triple_clicked() || response.double_clicked() {
            return at
                .and_then(|at| clicked_selection(&response, at))
                .map(SelectionEdit::Select);
        }
        response.clicked().then_some(SelectionEdit::Clear)
    }
}

fn text_lines(snapshot: &FileSnapshot) -> Vec<&str> {
    match &snapshot.content {
        Content::Text(lines) => lines.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

/// `840 B`, `1.4 KB`, `12 MB` — the phone's file sizes.
fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{} {}", value.round(), UNITS[unit])
    }
}
