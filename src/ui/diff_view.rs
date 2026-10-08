use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::files::file_type::file_type;
use crate::git::diff::{DiffFingerprint, DiffLine, FileDiff, Hunk, ImageBlob, LineOrigin};
use crate::git::edit::EditRequest;
use crate::git::intraline::{Columns, IntralineChanges};
use crate::review::{FileComments, ForgeThreads, ReviewIntent, ReviewPool};
use crate::theme::{Palette, PILL_SIZE, RADIUS_BUTTON, RADIUS_CARD, RADIUS_PILL, TITLE_SIZE};
use crate::ui::git_panel::{intent_pill, EditRefusal, GitIntent};
use crate::ui::inline_editor::{
    divergence_notice, editor_requested, inline_editor, save_requested, EditSession, EditTarget,
    EditorColumns, EditorLook, InlineEdit,
};
use crate::ui::review_notes::{
    note_at, note_block, review_recap, NoteAnchor, NoteBatch, NoteCtx, NoteLine, NoteSession,
};
use crate::ui::syntax_highlight::{display_text, HighlightedDiffCache, HighlightedSpan};
use crate::ui::text_selection::{
    clicked_selection, copy_requested, dragged_selection, paint_text_selection,
    text_click_position, TextPosition, TextRow, TextSelection,
};
use crate::ui::{with_alpha, FileIcon};

/// Per-hunk line selection, kept across frames. The key is the hunk index in
/// `FileDiff::hunks`; the value maps each chosen line index in `Hunk::lines` to
/// the fingerprint of the content it was chosen on, so a reload that keeps the
/// shape but changes the line drops it instead of retargeting (git.md §8).
/// Empty ⇒ the whole hunk (the Stage/Unstage hunk buttons take precedence).
#[derive(Debug, Default)]
pub struct DiffViewState {
    selection: HashMap<usize, HashMap<usize, u64>>,
    /// Extended context per hunk (git.md §4): number of extra context lines
    /// requested above **and** below (multiples of `EXTEND_STEP`). Display only —
    /// never enters staging.
    extensions: HashMap<usize, u32>,
    text_selection: Option<TextSelection>,
    syntax_cache: Option<HighlightedDiffCache>,
    /// Columns each changed line differs from its counterpart on (git.md §4):
    /// the rows paint them, they never compute them.
    intraline_cache: Option<IntralineChanges>,
    /// Widest displayed line, kept across frames: measuring it walks every
    /// displayed line, and only the diff content or an extension moves it.
    width_cache: Option<WidthCache>,
    /// Raised when a diff reload (disk change, git.md §8) dropped a selection that
    /// became invalid; the overlay shows it as a banner until the next interaction.
    stale: bool,
    /// Decoded preview of an image file, kept across frames and re-decoded only when
    /// the underlying blob changes (`ImageBlob::fingerprint`). git.md §4.
    image: Option<ImagePreview>,
    /// The review note editors (M-RC): under a line, or in the recap popover.
    /// Cleared on validate, `Esc`, or when the open file changes.
    notes: NoteSession,
    /// One-shot: focus the reply or conversation composer on its next frame (set when
    /// one opens so the caret lands in the field without an extra click).
    composer_focus: bool,
    /// One-shot new-side line to scroll into view on the next render (set when an
    /// inline comment is opened from the center, pull-requests.md §5). Consumed by
    /// the row whose `new_lineno` matches, so it survives the async diff load.
    reveal_line: Option<u32>,
    /// Thread root id whose reply editor is open, with `reply_buffer` holding the
    /// in-progress reply (pull-requests.md §11). Shared by the diff overlay and the
    /// center inline-comment card, so opening a reply in one surface shows it in both.
    active_reply: Option<u64>,
    reply_buffer: String,
    /// The reply composer open under a top-level conversation card, if any
    /// (pull-requests.md §11), with `conversation_buffer` holding its draft. The
    /// standalone add composer at the foot of the band is always open and keeps its
    /// own `conversation_add_buffer`, so a reply draft and a new-comment draft don't
    /// clobber each other.
    conversation_edit: Option<ConversationEdit>,
    conversation_buffer: String,
    /// The standalone "Add a comment" composer's draft (always-visible bar at the
    /// foot of the conversation band, pull-requests.md §11).
    conversation_add_buffer: String,
    /// Resolved thread roots (by comment id) the user has expanded in the center
    /// accordion (pull-requests.md §11). Resolved threads collapse to a summary row by
    /// default; expanding one adds its root id here.
    expanded_resolved: HashSet<u64>,
    /// The inline editor (git.md §4), anchored on a hunk index. None open ⇒ the diff
    /// renders its rows as usual.
    editing: EditSession<usize>,
}

/// Character width of the widest displayed line, with what it was measured on:
/// the diff's shape and the extension amounts (extended context can bring in a
/// longer line).
#[derive(Debug)]
struct WidthCache {
    shape: u64,
    extensions: HashMap<usize, u32>,
    chars: usize,
}

/// The open reply composer under a top-level conversation card at the given
/// conversation index (pull-requests.md §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversationEdit {
    Reply(usize),
}

impl DiffViewState {
    pub fn clear(&mut self) {
        self.selection.clear();
        self.extensions.clear();
        self.text_selection = None;
        self.syntax_cache = None;
        self.intraline_cache = None;
        self.width_cache = None;
        self.stale = false;
        self.image = None;
        self.notes = NoteSession::default();
        self.composer_focus = false;
        self.reveal_line = None;
        self.active_reply = None;
        self.reply_buffer.clear();
        self.conversation_edit = None;
        self.conversation_buffer.clear();
        self.conversation_add_buffer.clear();
        self.expanded_resolved.clear();
        self.editing = EditSession::default();
    }

    /// Whether some editor of this file is open — the diff owns `Esc` while one is,
    /// so a column of bands knows not to consume it itself (pull-requests.md §11).
    pub fn has_open_editor(&self) -> bool {
        self.editing.edit().is_some() || self.notes.is_open() || self.active_reply.is_some()
    }

    /// The open inline editor, if any (git.md §4) — the app reads it to write the
    /// buffer back to the working tree.
    pub fn inline_edit(&self) -> Option<&InlineEdit<usize>> {
        self.editing.edit()
    }

    /// Whether a review note editor is open (inline on a line, or in the recap
    /// popover): it holds the text input, so the app disarms the sidebar keys that
    /// would otherwise land in the buffer — `Cmd+Enter` above all, which sends the
    /// review batch here (keybindings.md §4).
    pub fn note_editing(&self) -> bool {
        self.notes.is_open()
    }

    /// The write the open editor still owes (git.md §4), read when the diff that holds
    /// it is torn down without another frame to blur on.
    pub fn pending_write(&self) -> Option<EditRequest> {
        self.editing.pending_write()
    }

    /// The write a divergence notice is currently offering to retry, if any.
    pub fn edit_divergence(&self) -> Option<&EditRequest> {
        self.editing.divergence()
    }

    /// Types `text` into the open editor, as the field would: the app-side tests need a
    /// buffer that differs from what is on disk.
    #[cfg(test)]
    pub fn type_for_test(&mut self, text: &str) {
        self.editing.type_for_test(text);
    }

    /// Opens an editor on `hunk` anchored on `lines` at `range`, as a click would: the
    /// seam the app-side tests use to have one open without driving a render.
    #[cfg(test)]
    pub fn open_editor_for_test(
        &mut self,
        path: &str,
        hunk: usize,
        range: Range<usize>,
        lines: &[&str],
    ) {
        let target = EditTarget {
            path: path.to_owned(),
            range,
            original: lines.iter().map(|l| (*l).to_owned()).collect(),
            stage_after: false,
            whole_file: false,
        };
        let caret = TextPosition { row: 0, col: 0 };
        self.editing
            .open(InlineEdit::new(hunk, target, caret), &mut Vec::new());
    }

    /// The write landed: the editor's anchor now names the lines just written (git.md §4).
    pub fn edit_written(&mut self, request: &EditRequest) {
        self.editing.written(request);
    }

    /// The worker refused the write: the file moved under the editor (git.md §4) —
    /// **Reload** takes the disk's version, **Overwrite** re-sends this request.
    pub fn edit_diverged(&mut self, request: EditRequest) {
        self.editing.diverged(request);
    }

    /// Whether the resolved thread rooted at `id` is expanded in the center accordion
    /// (pull-requests.md §11) — resolved threads collapse to a summary row by default.
    pub fn is_resolved_expanded(&self, id: u64) -> bool {
        self.expanded_resolved.contains(&id)
    }

    /// Toggles the collapsed/expanded state of the resolved thread rooted at `id`.
    pub fn toggle_resolved(&mut self, id: u64) {
        if !self.expanded_resolved.remove(&id) {
            self.expanded_resolved.insert(id);
        }
    }

    /// Requests that the diff scroll the given new-side line into view on its next
    /// render (one-shot, consumed when the matching row is drawn).
    pub fn reveal_line(&mut self, new_lineno: u32) {
        self.reveal_line = Some(new_lineno);
    }

    /// Thread root id whose reply editor is open, or `None` when no reply is being
    /// drafted — read by the center inline-comment card to mirror the overlay editor.
    pub fn reply_target(&self) -> Option<u64> {
        self.active_reply
    }

    /// Opens the reply editor for thread `comment_id`, clearing any prior draft and
    /// arming the one-shot focus so the caret lands in the field.
    pub fn open_reply(&mut self, comment_id: u64) {
        self.active_reply = Some(comment_id);
        self.reply_buffer.clear();
        self.composer_focus = true;
    }

    /// Closes the reply editor and discards its draft.
    pub fn cancel_reply(&mut self) {
        self.active_reply = None;
        self.reply_buffer.clear();
    }

    /// The in-progress reply text, for the center card's editor to bind to.
    pub fn reply_buffer_mut(&mut self) -> &mut String {
        &mut self.reply_buffer
    }

    /// The reply buffer paired with its one-shot focus flag — the center card's
    /// editor needs both lent at once (`reply_editor`'s `buffer` + `focus`).
    pub fn reply_fields(&mut self) -> (&mut String, &mut bool) {
        (&mut self.reply_buffer, &mut self.composer_focus)
    }

    /// Which conversation composer is open (the standalone add field or a reply under
    /// a top-level card), or `None` when none is being drafted (pull-requests.md §11).
    pub(crate) fn conversation_edit(&self) -> Option<ConversationEdit> {
        self.conversation_edit
    }

    /// The standalone add-comment composer's draft (the always-visible bar at the
    /// foot of the conversation band), for binding its field and reading it on send.
    pub fn conversation_add_buffer_mut(&mut self) -> &mut String {
        &mut self.conversation_add_buffer
    }

    /// Opens a reply under the top-level card at conversation `index`.
    pub fn open_conversation_reply(&mut self, index: usize) {
        self.conversation_edit = Some(ConversationEdit::Reply(index));
        self.conversation_buffer.clear();
        self.composer_focus = true;
    }

    /// Closes the conversation composer and discards its draft.
    pub fn cancel_conversation(&mut self) {
        self.conversation_edit = None;
        self.conversation_buffer.clear();
    }

    /// The conversation draft, for reading the body on send.
    pub fn conversation_buffer_mut(&mut self) -> &mut String {
        &mut self.conversation_buffer
    }

    /// The conversation draft paired with its one-shot focus flag, for the editor.
    pub fn conversation_fields(&mut self) -> (&mut String, &mut bool) {
        (&mut self.conversation_buffer, &mut self.composer_focus)
    }

    /// Reconciles the selection with a freshly reloaded diff: drops the (hunk,
    /// line) pairs now out of bounds, turned back into context, or whose content
    /// no longer matches what was picked. Returns `true` if a selection was lost
    /// (to signal to the user, git.md §8).
    pub fn reconcile(&mut self, diff: &FileDiff) -> bool {
        let mut dropped = false;
        self.selection.retain(|&hunk, lines| {
            let Some(h) = diff.hunks.get(hunk) else {
                dropped = true;
                return false;
            };
            let before = lines.len();
            lines.retain(|&line, &mut picked| {
                h.lines.get(line).is_some_and(|l| {
                    l.origin != LineOrigin::Context && line_fingerprint(l) == picked
                })
            });
            dropped |= lines.len() != before;
            !lines.is_empty()
        });
        self.stale |= dropped;
        // A reload can rewrite a line without moving the hunk shape the width cache
        // is keyed on, so re-measure on every reload.
        self.width_cache = None;
        // Display only: an orphaned extension is dropped without flagging stale.
        self.extensions.retain(|&hunk, _| hunk < diff.hunks.len());
        self.reconcile_text_selection(diff);
        self.reconcile_inline_edit(diff);
        dropped
    }

    /// An open editor is addressed by hunk **index** and anchored on lines read from the
    /// previous snapshot: a reload that renumbers the hunks or rewrites those lines
    /// would leave the caret over a hunk the user never pointed at. Dropped rather than
    /// re-anchored — the same guard the selection above and the armed hunk confirmation
    /// (`git_session::on_diff`) apply (git.md §8).
    fn reconcile_inline_edit(&mut self, diff: &FileDiff) {
        let Some(edit) = self.editing.edit() else {
            return;
        };
        let target = &edit.target;
        let anchored = diff.path == target.path
            && diff.source_lines.get(target.range.clone()) == Some(target.original.as_slice())
            && diff.hunks.get(edit.anchor).is_some_and(|hunk| {
                // The hunk still sits inside the window the editor took: another hunk at
                // that index covers other lines, extended context only widens the window.
                hunk.new_start.checked_sub(1).is_some_and(|start| {
                    let start = start as usize;
                    start >= target.range.start
                        && start + hunk.new_lines as usize <= target.range.end
                })
            });
        if !anchored {
            self.editing.drop_editor();
            self.stale = true;
        }
    }

    fn extend(&mut self, hunk: usize) {
        *self.extensions.entry(hunk).or_insert(0) += EXTEND_STEP;
    }

    fn is_stale(&self) -> bool {
        self.stale
    }

    fn toggle(&mut self, diff: &FileDiff, hunk: usize, line: usize) {
        let Some(picked) = diff
            .hunks
            .get(hunk)
            .and_then(|h| h.lines.get(line))
            .map(line_fingerprint)
        else {
            return;
        };
        self.stale = false;
        self.text_selection = None;
        let set = self.selection.entry(hunk).or_default();
        if set.insert(line, picked).is_some() {
            set.remove(&line);
        }
    }

    fn selected(&self, hunk: usize, line: usize) -> bool {
        self.selection
            .get(&hunk)
            .is_some_and(|s| s.contains_key(&line))
    }

