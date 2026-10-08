//! The inline editor the diff view and the file viewer share (git.md §4, files.md §4.1):
//! its session state, the frameless syntax-coloured buffer that takes the place of the
//! rows, and the notice that arbitrates a refused write.

use std::cell::RefCell;
use std::ops::Range;

use crate::git::edit::EditRequest;
use crate::theme::{Palette, PILL_SIZE, RADIUS_PILL};
use crate::ui::diff_view::{LINE_HEIGHT, LINE_SIZE, NUM_SIZE};
use crate::ui::git_panel::GitIntent;
use crate::ui::syntax_highlight::{HighlightedSpan, IncrementalHighlighter};
use crate::ui::text_selection::TextPosition;
use crate::ui::with_alpha;

/// Width of the accent bar marking the lines being edited (design-system §4).
const EDIT_BAR_W: f32 = 3.0;

/// The working-tree lines an editor opens on, and where its write lands: the range and
/// the lines read when the caret appeared are the write's precondition (git.md §4).
/// They are captured then and never re-derived from a reload: that is what makes the
/// write refuse rather than land on renumbered lines (git.md §7).
#[derive(Debug, Clone, PartialEq)]
pub struct EditTarget {
    /// File the lines were read from, captured with them: the surface on screen can
    /// move on to another file, and the buffer must reach the one it came from.
    pub path: String,
    /// 0-based, end-exclusive.
    pub range: Range<usize>,
    pub original: Vec<String>,
    /// The edit came from **Staged**: its write is followed by a file-level stage.
    pub stage_after: bool,
    /// The range is the whole file: nothing may have been added past it either.
    pub whole_file: bool,
}

impl EditTarget {
    /// Every line of an unstaged working-tree file (files.md §4.1).
    pub fn whole_file(path: &str, lines: &[String]) -> Self {
        Self {
            path: path.to_owned(),
            range: 0..lines.len(),
            original: lines.to_vec(),
            stage_after: false,
            whole_file: true,
        }
    }

    /// The write of `replacement` over these lines.
    pub fn request(&self, replacement: &str) -> EditRequest {
        EditRequest {
            path: self.path.clone(),
            range: self.range.clone(),
            original: self.original.clone(),
            replacement: replacement.to_owned(),
            stage_after: self.stage_after,
            whole_file: self.whole_file,
            force: false,
        }
    }
}

/// The inline editor's live state: what it stands in for on screen (`anchor` — the
/// diff's hunk index, nothing for a whole file), the lines it writes back and the buffer
/// being typed.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineEdit<A> {
    pub anchor: A,
    pub target: EditTarget,
    pub buffer: String,
    /// Buffer as of the last write handed to the worker: what the target is expected to
    /// hold on disk. Nothing to write while it equals `buffer`.
    flushed: String,
    /// Caret to place when the editor takes focus, in buffer coordinates.
    caret: Option<TextPosition>,
    /// One-shot: claim keyboard focus on the next frame, so the click that opened the
    /// editor is the only gesture needed.
    focus: bool,
}

impl<A> InlineEdit<A> {
    /// An editor seeded with the target's lines, caret at `caret` once it takes focus.
    pub(crate) fn new(anchor: A, target: EditTarget, caret: TextPosition) -> Self {
        let buffer = target.original.join("\n");
        Self {
            anchor,
            target,
            flushed: buffer.clone(),
            buffer,
            caret: Some(caret),
            focus: true,
        }
    }

    /// `true` once the buffer differs from the lines it was seeded with.
    pub fn is_dirty(&self) -> bool {
        self.buffer != self.target.original.join("\n")
    }

    /// The write this buffer asks for, against the lines it was opened on.
    pub fn request(&self) -> EditRequest {
        self.target.request(&self.buffer)
    }
}

/// One surface's editing: the open editor, one at a time, and the write the worker
/// refused because the file had moved under it (`EditError::Diverged`) — kept so
/// **Overwrite** can re-send it. The typed buffer is never dropped on our own initiative
/// (git.md §4).
#[derive(Debug)]
pub struct EditSession<A> {
    open: Option<InlineEdit<A>>,
    diverged: Option<EditRequest>,
}

