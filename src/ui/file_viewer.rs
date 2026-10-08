//! The file viewer over the center zone (specs/files.md §4): the diff view's card,
//! highlighter and image preview over a worktree file, and its inline editor over the
//! whole file (§4.1).

use crate::files::content::{Content, FileSnapshot, Stamp};
use crate::git::edit::EditRequest;
use crate::git::status::ChangeKind;
use crate::theme::{Palette, PILL_SIZE, RADIUS_PILL, TITLE_SIZE};
use crate::ui::diff_view::{
    close_button, header_file_icon, header_icon_of, image_preview, overlay_card,
    paint_line_content, ImagePreview, CONTENT_TRAILING_PAD, HIGHLIGHT_BUDGET, LINE_HEIGHT,
    LINE_PAD_X, LINE_SIZE, NUM_PAD_X, NUM_SIZE,
};
use crate::ui::file_list::{paint_status_icon, status_color, status_icon, status_label};
use crate::ui::git_panel::{EditRefusal, GitIntent};
use crate::ui::inline_editor::{
    divergence_notice, editor_requested, editor_spans, inline_editor, save_requested, EditSession,
    EditTarget, EditorColumns, EditorLook, InlineEdit,
};
use crate::ui::syntax_highlight::HighlightedFileCache;
use crate::ui::text_selection::{
    clicked_selection, copy_requested, dragged_selection, paint_text_selection,
    text_click_position, TextPosition, TextRow, TextSelection,
};
use crate::ui::with_alpha;

const MIN_NUMBER_DIGITS: usize = 3;
const CHIP_ALPHA: u8 = 30;
/// Above this many lines the whole-file editor does not open (files.md §4.1): every
/// keystroke lays the whole buffer out again, ~6 ms a frame at this cap.
const MAX_EDIT_LINES: usize = 3_000;

/// What the viewer derived from the file on screen, its selection and its editor.
#[derive(Debug, Default)]
pub struct FileViewerState {
    /// Path and stamp of the content everything below was derived from.
    shown: Option<(String, Option<Stamp>)>,
    /// Keyed by the syntax theme; `None` inside for a file with no known syntax.
    highlight: Option<(&'static str, Option<HighlightedFileCache>)>,
    widest_line: usize,
    selection: Option<TextSelection>,
    image: Option<ImagePreview>,
    editing: EditSession<()>,
    /// What leaving the editor sent to disk, shown in place of the last read until a
    /// read of it lands: the edit never flashes back to the text it replaced.
    written: Option<Written>,
}

#[derive(Debug)]
struct Written {
    path: String,
    lines: Vec<String>,
}

impl FileViewerState {
    /// Whether the whole-file editor is open — it holds the text input, and the file's
    /// live re-read waits for it to close (files.md §4.1).
    pub fn is_editing(&self) -> bool {
        self.editing.edit().is_some()
    }

    /// The open whole-file editor, if any.
    pub fn editor(&self) -> Option<&InlineEdit<()>> {
        self.editing.edit()
    }

    /// The write the open editor still owes, read when the viewer is torn down without
    /// another frame to blur on (files.md §4.1).
    pub fn pending_write(&self) -> Option<EditRequest> {
        self.editing.pending_write()
    }

    /// The editor's buffer was written by whoever tears the viewer down: it closes.
    pub fn hand_off_editor(&mut self) {
        self.editing.drop_editor();
    }

    /// The write landed: an editor still open re-anchors on what it wrote.
    pub fn edit_written(&mut self, request: &EditRequest) {
        self.editing.written(request);
    }

    /// The worker refused the write: the file moved under it (git.md §4).
    pub fn edit_diverged(&mut self, request: EditRequest) {
        self.editing.diverged(request);
    }

    /// The write a divergence notice is currently offering to retry, if any.
    pub fn edit_divergence(&self) -> Option<&EditRequest> {
        self.editing.divergence()
    }

    /// The write did not land: the viewer goes back to what the disk holds.
    pub fn drop_written(&mut self) {
        if self.written.take().is_some() {
            self.shown = None;
        }
    }