    fn selected_lines(&self, hunk: usize) -> Vec<usize> {
        let mut lines: Vec<usize> = self
            .selection
            .get(&hunk)
            .map(|s| s.keys().copied().collect())
            .unwrap_or_default();
        lines.sort_unstable();
        lines
    }

    fn selected_text(&self, diff: &FileDiff) -> Option<String> {
        let selection = self.text_selection?;
        selected_text(diff, &self.extensions, selection)
    }

    fn text_range_for_row(&self, row: usize, text: &str) -> Option<(usize, usize)> {
        self.text_selection?.range_for_row(row, text)
    }

    fn reconcile_text_selection(&mut self, diff: &FileDiff) {
        let lines = display_rows(diff, &self.extensions);
        let Some(selection) = self.text_selection else {
            return;
        };
        let Some(selection) = selection.clamped_to(&lines) else {
            self.text_selection = None;
            return;
        };
        self.text_selection = Some(selection);
    }

    /// Opens the cache on the current diff, then fills one frame's worth of it —
    /// the rest lands on the following frames, repaint after repaint, so opening
    /// a big file never costs a visible hitch.
    fn ensure_syntax_cache(
        &mut self,
        ui: &egui::Ui,
        diff: &FileDiff,
        fingerprint: DiffFingerprint,
        syntax_theme: &'static str,
    ) {
        if !self
            .syntax_cache
            .as_ref()
            .is_some_and(|cache| cache.is_current(fingerprint, syntax_theme))
        {
            self.syntax_cache = HighlightedDiffCache::new(diff, fingerprint, syntax_theme);
        }
        let incomplete = self
            .syntax_cache
            .as_mut()
            .is_some_and(|cache| cache.extend(diff, HIGHLIGHT_BUDGET));
        if incomplete {
            ui.ctx().request_repaint();
        }
    }

    fn syntax_line(&self, hunk: usize, line: usize) -> Option<&[HighlightedSpan]> {
        self.syntax_cache.as_ref()?.line(hunk, line)
    }

    /// Same deal as the syntax cache above: the pairing of a big diff is filled one
    /// frame's budget at a time, so opening it never costs a visible hitch.
    fn ensure_intraline_cache(
        &mut self,
        ui: &egui::Ui,
        diff: &FileDiff,
        fingerprint: DiffFingerprint,
    ) {
        if !self
            .intraline_cache
            .as_ref()
            .is_some_and(|changes| changes.is_current(fingerprint))
        {
            self.intraline_cache = Some(IntralineChanges::new(diff, fingerprint));
        }
        let incomplete = self
            .intraline_cache
            .as_mut()
            .is_some_and(|changes| changes.extend(diff, INTRALINE_BUDGET));
        if incomplete {
            ui.ctx().request_repaint();
        }
    }

    fn intraline_line(&self, hunk: usize, line: usize) -> &[Columns] {
        self.intraline_cache
            .as_ref()
            .map_or(&[], |changes| changes.line(hunk, line))
    }

    /// Width of the widest displayed line, in characters — the rows are allocated
    /// at it so egui exposes a horizontal scrollbar. Measuring rebuilds every
    /// display row, so it is served from the cache while the diff and the
    /// extensions are unchanged.
    fn content_chars(&mut self, diff: &FileDiff) -> usize {
        let shape = shape_fingerprint(diff);
        if let Some(cache) = &self.width_cache {
            if cache.shape == shape && cache.extensions == self.extensions {
                return cache.chars;
            }
        }
        let chars = display_rows(diff, &self.extensions)
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        self.width_cache = Some(WidthCache {
            shape,
            extensions: self.extensions.clone(),
            chars,
        });
        chars
    }
}

/// Time the syntax cache may fill per frame. ~300 lines at syntect's throughput:
/// a viewport's worth on the frame the file opens, and short enough to leave the
/// 16 ms frame intact.
pub(crate) const HIGHLIGHT_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);
/// Time the intra-line pairing may fill per frame, alongside the syntax budget
/// above: ~1 000 pairs, several viewports' worth on the frame the file opens.
const INTRALINE_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);
pub(crate) const LINE_SIZE: f32 = 12.0;
pub(crate) const LINE_PAD_X: f32 = 8.0;
/// Breathing room kept after the longest line so it isn't flush against the
/// right edge once scrolled fully right.
pub(crate) const CONTENT_TRAILING_PAD: f32 = 24.0;
pub(crate) const LINE_HEIGHT: f32 = 17.0;
const LINE_ACTION_SIZE: f32 = 14.0;
const LINE_ACTION_LEFT: f32 = 4.0;
/// Gap between the stage and review-note icons sharing the gutter.
const LINE_ACTION_GAP: f32 = 4.0;
/// Column reserved for the per-line stage/unstage button and, beside it, the
/// review-note (✦) button — left of the numbers.
const LINE_ACTION_W: f32 = 40.0;
/// One gutter button's share of that column, for a surface with no stage button: the
/// file viewer's note button (files.md §4.2).
pub(crate) const GUTTER_SLOT_W: f32 = LINE_ACTION_LEFT + LINE_ACTION_SIZE + LINE_ACTION_GAP;
/// Size of the gutter line numbers (more subdued than the content).
pub(crate) const NUM_SIZE: f32 = 11.0;
/// Inner padding of each number column.
pub(crate) const NUM_PAD_X: f32 = 6.0;
/// Column of the +/− sign between the gutter and the content.
const SIGN_W: f32 = 16.0;
/// Context lines added above **and** below per Extend click (git.md §4).
const EXTEND_STEP: u32 = 5;
const FILE_ICON_BOX: f32 = 24.0;
const FILE_ICON_SIZE: f32 = 14.0;
const HUNK_BAND_PAD_X: i8 = 8;
const HUNK_BAND_PAD_Y: i8 = 4;
/// Air either side of the rule that stands in for a bandless hunk header.
const HUNK_RULE_GAP: f32 = 7.0;
/// Band chrome (pull-requests.md §11): the strip's padding and the air around the
/// rows under it. Kept tight — a file header is a seam, not a section.
const BAND_PAD_X: i8 = 12;
const BAND_HEADER_PAD_Y: i8 = 6;
const BAND_BODY_PAD_Y: i8 = 8;
/// Tint of the changed columns inside a rewritten line, over the row's own (alpha
/// 30): enough of a step for the eye to land on the change first.
const WORD_CHANGE_ALPHA: u8 = 85;
/// Above this many working-tree lines a hunk does not open an inline editor
/// (git.md §4): the buffer is re-highlighted as it is typed, and a whole-file-sized
/// hunk belongs in the external editor.
const MAX_EDIT_LINES: usize = 2_000;
/// Lines of the commented hunk previewed atop an overlay thread (pull-requests.md §5).
const OVERLAY_SNIPPET_LINES: usize = 3;
/// Indent a reply nests under its thread root, wide enough to seat the rail drawn down
/// its middle (§11) — the Conversation tab's own reply indent.
const REPLY_NEST_INDENT: f32 = 26.0;

/// Horizontal geometry of the rows: two number columns (old | new) sized to the
/// largest number in the file, then the sign, then the content.
#[derive(Debug, Copy, Clone)]
struct RowLayout {
    num_w: f32,
}

impl RowLayout {
    fn for_diff(diff: &FileDiff, char_w: f32) -> Self {
        let max_lineno = diff
            .hunks
            .iter()
            .map(|h| (h.old_start + h.old_lines).max(h.new_start + h.new_lines))
            .max()
            .unwrap_or(1)
            .max(diff.source_lines.len() as u32)
            .max(1);
        let digits = max_lineno.to_string().len().max(3);
        Self {
            num_w: digits as f32 * char_w + NUM_PAD_X * 2.0,
        }
    }

    fn old_right(self, left: f32) -> f32 {
        left + LINE_ACTION_W + self.num_w - NUM_PAD_X
    }

    fn new_right(self, left: f32) -> f32 {
        left + LINE_ACTION_W + 2.0 * self.num_w - NUM_PAD_X
    }

    fn sign_left(self, left: f32) -> f32 {
        left + LINE_ACTION_W + 2.0 * self.num_w + 4.0
    }

    fn content_left(self, left: f32) -> f32 {
        left + LINE_ACTION_W + 2.0 * self.num_w + SIGN_W + LINE_PAD_X
    }

    /// The inline editor numbers its rows in the new column, keeps a muted sign.
    fn editor_columns(self) -> EditorColumns {
        EditorColumns {
            number_right: self.new_right(0.0),
            sign_left: Some(self.sign_left(0.0)),
            content_left: self.content_left(0.0),
        }
    }
}

/// X offset of a line's content from the left edge of its row — exposed so UI
/// tests can aim at a precise text column.
pub fn content_x_offset(diff: &FileDiff, char_w: f32) -> f32 {
    RowLayout::for_diff(diff, char_w).content_left(0.0)
}

/// X offset of the middle of the line-number strip, where a click picks the line for
/// partial staging (git.md §4) — exposed so UI tests can aim at that zone rather than
/// the content column, which carries the caret.
pub fn numbers_x_offset(diff: &FileDiff, char_w: f32) -> f32 {
    (LINE_ACTION_W + RowLayout::for_diff(diff, char_w).content_left(0.0)) / 2.0
}

/// A hunk's extended context: **new-side** line number ranges (1-based,
/// half-open) shown above and below its lines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ContextExtension {
    above: std::ops::Range<u32>,
    below: std::ops::Range<u32>,
}

/// Extended context ranges per hunk, computed top to bottom: each range is
/// clamped to the file bounds and to lines already shown (neighboring hunks, the
/// previous hunk's lower extension) — never a duplicate on screen.
fn context_extensions(diff: &FileDiff, amounts: &HashMap<usize, u32>) -> Vec<ContextExtension> {
    let file_len = diff.source_lines.len() as u32;
    let mut out = Vec::with_capacity(diff.hunks.len());
    // Last line (new side) already shown by the previous hunks.
    let mut shown_end = 0u32;
    for (idx, hunk) in diff.hunks.iter().enumerate() {
        if file_len == 0 || hunk.new_lines == 0 {
            out.push(ContextExtension::default());
            continue;
        }
        let amount = amounts.get(&idx).copied().unwrap_or(0);
        let hunk_end = hunk.new_start + hunk.new_lines - 1;
        let above_start = hunk
            .new_start
            .saturating_sub(amount)
            .max(shown_end + 1)
            .min(hunk.new_start);
        let next_start = diff
            .hunks
            .get(idx + 1)
            .filter(|next| next.new_lines > 0)
            .map(|next| next.new_start)
            .unwrap_or(u32::MAX);
        let below_end = hunk_end
            .saturating_add(amount)
            .min(file_len)
            .min(next_start.saturating_sub(1))
            .max(hunk_end);
        out.push(ContextExtension {
            above: above_start..hunk.new_start,
            below: hunk_end + 1..below_end + 1,
        });
        shown_end = below_end;
    }
    out
}

/// `true` if an Extend click on this hunk would show at least one more line.
/// `current` is the extension set already computed for `amounts` — asked once per
/// hunk while rendering, so it is passed in rather than recomputed per call.
fn can_extend(
    diff: &FileDiff,
    amounts: &HashMap<usize, u32>,
    current: &[ContextExtension],
    hunk: usize,
) -> bool {
    let mut more = amounts.clone();
    *more.entry(hunk).or_insert(0) += EXTEND_STEP;
    context_extensions(diff, &more) != current
}

/// Old-side number of an extended context line **above** the hunk: outside the
/// hunk both sides match, the offset is constant.
fn above_old_lineno(hunk: &Hunk, new_lineno: u32) -> u32 {
    (new_lineno as i64 + hunk.old_start as i64 - hunk.new_start as i64).max(0) as u32
}

/// Old-side number of an extended context line **below** the hunk.
fn below_old_lineno(hunk: &Hunk, new_lineno: u32) -> u32 {
    (new_lineno as i64 + (hunk.old_start + hunk.old_lines) as i64
        - (hunk.new_start + hunk.new_lines) as i64)
        .max(0) as u32
}

fn source_line_range<'a>(
    diff: &'a FileDiff,
    range: &std::ops::Range<u32>,
) -> impl Iterator<Item = &'a str> {
    range
        .clone()
        .filter_map(|n| diff.source_lines.get(n as usize - 1).map(String::as_str))
}

/// Text lines in exact render order (extensions included): common basis for text
/// selection and copy.
fn display_rows<'a>(diff: &'a FileDiff, amounts: &HashMap<usize, u32>) -> Vec<&'a str> {
    let extensions = context_extensions(diff, amounts);
    let mut rows = Vec::new();
    for (hunk, ext) in diff.hunks.iter().zip(&extensions) {
        rows.extend(source_line_range(diff, &ext.above));
        rows.extend(hunk.lines.iter().map(|line| display_text(&line.content)));
        rows.extend(source_line_range(diff, &ext.below));
    }
    rows
}

/// `(+additions, −deletions)` totals of the loaded hunks — header stats.
fn diff_line_stats(diff: &FileDiff) -> (usize, usize) {
    diff.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .fold((0, 0), |(adds, dels), line| match line.origin {
            LineOrigin::Addition => (adds + 1, dels),
            LineOrigin::Deletion => (adds, dels + 1),
            LineOrigin::Context => (adds, dels),
        })
}

fn selected_text(
    diff: &FileDiff,
    amounts: &HashMap<usize, u32>,
    selection: TextSelection,
) -> Option<String> {
    selection.text_of(&display_rows(diff, amounts))
}

/// In-diff review context (M-RC): the active repo's stored comments, the agent
/// CLI label for the Send button, and the sink for the actions the diff view
/// raises. When `None` the diff view is a plain viewer (no review chrome).
pub struct DiffReview<'a> {
    /// The agent pool: notes batched for the local agent (the Sparkles gutter
    /// button + the recap "Send to …"). The only pool on the working-tree / commit
    /// surfaces.
    pub comments: &'a FileComments,
    /// The forge pool: PR review comments posted to GitHub / Bitbucket on submit
    /// (the MessageSquarePlus gutter button). `Some` only on the PR review surface
    /// (pull-requests.md §11); kept apart from `comments` so a forge review is
    /// never forced through the agent.
    pub forge: Option<&'a FileComments>,
    /// Read-only comments already posted on the PR, anchored per line. Empty for
    /// the working-tree / commit diffs; populated only on the PR review surface
    /// (pull-requests.md §11). Rendered below the line, never editable.
    pub existing: &'a ForgeThreads,
    pub agent: &'a str,
    pub intents: &'a mut Vec<ReviewIntent>,
}