impl<A> Default for EditSession<A> {
    fn default() -> Self {
        Self {
            open: None,
            diverged: None,
        }
    }
}

impl<A> EditSession<A> {
    /// The open editor, if any.
    pub fn edit(&self) -> Option<&InlineEdit<A>> {
        self.open.as_ref()
    }

    pub fn edit_mut(&mut self) -> Option<&mut InlineEdit<A>> {
        self.open.as_mut()
    }

    /// Opens `edit` in place of the editor already open, whose buffer is written on the
    /// way: moving the caret elsewhere is an exit like any other (git.md §4).
    pub fn open(&mut self, edit: InlineEdit<A>, intents: &mut Vec<GitIntent>) {
        self.leave(intents);
        self.open = Some(edit);
    }

    /// Every way out that **keeps** the change — `Cmd+S`, a click elsewhere, a surface
    /// that stops being writable — goes through here, so the write on the way out has a
    /// single home (git.md §4); `Esc` is the one exit that rolls back instead
    /// (`cancel`). Nothing is emitted when the buffer is already on disk.
    pub fn leave(&mut self, intents: &mut Vec<GitIntent>) {
        let request = self.pending_write();
        self.open = None;
        intents.extend(request.map(GitIntent::FlushEdit));
    }

    /// `Esc` (git.md §4): the buffer is dropped and nothing reaches the working tree.
    /// Nothing landed while the editor was open, so dropping it *is* the rollback; a
    /// divergence notice offering to re-send that buffer goes with it.
    pub fn cancel(&mut self) {
        if self.open.take().is_some() {
            self.diverged = None;
        }
    }

    /// Closes the editor without writing, a pending divergence notice left standing: the
    /// lines it was anchored on are gone from the surface.
    pub fn drop_editor(&mut self) {
        self.open = None;
    }

    /// The write the open editor still owes: its buffer, when it differs from what has
    /// already reached the worker. Read when the surface that holds it is torn down
    /// without another frame to blur on — a repo switch is not a discard
    /// (keybindings.md §4).
    pub fn pending_write(&self) -> Option<EditRequest> {
        let edit = self.open.as_ref()?;
        (edit.buffer != edit.flushed).then(|| edit.request())
    }

    /// The write landed: the range now names the lines just written, so the next write
    /// compares against what is really on disk (it grows or shrinks with the buffer).
    /// Ignored when the editor has moved on — the reply is stale.
    pub fn written(&mut self, request: &EditRequest) {
        let Some(edit) = self.open.as_mut() else {
            return;
        };
        let target = &mut edit.target;
        if target.path != request.path || target.range != request.range {
            return;
        }
        let lines: Vec<String> = request.replacement.split('\n').map(str::to_owned).collect();
        target.range = request.range.start..request.range.start + lines.len();
        target.original = lines;
    }

    /// The worker refused the write: the file moved under the editor (git.md §4). The
    /// buffer stays as typed and the notice hands the arbitration to the user.
    pub fn diverged(&mut self, request: EditRequest) {
        self.diverged = Some(request);
    }

    /// The write a divergence notice is currently offering to retry, if any.
    pub fn divergence(&self) -> Option<&EditRequest> {
        self.diverged.as_ref()
    }

    /// Types `text` into the open editor, as the field would: the app-side tests need a
    /// buffer that differs from what is on disk.
    #[cfg(test)]
    pub fn type_for_test(&mut self, text: &str) {
        if let Some(edit) = self.open.as_mut() {
            edit.buffer = text.to_owned();
        }
    }
}

/// Where the editor's gutter and text sit, as x offsets from the left of its block —
/// the rows' own columns, so entering the editor shifts nothing.
#[derive(Debug, Clone, Copy)]
pub struct EditorColumns {
    /// Right edge of the line numbers.
    pub number_right: f32,
    /// Left edge of the muted `~` sign, on a surface whose rows carry a sign column.
    pub sign_left: Option<f32>,
    pub content_left: f32,
}

/// How the editor is drawn on its surface.
pub struct EditorLook<'a> {
    pub palette: &'a Palette,
    pub columns: EditorColumns,
    /// Width of the block: the surface's widest row, not the viewport.
    pub width: f32,
}