    /// Opens the editor on `lines` of `path`, as a click would: the seam the app-side
    /// tests use to have one open without driving a render.
    #[cfg(test)]
    pub fn open_editor_for_test(&mut self, path: &str, lines: &[&str]) {
        let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        let caret = TextPosition { row: 0, col: 0 };
        self.editing.open(
            InlineEdit::new((), EditTarget::whole_file(path, &lines), caret),
            &mut Vec::new(),
        );
    }

    /// Types `text` into the open editor, as the field would.
    #[cfg(test)]
    pub fn type_for_test(&mut self, text: &str) {
        self.editing.type_for_test(text);
    }

    /// Re-derives what the content on screen feeds once another read of it lands.
    /// A new read of the same file keeps its selection where the lines still reach;
    /// another file starts with none. A read of what the editor just wrote keeps
    /// everything: it is already on screen.
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
        self.shown = Some((snapshot.path.clone(), snapshot.stamp));
        let landed = self
            .written
            .take()
            .is_some_and(|written| written.path == snapshot.path && written.lines == lines);
        if landed {
            return;
        }
        self.highlight = None;
        self.widest_line = widest_line(&lines);
        self.selection = self
            .selection
            .filter(|_| same_file)
            .and_then(|selection| selection.clamped_to(&lines));
    }

    fn lines<'a>(&'a self, snapshot: &'a FileSnapshot) -> Option<&'a [String]> {
        shown_lines(self.written.as_ref(), snapshot)
    }

    fn selected_text(&self, snapshot: &FileSnapshot) -> Option<String> {
        let lines: Vec<&str> = self.lines(snapshot)?.iter().map(String::as_str).collect();
        self.selection?.text_of(&lines)
    }

    fn open_editor(
        &mut self,
        target: EditTarget,
        caret: TextPosition,
        intents: &mut Vec<GitIntent>,
    ) {
        self.selection = None;
        self.editing
            .open(InlineEdit::new((), target, caret), intents);
    }

    /// Leaves the editor keeping its change: the buffer is written, and shown until the
    /// read of it lands, coloured as the editor coloured it.
    fn leave_editor(&mut self, syntax_theme: &'static str, intents: &mut Vec<GitIntent>) {
        if let Some(request) = self.editing.pending_write() {
            let spans = editor_spans(&request.path, syntax_theme, &request.replacement);
            let lines = buffer_lines(&request.replacement);
            self.highlight = Some((
                syntax_theme,
                spans.map(|spans| HighlightedFileCache::filled(syntax_theme, spans)),
            ));
            self.widest_line = widest_line(&lines);
            self.written = Some(Written {
                path: request.path,
                lines,
            });
        }
        self.editing.leave(intents);
    }

    /// The rows; returns the caret a click or `Cmd+E` asks the editor to open at.
    fn show_text(
        &mut self,
        ui: &mut egui::Ui,
        file: &ViewedFile<'_>,
        lines: &[String],
    ) -> Option<TextPosition> {
        self.fill_highlight(ui, file, lines);
        let char_w = mono_char_width(ui);
        let gutter = Gutter::for_lines(lines.len(), char_w);
        let content_width =
            gutter.content_left(0.0) + self.widest_line as f32 * char_w + CONTENT_TRAILING_PAD;
        let highlight = self
            .highlight
            .as_ref()
            .and_then(|(_, cache)| cache.as_ref());
        let editable = edit_refusal(Some(lines), file.snapshot.writable).is_none();
        let mut text_rows = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut action = egui::ScrollArea::both()
            .id_salt(scroll_salt(file))
            .auto_shrink([false, false])
            .show_rows(ui, LINE_HEIGHT, lines.len(), |ui, range| {
                let rows = Rows {
                    palette: file.palette,
                    gutter,
                    char_w,
                    width: content_width.max(ui.available_width()),
                    highlight,
                    selection: self.selection,
                    editable,
                };
                let mut action = None;
                for index in range {
                    action = rows
                        .line(ui, index, &lines[index], &mut text_rows)
                        .or(action);
                }
                action
            })
            .inner;
        if let Some(selection) = dragged_selection(ui, &text_rows) {
            action = Some(RowAction::Select(selection));
        }
        match action? {
            RowAction::Select(selection) if self.selection != Some(selection) => {
                self.selection = Some(selection);
                ui.ctx().request_repaint();
            }
            RowAction::Edit(caret) => return Some(caret),
            RowAction::Select(_) => {}
            RowAction::Clear => self.selection = None,
        }
        None
    }