/// Which surface the diff is shown on — selects the available affordances.
/// Bundling the old `staged` / `read_only` flags into one value keeps a caller
/// from assembling an impossible combination (e.g. staged history).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSurface {
    /// Working tree: per-hunk / per-line staging; `staged` picks the direction
    /// (the index unstages, the worktree stages).
    WorkingTree { staged: bool },
    /// Working tree, showing the **previously** open file frozen while the
    /// requested one loads: the hunks on screen belong to another path, so the
    /// granular controls stay out until the requested diff arrives. It carries the
    /// frozen surface's own `staged` so the scroll salt does not change under the
    /// file that is still on screen — the freeze would otherwise send it back to
    /// the top for the whole round-trip.
    WorkingTreeFrozen { staged: bool },
    /// A historical commit (M9-7 / git.md §9): read-only, no staging.
    Commit,
    /// The PR review surface (pull-requests.md §11): read-only, with the same
    /// per-line review annotation as the other surfaces.
    PrReview,
}

impl DiffSurface {
    /// History, PR review and a frozen inherited diff are read-only: no staging
    /// controls, no line selection — only review annotation stays available on
    /// every line.
    fn read_only(self) -> bool {
        !matches!(self, DiffSurface::WorkingTree { .. })
    }

    /// Identity the scroll offset is keyed on, alongside the path. Freezing a
    /// working-tree diff does not change *which* file is on screen, so both
    /// working-tree surfaces answer the same key — keying on the variant itself
    /// would drop the offset to 0 the moment the freeze starts.
    fn scroll_key(self) -> (u8, bool) {
        match self {
            DiffSurface::WorkingTree { staged } | DiffSurface::WorkingTreeFrozen { staged } => {
                (0, staged)
            }
            DiffSurface::Commit => (1, false),
            DiffSurface::PrReview => (2, false),
        }
    }

    fn staged(self) -> bool {
        matches!(self, DiffSurface::WorkingTree { staged: true })
    }

    /// Section an inline edit made on this surface lands in (git.md §4). Unlike
    /// `staged`, the frozen twin answers too: the freeze *is* the file switch that
    /// flushes the buffer, and that buffer still belongs to the section it was typed in.
    fn edit_section_staged(self) -> bool {
        matches!(
            self,
            DiffSurface::WorkingTree { staged: true }
                | DiffSurface::WorkingTreeFrozen { staged: true }
        )
    }

    /// The PR surface carries a forge pool alongside the agent pool: its gutter
    /// gets a second `MessageSquarePlus` button (slot 0) feeding the forge review
    /// comments posted to GitHub / Bitbucket on submit.
    fn forge_review(self) -> bool {
        matches!(self, DiffSurface::PrReview)
    }
}

/// How the diff draws its own chrome. **Card**: the standalone overlay — own
/// frame, Close, and a vertical scroll of its own. **Band**: one file of a
/// caller's continuous column (pull-requests.md §11) — a collapse chevron in
/// place of Close, and **horizontal** scrolling only, so the column owns the
/// vertical axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffChrome {
    Card,
    Band { collapsed: bool },
}

impl DiffChrome {
    fn band(self) -> bool {
        matches!(self, DiffChrome::Band { .. })
    }
}

/// What a rendered diff asks its caller for.
#[derive(Debug, Default)]
pub struct DiffOutcome {
    /// Close requested (Close button or `Esc`) — card chrome only.
    pub close: bool,
    /// The band's chevron was clicked: collapse or expand this file.
    pub toggled: bool,
    /// Screen rect of the row a `reveal_line` asked for, in a band: the band's own
    /// scroll area only owns the horizontal axis, and egui lets a scroll target
    /// reach **no** enclosing area (`scroll_area.rs`: both targets are taken
    /// whatever the enabled axes), so the column scrolls to it instead.
    pub reveal: Option<egui::Rect>,
}

/// Overlay diff view (central zone, design-system §4 card). Renders the file's
/// `FileDiff`, a Stage/Unstage button per hunk, and a line selection for partial
/// staging. Returns `true` if the user requested closing (Close button or `Esc`)
/// — the caller then returns to the terminal. The `surface` decides which
/// affordances are live (staging, the per-line agent button); review annotation
/// stays available on every surface, read-only lines included.
pub fn diff_view(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff: &FileDiff,
    surface: DiffSurface,
    state: &mut DiffViewState,
    intents: &mut Vec<GitIntent>,
    review: Option<&mut DiffReview<'_>>,
) -> bool {
    diff_render(
        ui,
        palette,
        diff,
        surface,
        DiffChrome::Card,
        state,
        intents,
        review,
    )
    .close
}

/// One file of a continuous column (pull-requests.md §11): the same renderer
/// under band chrome.
pub fn diff_view_band(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff: &FileDiff,
    state: &mut DiffViewState,
    collapsed: bool,
    review: Option<&mut DiffReview<'_>>,
) -> DiffOutcome {
    let mut throwaway = Vec::new();
    diff_render(
        ui,
        palette,
        diff,
        DiffSurface::PrReview,
        DiffChrome::Band { collapsed },
        state,
        &mut throwaway,
        review,
    )
}

#[allow(clippy::too_many_arguments)]
fn diff_render(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff: &FileDiff,
    surface: DiffSurface,
    chrome: DiffChrome,
    state: &mut DiffViewState,
    intents: &mut Vec<GitIntent>,
    review: Option<&mut DiffReview<'_>>,
) -> DiffOutcome {
    let staged = surface.staged();
    let read_only = surface.read_only();
    let edit_staged = surface.edit_section_staged();
    // Only the working tree can be written back, and only when the worker judged
    // the file writable while computing the diff (git.md §4).
    let editable = diff.editable && !read_only;
    // A surface that stopped being writable keeps no editor: switching files freezes
    // the diff (read-only) while the previous file's rows are still on screen — and
    // that switch is precisely a flush point.
    if !editable {
        state.editing.leave(intents);
    }
    let empty = FileComments::new();
    let empty_threads = ForgeThreads::new();
    let review_available = review.is_some();
    let review_comments: &FileComments = match &review {
        Some(r) => r.comments,
        None => &empty,
    };
    let review_forge_store: &FileComments = match &review {
        Some(r) => r.forge.unwrap_or(&empty),
        None => &empty,
    };
    let review_existing: &ForgeThreads = match &review {
        Some(r) => r.existing,
        None => &empty_threads,
    };
    let review_agent = review.as_ref().map(|r| r.agent).unwrap_or_default();
    let review_forge = surface.forge_review();
    let mut review_out: Vec<ReviewIntent> = Vec::new();

    // `Esc` cascade (keybindings.md §3): the inline editor first — where it **rolls the
    // buffer back** (git.md §4) — then an open note editor (inline or popover), then the
    // diff itself. A band never escalates to closing: the column, not one of its files,
    // owns what `Esc` does once no editor is open.
    let mut out = DiffOutcome::default();
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if state.editing.edit().is_some() {
            state.editing.cancel();
        } else if state.notes.is_open() {
            state.notes.cancel();
        } else if !chrome.band() {
            out.close = true;
        }
    }
    // `Cmd+S` is the keyboard's click-elsewhere (keybindings.md §3): it writes the buffer
    // and leaves, where `Esc` above leaves without it.
    if state.editing.edit().is_some() && save_requested(ui) {
        state.editing.leave(intents);
    }
    // `Cmd+E` where no caret can open (git.md §4): the click stays silent, but the
    // keyboard ask deserves an answer — the app names the reason and offers the
    // external editor. One answer per key press: a column of bands would raise the
    // same refusal once per file on screen.
    if !editable && !chrome.band() && editor_requested(ui) {
        intents.push(GitIntent::EditRefused {
            path: diff.path.clone(),
            reason: EditRefusal::File,
        });
    }
    if copy_requested(ui) {
        if let Some(text) = state.selected_text(diff) {
            ui.ctx().copy_text(text);
        }
    }

    // A band is **flat**: no card, no outline, no corners. What tells two files apart
    // is a full-bleed header strip — a filled bar with a hairline over and under it,
    // running edge to edge. A rounded outlined card around a wall of code reads as a
    // heavy object; a bar is just a seam.
    let frame = match chrome {
        DiffChrome::Card => overlay_card(palette),
        DiffChrome::Band { .. } => egui::Frame::NONE,
    };
    let header_frame = match chrome {
        DiffChrome::Card => egui::Frame::NONE,
        // `bg_surface` is a hair off `bg_canvas` — on a page of stacked bands that is
        // not a header, it is a slightly different nothing. The strip takes the
        // hover step up, the quietest fill that actually reads as a header.
        DiffChrome::Band { .. } => egui::Frame::new()
            .fill(palette.bg_surface_hover)
            .inner_margin(egui::Margin::symmetric(BAND_PAD_X, BAND_HEADER_PAD_Y)),
    };

    frame.show(ui, |ui| {
        let mut collapsed = false;
        let header = header_frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                // The card gives its icon a tile; a band's strip is already a fill, so
                // a rounded tile on it is one shape too many.
                let tile = (!chrome.band()).then_some(palette.bg_surface);
                header_file_icon(ui, header_icon_of(palette, &diff.path), tile);
                ui.label(
                    egui::RichText::new(&diff.path)
                        .size(TITLE_SIZE)
                        .color(palette.text_primary),
                );
                let (additions, deletions) = diff_line_stats(diff);
                if additions + deletions > 0 {
                    ui.label(
                        egui::RichText::new(format!("+{additions}"))
                            .size(PILL_SIZE)
                            .monospace()
                            .color(palette.git_added),
                    );
                    ui.label(
                        egui::RichText::new(format!("−{deletions}"))
                            .size(PILL_SIZE)
                            .monospace()
                            .color(palette.git_deleted),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match chrome {
                        DiffChrome::Card => {
                            if close_button(ui, palette) {
                                out.close = true;
                            }
                        }
                        DiffChrome::Band { collapsed: folded } => {
                            collapsed = folded;
                            if collapse_chevron(ui, palette, folded) {
                                out.toggled = true;
                            }
                        }
                    }
                    if review_available {
                        let mut notes = NoteCtx {
                            palette,
                            session: &mut state.notes,
                            out: &mut review_out,
                        };
                        let batch = NoteBatch {
                            comments: review_comments,
                            agent: review_agent,
                        };
                        review_recap(ui, &mut notes, &batch);
                    }
                });
            });
        });
        // The strip is closed top and bottom by a hairline: on a page of code the fill
        // alone is not a boundary, and two rules cost nothing where an outline would
        // have boxed the whole file in.
        if chrome.band() {
            let rule = egui::Stroke::new(1.0_f32, palette.border_subtle);
            let x = header.response.rect.x_range();
            ui.painter().hline(x, header.response.rect.top(), rule);
            ui.painter().hline(x, header.response.rect.bottom(), rule);
        }
        // A folded band is its header and nothing else — the rows, and the syntax
        // highlighting they would ask for, are never built.
        if collapsed {
            return;
        }
        let body_frame = match chrome {
            DiffChrome::Card => egui::Frame::NONE.inner_margin(egui::Margin {
                top: 8,
                ..egui::Margin::ZERO
            }),
            DiffChrome::Band { .. } => egui::Frame::NONE.inner_margin(egui::Margin {
                left: BAND_PAD_X,
                right: BAND_PAD_X,
                top: BAND_BODY_PAD_Y,
                bottom: BAND_BODY_PAD_Y,
            }),
        };
        body_frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            if state.is_stale() {
                ui.label(
                    egui::RichText::new("File changed on disk — selection no longer applies")
                        .size(LINE_SIZE)
                        .color(palette.git_modified),
                );
                ui.add_space(8.0);
            }

            if let Some(request) = divergence_notice(ui, palette, &mut state.editing, intents) {
                intents.push(GitIntent::OpenDiff {
                    path: request.path,
                    staged: request.stage_after,
                });
            }

            if diff.binary {
                match &diff.image {
                    Some(blob) => image_preview(ui, palette, blob, &mut state.image),
                    None => {
                        ui.label(
                            egui::RichText::new("Binary file — no line diff")
                                .size(LINE_SIZE)
                                .color(palette.text_muted),
                        );
                    }
                }
                return;
            }
            if diff.oversize {
                ui.label(
                    egui::RichText::new("Large diff — file-level staging only")
                        .size(LINE_SIZE)
                        .color(palette.text_muted),
                );
                return;
            }
            if diff.hunks.is_empty() {
                ui.label(
                    egui::RichText::new("No changes")
                        .size(LINE_SIZE)
                        .color(palette.text_muted),
                );
                return;
            }

            // Both caches key on the diff's content: hashing it walks the whole diff,
            // so the frame does it once and hands the result to each of them.
            let fingerprint = diff.fingerprint();
            state.ensure_syntax_cache(ui, diff, fingerprint, palette.syntax);
            state.ensure_intraline_cache(ui, diff, fingerprint);
            let char_w = ui.ctx().fonts_mut(|fonts| {
                fonts
                    .glyph_width(&egui::FontId::monospace(LINE_SIZE), ' ')
                    .max(1.0)
            });
            let layout = RowLayout::for_diff(diff, char_w);
            let extensions = context_extensions(diff, &state.extensions);
            // Width of the widest displayed line: rows are allocated at this width
            // (not the viewport's) so egui exposes a horizontal scrollbar for lines
            // longer than the preview.
            let max_chars = state.content_chars(diff);
            let content_width =
                layout.content_left(0.0) + max_chars as f32 * char_w + CONTENT_TRAILING_PAD;
            let mut extend_requests = Vec::new();
            let mut reveal = None;
            // A band scrolls **horizontally only**: the column it stacks in owns the
            // vertical axis, and a vertical scroll of its own would trap the rows in a
            // fixed-height box.
            let axes = if chrome.band() {
                [true, false]
            } else {
                [true, true]
            };
            let scrolled = egui::ScrollArea::new(axes)
                .auto_shrink([false, !axes[1]])
                // Without a salt the offset is keyed on the position in the Ui tree
                // alone, so the next file opened here inherits the scroll of the
                // previous one.
                .id_salt((surface.scroll_key(), diff.path.as_str()))
                .show(ui, |ui| {
                    let row_w = content_width.max(ui.available_width());
                    let mut text_rows = Vec::new();
                    let mut text_row = 0;
                    for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
                        if hunk_header(
                            ui,
                            palette,
                            staged,
                            read_only,
                            hunk_idx,
                            state,
                            can_extend(diff, &state.extensions, &extensions, hunk_idx),
                            intents,
                        ) {
                            extend_requests.push(hunk_idx);
                        }
                        ui.add_space(4.0);
                        let previous_spacing_y = ui.spacing().item_spacing.y;
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // The edited hunk shows its buffer in place of its rows: the buffer
                        // holds the working tree's own lines, so the deletions — which have
                        // no counterpart there — step aside with them (git.md §4).
                        if let Some(edit) = state
                            .editing
                            .edit_mut()
                            .filter(|edit| edit.anchor == hunk_idx)
                        {
                            let look = EditorLook {
                                palette,
                                columns: layout.editor_columns(),
                                width: row_w,
                            };
                            if inline_editor(ui, edit, &look) {
                                state.editing.leave(intents);
                            }
                            ui.spacing_mut().item_spacing.y = previous_spacing_y;
                            ui.add_space(12.0);
                            continue;
                        }
                        let ext = extensions[hunk_idx].clone();
                        let caret = caret_offer(editable, diff, hunk, &extensions[hunk_idx]);
                        for new_no in ext.above {
                            let action = extension_line(
                                ui,
                                ExtensionRow {
                                    diff,
                                    old_no: above_old_lineno(hunk, new_no),
                                    new_no,
                                    staged,
                                    read_only,
                                    caret,
                                    text_row,
                                    char_w,
                                    layout,
                                    row_w,
                                },
                                palette,
                                state,
                                &mut text_rows,
                            );
                            text_row += 1;
                            match action {
                                Some(DiffLineAction::OpenEditor { col }) => open_hunk_editor(
                                    state,
                                    diff,
                                    &extensions[hunk_idx],
                                    hunk_idx,
                                    Some(new_no),
                                    col,
                                    edit_staged,
                                    intents,
                                ),
                                other => apply_line_action(state, diff, intents, other),
                            }
                        }
                        // New-side line the caret lands on for a click on a row that has
                        // none of its own: a deletion is gone from the buffer, so the caret
                        // goes to the working-tree line just above it.
                        let mut last_new = None;
                        for (line_idx, line) in hunk.lines.iter().enumerate() {
                            let text = display_text(&line.content);
                            if state.reveal_line.is_some() && line.new_lineno == state.reveal_line {
                                if chrome.band() {
                                    reveal = Some(ui.cursor().min);
                                } else {
                                    ui.scroll_to_cursor(Some(egui::Align::Center));
                                }
                                state.reveal_line = None;
                            }
                            let action = {
                                let row = RowData {
                                    origin: line.origin,
                                    text,
                                    old_lineno: line.old_lineno,
                                    new_lineno: line.new_lineno,
                                };
                                let line_ctx = DiffLineCtx {
                                    palette,
                                    staged,
                                    read_only,
                                    review: review_available,
                                    forge: review_forge,
                                    selected: state.selected(hunk_idx, line_idx),
                                    caret,
                                    highlighted: state.syntax_line(hunk_idx, line_idx),
                                    changed: state.intraline_line(hunk_idx, line_idx),
                                    text_range: state.text_range_for_row(text_row, text),
                                    text_row,
                                    char_w,
                                    layout,
                                    row_w,
                                };
                                diff_line(ui, &row, hunk_idx, line_idx, &line_ctx, &mut text_rows)
                            };
                            text_row += 1;
                            match action {
                                Some(DiffLineAction::OpenComment { pool, old, new }) => {
                                    let store = match pool {
                                        ReviewPool::Forge => review_forge_store,
                                        ReviewPool::Agent => review_comments,
                                    };
                                    let saved = note_at(store, &diff.path, old, new);
                                    state.notes.open(NoteAnchor { pool, old, new }, saved);
                                }
                                Some(DiffLineAction::OpenEditor { col }) => open_hunk_editor(
                                    state,
                                    diff,
                                    &extensions[hunk_idx],
                                    hunk_idx,
                                    line.new_lineno.or(last_new),
                                    col,
                                    edit_staged,
                                    intents,
                                ),
                                other => apply_line_action(state, diff, intents, other),
                            }
                            last_new = line.new_lineno.or(last_new);
                            existing_block(
                                ui,
                                palette,
                                &diff.path,
                                line.old_lineno,
                                line.new_lineno,
                                review_existing,
                                review_agent,
                                state,
                                &mut review_out,
                                0.0,
                            );
                            let mut notes = NoteCtx {
                                palette,
                                session: &mut state.notes,
                                out: &mut review_out,
                            };
                            let (old, new) = (line.old_lineno, line.new_lineno);
                            let agent_line = NoteLine {
                                path: &diff.path,
                                anchor: NoteAnchor {
                                    pool: ReviewPool::Agent,
                                    old,
                                    new,
                                },
                                code: text,
                                saved: note_at(review_comments, &diff.path, old, new),
                            };
                            if review_forge {
                                let forge_line = NoteLine {
                                    anchor: NoteAnchor {
                                        pool: ReviewPool::Forge,
                                        ..agent_line.anchor
                                    },
                                    saved: note_at(review_forge_store, &diff.path, old, new),
                                    ..agent_line
                                };
                                note_block(ui, &forge_line, &mut notes);
                            }
                            note_block(ui, &agent_line, &mut notes);
                        }
                        for new_no in ext.below {
                            let action = extension_line(
                                ui,
                                ExtensionRow {
                                    diff,
                                    old_no: below_old_lineno(hunk, new_no),
                                    new_no,
                                    staged,
                                    read_only,
                                    caret,
                                    text_row,
                                    char_w,
                                    layout,
                                    row_w,
                                },
                                palette,
                                state,
                                &mut text_rows,
                            );
                            text_row += 1;
                            match action {
                                Some(DiffLineAction::OpenEditor { col }) => open_hunk_editor(
                                    state,
                                    diff,
                                    &extensions[hunk_idx],
                                    hunk_idx,
                                    Some(new_no),
                                    col,
                                    edit_staged,
                                    intents,
                                ),
                                other => apply_line_action(state, diff, intents, other),
                            }
                        }
                        ui.spacing_mut().item_spacing.y = previous_spacing_y;
                        ui.add_space(12.0);
                    }
                    update_text_selection(ui, state, &text_rows);
                });
            // A code line wider than the band owns rightward swipes over it until it is
            // back at its left edge, so the PR review's back gesture cannot steal one
            // (pull-requests.md §11).
            crate::ui::note_h_scroll_room(ui.ctx(), scrolled.inner_rect, scrolled.state.offset.x);
            out.reveal =
                reveal.map(|pos| egui::Rect::from_min_size(pos, egui::vec2(1.0, LINE_HEIGHT)));
            for hunk in extend_requests {
                state.extend(hunk);
                ui.ctx().request_repaint();
            }
        });
    });

    if let Some(r) = review {
        r.intents.append(&mut review_out);
    }
    out
}