/// The open editor, in place of the rows it edits (git.md §4, design-system §4): same
/// mono font, same line height, same content x offset, same syntax colours — the only
/// perceptible change is the caret. No frame, no toolbar, no button: an accent bar marks
/// the block, the gutter is renumbered off the laid-out galley, and one muted hint closes
/// it. Returns `true` when the editor has been left: egui surrenders the buffer's focus
/// as soon as the pointer presses anything else, and that *is* the exit gesture.
pub fn inline_editor<A>(
    ui: &mut egui::Ui,
    edit: &mut InlineEdit<A>,
    look: &EditorLook<'_>,
) -> bool {
    let palette = look.palette;
    let id = ui.id().with("inline_edit");
    let rows = edit.buffer.split('\n').count();
    let (block, _) = ui.allocate_exact_size(
        egui::vec2(look.width, rows as f32 * LINE_HEIGHT),
        egui::Sense::hover(),
    );
    let syntax = palette.syntax;
    let path = edit.target.path.clone();
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
        editor_galley(ui, &path, syntax, egui::TextBuffer::as_str(buf))
    };
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(block.left() + look.columns.content_left, block.top()),
        block.max,
    );
    let output = ui
        .scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
            egui::TextEdit::multiline(&mut edit.buffer)
                .id(id)
                // No frame at all (design-system §4): the surface's own background
                // shows through, so the rows keep the colours they had before the caret.
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .desired_rows(rows)
                .text_color(palette.text_secondary)
                .layouter(&mut layouter)
                .show(ui)
        })
        .inner;

    if let Some(caret) = edit.caret.take() {
        let index = caret_char_index(&edit.buffer, caret);
        let mut opened = output.state.clone();
        opened
            .cursor
            .set_char_range(Some(egui::text_selection::CCursorRange::one(
                egui::text::CCursor::new(index),
            )));
        // The widget id is the same for every editor, so egui's undo history outlives
        // it: without this reset, one `Cmd+Z` in a freshly opened editor restores the
        // *previous* one's buffer — which the save would then write to this range.
        opened.clear_undoer();
        opened.store(ui.ctx(), id);
    }
    let left = if std::mem::take(&mut edit.focus) {
        ui.memory_mut(|memory| memory.request_focus(id));
        false
    } else {
        !ui.memory(|memory| memory.has_focus(id))
    };

    // Off the laid-out galley, not the block: the field takes this frame's keystrokes
    // *after* the block was allocated from the buffer as it read before them, so the row
    // a newline just added would otherwise be left outside the bar for a frame.
    let painted = block.height().max(output.galley.size().y);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(block.min, egui::vec2(EDIT_BAR_W, painted)),
        egui::CornerRadius::ZERO,
        palette.accent,
    );
    paint_editor_gutter(ui, look, &output, block.left(), edit.target.range.start);
    ui.label(
        egui::RichText::new("Saved when you leave the editor · Esc discards")
            .size(NUM_SIZE)
            .color(palette.text_muted),
    );
    left
}

/// Line numbers of the editor's rows, read off the laid-out galley so they follow the
/// text the user is typing (a row added in the buffer numbers itself), plus the dimmed
/// sign column where the surface has one: the signs belong to the diff the editor
/// replaced, so they are shown muted — informational, no longer a `+`/`−` to act on.
fn paint_editor_gutter(
    ui: &egui::Ui,
    look: &EditorLook<'_>,
    output: &egui::text_edit::TextEditOutput,
    left: f32,
    first_line: usize,
) {
    let palette = look.palette;
    let clip = ui.clip_rect();
    let num_font = egui::FontId::monospace(NUM_SIZE);
    let sign_color = with_alpha(palette.text_muted, 90);
    let painter = ui.painter();
    // A wrapped row would number twice; the editor never wraps (no wrap width).
    for (line, row) in (first_line..).zip(output.galley.rows.iter()) {
        let center_y = output.galley_pos.y + (row.min_y() + row.max_y()) / 2.0;
        if center_y < clip.top() || center_y > clip.bottom() {
            continue;
        }
        painter.text(
            egui::pos2(left + look.columns.number_right, center_y),
            egui::Align2::RIGHT_CENTER,
            (line + 1).to_string(),
            num_font.clone(),
            palette.text_muted,
        );
        if let Some(sign_left) = look.columns.sign_left {
            painter.text(
                egui::pos2(left + sign_left, center_y),
                egui::Align2::LEFT_CENTER,
                "~",
                egui::FontId::monospace(LINE_SIZE),
                sign_color,
            );
        }
    }
}