    /// The editor in place of the rows, in the same scroll area: the view keeps its
    /// scroll as the editor opens and closes (files.md §4.1).
    fn show_editor(
        &mut self,
        ui: &mut egui::Ui,
        file: &ViewedFile<'_>,
        intents: &mut Vec<GitIntent>,
    ) {
        let char_w = mono_char_width(ui);
        let widest_line = self.widest_line;
        let Some(edit) = self.editing.edit_mut() else {
            return;
        };
        let gutter = Gutter::for_lines(edit.target.original.len(), char_w);
        let content_width =
            gutter.content_left(0.0) + widest_line as f32 * char_w + CONTENT_TRAILING_PAD;
        ui.spacing_mut().item_spacing.y = 0.0;
        let left = egui::ScrollArea::both()
            .id_salt(scroll_salt(file))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let look = EditorLook {
                    palette: file.palette,
                    columns: gutter.editor_columns(),
                    width: content_width.max(ui.available_width()),
                };
                inline_editor(ui, edit, &look)
            })
            .inner;
        if left {
            self.leave_editor(file.palette.syntax, intents);
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

/// Renders the viewer; `true` when closing is asked (Close button or `Esc`). The
/// editor's writes and refusals land in `intents`.
pub fn file_viewer(
    ui: &mut egui::Ui,
    file: &ViewedFile<'_>,
    state: &mut FileViewerState,
    intents: &mut Vec<GitIntent>,
) -> bool {
    state.follow(file.snapshot);
    // `Esc` cascade (files.md §4.1): the editor first, where it rolls the buffer back,
    // then the viewer.
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let close_on_escape = escape && !state.is_editing();
    if escape {
        state.editing.cancel();
    }
    if state.is_editing() && save_requested(ui) {
        state.leave_editor(file.palette.syntax, intents);
    }
    if let Some(reason) = edit_refusal(state.lines(file.snapshot), file.snapshot.writable) {
        if editor_requested(ui) {
            intents.push(GitIntent::EditRefused {
                path: file.snapshot.path.clone(),
                reason,
            });
        }
    }
    if copy_requested(ui) {
        if let Some(text) = state.selected_text(file.snapshot) {
            ui.ctx().copy_text(text);
        }
    }
    let closed = overlay_card(file.palette)
        .show(ui, |ui| {
            let closed = header(ui, file);
            ui.add_space(8.0);
            if let Some(request) = divergence_notice(ui, file.palette, &mut state.editing, intents)
            {
                state.drop_written();
                intents.push(GitIntent::OpenFile(request.path));
            }
            body(ui, file, state, intents);
            closed
        })
        .inner;
    close_on_escape || closed
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

fn body(
    ui: &mut egui::Ui,
    file: &ViewedFile<'_>,
    state: &mut FileViewerState,
    intents: &mut Vec<GitIntent>,
) {
    if state.is_editing() {
        state.show_editor(ui, file, intents);
        return;
    }
    // Lent out for the frame: the rows read the lines while the state takes their clicks.
    let written = state.written.take();
    match shown_lines(written.as_ref(), file.snapshot) {
        Some([]) => placeholder(ui, file.palette, "Empty file"),
        Some(lines) => {
            if let Some(caret) = state.show_text(ui, file, lines) {
                let target = EditTarget::whole_file(&file.snapshot.path, lines);
                state.open_editor(target, caret, intents);
            }
        }
        None => placeholder_body(ui, file, &mut state.image),
    }
    state.written = written;
}

/// Everything the viewer shows for a file it has no lines of.
fn placeholder_body(ui: &mut egui::Ui, file: &ViewedFile<'_>, image: &mut Option<ImagePreview>) {
    let palette = file.palette;
    let size = size_label(file.snapshot.size);
    match &file.snapshot.content {
        Content::Image(blob) => image_preview(ui, palette, blob, image),
        Content::Symlink(target) => placeholder(ui, palette, &format!("Symlink to {target}")),
        Content::Binary => placeholder(ui, palette, &format!("Binary file · {size}")),
        Content::TooLarge => {
            placeholder(ui, palette, &format!("File too large to display · {size}"))
        }
        Content::Unreadable => placeholder(ui, palette, "File can't be read"),
        Content::Missing => placeholder(ui, palette, "File no longer exists"),
        Content::Text(_) => {}
    }
}

fn placeholder(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(LINE_SIZE)
            .color(palette.text_muted),
    );
}

/// What a click or a key on a row asks for.
enum RowAction {
    Select(TextSelection),
    Clear,
    /// Open the editor with its caret there.
    Edit(TextPosition),
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

    /// The editor numbers its rows where the rows did; the viewer has no sign column.
    fn editor_columns(self) -> EditorColumns {
        EditorColumns {
            number_right: self.number_right(0.0),
            sign_left: None,
            content_left: self.content_left(0.0),
        }
    }
}

struct Rows<'a> {
    palette: &'a Palette,
    gutter: Gutter,
    char_w: f32,
    width: f32,
    highlight: Option<&'a HighlightedFileCache>,
    selection: Option<TextSelection>,
    editable: bool,
}