/// The hairline that separates two hunks (git.md §4).
fn hunk_rule(ui: &mut egui::Ui, palette: &Palette) {
    ui.painter().hline(
        ui.available_rect_before_wrap().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(1.0_f32, with_alpha(palette.border_subtle, 140)),
    );
}

/// **Extend context**, as a quiet inline control rather than an outlined pill: it
/// sits over every hunk of every diff, and a bordered button there is a row of
/// furniture between two rows of code (git.md §4).
fn extend_link(ui: &mut egui::Ui, palette: &Palette) -> bool {
    let font = egui::FontId::proportional(PILL_SIZE);
    let galley = ui.painter().layout_no_wrap(
        "Extend context".to_owned(),
        font,
        egui::Color32::PLACEHOLDER,
    );
    // Wider and taller than the text it draws: a 12pt label is a 12pt hit area
    // otherwise, and this one is clicked repeatedly while reading.
    let size = galley.size() + egui::vec2(12.0, 12.0);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let color = if hovered {
        palette.accent
    } else {
        palette.text_muted
    };
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, color);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Extend context")
    });
    response.clicked()
}

/// The band's fold control: a chevron pointing down when the diff is open, right
/// when it is folded away (pull-requests.md §11).
fn collapse_chevron(ui: &mut egui::Ui, palette: &Palette, collapsed: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(PILL_HEIGHT, PILL_HEIGHT), egui::Sense::click());
    let icon = if collapsed {
        lucide_icons::Icon::ChevronRight
    } else {
        lucide_icons::Icon::ChevronDown
    };
    let color = if response.hovered() {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), 16.0, icon, color);
    let label = if collapsed { "Expand" } else { "Collapse" };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.on_hover_text(label).clicked()
}

/// Small square icon button (hover-tinted) used for the note editor and popover
/// controls; `label` is its accessibility name.
pub(crate) fn icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: lucide_icons::Icon,
    color: egui::Color32,
    label: &str,
) -> egui::Response {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(LINE_HEIGHT, LINE_HEIGHT), true);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            with_alpha(color, 36),
        );
    }
    let tint = if hovered { color } else { palette.text_muted };
    crate::ui::paint_icon(ui.painter(), rect.center(), LINE_SIZE, icon, tint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// What the reply editor raised this frame (pull-requests.md §11).
pub(crate) enum ReplyEdit {
    Idle,
    Send,
    Cancel,
}

/// Geometry of the reply editor (pull-requests.md §11): **one framed object** — a padded
/// field over a hairline and an action bar — the same shape as the conversation composer.
/// The radius is deliberately below the cards it nests in (10 there, 6 here): a nested
/// surface wearing its parent's radius is what makes an interface read as "unstyled".
pub(crate) const EDITOR_RADIUS: u8 = 6;
pub(crate) const EDITOR_PAD_X: f32 = 12.0;
pub(crate) const EDITOR_PAD_Y: f32 = 10.0;
pub(crate) const EDITOR_TEXT_SIZE: f32 = 13.5;
const EDITOR_HINT_SIZE: f32 = 12.0;
pub(crate) const EDITOR_BAR_HEIGHT: f32 = 40.0;
const EDITOR_BUTTON_SIZE: f32 = 12.0;
const EDITOR_BUTTON_HEIGHT: f32 = 28.0;
const EDITOR_HINT_GAP: f32 = 6.0;

/// The hint and button captions for a `reply_editor`, so the same field reads as a
/// reply or a new comment depending on where it is opened (pull-requests.md §11).
pub(crate) struct EditorLabels {
    pub hint: &'static str,
    pub send: &'static str,
    pub cancel: &'static str,
}

/// Replying to an existing thread or conversation card.
pub(crate) const REPLY_LABELS: EditorLabels = EditorLabels {
    hint: "Reply…",
    send: "Send reply",
    cancel: "Cancel reply",
};

/// Shared reply field: a multiline input (Enter sends, Shift+Enter inserts a
/// newline) with a Send / Cancel footer. Used by the diff overlay and the center
/// inline-comment card so a reply reads the same in both. Unlike `note_editor` it
/// never validates on lost focus — the two surfaces share the buffer, so moving
/// between them must not fire the reply.
pub(crate) fn reply_editor(
    ui: &mut egui::Ui,
    palette: &Palette,
    buffer: &mut String,
    focus: &mut bool,
    width: f32,
    labels: &EditorLabels,
) -> ReplyEdit {
    let submit_key = ui.input_mut(|i| {
        let mut submit = false;
        i.events.retain(|e| {
            let is_submit = matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.shift
            );
            submit |= is_submit;
            !is_submit
        });
        submit
    });
    // `Esc` closes the field and stops there — the key is consumed so the surface's own
    // cascade (drop the file, leave the review) never fires on the same press: an open
    // composer is what the user is aiming at (pull-requests.md §11).
    let cancel_key = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let mut edit = if submit_key {
        ReplyEdit::Send
    } else if cancel_key {
        ReplyEdit::Cancel
    } else {
        ReplyEdit::Idle
    };
    // The focus ring is read *before* the field is added, so the frame around it can
    // carry the ring — egui's own widget stroke sits inside the frame and is invisible
    // once the field is frameless.
    let field_id = ui.id().with("reply_editor_field");
    let ring = if ui.memory(|m| m.has_focus(field_id)) {
        egui::Stroke::new(1.5_f32, palette.accent)
    } else {
        egui::Stroke::new(1.0_f32, palette.border_input)
    };
    let response = egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(ring)
        .corner_radius(egui::CornerRadius::same(EDITOR_RADIUS))
        .show(ui, |ui| {
            // The callers place the editor inside horizontal layouts too, where the field
            // and its action bar would sit side by side: stack them explicitly.
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.visuals_mut().selection.stroke = egui::Stroke::new(1.5_f32, palette.accent);
                let response = ui.add(
                    egui::TextEdit::multiline(buffer)
                        .id(field_id)
                        // `TextEdit::margin` is ignored once a custom frame is given
                        // (egui builds `Frame::new().inner_margin(margin)` only when it
                        // owns the frame), so the padding rides on the frame itself.
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
                            EDITOR_PAD_X as i8,
                            EDITOR_PAD_Y as i8,
                        )))
                        .desired_rows(2)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::proportional(EDITOR_TEXT_SIZE))
                        .hint_text(labels.hint),
                );
                editor_hairline(ui, palette);
                // Pin the bar to its own height: a bare `right_to_left` layout inherits the
                // parent's full remaining height and would center the buttons far below the
                // field when the editor sits high in a tall scroll area (the center card),
                // pulling them out of clicking range.
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), EDITOR_BAR_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(EDITOR_PAD_X);
                        ui.label(
                            egui::RichText::new("Enter to send")
                                .size(EDITOR_HINT_SIZE)
                                .color(palette.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if bar_button(
                                ui,
                                palette,
                                labels.send,
                                None,
                                true,
                                !buffer.trim().is_empty(),
                            ) {
                                edit = ReplyEdit::Send;
                            }
                            if bar_button(ui, palette, labels.cancel, None, false, true) {
                                edit = ReplyEdit::Cancel;
                            }
                        });
                    },
                );
                response
            })
            .inner
        })
        .inner;
    if *focus {
        response.request_focus();
        *focus = false;
    }
    edit
}