/// Char offset of a buffer position, for the caret the opening gesture asks for.
fn caret_char_index(buffer: &str, at: TextPosition) -> usize {
    let mut index = 0;
    for (row, text) in buffer.split('\n').enumerate() {
        let len = text.chars().count();
        if row == at.row {
            return index + at.col.min(len);
        }
        index += len + 1;
    }
    index.saturating_sub(1)
}

// Incremental highlighter of the editor's buffer: syntect is not incremental and the
// layouter runs every frame, so the spans are kept across frames and only the lines a
// keystroke touched are re-parsed (same reasoning — and same `!Send` parse state — as
// the conflict editor's Output).
thread_local! {
    static EDITOR_HL: RefCell<IncrementalHighlighter> =
        RefCell::new(IncrementalHighlighter::default());
}

/// Per-line spans of `text` as the editor coloured it — what a surface showing the
/// buffer's lines once the editor is left can paint without highlighting them again.
/// `None` when the path has no known syntax.
pub fn editor_spans(
    path: &str,
    syntax_theme: &'static str,
    text: &str,
) -> Option<Vec<Vec<HighlightedSpan>>> {
    EDITOR_HL.with_borrow_mut(|hl| hl.highlight(path, syntax_theme, text).map(<[_]>::to_vec))
}

/// The editor's galley at the rows' exact metrics: mono `LINE_SIZE` glyphs centred in
/// `LINE_HEIGHT` rows, no wrapping — a buffer row and a surface row occupy the same
/// band, so entering the editor shifts nothing.
fn editor_galley(
    ui: &egui::Ui,
    path: &str,
    syntax_theme: &'static str,
    text: &str,
) -> std::sync::Arc<egui::Galley> {
    let font = egui::FontId::monospace(LINE_SIZE);
    let mut job = egui::text::LayoutJob::default();
    EDITOR_HL.with_borrow_mut(|hl| {
        let format = |color| egui::text::TextFormat {
            font_id: font.clone(),
            color,
            line_height: Some(LINE_HEIGHT),
            valign: egui::Align::Center,
            ..Default::default()
        };
        match hl.highlight(path, syntax_theme, text) {
            Some(lines) => {
                for (i, spans) in lines.iter().enumerate() {
                    if i > 0 {
                        job.append("\n", 0.0, format(egui::Color32::PLACEHOLDER));
                    }
                    for span in spans {
                        job.append(&span.text, 0.0, format(span.color));
                    }
                }
            }
            None => job.append(text, 0.0, format(egui::Color32::PLACEHOLDER)),
        }
    });
    ui.painter().layout_job(job)
}

/// The refused-write notice (git.md §4): the file moved under the editor, so the write
/// did not happen and the typed buffer is still there. The arbitration is the user's —
/// **Overwrite** re-sends the very same request, precondition dropped; **Reload** closes
/// the editor and hands the request back, for the surface to read the file again.
pub fn divergence_notice<A>(
    ui: &mut egui::Ui,
    palette: &Palette,
    session: &mut EditSession<A>,
    intents: &mut Vec<GitIntent>,
) -> Option<EditRequest> {
    let request = session.diverged.clone()?;
    let mut answered = false;
    let mut reload = None;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("File changed on disk — the save was refused")
                .size(LINE_SIZE)
                .color(palette.git_modified),
        );
        if notice_button(ui, palette, "Reload") {
            session.open = None;
            reload = Some(request.clone());
            answered = true;
        }
        if notice_button(ui, palette, "Overwrite") {
            intents.push(GitIntent::FlushEdit(EditRequest {
                force: true,
                ..request.clone()
            }));
            answered = true;
        }
    });
    if answered {
        session.diverged = None;
    }
    ui.add_space(8.0);
    reload
}