impl Rows<'_> {
    /// Paints line `index`; returns what its click or `Cmd+E` asks for.
    fn line(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        text: &str,
        text_rows: &mut Vec<TextRow>,
    ) -> Option<RowAction> {
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
                .map(RowAction::Select);
        }
        if response.clicked() {
            return Some(self.click_action(&response, index, text_len));
        }
        let keyed = self.editable && response.hovered() && editor_requested(ui);
        keyed.then_some(RowAction::Edit(TextPosition { row: index, col: 0 }))
    }

    /// A plain click in the text opens the editor there (files.md §4.1); anywhere else
    /// on the row, or on a file that cannot be edited, it clears the selection.
    fn click_action(&self, response: &egui::Response, index: usize, text_len: usize) -> RowAction {
        let content_left = self.gutter.content_left(response.rect.left());
        let Some(pos) = response
            .interact_pointer_pos()
            .filter(|pos| self.editable && pos.x >= content_left)
        else {
            return RowAction::Clear;
        };
        let col = ((pos.x - content_left) / self.char_w).floor() as usize;
        RowAction::Edit(TextPosition {
            row: index,
            col: col.min(text_len),
        })
    }
}

fn text_lines(snapshot: &FileSnapshot) -> Vec<&str> {
    match &snapshot.content {
        Content::Text(lines) => lines.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

/// The lines on screen: those the editor just wrote while the read of them is on its
/// way, else the last read's; `None` for a file shown as a placeholder.
fn shown_lines<'a>(
    written: Option<&'a Written>,
    snapshot: &'a FileSnapshot,
) -> Option<&'a [String]> {
    if let Some(written) = written.filter(|written| written.path == snapshot.path) {
        return Some(&written.lines);
    }
    match &snapshot.content {
        Content::Text(lines) => Some(lines),
        _ => None,
    }
}

/// Why the editor cannot open on the lines shown, if it cannot (files.md §4.1).
fn edit_refusal(lines: Option<&[String]>, writable: bool) -> Option<EditRefusal> {
    match lines {
        None | Some([]) => Some(EditRefusal::File),
        Some(_) if !writable => Some(EditRefusal::ReadOnly),
        Some(lines) if lines.len() > MAX_EDIT_LINES => Some(EditRefusal::FileTooLong),
        Some(_) => None,
    }
}

/// The buffer's lines as the write lays them on disk: an emptied buffer is no line.
fn buffer_lines(buffer: &str) -> Vec<String> {
    if buffer.is_empty() {
        return Vec::new();
    }
    buffer.split('\n').map(str::to_owned).collect()
}

fn widest_line<S: AsRef<str>>(lines: &[S]) -> usize {
    lines
        .iter()
        .map(|line| line.as_ref().chars().count())
        .max()
        .unwrap_or(0)
}

fn mono_char_width(ui: &egui::Ui) -> f32 {
    ui.ctx().fonts_mut(|fonts| {
        fonts
            .glyph_width(&egui::FontId::monospace(LINE_SIZE), ' ')
            .max(1.0)
    })
}

/// One scroll position per file, shared by the rows and the editor.
fn scroll_salt<'a>(file: &'a ViewedFile<'_>) -> (&'static str, &'a str) {
    ("file_viewer", file.snapshot.path.as_str())
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