/// A full-strength 1px rule between an editor's field and its action bar. The list's
/// `row_separator` is alpha'd down for dense rows and disappears inside a framed input.
pub(crate) fn editor_hairline(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

/// One button of an editor's action bar: `filled` paints the accent primary (greyed while
/// `enabled` is false, since an empty reply has nothing to send), otherwise a quiet ghost.
/// `shortcut` rides inside the button, one notch quieter than the caption, so the keyboard
/// path is read off the action itself rather than learned elsewhere.
/// Sized to the app's dense-desktop hit area rather than to its glyph.
pub(crate) fn bar_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    shortcut: Option<&str>,
    filled: bool,
    enabled: bool,
) -> bool {
    let label = label.to_owned();
    let font = egui::FontId::new(EDITOR_BUTTON_SIZE, crate::theme::medium_family(ui.ctx()));
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font.clone(), egui::Color32::PLACEHOLDER);
    let hint = shortcut.map(|s| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
    });
    let caption = galley.size();
    let hint_width = hint.as_ref().map_or(0.0, |g| EDITOR_HINT_GAP + g.size().x);
    let size = egui::vec2(caption.x + hint_width + 24.0, EDITOR_BUTTON_HEIGHT);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, enabled);
    let (fill, ink) = match (filled, enabled, hovered) {
        (true, false, _) => (palette.bg_surface_hover, palette.state_disabled),
        (true, _, true) => (palette.accent_hover, palette.lane_node_text),
        (true, _, false) => (palette.accent, palette.lane_node_text),
        (false, _, true) => (palette.bg_surface_hover, palette.text_primary),
        (false, _, false) => (egui::Color32::TRANSPARENT, palette.text_secondary),
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    }
    let left = rect.center().x - (caption.x + hint_width) / 2.0;
    ui.painter().galley(
        egui::pos2(left, rect.center().y - caption.y / 2.0),
        galley,
        ink,
    );
    if let Some(hint) = hint {
        let top = rect.center().y - hint.size().y / 2.0;
        ui.painter().galley(
            egui::pos2(left + caption.x + EDITOR_HINT_GAP, top),
            hint,
            with_alpha(ink, 150),
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.clone())
    });
    response.clicked()
}

/// Renders the read-only PR thread anchored at `line` of `path` (if any) below the
/// diff line, one card per comment, plus an "Ask {agent}" pill when `agent` is set
/// and a Reply affordance that opens an inline reply editor (pull-requests.md §11).
#[allow(clippy::too_many_arguments)]
fn existing_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    old: Option<u32>,
    new: Option<u32>,
    existing: &ForgeThreads,
    agent: &str,
    state: &mut DiffViewState,
    out: &mut Vec<ReviewIntent>,
    indent: f32,
) {
    let Some(file) = existing.get(path) else {
        return;
    };
    // A forge comment anchors to one side: a new-side note matches this row by its
    // new line, an old-side (deleted-line) note by its old line. Looking up each
    // side against its own anchor keeps a modified line — a deleted row and an
    // added row sharing a number — from rendering the same thread on both.
    let anchors = [new.map(|n| (None, Some(n))), old.map(|o| (Some(o), None))];
    for anchor in anchors.into_iter().flatten() {
        let Some(thread) = file.get(&anchor) else {
            continue;
        };
        let now = crate::ui::pull_requests_view::now_epoch_secs();
        // The commented code, shown once atop the thread; replies reuse the root's
        // hunk so it isn't redrawn per comment.
        let snippet = thread
            .iter()
            .find_map(|c| c.context.as_deref())
            .map(|h| crate::pull_requests::model::hunk_snippet(h, OVERLAY_SNIPPET_LINES));
        // Thread-level handles, read once: the actions they drive all hang off the
        // root, not off the comment whose card carries them.
        let resolved = thread.first().is_some_and(|c| c.resolved);
        let thread_id = thread.iter().find_map(|c| c.thread_id.clone());
        let reply_id = thread.iter().find_map(|c| c.id);
        let replying = reply_id.is_some_and(|id| state.active_reply == Some(id));
        // A resolved thread folds to a single row, like the Conversation tab's resolved
        // group: what is settled must not push the code apart (pull-requests.md §11).
        let fold = reply_id
            .filter(|_| resolved)
            .map(|id| state.is_resolved_expanded(id));
        let actions = ThreadActions {
            ask: (!agent.is_empty()).then(|| format!("Ask {agent}")),
            reply: reply_id.is_some() && !replying,
            resolve: reply_id.map(|_| resolved),
        };
        ui.add_space(2.0);
        let mut click = ThreadClick::default();
        let mut edit = ReplyEdit::Idle;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            // The open composer takes the bar's place inside the card, on the same
            // gutter: answering a thread must not split it into two objects.
            let mut composer = |ui: &mut egui::Ui| {
                let (buffer, focus) = state.reply_fields();
                let width = ui.available_width();
                edit = reply_editor(ui, palette, buffer, focus, width, &REPLY_LABELS);
            };
            click = thread_card(
                ui,
                palette,
                path,
                thread,
                &actions,
                snippet.as_deref(),
                now,
                fold,
                replying.then_some(&mut composer as &mut dyn FnMut(&mut egui::Ui)),
            );
        });
        match edit {
            ReplyEdit::Send => {
                let body = state.reply_buffer.trim().to_owned();
                if let Some(id) = reply_id.filter(|_| !body.is_empty()) {
                    out.push(ReviewIntent::ReplyToThread {
                        comment_id: id,
                        body,
                    });
                }
                state.cancel_reply();
            }
            ReplyEdit::Cancel => state.cancel_reply(),
            ReplyEdit::Idle => {}
        }
        if click.ask {
            out.push(ReviewIntent::AskAgentOnThread {
                file: path.to_owned(),
                old: anchor.0,
                new: anchor.1,
            });
        }
        if let Some(id) = reply_id {
            if click.toggle {
                state.toggle_resolved(id);
            }
            if click.reply {
                state.open_reply(id);
            }
            if click.resolve {
                out.push(ReviewIntent::ResolveThread {
                    thread_id: thread_id.clone(),
                    comment_id: id,
                    resolved: !resolved,
                });
            }
        }
        ui.add_space(2.0);
    }
}

/// The thread-level resolve toggle: "Resolve" (a check) when open, "Reopen" (an undo
/// arc) when resolved — a quiet neutral pill beside Reply (pull-requests.md §11).
pub(crate) fn resolve_pill(ui: &mut egui::Ui, palette: &Palette, resolved: bool) -> bool {
    let (icon, label) = if resolved {
        (lucide_icons::Icon::RotateCcw, "Reopen")
    } else {
        (lucide_icons::Icon::CheckCircle, "Resolve")
    };
    pill_button(
        ui,
        palette,
        palette.text_primary,
        icon,
        label,
        RADIUS_BUTTON,
    )
}

/// The "Reply" pill that opens a thread's reply editor — a quiet, neutral button
/// (the forge-write accent stays on the editor's Send), matching the PR mockup.
/// Returns `true` on click.
pub(crate) fn reply_pill(ui: &mut egui::Ui, palette: &Palette) -> bool {
    pill_button(
        ui,
        palette,
        palette.text_primary,
        lucide_icons::Icon::MessageSquare,
        "Reply",
        RADIUS_BUTTON,
    )
}

/// The thread-level controls in a thread block's action bar. `reply` is false while its
/// editor is already open below, `resolve` is `None` on a thread the forge gave no id.
struct ThreadActions {
    ask: Option<String>,
    reply: bool,
    resolve: Option<bool>,
}

/// What a thread block raised this frame — its action bar, or the summary row of a
/// resolved thread.
#[derive(Default)]
struct ThreadClick {
    ask: bool,
    reply: bool,
    resolve: bool,
    toggle: bool,
}

const THREAD_CARD_PAD: i8 = 10;
const THREAD_CARD_RADIUS: u8 = 10;
const THREAD_AVATAR_GAP: f32 = 10.0;

/// A whole posted thread as **one framed object**, the shape the Conversation tab gives
/// it: the root comment at full weight, its replies nested under a left thread-rail with
/// a lighter avatar, then a hairline and the thread's action bar. One block — not a
/// stack of drifting cards, one per comment. Read-only ink, so a fetched comment reads
/// apart from a forge review (`accent`) or an agent note (`accent_ai`).
#[allow(clippy::too_many_arguments)]
fn thread_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    thread: &[crate::review::ThreadComment],
    actions: &ThreadActions,
    snippet: Option<&[crate::pull_requests::model::SnippetLine]>,
    now: i64,
    fold: Option<bool>,
    composer: Option<&mut dyn FnMut(&mut egui::Ui)>,
) -> ThreadClick {
    let mut click = ThreadClick::default();
    let Some((root, replies)) = thread.split_first() else {
        return click;
    };
    let width = card_width(ui);
    egui::Frame::new()
        .fill(palette.bg_surface)
        .corner_radius(egui::CornerRadius::same(THREAD_CARD_RADIUS))
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .show(ui, |ui| {
            // The hairline must run edge to edge, so the padding rides on the body
            // block rather than on the card — the body keeps the ambient spacing the
            // stack around it is zeroed of.
            let body_spacing = ui.spacing().item_spacing;
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(expanded) = fold {
                    if resolved_summary(ui, palette, thread, expanded) {
                        click.toggle = true;
                    }
                    if !expanded {
                        return;
                    }
                    editor_hairline(ui, palette);
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(THREAD_CARD_PAD))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = body_spacing;
                        thread_member(ui, palette, path, root, snippet, now, false);
                        thread_replies(ui, palette, path, replies, now);
                    });
                editor_hairline(ui, palette);
                if let Some(composer) = composer {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(THREAD_CARD_PAD))
                        .show(ui, |ui| composer(ui));
                    return;
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), EDITOR_BAR_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(THREAD_CARD_PAD as f32);
                        // The hand-off to the agent stands apart from the two forge
                        // actions: it writes nothing to the thread.
                        if let Some(label) = &actions.ask {
                            if pill_button(
                                ui,
                                palette,
                                palette.accent_ai,
                                lucide_icons::Icon::Bot,
                                label,
                                RADIUS_BUTTON,
                            ) {
                                click.ask = true;
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(THREAD_CARD_PAD as f32);
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if let Some(resolved) = actions.resolve {
                                if resolve_pill(ui, palette, resolved) {
                                    click.resolve = true;
                                }
                            }
                            if actions.reply && reply_pill(ui, palette) {
                                click.reply = true;
                            }
                        });
                    },
                );
            });
        });
    click
}

/// A resolved thread folded to one row: the check, the comment tally and the first line
/// of its root elided, with a chevron. Clicking it toggles the body below. Returns
/// `true` on click (pull-requests.md §11).
fn resolved_summary(
    ui: &mut egui::Ui,
    palette: &Palette,
    thread: &[crate::review::ThreadComment],
    expanded: bool,
) -> bool {
    let (rect, response, hovered) = crate::ui::clickable(
        ui,
        egui::vec2(ui.available_width(), THREAD_FOLD_ROW_H),
        true,
    );
    // Folded, the row *is* the card: it wears all four corners. Expanded, the body
    // below owns the bottom pair.
    let radius = if expanded {
        egui::CornerRadius {
            nw: THREAD_CARD_RADIUS,
            ne: THREAD_CARD_RADIUS,
            sw: 0,
            se: 0,
        }
    } else {
        egui::CornerRadius::same(THREAD_CARD_RADIUS)
    };
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, radius, palette.bg_surface_hover);
    }
    let pad = THREAD_CARD_PAD as f32;
    crate::ui::paint_icon(
        painter,
        egui::pos2(rect.left() + pad + LINE_SIZE / 2.0, rect.center().y),
        LINE_SIZE,
        lucide_icons::Icon::Check,
        palette.git_added,
    );
    let chevron_x = rect.right() - pad - LINE_SIZE / 2.0;
    crate::ui::paint_icon(
        painter,
        egui::pos2(chevron_x, rect.center().y),
        LINE_SIZE,
        if expanded {
            lucide_icons::Icon::ChevronUp
        } else {
            lucide_icons::Icon::ChevronDown
        },
        palette.text_muted,
    );
    let mut left = rect.left() + pad + LINE_SIZE + 6.0;
    let font = egui::FontId::proportional(LINE_SIZE);
    for (text, color) in [
        ("Resolved".to_owned(), palette.text_secondary),
        (thread_tally(thread.len()), palette.text_muted),
    ] {
        let galley = painter.layout_no_wrap(text, font.clone(), color);
        painter.galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.0),
            galley.clone(),
            color,
        );
        left += galley.size().x + 8.0;
    }
    // The excerpt takes what is left, elided rather than run under the chevron.
    let excerpt = thread
        .first()
        .and_then(|c| c.body.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(excerpt_text)
        .unwrap_or_default();
    let available = chevron_x - LINE_SIZE / 2.0 - 8.0 - left;
    if !excerpt.is_empty() && available > 24.0 {
        let mut job = egui::text::LayoutJob::single_section(
            excerpt.to_owned(),
            egui::text::TextFormat {
                font_id: font,
                color: palette.text_muted,
                ..Default::default()
            },
        );
        job.wrap = egui::text::TextWrapping {
            max_width: available,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let galley = painter.layout_job(job);
        painter.galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text_muted,
        );
    }
    let label = format!("Resolved thread · {}", thread_tally(thread.len()));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// The first line of a comment as a one-line excerpt: the markdown that would render as
/// weight or code is only noise once it is painted raw (`**SUGGESTION**`).
fn excerpt_text(line: &str) -> String {
    line.trim_start_matches(['#', '>', '-', ' '])
        .replace(['*', '_', '`'], "")
        .trim()
        .to_owned()
}

fn thread_tally(n: usize) -> String {
    if n == 1 {
        "1 comment".to_owned()
    } else {
        format!("{n} comments")
    }
}

const THREAD_FOLD_ROW_H: f32 = 34.0;

/// Width a comment surface takes inside the diff. The rows are allocated at the width of
/// the **longest line** so egui exposes a horizontal scrollbar for lines past the
/// preview — a card sized on that available width would run its right-edge controls off
/// the viewport, unreachable. Clamp to what is actually visible from here.
pub(crate) fn card_width(ui: &egui::Ui) -> f32 {
    let visible = ui.clip_rect().right() - ui.max_rect().left() - CARD_TRAILING_PAD;
    ui.available_width().min(visible).max(MIN_CARD_WIDTH)
}

const CARD_TRAILING_PAD: f32 = 8.0;
const MIN_CARD_WIDTH: f32 = 160.0;

/// The replies of a thread, indented under a vertical rail so they read as answers to
/// the root rather than as comments of their own (the Conversation tab's grammar).
fn thread_replies(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    replies: &[crate::review::ThreadComment],
    now: i64,
) {
    if replies.is_empty() {
        return;
    }
    let block = ui.horizontal_top(|ui| {
        ui.add_space(REPLY_NEST_INDENT);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            for reply in replies {
                ui.add_space(8.0);
                thread_member(ui, palette, path, reply, None, now, true);
            }
        });
    });
    let rect = block.response.rect;
    ui.painter().vline(
        rect.left() + REPLY_NEST_INDENT * 0.5,
        egui::Rangef::new(rect.top() + 6.0, rect.bottom() - 2.0),
        egui::Stroke::new(2.0_f32, palette.border_input),
    );
}