fn notice_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> bool {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(PILL_SIZE)
                .color(palette.text_secondary),
        )
        .fill(palette.bg_surface)
        .corner_radius(egui::CornerRadius::same(RADIUS_PILL)),
    )
    .clicked()
}

/// `Cmd+S` while the editor is open (keybindings.md §3): the keyboard's way of stepping
/// out of the buffer.
pub fn save_requested(ui: &egui::Ui) -> bool {
    ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session holding an open editor on lines 1..3 of `f`, buffer as seeded.
    fn session_editing() -> EditSession<usize> {
        let target = EditTarget {
            path: "f".to_owned(),
            range: 1..3,
            original: vec!["one".to_owned(), "two".to_owned()],
            stage_after: false,
            whole_file: false,
        };
        let mut session = EditSession::default();
        session.open(
            InlineEdit::new(0, target, TextPosition { row: 0, col: 0 }),
            &mut Vec::new(),
        );
        session
    }

    #[test]
    fn a_whole_file_editor_writes_every_line_back_unstaged() {
        let lines = vec!["one".to_owned(), "two".to_owned(), String::new()];
        let mut session = EditSession::default();
        let target = EditTarget::whole_file("src/a.rs", &lines);
        session.open(
            InlineEdit::new((), target, TextPosition { row: 1, col: 0 }),
            &mut Vec::new(),
        );
        session.type_for_test("one\nTWO\n");

        let request = session.pending_write().expect("a typed buffer is owed");

        assert_eq!(request.path, "src/a.rs");
        assert_eq!(request.range, 0..3, "the range spans the whole file");
        assert_eq!(
            request.original, lines,
            "the whole file is the precondition"
        );
        assert_eq!(request.replacement, "one\nTWO\n");
        assert!(
            request.whole_file,
            "nothing may be added past the range either"
        );
        assert!(!request.stage_after && !request.force);
    }

    #[test]
    fn leaving_writes_the_buffer_once() {
        let mut session = session_editing();
        session.type_for_test("one\nTWO");
        let mut intents = Vec::new();

        session.leave(&mut intents);

        let GitIntent::FlushEdit(request) = intents.pop().expect("the exit writes") else {
            panic!("got {intents:?}");
        };
        assert!(intents.is_empty(), "got {intents:?}");
        assert_eq!(request.range, 1..3);
        assert_eq!(request.replacement, "one\nTWO");
        assert!(!request.force);
    }

    #[test]
    fn esc_rolls_the_buffer_back_instead_of_writing_it() {
        let mut session = session_editing();
        session.type_for_test("one\nTWO");
        let refused = session.pending_write().expect("a typed buffer is owed");
        session.diverged(refused);

        session.cancel();

        assert!(session.edit().is_none(), "the editor is gone");
        assert!(
            session.divergence().is_none(),
            "the notice offered to re-send the very buffer just dropped"
        );
    }

    #[test]
    fn the_landed_write_re_anchors_the_editor_on_what_it_wrote() {
        let mut session = session_editing();
        session.type_for_test("one\nTWO\nextra");
        let request = session.pending_write().expect("a typed buffer is owed");

        session.written(&request);

        let edit = &session.edit().unwrap().target;
        assert_eq!(
            edit.range,
            1..4,
            "a line added by the buffer widens the range it owns"
        );
        assert_eq!(edit.original, vec!["one", "TWO", "extra"]);
        assert!(
            !session.edit().unwrap().is_dirty(),
            "the anchor now agrees with the buffer on disk"
        );
    }

    #[test]
    fn a_reply_for_another_anchor_leaves_the_editor_alone() {
        let mut session = session_editing();
        let stale = EditRequest {
            path: "f".to_owned(),
            range: 7..9,
            original: vec!["x".to_owned()],
            replacement: "y".to_owned(),
            stage_after: false,
            whole_file: false,
            force: false,
        };

        session.written(&stale);

        let edit = &session.edit().unwrap().target;
        assert_eq!(edit.range, 1..3);
        assert_eq!(edit.original, vec!["one", "two"]);
    }
}