/// One comment of a thread: the avatar in a fixed left gutter (lighter for a reply), the
/// author line — with the age pushed to the right edge — and the body in the column
/// beside it. Reuses the shared `detail::author_avatar`, so inline threads and the PR
/// conversation rail wear the same face.
fn thread_member(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    comment: &crate::review::ThreadComment,
    snippet: Option<&[crate::pull_requests::model::SnippetLine]>,
    now: i64,
    reply: bool,
) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        if reply {
            crate::ui::detail::author_avatar_small(ui, palette, &comment.author);
        } else {
            crate::ui::detail::author_avatar(ui, palette, &comment.author);
        }
        ui.add_space(THREAD_AVATAR_GAP);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&comment.author)
                        .size(LINE_SIZE)
                        .color(palette.text_primary)
                        .strong(),
                );
                let age = crate::pull_requests::model::relative_age(&comment.created_at, now);
                if !age.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(age)
                                .size(LINE_SIZE - 1.0)
                                .color(palette.text_muted),
                        );
                    });
                }
            });
            if let Some(snip) = snippet.filter(|s| !s.is_empty()) {
                ui.add_space(4.0);
                crate::ui::detail::code_snippet(ui, palette, path, snip);
            }
            ui.add_space(3.0);
            crate::ui::pull_requests_view::markdown(ui, palette, &comment.body);
        });
    });
}

/// Metrics of the thread pills (Reply / Resolve / Ask): the **outlined** button the
/// app already uses for its secondary actions — the review header's *Finish review* and
/// *Checkout* wear the same shape — at the size the design canvas draws them.
const PILL_LABEL_SIZE: f32 = 12.0;
const PILL_ICON_SIZE: f32 = 14.0;
const PILL_HEIGHT: f32 = 28.0;
const PILL_PAD_X: f32 = 10.0;

/// A pill button: a leading glyph and `label` in a rounded rect, **outlined** at rest
/// (no fill, `border.input` stroke, primary ink) and washed to `hover_accent` on hover.
/// The rest state carries no fill on purpose: these pills sit on cards *and* on raised
/// blocks, and a `bg.surface` fill disappeared against the latter. `radius` picks
/// stadium (`RADIUS_PILL`) vs button corners. Returns `true` on click.
pub(crate) fn pill_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    hover_accent: egui::Color32,
    icon: lucide_icons::Icon,
    label: &str,
    radius: u8,
) -> bool {
    let label = label.to_owned();
    let font = egui::FontId::new(PILL_LABEL_SIZE, crate::theme::medium_family(ui.ctx()));
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font.clone(), egui::Color32::PLACEHOLDER);
    let size = egui::vec2(
        PILL_ICON_SIZE + 6.0 + galley.size().x + PILL_PAD_X * 2.0,
        PILL_HEIGHT,
    );
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let (fill, content) = if hovered {
        (with_alpha(hover_accent, 36), hover_accent)
    } else {
        (egui::Color32::TRANSPARENT, palette.text_primary)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(radius),
        fill,
        egui::Stroke::new(1.0_f32, palette.border_input),
        egui::StrokeKind::Inside,
    );
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(
            rect.left() + PILL_PAD_X + PILL_ICON_SIZE / 2.0,
            rect.center().y,
        ),
        PILL_ICON_SIZE,
        icon,
        content,
    );
    ui.painter().text(
        egui::pos2(
            rect.left() + PILL_PAD_X + PILL_ICON_SIZE + 6.0,
            rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        label.clone(),
        font,
        content,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    response.clicked()
}

/// Identity of the diff the width measure was taken on. Only the per-hunk geometry
/// is hashed: hashing every line costs *more* than the measure it would spare, and
/// a same-shape reload already invalidates through `reconcile`.
fn shape_fingerprint(diff: &FileDiff) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    diff.path.hash(&mut hasher);
    diff.hunks.len().hash(&mut hasher);
    for hunk in &diff.hunks {
        hunk.header.hash(&mut hasher);
        hunk.old_start.hash(&mut hasher);
        hunk.old_lines.hash(&mut hasher);
        hunk.new_start.hash(&mut hasher);
        hunk.new_lines.hash(&mut hasher);
        hunk.lines.len().hash(&mut hasher);
    }
    hasher.finish()
}

/// What a line selection is anchored on: the line's origin and content, so a
/// reload that keeps the hunk shape but rewrites the line invalidates the pick
/// (git.md §8). The origin is part of it because the same text flipping from
/// added to deleted at the same index would otherwise keep the pick and stage
/// the opposite of what was selected.
fn line_fingerprint(line: &DiffLine) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.origin.hash(&mut hasher);
    line.content.hash(&mut hasher);
    hasher.finish()
}

fn apply_line_action(
    state: &mut DiffViewState,
    diff: &FileDiff,
    intents: &mut Vec<GitIntent>,
    action: Option<DiffLineAction>,
) {
    match action {
        Some(DiffLineAction::Intent(intent)) => intents.push(intent),
        Some(DiffLineAction::ToggleSelection { hunk, line }) => state.toggle(diff, hunk, line),
        Some(DiffLineAction::SelectText(selection)) => {
            state.selection.clear();
            state.text_selection = Some(selection);
        }
        Some(DiffLineAction::ClearTextSelection) => state.text_selection = None,
        Some(DiffLineAction::RefuseEditor(reason)) => intents.push(GitIntent::EditRefused {
            path: diff.path.clone(),
            reason,
        }),
        // Handled by the caller (needs the stored comments to prefill the editor,
        // resp. the hunk the row belongs to).
        Some(DiffLineAction::OpenComment { .. }) | Some(DiffLineAction::OpenEditor { .. }) => {}
        None => {}
    }
}

/// Parameters of an extended context line: pure context, identified by its
/// numbers on both sides — never stageable, but text-selectable.
struct ExtensionRow<'a> {
    diff: &'a FileDiff,
    old_no: u32,
    new_no: u32,
    staged: bool,
    read_only: bool,
    caret: CaretOffer,
    text_row: usize,
    char_w: f32,
    layout: RowLayout,
    row_w: f32,
}

fn extension_line(
    ui: &mut egui::Ui,
    ext: ExtensionRow<'_>,
    palette: &Palette,
    state: &DiffViewState,
    text_rows: &mut Vec<TextRow>,
) -> Option<DiffLineAction> {
    // Extension ranges are clamped to the file bounds at construction
    // (`context_extensions`): direct indexing is safe.
    let text = ext.diff.source_lines[ext.new_no as usize - 1].as_str();
    let row = RowData {
        origin: LineOrigin::Context,
        text,
        old_lineno: Some(ext.old_no),
        new_lineno: Some(ext.new_no),
    };
    let line_ctx = DiffLineCtx {
        palette,
        staged: ext.staged,
        read_only: ext.read_only,
        review: false,
        forge: false,
        selected: false,
        caret: ext.caret,
        highlighted: None,
        changed: &[],
        text_range: state.text_range_for_row(ext.text_row, text),
        text_row: ext.text_row,
        char_w: ext.char_w,
        layout: ext.layout,
        row_w: ext.row_w,
    };
    diff_line(ui, &row, 0, 0, &line_ctx, text_rows)
}

/// Frame of an overlay over the center zone: the diff's card, the file viewer's.
pub(crate) fn overlay_card(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.bg_canvas)
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(RADIUS_CARD))
}

/// The icon opening an overlay header, set on a `tile` when one is given.
pub(crate) fn header_file_icon(ui: &mut egui::Ui, icon: FileIcon, tile: Option<egui::Color32>) {
    let (icon_rect, _) = ui.allocate_exact_size(
        egui::vec2(FILE_ICON_BOX, FILE_ICON_BOX),
        egui::Sense::hover(),
    );
    if let Some(fill) = tile {
        ui.painter()
            .rect_filled(icon_rect, egui::CornerRadius::same(6), fill);
    }
    let icon_box =
        egui::Rect::from_center_size(icon_rect.center(), egui::Vec2::splat(FILE_ICON_SIZE));
    icon.paint(ui.painter(), icon_box);
}

/// The file at `path` in an overlay header: its type's glyph (files.md §3.1), else
/// a plain text file.
pub(crate) fn header_icon_of(palette: &Palette, path: &str) -> FileIcon {
    match file_type(path) {
        Some(kind) => FileIcon::NerdFont {
            glyph: kind.glyph(),
            color: palette.file_type_color(kind),
        },
        None => FileIcon::Lucide {
            icon: lucide_icons::Icon::FileText,
            color: palette.text_secondary,
        },
    }
}

pub(crate) fn close_button(ui: &mut egui::Ui, palette: &Palette) -> bool {
    icon_button(
        ui,
        palette,
        lucide_icons::Icon::X,
        palette.text_primary,
        "Close",
    )
    .on_hover_text("Close (Esc)")
    .clicked()
}

const ZOOM_STEP: f32 = 1.25;
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;

/// Decoded image kept across frames for the diff view's preview. `texture` is `None`
/// when decoding failed — cached so the failure is not retried every frame.
pub(crate) struct ImagePreview {
    key: u64,
    texture: Option<egui::TextureHandle>,
    size: egui::Vec2,
    zoom: f32,
    /// Fit-to-viewport is applied once, on the first frame that knows the viewport.
    fitted: bool,
}

// `egui::TextureHandle` is not `Debug`; keep `DiffViewState`'s derive working without
// printing the handle.
impl std::fmt::Debug for ImagePreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImagePreview")
            .field("key", &self.key)
            .field("has_texture", &self.texture.is_some())
            .field("size", &self.size)
            .field("zoom", &self.zoom)
            .field("fitted", &self.fitted)
            .finish()
    }
}

fn decode_image(ctx: &egui::Context, blob: &ImageBlob) -> ImagePreview {
    let texture = image::load_from_memory(&blob.bytes).ok().map(|img| {
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        ctx.load_texture(
            format!("image-preview-{:x}", blob.fingerprint),
            color,
            egui::TextureOptions::LINEAR,
        )
    });
    let size = texture
        .as_ref()
        .map(|t| t.size_vec2())
        .unwrap_or(egui::Vec2::ZERO);
    ImagePreview {
        key: blob.fingerprint,
        texture,
        size,
        zoom: 1.0,
        fitted: false,
    }
}

/// Image preview replacing the binary placeholder (git.md §4): a zoomable, pannable
/// view of the new-side blob, decoded once into `decoded`. The toolbar sets discrete
/// zoom levels; a trackpad pinch or ⌘+scroll zooms; two-finger scroll pans the
/// surrounding scroll area.
pub(crate) fn image_preview(
    ui: &mut egui::Ui,
    palette: &Palette,
    blob: &ImageBlob,
    decoded: &mut Option<ImagePreview>,
) {
    if decoded.as_ref().map(|p| p.key) != Some(blob.fingerprint) {
        *decoded = Some(decode_image(ui.ctx(), blob));
    }
    let Some(preview) = decoded.as_mut() else {
        return;
    };
    let Some(texture) = preview.texture.clone() else {
        ui.label(
            egui::RichText::new("Image file — could not decode for preview")
                .size(LINE_SIZE)
                .color(palette.text_muted),
        );
        return;
    };

    let avail = ui.available_size();
    if !preview.fitted {
        preview.zoom = fit_zoom(preview.size, avail);
        preview.fitted = true;
    }

    ui.horizontal(|ui| {
        if intent_pill(ui, palette, "Fit", palette.text_secondary, true) {
            preview.zoom = fit_zoom(preview.size, avail);
        }
        if intent_pill(ui, palette, "100%", palette.text_secondary, true) {
            preview.zoom = 1.0;
        }
        if intent_pill(ui, palette, "−", palette.text_secondary, true) {
            preview.zoom = (preview.zoom / ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        ui.label(
            egui::RichText::new(format!("{:.0}%", preview.zoom * 100.0))
                .size(PILL_SIZE)
                .monospace()
                .color(palette.text_secondary),
        );
        if intent_pill(ui, palette, "+", palette.text_secondary, true) {
            preview.zoom = (preview.zoom * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        ui.label(
            egui::RichText::new(format!(
                "{}×{}",
                preview.size.x as u32, preview.size.y as u32
            ))
            .size(PILL_SIZE)
            .color(palette.text_muted),
        );
    });
    ui.add_space(8.0);

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if (zoom_delta - 1.0).abs() > f32::EPSILON {
                preview.zoom = (preview.zoom * zoom_delta).clamp(MIN_ZOOM, MAX_ZOOM);
            }
            let display = preview.size * preview.zoom;
            // Centre the image while it is smaller than the viewport; once it grows
            // past it the padding clamps to zero and the scroll area takes over.
            let pad = ((ui.available_size() - display) * 0.5).max(egui::Vec2::ZERO);
            ui.allocate_space(egui::vec2(0.0, pad.y));
            ui.horizontal(|ui| {
                ui.allocate_space(egui::vec2(pad.x, 0.0));
                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                    texture.id(),
                    display,
                )));
            });
        });
}

fn fit_zoom(size: egui::Vec2, avail: egui::Vec2) -> f32 {
    if size.x <= 0.0 || size.y <= 0.0 {
        return 1.0;
    }
    (avail.x / size.x)
        .min(avail.y / size.y)
        .clamp(MIN_ZOOM, 1.0)
}

/// The working-tree lines an inline editor on this hunk would replace: its **new side**
/// (the new side of an Unstaged diff *is* the working tree, git.md §4) widened by the
/// extended context displayed with it — those rows are editable too. 0-based and
/// half-open. The error is the reason to show: a refusal here is silent otherwise, and
/// the rows would keep offering a caret that never comes.
fn edit_range(
    diff: &FileDiff,
    hunk: &Hunk,
    ext: &ContextExtension,
) -> Result<Range<usize>, EditRefusal> {
    let first = if ext.above.is_empty() {
        hunk.new_start
    } else {
        ext.above.start
    };
    let last = if ext.below.is_empty() {
        hunk.new_start + hunk.new_lines
    } else {
        ext.below.end
    };
    // A hunk that only deletes lines has no new side at all: `new_lines == 0`, and its
    // `new_start` is the line it was deleted after.
    let (Some(start), Some(end)) = (first.checked_sub(1), last.checked_sub(1)) else {
        return Err(EditRefusal::DeletedLines);
    };
    let (start, end) = (start as usize, end as usize);
    if end <= start {
        return Err(EditRefusal::DeletedLines);
    }
    if end - start > MAX_EDIT_LINES {
        return Err(EditRefusal::TooManyLines);
    }
    if diff.source_lines.get(start..end).is_none() {
        // The new side was not loaded, or is shorter than the hunk claims: nothing the
        // view can name — the file itself is what refuses.
        return Err(EditRefusal::File);
    }
    Ok(start..end)
}

/// What the rows of this hunk answer a caret ask with (git.md §4).
fn caret_offer(editable: bool, diff: &FileDiff, hunk: &Hunk, ext: &ContextExtension) -> CaretOffer {
    if !editable {
        return CaretOffer::None;
    }
    match edit_range(diff, hunk, ext) {
        Ok(_) => CaretOffer::Open,
        Err(reason) => CaretOffer::Refuse(reason),
    }
}

/// Opens the inline editor on `hunk_idx`, caret on the row carrying the new-side line
/// `at` (the clicked row; `None` for a deletion row, which has no working-tree
/// counterpart) at column `col`. A diff that can't back an editor leaves the state
/// untouched — the click does nothing, as spec'd (git.md §4). Moving the caret to
/// another hunk is an exit like any other: the outgoing buffer is written first, in the
/// same gesture.
#[allow(clippy::too_many_arguments)]
fn open_hunk_editor(
    state: &mut DiffViewState,
    diff: &FileDiff,
    ext: &ContextExtension,
    hunk_idx: usize,
    at: Option<u32>,
    col: usize,
    staged: bool,
    intents: &mut Vec<GitIntent>,
) {
    let Some(hunk) = diff.hunks.get(hunk_idx) else {
        return;
    };
    let Ok(range) = edit_range(diff, hunk, ext) else {
        return;
    };
    let Some(original) = diff.source_lines.get(range.clone()).map(<[String]>::to_vec) else {
        return;
    };
    let row = at
        .and_then(|n| (n as usize).checked_sub(1))
        .and_then(|n| n.checked_sub(range.start))
        .unwrap_or(0)
        .min(original.len() - 1);
    state.text_selection = None;
    state.selection.clear();
    let target = EditTarget {
        path: diff.path.clone(),
        range,
        original,
        stage_after: staged,
        whole_file: false,
    };
    let caret = TextPosition { row, col };
    state
        .editing
        .open(InlineEdit::new(hunk_idx, target, caret), intents);
}

/// Hunk header band: `@@ … @@` on a surface background, controls on the right —
/// Stage/Unstage (outside read-only) and **Extend context** (+5, git.md §4;
/// returns `true` on click, also available read-only: a view action).
#[allow(clippy::too_many_arguments)]
fn hunk_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    staged: bool,
    read_only: bool,
    hunk_idx: usize,
    state: &DiffViewState,
    can_extend: bool,
    intents: &mut Vec<GitIntent>,
) -> bool {
    // What separates two hunks is a **hairline**, not a filled band: the
    // `@@ -19,7 +20,6 @@` line restated numbers every row's gutter already carries,
    // and once it is gone a band is a box drawn around a couple of controls.
    let seam = hunk_idx > 0;
    if read_only && !can_extend {
        // The first hunk has the file header right above it; a rule there would be the
        // second line in a row.
        if !seam {
            return false;
        }
        ui.add_space(HUNK_RULE_GAP);
        hunk_rule(ui, palette);
        ui.add_space(HUNK_RULE_GAP);
        return false;
    }
    if seam {
        ui.add_space(HUNK_RULE_GAP);
        hunk_rule(ui, palette);
    }
    let mut extend = false;
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(HUNK_BAND_PAD_X, HUNK_BAND_PAD_Y))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                // Extend context reads **left**, where the hunk's own rows begin: it
                // widens what is on screen rather than acting on the change, unlike the
                // staging pills it used to sit beside. Quiet, too — it is offered on
                // every hunk of every diff, so it must not read as loudly as staging.
                if can_extend && extend_link(ui, palette) {
                    extend = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !read_only {
                        let selected = state.selected_lines(hunk_idx);
                        // Active selection ⇒ stage/unstage the chosen lines;
                        // otherwise, the whole hunk.
                        let (label, intent): (&str, GitIntent) = if staged {
                            if selected.is_empty() {
                                ("Unstage hunk", GitIntent::UnstageHunk(hunk_idx))
                            } else {
                                (
                                    "Unstage lines",
                                    GitIntent::UnstageLines {
                                        hunk: hunk_idx,
                                        lines: selected,
                                    },
                                )
                            }
                        } else if selected.is_empty() {
                            ("Stage hunk", GitIntent::StageHunk(hunk_idx))
                        } else {
                            (
                                "Stage lines",
                                GitIntent::StageLines {
                                    hunk: hunk_idx,
                                    lines: selected,
                                },
                            )
                        };
                        let color = if staged {
                            palette.git_deleted
                        } else {
                            palette.git_added
                        };
                        if intent_pill(ui, palette, label, color, true) {
                            intents.push(intent);
                        }
                        // Discard reverts the working tree, so it is offered on the
                        // Unstaged side only (git.md §4); the app confirms it.
                        if !staged
                            && intent_pill(ui, palette, "Discard hunk", palette.git_deleted, true)
                        {
                            intents.push(GitIntent::DiscardHunk(hunk_idx));
                        }
                    }
                });
            });
        });
    extend
}

struct DiffLineCtx<'a> {
    palette: &'a Palette,
    staged: bool,
    read_only: bool,
    /// Review annotation available (the note icon shows on hover; a click opens
    /// the inline editor). `false` only at the non-review call sites (tests).
    review: bool,
    /// PR surface: render the second `MessageSquarePlus` gutter button (slot 0)
    /// for a forge review comment, alongside the agent Sparkles (slot 1).
    forge: bool,
    selected: bool,
    /// What this row's content column answers a caret ask with (git.md §4).
    caret: CaretOffer,
    highlighted: Option<&'a [HighlightedSpan]>,
    /// Columns this line differs from its counterpart on (git.md §4); empty when it
    /// has none, or when the two lines are too far apart to pair.
    changed: &'a [Columns],
    text_range: Option<(usize, usize)>,
    text_row: usize,
    char_w: f32,
    layout: RowLayout,
    /// Shared content width (widest line in the file), not the viewport width.
    row_w: f32,
}

/// Display data for a diff row: a hunk line or extended context. `text` is
/// already stripped of its line ending (`display_text`).
struct RowData<'a> {
    origin: LineOrigin,
    text: &'a str,
    old_lineno: Option<u32>,
    new_lineno: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffLineAction {
    ToggleSelection {
        hunk: usize,
        line: usize,
    },
    SelectText(TextSelection),
    ClearTextSelection,
    Intent(GitIntent),
    /// Review-mode click on a line (read-only ones included): open its note
    /// editor for `pool`, keyed by the line's `(old, new)` numbers.
    OpenComment {
        pool: ReviewPool,
        old: Option<u32>,
        new: Option<u32>,
    },
    /// Plain click in the content column of an editable row (git.md §4): open the
    /// inline editor on the hunk this row belongs to, caret at column `col`. The
    /// caller owns the hunk identity, so it resolves which row the caret lands on.
    OpenEditor {
        col: usize,
    },
    /// `Cmd+E` on a row whose hunk cannot back a buffer (git.md §4): the ask gets an
    /// answer with the reason, rather than nothing at all.
    RefuseEditor(EditRefusal),
}

/// What the content column of a row does with a caret ask (git.md §4). A row whose
/// hunk has no new side, or too many lines, still selects text like any other — it
/// simply says so when asked for a caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaretOffer {
    Open,
    Refuse(EditRefusal),
    /// Not an editable diff: the refusal belongs to the diff as a whole, which answers
    /// `Cmd+E` before the rows are drawn.
    None,
}

/// Where a pointer x lands on a row: the **number strip** (both number columns and the
/// sign) picks the line for partial staging, the **content** column carries the caret
/// (git.md §4). The action column left of the strip belongs to its hover buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowZone {
    Numbers,
    Content { col: usize },
}

fn row_zone(x: f32, left: f32, content_left: f32, char_w: f32) -> Option<RowZone> {
    if x >= left + LINE_ACTION_W && x < content_left {
        Some(RowZone::Numbers)
    } else if x >= content_left {
        Some(RowZone::Content {
            col: ((x - content_left) / char_w).floor().max(0.0) as usize,
        })
    } else {
        None
    }
}

fn diff_line(
    ui: &mut egui::Ui,
    row: &RowData<'_>,
    hunk_idx: usize,
    line_idx: usize,
    ctx: &DiffLineCtx<'_>,
    text_rows: &mut Vec<TextRow>,
) -> Option<DiffLineAction> {
    let full_w = ctx.row_w;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(full_w, LINE_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    let content_left = ctx.layout.content_left(rect.left());
    let text_len = row.text.chars().count();
    text_rows.push(TextRow {
        row: ctx.text_row,
        rect,
        content_left,
        char_w: ctx.char_w,
        text_len,
    });

    // Off-screen rows keep their geometry (allocated above, recorded in
    // `text_rows`) but skip the per-line paint + galley layout, so scrolling a
    // long file stays fluid. They can't be hovered or clicked, so no action.
    if !ui.clip_rect().intersects(rect) {
        return None;
    }

    // Read-only (commit diff, M9-7): no line is interactive — no selection, no
    // action button — it's history, not the current index.
    let selectable = !ctx.read_only && row.origin != LineOrigin::Context;
    let selected = selectable && ctx.selected;
    let (bg, fg) = match row.origin {
        LineOrigin::Addition => (with_alpha(ctx.palette.git_added, 30), ctx.palette.git_added),
        LineOrigin::Deletion => (
            with_alpha(ctx.palette.git_deleted, 30),
            ctx.palette.git_deleted,
        ),
        LineOrigin::Context => (egui::Color32::TRANSPARENT, ctx.palette.text_secondary),
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, bg);
    }
    paint_changed_columns(ui, rect, content_left, ctx, fg);
    if selected {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.5_f32, ctx.palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    if let Some((from, to)) = ctx.text_range {
        paint_text_selection(ui, ctx.palette, rect, content_left, ctx.char_w, from, to);
    }

    let click_position =
        text_click_position(&response, content_left, ctx.char_w, ctx.text_row, text_len);
    let line_action_clicked =
        selectable && line_action_button(ui, ctx.palette, rect, response.hovered(), ctx.staged);
    // Gutter note buttons (beside the stage button): the agent Sparkles on every
    // review line, plus a forge MessageSquarePlus at slot 0 on the PR surface — a
    // click opens the matching pool's inline editor without leaving a "review mode".
    let forge_clicked = ctx.review
        && ctx.forge
        && gutter_icon_button(
            ui,
            ctx.palette,
            rect,
            response.hovered(),
            0,
            lucide_icons::Icon::MessageSquarePlus,
            "Comment for review",
        );
    let agent_clicked = ctx.review
        && gutter_icon_button(
            ui,
            ctx.palette,
            rect,
            response.hovered(),
            1,
            lucide_icons::Icon::Sparkles,
            "Comment line",
        );
    let action = if response.triple_clicked() || response.double_clicked() {
        click_position
            .and_then(|at| clicked_selection(&response, at))
            .map(DiffLineAction::SelectText)
    } else if forge_clicked {
        Some(DiffLineAction::OpenComment {
            pool: ReviewPool::Forge,
            old: row.old_lineno,
            new: row.new_lineno,
        })
    } else if agent_clicked {
        Some(DiffLineAction::OpenComment {
            pool: ReviewPool::Agent,
            old: row.old_lineno,
            new: row.new_lineno,
        })
    } else if line_action_clicked {
        let intent = if ctx.staged {
            GitIntent::UnstageLines {
                hunk: hunk_idx,
                lines: vec![line_idx],
            }
        } else {
            GitIntent::StageLines {
                hunk: hunk_idx,
                lines: vec![line_idx],
            }
        };
        Some(DiffLineAction::Intent(intent))
    } else if response.clicked() {
        // The line pick lives on the number strip: a plain click in the content
        // column is the caret's gesture now (git.md §4).
        match response
            .interact_pointer_pos()
            .and_then(|pos| row_zone(pos.x, rect.left(), content_left, ctx.char_w))
        {
            Some(RowZone::Numbers) if selectable => Some(DiffLineAction::ToggleSelection {
                hunk: hunk_idx,
                line: line_idx,
            }),
            Some(RowZone::Content { col }) if ctx.caret == CaretOffer::Open => {
                Some(DiffLineAction::OpenEditor {
                    col: col.min(text_len),
                })
            }
            _ => Some(DiffLineAction::ClearTextSelection),
        }
    } else if ctx.caret != CaretOffer::None && response.hovered() && editor_requested(ui) {
        match ctx.caret {
            CaretOffer::Open => Some(DiffLineAction::OpenEditor { col: 0 }),
            CaretOffer::Refuse(reason) => Some(DiffLineAction::RefuseEditor(reason)),
            CaretOffer::None => None,
        }
    } else {
        None
    };
    // Cursor rule (design-system §4): the pick is a click target, the content column
    // is a text zone — and the text cursor is what announces the inline editor.
    if let Some(pos) = response.hover_pos() {
        match row_zone(pos.x, rect.left(), content_left, ctx.char_w) {
            Some(RowZone::Numbers) if selectable => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            Some(RowZone::Content { .. }) => ui.ctx().set_cursor_icon(egui::CursorIcon::Text),
            _ => {}
        }
    }

    let sign = match row.origin {
        LineOrigin::Addition => "+",
        LineOrigin::Deletion => "-",
        LineOrigin::Context => " ",
    };
    let center_y = rect.center().y;
    let num_font = egui::FontId::monospace(NUM_SIZE);
    if let Some(n) = row.old_lineno {
        ui.painter().text(
            egui::pos2(ctx.layout.old_right(rect.left()), center_y),
            egui::Align2::RIGHT_CENTER,
            n.to_string(),
            num_font.clone(),
            ctx.palette.text_muted,
        );
    }
    if let Some(n) = row.new_lineno {
        ui.painter().text(
            egui::pos2(ctx.layout.new_right(rect.left()), center_y),
            egui::Align2::RIGHT_CENTER,
            n.to_string(),
            num_font,
            ctx.palette.text_muted,
        );
    }
    ui.painter().text(
        egui::pos2(ctx.layout.sign_left(rect.left()), center_y),
        egui::Align2::LEFT_CENTER,
        sign,
        egui::FontId::monospace(LINE_SIZE),
        fg,
    );
    paint_line_content(ui, content_left, rect, row.text, ctx.highlighted, fg);

    let (old_lineno, new_lineno, text) = (row.old_lineno, row.new_lineno, row.text);
    response.widget_info(move || {
        let label = format!(
            "{} {} {sign}{}",
            old_lineno.map(|n| n.to_string()).unwrap_or_default(),
            new_lineno.map(|n| n.to_string()).unwrap_or_default(),
            text
        );
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
    });
    action
}

pub(crate) fn paint_line_content(
    ui: &mut egui::Ui,
    content_left: f32,
    rect: egui::Rect,
    text: &str,
    highlighted: Option<&[HighlightedSpan]>,
    fallback: egui::Color32,
) {
    let pos = egui::pos2(content_left, rect.center().y);
    let Some(spans) = highlighted else {
        ui.painter().text(
            pos,
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::monospace(LINE_SIZE),
            fallback,
        );
        return;
    };

    let mut job = egui::text::LayoutJob::default();
    for span in spans {
        job.append(
            &span.text,
            0.0,
            egui::text::TextFormat::simple(egui::FontId::monospace(LINE_SIZE), span.color),
        );
    }
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(pos.x, pos.y - galley.size().y / 2.0),
        galley,
        fallback,
    );
}

/// The columns that actually changed, over the row's own tint: on a line rewritten
/// in part, what reads first is the word that moved, not the whole line (git.md §4).
fn paint_changed_columns(
    ui: &mut egui::Ui,
    row: egui::Rect,
    content_left: f32,
    ctx: &DiffLineCtx<'_>,
    color: egui::Color32,
) {
    for columns in ctx.changed {
        let left = content_left + columns.start as f32 * ctx.char_w;
        let right = content_left + columns.end as f32 * ctx.char_w;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(egui::pos2(left, row.top()), egui::pos2(right, row.bottom())),
            egui::CornerRadius::ZERO,
            with_alpha(color, WORD_CHANGE_ALPHA),
        );
    }
}

fn update_text_selection(ui: &egui::Ui, state: &mut DiffViewState, rows: &[TextRow]) {
    let Some(selection) = dragged_selection(ui, rows) else {
        return;
    };
    if state.text_selection != Some(selection) {
        state.selection.clear();
        state.text_selection = Some(selection);
        ui.ctx().request_repaint();
    }
}

fn line_action_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: egui::Rect,
    row_hovered: bool,
    staged: bool,
) -> bool {
    let rect = egui::Rect::from_center_size(
        egui::pos2(
            row.left() + LINE_ACTION_LEFT + LINE_ACTION_SIZE / 2.0,
            row.center().y,
        ),
        egui::vec2(LINE_ACTION_SIZE, LINE_ACTION_SIZE),
    );
    let response = ui
        .interact(
            rect,
            ui.id().with((
                "line_action",
                staged,
                row.min.x.to_bits(),
                row.min.y.to_bits(),
            )),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let intent = if staged {
        palette.git_deleted
    } else {
        palette.git_added
    };
    let label = if staged { "Unstage line" } else { "Stage line" };
    if row_hovered || response.hovered() {
        let fill = if response.hovered() {
            with_alpha(intent, 36)
        } else {
            palette.bg_surface
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            fill,
            egui::Stroke::new(1.0_f32, intent),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if staged { "-" } else { "+" },
            egui::FontId::monospace(PILL_SIZE),
            intent,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.clicked()
}

/// Center of gutter button `slot` (0-based, left to right) on `row`, packed from
/// the left edge so the stage / comment / agent icons share the action column.
fn gutter_button_rect(row: egui::Rect, slot: usize) -> egui::Rect {
    let x = row.left()
        + LINE_ACTION_LEFT
        + slot as f32 * (LINE_ACTION_SIZE + LINE_ACTION_GAP)
        + LINE_ACTION_SIZE / 2.0;
    egui::Rect::from_center_size(
        egui::pos2(x, row.center().y),
        egui::vec2(LINE_ACTION_SIZE, LINE_ACTION_SIZE),
    )
}

/// A hover-only gutter icon button at `slot`; the icon is muted at rest and
/// tinted (with a hover fill) when pointed at. Returns `true` on click.
pub(crate) fn gutter_icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: egui::Rect,
    row_hovered: bool,
    slot: usize,
    icon: lucide_icons::Icon,
    label: &str,
) -> bool {
    let rect = gutter_button_rect(row, slot);
    let response = ui
        .interact(
            rect,
            ui.id()
                .with((label, row.min.x.to_bits(), row.min.y.to_bits())),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if row_hovered || response.hovered() {
        if response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(RADIUS_PILL),
                palette.bg_surface_hover,
            );
        }
        let color = if response.hovered() {
            palette.accent
        } else {
            palette.text_muted
        };
        crate::ui::paint_icon(ui.painter(), rect.center(), LINE_SIZE, icon, color);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_a_line_adds_then_removes_it_from_the_selection() {
        let diff = diff_of(vec![vec![
            (LineOrigin::Context, "ctx"),
            (LineOrigin::Addition, "add"),
        ]]);
        let mut state = DiffViewState::default();
        assert!(!state.selected(0, 1));
        state.toggle(&diff, 0, 1);
        assert!(state.selected(0, 1));
        assert_eq!(state.selected_lines(0), vec![1]);
        state.toggle(&diff, 0, 1);
        assert!(!state.selected(0, 1));
        assert!(state.selected_lines(0).is_empty());
    }

    #[test]
    fn selected_lines_are_sorted_and_scoped_per_hunk() {
        let diff = diff_of(vec![
            vec![(LineOrigin::Addition, "a"); 4],
            vec![(LineOrigin::Addition, "b"); 6],
        ]);
        let mut state = DiffViewState::default();
        state.toggle(&diff, 0, 3);
        state.toggle(&diff, 0, 1);
        state.toggle(&diff, 1, 5);
        assert_eq!(state.selected_lines(0), vec![1, 3]);
        assert_eq!(state.selected_lines(1), vec![5]);
        assert!(state.selected_lines(2).is_empty());
    }

    use crate::git::diff::DiffLine;

    fn diff_with(lines: Vec<LineOrigin>) -> FileDiff {
        diff_of(vec![lines.into_iter().map(|origin| (origin, "")).collect()])
    }

    /// Multi-hunk diff whose lines carry content — what a line selection is
    /// anchored on.
    fn diff_of(hunks: Vec<Vec<(LineOrigin, &str)>>) -> FileDiff {
        FileDiff {
            path: "f".into(),
            binary: false,
            oversize: false,
            hunks: hunks
                .into_iter()
                .map(|lines| Hunk {
                    header: String::new(),
                    old_start: 1,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 0,
                    lines: lines
                        .into_iter()
                        .map(|(origin, content)| DiffLine {
                            origin,
                            content: content.to_owned(),
                            old_lineno: None,
                            new_lineno: None,
                        })
                        .collect(),
                })
                .collect(),
            source_lines: Vec::new(),
            image: None,
            editable: false,
        }
    }

    /// Test diff for context extension: geometric hunks
    /// `(old_start, old_lines, new_start, new_lines)` over a file of `file_len`
    /// lines `l1..lN`.
    fn ext_diff(hunks: Vec<(u32, u32, u32, u32)>, file_len: usize) -> FileDiff {
        FileDiff {
            path: "f".into(),
            binary: false,
            oversize: false,
            hunks: hunks
                .into_iter()
                .map(|(old_start, old_lines, new_start, new_lines)| Hunk {
                    header: String::new(),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    lines: Vec::new(),
                })
                .collect(),
            source_lines: (1..=file_len).map(|n| format!("l{n}")).collect(),
            image: None,
            editable: false,
        }
    }

    fn amounts(pairs: &[(usize, u32)]) -> HashMap<usize, u32> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn context_extension_clamps_at_file_bounds() {
        let diff = ext_diff(vec![(5, 3, 5, 3)], 10);
        let ext = context_extensions(&diff, &amounts(&[(0, 5)]));
        assert_eq!(ext[0].above, 1..5, "5 requested but the file starts at 1");
        assert_eq!(ext[0].below, 8..11, "5 requested but the file ends at 10");
    }

    #[test]
    fn context_extension_is_empty_without_a_request() {
        let diff = ext_diff(vec![(5, 3, 5, 3)], 10);
        let ext = context_extensions(&diff, &HashMap::new());
        assert!(ext[0].above.is_empty());
        assert!(ext[0].below.is_empty());
    }

    #[test]
    fn context_extension_never_overlaps_the_neighbor_hunk() {
        let diff = ext_diff(vec![(5, 2, 5, 2), (10, 2, 10, 2)], 20);
        let ext = context_extensions(&diff, &amounts(&[(0, 5), (1, 5)]));
        // Hunk 0's lower extension stops before hunk 1; hunk 1's upper extension
        // starts after the lines already shown — no duplicate.
        assert_eq!(ext[0].below, 7..10);
        assert!(ext[1].above.is_empty());
        assert_eq!(ext[1].below, 12..17);
    }

    #[test]
    fn context_extension_between_hunks_fills_the_gap_top_down() {
        let diff = ext_diff(vec![(5, 2, 5, 2), (10, 2, 10, 2)], 20);
        let ext = context_extensions(&diff, &amounts(&[(1, 5)]));
        assert_eq!(
            ext[1].above,
            7..10,
            "clamped to the end of the previous hunk"
        );
    }

    #[test]
    fn context_extension_skips_a_new_side_less_hunk_and_an_empty_file() {
        let deletion_only = ext_diff(vec![(1, 3, 0, 0)], 10);
        let ext = context_extensions(&deletion_only, &amounts(&[(0, 5)]));
        assert_eq!(ext[0], ContextExtension::default());

        let no_source = ext_diff(vec![(5, 3, 5, 3)], 0);
        let ext = context_extensions(&no_source, &amounts(&[(0, 5)]));
        assert_eq!(ext[0], ContextExtension::default());
    }

    #[test]
    fn can_extend_reflects_remaining_context() {
        let extendable = |diff: &FileDiff, amounts: &HashMap<usize, u32>, hunk: usize| {
            can_extend(diff, amounts, &context_extensions(diff, amounts), hunk)
        };
        let diff = ext_diff(vec![(5, 3, 5, 3)], 10);
        assert!(extendable(&diff, &HashMap::new(), 0));
        assert!(
            !extendable(&diff, &amounts(&[(0, 10)]), 0),
            "the whole file is shown: nothing to extend"
        );

        let full = ext_diff(vec![(1, 4, 1, 4)], 4);
        assert!(
            !extendable(&full, &HashMap::new(), 0),
            "the hunk already covers the whole file"
        );
    }

    #[test]
    fn extension_linenos_map_to_the_old_side_with_the_hunk_offset() {
        let diff = ext_diff(vec![(10, 3, 12, 5)], 30);
        let hunk = &diff.hunks[0];
        // Above: old_start − new_start offset; below: offset of the ends.
        assert_eq!(above_old_lineno(hunk, 11), 9);
        assert_eq!(below_old_lineno(hunk, 17), 13);
    }

    #[test]
    fn display_rows_interleave_extensions_in_render_order() {
        let mut diff = ext_diff(vec![(4, 2, 4, 2)], 8);
        diff.hunks[0].lines = vec![
            DiffLine {
                origin: LineOrigin::Context,
                content: "ctx\n".into(),
                old_lineno: Some(4),
                new_lineno: Some(4),
            },
            DiffLine {
                origin: LineOrigin::Addition,
                content: "add\n".into(),
                old_lineno: None,
                new_lineno: Some(5),
            },
        ];
        let rows = display_rows(&diff, &amounts(&[(0, 5)]));
        assert_eq!(rows, vec!["l1", "l2", "l3", "ctx", "add", "l6", "l7", "l8"]);
    }

    #[test]
    fn diff_line_stats_count_additions_and_deletions() {
        let diff = diff_with(vec![
            LineOrigin::Context,
            LineOrigin::Addition,
            LineOrigin::Addition,
            LineOrigin::Deletion,
        ]);
        assert_eq!(diff_line_stats(&diff), (2, 1));
    }

    #[test]
    fn reconcile_drops_extensions_of_vanished_hunks_without_flagging_stale() {
        let mut state = DiffViewState::default();
        state.extend(0);
        state.extend(3);
        let diff = diff_with(vec![LineOrigin::Addition]);

        assert!(!state.reconcile(&diff));
        assert!(!state.is_stale(), "a lost extension does not flag stale");
        assert_eq!(state.extensions.len(), 1);
        assert_eq!(state.extensions.get(&0), Some(&EXTEND_STEP));
    }

    #[test]
    fn reconcile_keeps_a_still_valid_selection_without_flagging_stale() {
        let diff = diff_of(vec![vec![
            (LineOrigin::Context, "ctx"),
            (LineOrigin::Addition, "add"),
        ]]);
        let mut state = DiffViewState::default();
        state.toggle(&diff, 0, 1);

        assert!(!state.reconcile(&diff));
        assert!(!state.is_stale());
        assert_eq!(state.selected_lines(0), vec![1]);
    }

    #[test]
    fn reconcile_drops_an_out_of_range_selection_and_flags_stale() {
        let mut state = DiffViewState::default();
        state.toggle(&diff_of(vec![vec![(LineOrigin::Addition, "add")]; 3]), 2, 0);
        let diff = diff_with(vec![LineOrigin::Addition]);

        assert!(state.reconcile(&diff));
        assert!(state.is_stale());
        assert!(state.selected_lines(2).is_empty());
    }

    #[test]
    fn reconcile_drops_a_selection_that_became_context() {
        let mut state = DiffViewState::default();
        state.toggle(&diff_with(vec![LineOrigin::Addition]), 0, 0);
        let diff = diff_with(vec![LineOrigin::Context]);

        assert!(state.reconcile(&diff));
        assert!(state.is_stale());
    }

    #[test]
    fn reconcile_drops_a_selection_whose_line_content_changed() {
        let picked = diff_of(vec![vec![
            (LineOrigin::Context, "fn main() {"),
            (LineOrigin::Addition, "    new();"),
        ]]);
        let mut state = DiffViewState::default();
        state.toggle(&picked, 0, 1);
        // Same shape, same origins: only the content of the picked line moved on.
        let reloaded = diff_of(vec![vec![
            (LineOrigin::Context, "fn main() {"),
            (LineOrigin::Addition, "    something_else();"),
        ]]);

        assert!(state.reconcile(&reloaded));
        assert!(state.is_stale());
        assert!(state.selected_lines(0).is_empty());
    }

    #[test]
    fn reconcile_drops_a_selection_whose_line_flipped_side() {
        let picked = diff_of(vec![vec![
            (LineOrigin::Context, "fn main() {"),
            (LineOrigin::Addition, "    new();"),
        ]]);
        let mut state = DiffViewState::default();
        state.toggle(&picked, 0, 1);
        // Same shape, same text: staging it now would remove the line the user
        // picked to add.
        let reloaded = diff_of(vec![vec![
            (LineOrigin::Context, "fn main() {"),
            (LineOrigin::Deletion, "    new();"),
        ]]);

        assert!(state.reconcile(&reloaded));
        assert!(state.is_stale());
        assert!(state.selected_lines(0).is_empty());
    }
}
