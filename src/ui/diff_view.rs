use std::collections::{HashMap, HashSet};

use crate::git::diff::{FileDiff, Hunk, ImageBlob, LineOrigin};
use crate::review::{count, FileComments, ForgeThreads, LineComment, ReviewIntent, ReviewPool};
use crate::theme::{Palette, PILL_SIZE, RADIUS_BUTTON, RADIUS_CARD, RADIUS_PILL, TITLE_SIZE};
use crate::ui::git_panel::{intent_pill, GitIntent};
use crate::ui::syntax_highlight::{display_text, HighlightedDiffCache, HighlightedSpan};
use crate::ui::with_alpha;

/// Per-hunk line selection, kept across frames. The key is the hunk index in
/// `FileDiff::hunks`; the value, the set of chosen line indices in `Hunk::lines`.
/// Empty ⇒ the whole hunk (the Stage/Unstage hunk buttons take precedence).
#[derive(Debug, Default)]
pub struct DiffViewState {
    selection: HashMap<usize, HashSet<usize>>,
    /// Extended context per hunk (git.md §4): number of extra context lines
    /// requested above **and** below (multiples of `EXTEND_STEP`). Display only —
    /// never enters staging.
    extensions: HashMap<usize, u32>,
    text_selection: Option<TextSelection>,
    syntax_cache: Option<HighlightedDiffCache>,
    /// Raised when a diff reload (disk change, git.md §8) dropped a selection that
    /// became invalid; the overlay shows it as a banner until the next interaction.
    stale: bool,
    /// Decoded preview of an image file, kept across frames and re-decoded only when
    /// the underlying blob changes (`ImageBlob::fingerprint`). git.md §4.
    image: Option<ImagePreview>,
    /// Line whose review note editor is open (M-RC), keyed by its pool and its
    /// `(old, new)` line numbers; `comment_buffer` holds the in-progress text.
    /// The pool distinguishes the PR surface's two gutter buttons (forge vs agent)
    /// so each opens its own editor on the same line. Cleared on validate, `Esc`,
    /// or when the open file changes.
    active_comment: Option<(ReviewPool, Option<u32>, Option<u32>)>,
    comment_buffer: String,
    /// Comment being edited from the review recap popover (M-RC), keyed by
    /// `(file, line_ref)`; `popover_buffer` holds its in-progress text.
    popover_edit: Option<(String, Option<u32>)>,
    popover_buffer: String,
    /// One-shot: focus the note editor on its next frame (set when an editor
    /// opens so the caret lands in the field without an extra click).
    note_focus: bool,
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
        self.stale = false;
        self.image = None;
        self.active_comment = None;
        self.comment_buffer.clear();
        self.popover_edit = None;
        self.popover_buffer.clear();
        self.note_focus = false;
        self.reveal_line = None;
        self.active_reply = None;
        self.reply_buffer.clear();
        self.conversation_edit = None;
        self.conversation_buffer.clear();
        self.conversation_add_buffer.clear();
        self.expanded_resolved.clear();
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
        self.note_focus = true;
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
        (&mut self.reply_buffer, &mut self.note_focus)
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
        self.note_focus = true;
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
        (&mut self.conversation_buffer, &mut self.note_focus)
    }

    /// Reconciles the selection with a freshly reloaded diff: drops the (hunk,
    /// line) pairs now out of bounds or turned back into context. Returns `true`
    /// if a selection was lost (to signal to the user, git.md §8).
    pub fn reconcile(&mut self, diff: &FileDiff) -> bool {
        let mut dropped = false;
        self.selection.retain(|&hunk, lines| {
            let Some(h) = diff.hunks.get(hunk) else {
                dropped = true;
                return false;
            };
            let before = lines.len();
            lines.retain(|&line| {
                h.lines
                    .get(line)
                    .is_some_and(|l| l.origin != LineOrigin::Context)
            });
            dropped |= lines.len() != before;
            !lines.is_empty()
        });
        self.stale |= dropped;
        // Display only: an orphaned extension is dropped without flagging stale.
        self.extensions.retain(|&hunk, _| hunk < diff.hunks.len());
        self.reconcile_text_selection(diff);
        dropped
    }

    fn extend(&mut self, hunk: usize) {
        *self.extensions.entry(hunk).or_insert(0) += EXTEND_STEP;
    }

    fn is_stale(&self) -> bool {
        self.stale
    }

    fn toggle(&mut self, hunk: usize, line: usize) {
        self.stale = false;
        self.text_selection = None;
        let set = self.selection.entry(hunk).or_default();
        if !set.insert(line) {
            set.remove(&line);
        }
    }

    fn selected(&self, hunk: usize, line: usize) -> bool {
        self.selection.get(&hunk).is_some_and(|s| s.contains(&line))
    }

    fn selected_lines(&self, hunk: usize) -> Vec<usize> {
        let mut lines: Vec<usize> = self
            .selection
            .get(&hunk)
            .map(|s| s.iter().copied().collect())
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

    fn ensure_syntax_cache(&mut self, diff: &FileDiff, syntax_theme: &'static str) {
        if self
            .syntax_cache
            .as_ref()
            .is_some_and(|cache| cache.is_current(diff, syntax_theme))
        {
            return;
        }
        self.syntax_cache = HighlightedDiffCache::for_diff(diff, syntax_theme);
    }

    fn syntax_line(&self, hunk: usize, line: usize) -> Option<&[HighlightedSpan]> {
        self.syntax_cache.as_ref()?.line(hunk, line)
    }
}

const LINE_SIZE: f32 = 12.0;
const HUNK_HEADER_SIZE: f32 = 11.0;
const LINE_PAD_X: f32 = 8.0;
/// Breathing room kept after the longest line so it isn't flush against the
/// right edge once scrolled fully right.
const CONTENT_TRAILING_PAD: f32 = 24.0;
const LINE_HEIGHT: f32 = 17.0;
const LINE_ACTION_SIZE: f32 = 14.0;
const LINE_ACTION_LEFT: f32 = 4.0;
/// Gap between the stage and review-note icons sharing the gutter.
const LINE_ACTION_GAP: f32 = 4.0;
/// Column reserved for the per-line stage/unstage button and, beside it, the
/// review-note (✦) button — left of the numbers.
const LINE_ACTION_W: f32 = 40.0;
/// Size of the gutter line numbers (more subdued than the content).
const NUM_SIZE: f32 = 11.0;
/// Inner padding of each number column.
const NUM_PAD_X: f32 = 6.0;
/// Column of the +/− sign between the gutter and the content.
const SIGN_W: f32 = 16.0;
/// Context lines added above **and** below per Extend click (git.md §4).
const EXTEND_STEP: u32 = 5;
const FILE_ICON_BOX: f32 = 24.0;
const FILE_ICON_SIZE: f32 = 14.0;
const HUNK_BAND_PAD_X: i8 = 8;
const HUNK_BAND_PAD_Y: i8 = 4;
const TEXT_DRAG_THRESHOLD: f32 = 2.0;
const TEXT_SELECTION_ALPHA: u8 = 70;
/// Lines of the commented hunk previewed atop an overlay thread (pull-requests.md §5).
const OVERLAY_SNIPPET_LINES: usize = 3;
/// Extra indent a reply nests under its thread root in the overlay (§11).
const REPLY_NEST_INDENT: f32 = 14.0;

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
}

/// X offset of a line's content from the left edge of its row — exposed so UI
/// tests can aim at a precise text column.
pub fn content_x_offset(diff: &FileDiff, char_w: f32) -> f32 {
    RowLayout::for_diff(diff, char_w).content_left(0.0)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TextPosition {
    row: usize,
    col: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TextSelectionMode {
    Char,
    Word,
    Line,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct TextSelection {
    anchor: TextPosition,
    head: TextPosition,
    mode: TextSelectionMode,
}

impl TextSelection {
    fn ordered(self) -> (TextPosition, TextPosition) {
        if self.head < self.anchor {
            (self.head, self.anchor)
        } else {
            (self.anchor, self.head)
        }
    }

    fn is_empty(self) -> bool {
        self.mode == TextSelectionMode::Char && self.anchor == self.head
    }

    fn range_for_row(self, row: usize, text: &str) -> Option<(usize, usize)> {
        if self.is_empty() {
            return None;
        }
        let text_len = text.chars().count();
        if text_len == 0 {
            return None;
        }
        let (start, end) = self.ordered();
        if row < start.row || row > end.row {
            return None;
        }
        let (mut from, mut to) = match self.mode {
            TextSelectionMode::Line => (0, text_len),
            TextSelectionMode::Word if row == start.row && row == end.row => {
                word_bounds(text, start.col)
            }
            TextSelectionMode::Word if row == start.row => {
                (word_bounds(text, start.col).0, text_len)
            }
            TextSelectionMode::Word if row == end.row => (0, word_bounds(text, end.col).1),
            TextSelectionMode::Word => (0, text_len),
            TextSelectionMode::Char => {
                let from = if row == start.row {
                    start.col.min(text_len)
                } else {
                    0
                };
                let to = if row == end.row {
                    end.col.saturating_add(1).min(text_len)
                } else {
                    text_len
                };
                (from, to)
            }
        };
        from = from.min(text_len);
        to = to.min(text_len);
        (from < to).then_some((from, to))
    }

    fn clamped_to(self, lines: &[&str]) -> Option<Self> {
        if lines.is_empty() || self.anchor.row >= lines.len() || self.head.row >= lines.len() {
            return None;
        }
        Some(Self {
            anchor: clamp_text_position(self.anchor, lines),
            head: clamp_text_position(self.head, lines),
            mode: self.mode,
        })
    }
}

#[derive(Debug, Copy, Clone)]
struct TextRow {
    row: usize,
    rect: egui::Rect,
    content_left: f32,
    char_w: f32,
    text_len: usize,
}

fn clamp_text_position(position: TextPosition, lines: &[&str]) -> TextPosition {
    let text_len = lines[position.row].chars().count();
    TextPosition {
        row: position.row,
        col: position.col.min(text_len.saturating_sub(1)),
    }
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
fn can_extend(diff: &FileDiff, amounts: &HashMap<usize, u32>, hunk: usize) -> bool {
    let mut more = amounts.clone();
    *more.entry(hunk).or_insert(0) += EXTEND_STEP;
    context_extensions(diff, &more) != context_extensions(diff, amounts)
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
    if selection.is_empty() {
        return None;
    }
    let lines = display_rows(diff, amounts);
    let selection = selection.clamped_to(&lines)?;
    let (start, end) = selection.ordered();
    let mut out = String::new();
    for (row, text) in lines.iter().enumerate().take(end.row + 1).skip(start.row) {
        if row != start.row {
            out.push('\n');
        }
        let Some((from, to)) = selection.range_for_row(row, text) else {
            continue;
        };
        out.push_str(slice_chars(text, from, to));
    }
    (!out.is_empty()).then_some(out)
}

fn word_bounds(text: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let col = col.min(chars.len() - 1);
    if !is_word_char(chars[col]) {
        return (col, col + 1);
    }
    let mut start = col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col + 1;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    (start, end)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~')
}

fn slice_chars(text: &str, from: usize, to: usize) -> &str {
    let start = char_byte_index(text, from);
    let end = char_byte_index(text, to);
    &text[start..end]
}

fn char_byte_index(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSurface {
    /// Working tree: per-hunk / per-line staging; `staged` picks the direction
    /// (the index unstages, the worktree stages).
    WorkingTree { staged: bool },
    /// Working tree, showing the **previously** open file frozen while the
    /// requested one loads: the hunks on screen belong to another path, so the
    /// granular controls stay out until the requested diff arrives.
    WorkingTreeFrozen,
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

    fn staged(self) -> bool {
        matches!(self, DiffSurface::WorkingTree { staged: true })
    }

    /// The PR surface carries a forge pool alongside the agent pool: its gutter
    /// gets a second `MessageSquarePlus` button (slot 0) feeding the forge review
    /// comments posted to GitHub / Bitbucket on submit.
    fn forge_review(self) -> bool {
        matches!(self, DiffSurface::PrReview)
    }
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
    let staged = surface.staged();
    let read_only = surface.read_only();
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

    // First `Esc` cancels an open note editor (inline or popover); a second one
    // closes the diff.
    let mut close = false;
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if state.active_comment.is_some() || state.popover_edit.is_some() {
            state.active_comment = None;
            state.comment_buffer.clear();
            state.popover_edit = None;
            state.popover_buffer.clear();
        } else {
            close = true;
        }
    }
    if copy_requested(ui) {
        if let Some(text) = state.selected_text(diff) {
            ui.ctx().copy_text(text);
        }
    }

    let frame = egui::Frame::new()
        .fill(palette.bg_canvas)
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(RADIUS_CARD));

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let (icon_rect, _) = ui.allocate_exact_size(
                egui::vec2(FILE_ICON_BOX, FILE_ICON_BOX),
                egui::Sense::hover(),
            );
            ui.painter()
                .rect_filled(icon_rect, egui::CornerRadius::same(6), palette.bg_surface);
            crate::ui::paint_icon(
                ui.painter(),
                icon_rect.center(),
                FILE_ICON_SIZE,
                lucide_icons::Icon::FileText,
                palette.text_secondary,
            );
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
                if close_button(ui, palette) {
                    close = true;
                }
                let n = count(review_comments);
                if review_available && n > 0 {
                    let chip = review_chip(ui, palette, n);
                    egui::Popup::from_toggle_button_response(&chip)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            review_popover(
                                ui,
                                palette,
                                review_agent,
                                review_comments,
                                state,
                                &mut review_out,
                            );
                        });
                }
            });
        });
        ui.add_space(8.0);

        if state.is_stale() {
            ui.label(
                egui::RichText::new("File changed on disk — selection no longer applies")
                    .size(LINE_SIZE)
                    .color(palette.git_modified),
            );
            ui.add_space(8.0);
        }

        if diff.binary {
            match &diff.image {
                Some(blob) => image_preview(ui, palette, blob, &diff.path, state),
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

        state.ensure_syntax_cache(diff, palette.syntax);
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
        let max_chars = display_rows(diff, &state.extensions)
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        let content_width =
            layout.content_left(0.0) + max_chars as f32 * char_w + CONTENT_TRAILING_PAD;
        let mut extend_requests = Vec::new();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let row_w = content_width.max(ui.available_width());
                let mut text_rows = Vec::new();
                let mut text_row = 0;
                for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
                    if hunk_header(
                        ui,
                        palette,
                        &hunk.header,
                        staged,
                        read_only,
                        hunk_idx,
                        state,
                        can_extend(diff, &state.extensions, hunk_idx),
                        intents,
                    ) {
                        extend_requests.push(hunk_idx);
                    }
                    ui.add_space(4.0);
                    let previous_spacing_y = ui.spacing().item_spacing.y;
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let ext = extensions[hunk_idx].clone();
                    for new_no in ext.above {
                        let action = extension_line(
                            ui,
                            ExtensionRow {
                                diff,
                                old_no: above_old_lineno(hunk, new_no),
                                new_no,
                                staged,
                                read_only,
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
                        apply_line_action(state, intents, action);
                    }
                    for (line_idx, line) in hunk.lines.iter().enumerate() {
                        let text = display_text(&line.content);
                        if state.reveal_line.is_some() && line.new_lineno == state.reveal_line {
                            ui.scroll_to_cursor(Some(egui::Align::Center));
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
                                highlighted: state.syntax_line(hunk_idx, line_idx),
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
                                open_inline_editor(state, pool, store, &diff.path, old, new);
                            }
                            other => apply_line_action(state, intents, other),
                        }
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
                        if review_forge {
                            comment_block(
                                ui,
                                palette,
                                &diff.path,
                                line.old_lineno,
                                line.new_lineno,
                                text,
                                ReviewPool::Forge,
                                review_forge_store,
                                state,
                                &mut review_out,
                                0.0,
                            );
                        }
                        comment_block(
                            ui,
                            palette,
                            &diff.path,
                            line.old_lineno,
                            line.new_lineno,
                            text,
                            ReviewPool::Agent,
                            review_comments,
                            state,
                            &mut review_out,
                            0.0,
                        );
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
                        apply_line_action(state, intents, action);
                    }
                    ui.spacing_mut().item_spacing.y = previous_spacing_y;
                    ui.add_space(12.0);
                }
                update_text_selection(ui, state, &text_rows);
            });
        for hunk in extend_requests {
            state.extend(hunk);
            ui.ctx().request_repaint();
        }
    });

    if let Some(r) = review {
        r.intents.append(&mut review_out);
    }
    close
}

/// Stored note anchored at the `(old, new)` row of `path`, if any. Matches the full
/// pair — not `line_ref()` — so a deleted row (old N) and an added row (new N) sharing
/// a number don't collide and render the same note twice.
fn note_at(
    comments: &FileComments,
    path: &str,
    old: Option<u32>,
    new: Option<u32>,
) -> Option<String> {
    comments
        .get(path)?
        .iter()
        .find(|c| c.old_lineno == old && c.new_lineno == new)
        .map(|c| c.note.clone())
}

/// Opens the inline note editor on a diff line, prefilled with its stored note,
/// and focuses the field. Closes any popover edit so a single editor is live.
fn open_inline_editor(
    state: &mut DiffViewState,
    pool: ReviewPool,
    comments: &FileComments,
    path: &str,
    old: Option<u32>,
    new: Option<u32>,
) {
    state.comment_buffer = note_at(comments, path, old, new).unwrap_or_default();
    state.active_comment = Some((pool, old, new));
    state.popover_edit = None;
    state.note_focus = true;
}

/// Outcome of a note editor frame.
enum NoteEdit {
    Idle,
    Delete,
    Save,
}

/// Visual identity of a review pool — the color, icon, header label and editor
/// hint that tell a forge review comment (`accent`) apart from an agent note
/// (`accent_ai`) wherever the two share the diff gutter (pull-requests.md §11).
struct PoolStyle {
    color: egui::Color32,
    icon: lucide_icons::Icon,
    hint: &'static str,
}

fn pool_style(palette: &Palette, pool: ReviewPool) -> PoolStyle {
    match pool {
        ReviewPool::Forge => PoolStyle {
            color: palette.accent,
            icon: lucide_icons::Icon::MessageSquarePlus,
            hint: "Leave a review comment…",
        },
        ReviewPool::Agent => PoolStyle {
            color: palette.accent_ai,
            icon: lucide_icons::Icon::Sparkles,
            hint: "Describe what the agent should inspect…",
        },
    }
}

/// Shared note field: a multiline input (Enter validates, Shift+Enter inserts a
/// newline) with a compact Delete / validate footer underneath. Clicking outside
/// the field also validates. `focus` is a one-shot that lands the caret in the
/// field the frame the editor opens; `style` colors the caret/selection and the
/// validate button to the pool's identity.
fn note_editor(
    ui: &mut egui::Ui,
    palette: &Palette,
    style: &PoolStyle,
    buffer: &mut String,
    focus: &mut bool,
    width: f32,
) -> NoteEdit {
    // Consume the bare Enter before the field sees it so it validates instead of
    // inserting a newline; Shift+Enter falls through to the field as a newline.
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
    let response = ui
        .scope(|ui| {
            let radius = egui::CornerRadius::same(RADIUS_BUTTON);
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.corner_radius = radius;
            w.hovered.corner_radius = radius;
            w.active.corner_radius = radius;
            ui.visuals_mut().selection.stroke = egui::Stroke::new(1.5, style.color);
            ui.add(
                egui::TextEdit::multiline(buffer)
                    .desired_rows(2)
                    .desired_width(width)
                    .hint_text(style.hint),
            )
        })
        .inner;
    if *focus {
        response.request_focus();
        *focus = false;
    }
    // A click outside the field (it loses focus) validates, like Enter or ✓.
    let mut edit = if submit_key || response.lost_focus() {
        NoteEdit::Save
    } else {
        NoteEdit::Idle
    };
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if icon_button(
            ui,
            palette,
            lucide_icons::Icon::Check,
            style.color,
            "Validate note",
        ) {
            edit = NoteEdit::Save;
        }
        ui.add_space(2.0);
        if icon_button(
            ui,
            palette,
            lucide_icons::Icon::Trash2,
            palette.git_deleted,
            "Delete note",
        ) {
            edit = NoteEdit::Delete;
        }
    });
    edit
}

/// Small square icon button (hover-tinted) used for the note editor and popover
/// controls; `label` is its accessibility name.
fn icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: lucide_icons::Icon,
    color: egui::Color32,
    label: &str,
) -> bool {
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
    response.clicked()
}

/// What the reply editor raised this frame (pull-requests.md §11).
pub(crate) enum ReplyEdit {
    Idle,
    Send,
    Cancel,
}

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
    let response = ui
        .scope(|ui| {
            let radius = egui::CornerRadius::same(RADIUS_BUTTON);
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.corner_radius = radius;
            w.hovered.corner_radius = radius;
            w.active.corner_radius = radius;
            ui.visuals_mut().selection.stroke = egui::Stroke::new(1.5, palette.accent);
            ui.add(
                egui::TextEdit::multiline(buffer)
                    .desired_rows(2)
                    .desired_width(width)
                    .hint_text(labels.hint),
            )
        })
        .inner;
    if *focus {
        response.request_focus();
        *focus = false;
    }
    let mut edit = if submit_key {
        ReplyEdit::Send
    } else {
        ReplyEdit::Idle
    };
    ui.add_space(4.0);
    // Pin the footer to its own line height: a bare `right_to_left` layout inherits
    // the parent's full remaining height and would vertically center the buttons far
    // below the field when the editor sits high in a tall scroll area (the center
    // inline card), pulling them out of clicking range.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), LINE_HEIGHT),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if icon_button(
                ui,
                palette,
                lucide_icons::Icon::Check,
                palette.accent,
                labels.send,
            ) {
                edit = ReplyEdit::Send;
            }
            ui.add_space(2.0);
            if icon_button(
                ui,
                palette,
                lucide_icons::Icon::X,
                palette.text_muted,
                labels.cancel,
            ) {
                edit = ReplyEdit::Cancel;
            }
        },
    );
    edit
}

/// Either the open inline editor (when this line is active) or the saved note as
/// a clickable card (clicking it re-opens the editor), rendered left-aligned just
/// below its diff line.
#[allow(clippy::too_many_arguments)]
fn comment_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    old: Option<u32>,
    new: Option<u32>,
    code: &str,
    pool: ReviewPool,
    comments: &FileComments,
    state: &mut DiffViewState,
    review_out: &mut Vec<ReviewIntent>,
    indent: f32,
) {
    let line = new.or(old);
    let style = pool_style(palette, pool);
    if state.active_comment == Some((pool, old, new)) {
        ui.add_space(3.0);
        let mut edit = NoteEdit::Idle;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| {
                let width = (ui.available_width() - 8.0).max(160.0);
                edit = note_editor(
                    ui,
                    palette,
                    &style,
                    &mut state.comment_buffer,
                    &mut state.note_focus,
                    width,
                );
            });
        });
        ui.add_space(3.0);
        match edit {
            NoteEdit::Save => {
                save_note(
                    review_out,
                    pool,
                    path,
                    old,
                    new,
                    code,
                    state.comment_buffer.trim(),
                );
                state.active_comment = None;
                state.comment_buffer.clear();
            }
            NoteEdit::Delete => {
                review_out.push(ReviewIntent::DeleteComment {
                    pool,
                    file: path.to_owned(),
                    line,
                });
                state.active_comment = None;
                state.comment_buffer.clear();
            }
            NoteEdit::Idle => {}
        }
    } else if let Some(note) = note_at(comments, path, old, new) {
        ui.add_space(3.0);
        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            clicked = note_card(ui, palette, &style, &note, line);
        });
        ui.add_space(3.0);
        if clicked {
            open_inline_editor(state, pool, comments, path, old, new);
        }
    }
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
        for (idx, comment) in thread.iter().enumerate() {
            ui.add_space(2.0);
            let ask_label =
                (!agent.is_empty() && idx + 1 == thread.len()).then(|| format!("Ask {agent}"));
            let mut ask_clicked = false;
            ui.horizontal(|ui| {
                // Replies nest a step in under the thread root (pull-requests.md §11).
                ui.add_space(indent + if idx == 0 { 0.0 } else { REPLY_NEST_INDENT });
                ask_clicked = thread_card(
                    ui,
                    palette,
                    path,
                    comment,
                    ask_label.as_deref(),
                    (idx == 0).then_some(snippet.as_deref()).flatten(),
                    now,
                );
            });
            if ask_clicked {
                out.push(ReviewIntent::AskAgentOnThread {
                    file: path.to_owned(),
                    old: anchor.0,
                    new: anchor.1,
                });
            }
        }
        let resolved = thread.first().is_some_and(|c| c.resolved);
        let thread_id = thread.iter().find_map(|c| c.thread_id.clone());
        if let Some(reply_id) = thread.iter().find_map(|c| c.id) {
            reply_block(
                ui, palette, state, reply_id, resolved, thread_id, out, indent,
            );
        }
        ui.add_space(2.0);
    }
}

/// The reply affordance under a thread: a "Reply" pill that swaps to the inline
/// reply editor for thread `reply_id` when clicked, raising `ReplyToThread` on send.
#[allow(clippy::too_many_arguments)]
fn reply_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut DiffViewState,
    reply_id: u64,
    resolved: bool,
    thread_id: Option<String>,
    out: &mut Vec<ReviewIntent>,
    indent: f32,
) {
    if state.active_reply == Some(reply_id) {
        ui.add_space(3.0);
        let mut edit = ReplyEdit::Idle;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| {
                let width = (ui.available_width() - 8.0).max(160.0);
                edit = reply_editor(
                    ui,
                    palette,
                    &mut state.reply_buffer,
                    &mut state.note_focus,
                    width,
                    &REPLY_LABELS,
                );
            });
        });
        match edit {
            ReplyEdit::Send => {
                let body = state.reply_buffer.trim().to_owned();
                if !body.is_empty() {
                    out.push(ReviewIntent::ReplyToThread {
                        comment_id: reply_id,
                        body,
                    });
                }
                state.cancel_reply();
            }
            ReplyEdit::Cancel => state.cancel_reply(),
            ReplyEdit::Idle => {}
        }
    } else {
        ui.add_space(2.0);
        let mut reply_clicked = false;
        let mut resolve_clicked = false;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.spacing_mut().item_spacing.x = 6.0;
            reply_clicked = reply_pill(ui, palette);
            resolve_clicked = resolve_pill(ui, palette, resolved);
        });
        if reply_clicked {
            state.open_reply(reply_id);
        }
        if resolve_clicked {
            out.push(ReviewIntent::ResolveThread {
                thread_id,
                comment_id: reply_id,
                resolved: !resolved,
            });
        }
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

/// One posted PR comment: an initials avatar beside the author and the body —
/// read-only. Wears the same tinted-card-with-left-edge grammar as the editable
/// note card, but in a neutral ink so a fetched comment reads apart from a forge
/// review (`accent`) or an agent note (`accent_ai`). Reuses the shared
/// `detail::author_avatar`, so inline threads and the PR conversation rail wear
/// the same face.
fn thread_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    comment: &crate::review::ThreadComment,
    ask_label: Option<&str>,
    snippet: Option<&[crate::pull_requests::model::SnippetLine]>,
    now: i64,
) -> bool {
    let mut ask_clicked = false;
    egui::Frame::new()
        .fill(palette.bg_surface)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(10))
        .stroke(egui::Stroke::new(1.0, palette.border_subtle))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                crate::ui::detail::author_avatar(ui, palette, &comment.author);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            egui::RichText::new(&comment.author)
                                .size(LINE_SIZE)
                                .color(palette.text_primary)
                                .strong(),
                        );
                        let age =
                            crate::pull_requests::model::relative_age(&comment.created_at, now);
                        if !age.is_empty() {
                            ui.label(
                                egui::RichText::new(age)
                                    .size(LINE_SIZE - 1.0)
                                    .color(palette.text_muted),
                            );
                        }
                    });
                    if let Some(snip) = snippet.filter(|s| !s.is_empty()) {
                        ui.add_space(4.0);
                        crate::ui::detail::code_snippet(ui, palette, path, snip);
                    }
                    ui.add_space(3.0);
                    crate::ui::pull_requests_view::markdown(ui, palette, &comment.body);
                    if let Some(label) = ask_label {
                        ui.add_space(4.0);
                        if pill_button(
                            ui,
                            palette,
                            palette.accent_ai,
                            lucide_icons::Icon::Bot,
                            label,
                            RADIUS_PILL,
                        ) {
                            ask_clicked = true;
                        }
                    }
                });
            });
        });
    ask_clicked
}

/// Saved note rendered as a compact identity-tinted card — the pool's icon beside
/// the note body, with an accent left edge — the whole surface clickable to
/// re-open its editor. Returns `true` on click.
fn note_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    style: &PoolStyle,
    note: &str,
    line: Option<u32>,
) -> bool {
    let inner = egui::Frame::new()
        .fill(with_alpha(style.color, 20))
        .inner_margin(egui::Margin::symmetric(9, 6))
        .corner_radius(egui::CornerRadius::same(RADIUS_PILL))
        .stroke(egui::Stroke::new(1.0, with_alpha(style.color, 70)))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(LINE_SIZE, LINE_SIZE), egui::Sense::hover());
                crate::ui::paint_icon(
                    ui.painter(),
                    r.center(),
                    LINE_SIZE - 1.0,
                    style.icon,
                    style.color,
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(note)
                        .size(LINE_SIZE)
                        .color(palette.text_secondary),
                );
            });
        });
    let rect = inner.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
        egui::CornerRadius::same(RADIUS_PILL),
        style.color,
    );
    let response = ui
        .interact(
            rect,
            ui.id().with(("note_card", line, rect.min.y.to_bits())),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Edit review note")
    });
    response.clicked()
}

/// Pushes a save (or a delete when the note is blank) for the line `(old, new)`.
fn save_note(
    out: &mut Vec<ReviewIntent>,
    pool: ReviewPool,
    path: &str,
    old: Option<u32>,
    new: Option<u32>,
    code: &str,
    note: &str,
) {
    if note.is_empty() {
        out.push(ReviewIntent::DeleteComment {
            pool,
            file: path.to_owned(),
            line: new.or(old),
        });
        return;
    }
    out.push(ReviewIntent::SaveComment {
        pool,
        file: path.to_owned(),
        comment: LineComment {
            old_lineno: old,
            new_lineno: new,
            code: code.to_owned(),
            note: note.to_owned(),
        },
    });
}

/// Header chip — a Sparkles glyph and the comment count — that toggles the review
/// recap popover. Returns its response so the popover can anchor to it.
fn review_chip(ui: &mut egui::Ui, palette: &Palette, n: usize) -> egui::Response {
    let label = n.to_string();
    let font = egui::FontId::proportional(PILL_SIZE);
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font.clone(), egui::Color32::PLACEHOLDER);
    let icon_w = LINE_SIZE;
    let size = egui::vec2(icon_w + 4.0 + galley.size().x + 16.0, PILL_SIZE + 10.0);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let (fill, content) = if hovered {
        (with_alpha(palette.accent_ai, 36), palette.accent_ai)
    } else {
        (palette.bg_surface, palette.text_secondary)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        fill,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 8.0 + icon_w / 2.0, rect.center().y),
        icon_w,
        lucide_icons::Icon::Sparkles,
        content,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 8.0 + icon_w + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        content,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Review notes"));
    response
}

/// Review recap popover: every stored note grouped by file, each editable in
/// place (click) and deletable (✕), with a Send-to-agent footer.
fn review_popover(
    ui: &mut egui::Ui,
    palette: &Palette,
    agent: &str,
    comments: &FileComments,
    state: &mut DiffViewState,
    out: &mut Vec<ReviewIntent>,
) {
    ui.set_max_width(360.0);
    ui.spacing_mut().item_spacing.y = 6.0;
    let style = pool_style(palette, ReviewPool::Agent);
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for (file, file_comments) in comments {
                ui.label(
                    egui::RichText::new(file)
                        .size(PILL_SIZE)
                        .color(palette.text_muted),
                );
                for c in file_comments {
                    let line = c.line_ref();
                    let editing = state
                        .popover_edit
                        .as_ref()
                        .is_some_and(|(f, l)| f == file && *l == line);
                    if editing {
                        let edit = note_editor(
                            ui,
                            palette,
                            &style,
                            &mut state.popover_buffer,
                            &mut state.note_focus,
                            ui.available_width(),
                        );
                        match edit {
                            NoteEdit::Save => {
                                save_note(
                                    out,
                                    ReviewPool::Agent,
                                    file,
                                    c.old_lineno,
                                    c.new_lineno,
                                    &c.code,
                                    state.popover_buffer.trim(),
                                );
                                state.popover_edit = None;
                                state.popover_buffer.clear();
                            }
                            NoteEdit::Delete => {
                                out.push(ReviewIntent::DeleteComment {
                                    pool: ReviewPool::Agent,
                                    file: file.clone(),
                                    line,
                                });
                                state.popover_edit = None;
                                state.popover_buffer.clear();
                            }
                            NoteEdit::Idle => {}
                        }
                    } else {
                        let loc = match line {
                            Some(n) => format!("L{n}"),
                            None => "·".to_owned(),
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(loc)
                                    .monospace()
                                    .size(PILL_SIZE)
                                    .color(palette.text_muted),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(truncate_code(&c.code))
                                        .monospace()
                                        .size(PILL_SIZE)
                                        .color(palette.text_muted),
                                )
                                .truncate(),
                            );
                        });
                        ui.horizontal(|ui| {
                            if icon_button(
                                ui,
                                palette,
                                lucide_icons::Icon::Trash2,
                                palette.git_deleted,
                                "Delete review note",
                            ) {
                                out.push(ReviewIntent::DeleteComment {
                                    pool: ReviewPool::Agent,
                                    file: file.clone(),
                                    line,
                                });
                            }
                            let note = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&c.note)
                                        .size(LINE_SIZE)
                                        .color(palette.text_secondary),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if note.clicked() {
                                state.popover_edit = Some((file.clone(), line));
                                state.popover_buffer = c.note.clone();
                                state.active_comment = None;
                                state.note_focus = true;
                            }
                        });
                    }
                }
            }
        });
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if send_pill(ui, palette, agent) {
            out.push(ReviewIntent::SendToAgent);
        }
    });
}

/// Recap footer action: a Sparkles glyph and a "Send to {agent}" label in a pill
/// (the AI-call icon used by the commit message), hover-tinted to the accent.
fn send_pill(ui: &mut egui::Ui, palette: &Palette, agent: &str) -> bool {
    pill_button(
        ui,
        palette,
        palette.accent_ai,
        lucide_icons::Icon::Sparkles,
        &format!("Send to {agent}"),
        RADIUS_PILL,
    )
}

/// A pill button: a leading glyph and `label` in a rounded rect, neutral at rest
/// (bg.surface + subtle border, secondary ink) and washed to `hover_accent` on
/// hover. `radius` picks stadium (`RADIUS_PILL`) vs button corners. Returns `true`
/// on click.
fn pill_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    hover_accent: egui::Color32,
    icon: lucide_icons::Icon,
    label: &str,
    radius: u8,
) -> bool {
    let label = label.to_owned();
    let font = egui::FontId::proportional(PILL_SIZE);
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font.clone(), egui::Color32::PLACEHOLDER);
    let icon_w = LINE_SIZE;
    let size = egui::vec2(icon_w + 6.0 + galley.size().x + 16.0, PILL_SIZE + 10.0);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let (fill, content) = if hovered {
        (with_alpha(hover_accent, 36), hover_accent)
    } else {
        (palette.bg_surface, palette.text_secondary)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(radius),
        fill,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 8.0 + icon_w / 2.0, rect.center().y),
        icon_w,
        icon,
        content,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 8.0 + icon_w + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label.clone(),
        font,
        content,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    response.clicked()
}

/// Single-line, trimmed-and-capped code snippet used as the anchor shown beside a
/// note in the recap popover.
fn truncate_code(code: &str) -> String {
    const MAX: usize = 40;
    let trimmed = code.trim();
    if trimmed.chars().count() > MAX {
        let head: String = trimmed.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        trimmed.to_owned()
    }
}

fn apply_line_action(
    state: &mut DiffViewState,
    intents: &mut Vec<GitIntent>,
    action: Option<DiffLineAction>,
) {
    match action {
        Some(DiffLineAction::Intent(intent)) => intents.push(intent),
        Some(DiffLineAction::ToggleSelection { hunk, line }) => state.toggle(hunk, line),
        Some(DiffLineAction::SelectText(selection)) => {
            state.selection.clear();
            state.text_selection = Some(selection);
        }
        Some(DiffLineAction::ClearTextSelection) => state.text_selection = None,
        // Handled by the caller (needs the stored comments to prefill the editor).
        Some(DiffLineAction::OpenComment { .. }) => {}
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
        highlighted: None,
        text_range: state.text_range_for_row(ext.text_row, text),
        text_row: ext.text_row,
        char_w: ext.char_w,
        layout: ext.layout,
        row_w: ext.row_w,
    };
    diff_line(ui, &row, 0, 0, &line_ctx, text_rows)
}

fn close_button(ui: &mut egui::Ui, palette: &Palette) -> bool {
    let response = ui.add(
        egui::Button::new(
            egui::RichText::new("Close")
                .size(PILL_SIZE)
                .color(palette.text_secondary),
        )
        .fill(palette.bg_surface)
        .corner_radius(egui::CornerRadius::same(RADIUS_PILL)),
    );
    response.on_hover_text("Close (Esc)").clicked()
}

const ZOOM_STEP: f32 = 1.25;
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;

/// Decoded image kept across frames for the diff view's preview. `texture` is `None`
/// when decoding failed — cached so the failure is not retried every frame.
struct ImagePreview {
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

fn decode_image(ctx: &egui::Context, blob: &ImageBlob, path: &str) -> ImagePreview {
    let texture = image::load_from_memory(&blob.bytes).ok().map(|img| {
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        ctx.load_texture(
            format!("diff-image-{path}"),
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
/// view of the new-side blob. The toolbar sets discrete zoom levels; a trackpad pinch
/// or ⌘+scroll zooms; two-finger scroll pans the surrounding scroll area.
fn image_preview(
    ui: &mut egui::Ui,
    palette: &Palette,
    blob: &ImageBlob,
    path: &str,
    state: &mut DiffViewState,
) {
    if state.image.as_ref().map(|p| p.key) != Some(blob.fingerprint) {
        state.image = Some(decode_image(ui.ctx(), blob, path));
    }
    let Some(preview) = state.image.as_mut() else {
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

/// Hunk header band: `@@ … @@` on a surface background, controls on the right —
/// Stage/Unstage (outside read-only) and **Extend context** (+5, git.md §4;
/// returns `true` on click, also available read-only: a view action).
#[allow(clippy::too_many_arguments)]
fn hunk_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    header: &str,
    staged: bool,
    read_only: bool,
    hunk_idx: usize,
    state: &DiffViewState,
    can_extend: bool,
    intents: &mut Vec<GitIntent>,
) -> bool {
    let mut extend = false;
    egui::Frame::new()
        .fill(palette.bg_surface)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(HUNK_BAND_PAD_X, HUNK_BAND_PAD_Y))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(header.trim_end())
                        .size(HUNK_HEADER_SIZE)
                        .monospace()
                        .color(palette.text_muted),
                );
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
                    if can_extend
                        && intent_pill(ui, palette, "Extend context", palette.accent, true)
                    {
                        extend = true;
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
    highlighted: Option<&'a [HighlightedSpan]>,
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
    if selected {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.5, ctx.palette.accent),
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
    let action = if response.triple_clicked() {
        click_position.map(|at| {
            DiffLineAction::SelectText(TextSelection {
                anchor: at,
                head: at,
                mode: TextSelectionMode::Line,
            })
        })
    } else if response.double_clicked() {
        click_position.map(|at| {
            DiffLineAction::SelectText(TextSelection {
                anchor: at,
                head: at,
                mode: TextSelectionMode::Word,
            })
        })
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
    } else if selectable && response.clicked() {
        Some(DiffLineAction::ToggleSelection {
            hunk: hunk_idx,
            line: line_idx,
        })
    } else if response.clicked() {
        Some(DiffLineAction::ClearTextSelection)
    } else {
        None
    };

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

fn paint_line_content(
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

fn paint_text_selection(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: egui::Rect,
    content_left: f32,
    char_w: f32,
    from: usize,
    to: usize,
) {
    let left = content_left + from as f32 * char_w;
    let right = content_left + to as f32 * char_w;
    let rect =
        egui::Rect::from_min_max(egui::pos2(left, row.top()), egui::pos2(right, row.bottom()));
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        with_alpha(palette.accent, TEXT_SELECTION_ALPHA),
    );
}

fn update_text_selection(ui: &egui::Ui, state: &mut DiffViewState, rows: &[TextRow]) {
    let selection = ui.input(|input| {
        if !input.pointer.primary_down() {
            return None;
        }
        let press = input.pointer.press_origin()?;
        let current = input.pointer.interact_pos()?;
        if press.distance(current) < TEXT_DRAG_THRESHOLD {
            return None;
        }
        let anchor = text_position_at(press, rows, true)?;
        let head = text_position_at(current, rows, false)?;
        Some(TextSelection {
            anchor,
            head,
            mode: TextSelectionMode::Char,
        })
    });
    if let Some(selection) = selection {
        if state.text_selection != Some(selection) {
            state.selection.clear();
            state.text_selection = Some(selection);
            ui.ctx().request_repaint();
        }
    }
}

fn text_position_at(
    pos: egui::Pos2,
    rows: &[TextRow],
    require_text_hit: bool,
) -> Option<TextPosition> {
    let row = row_at_position(pos, rows, require_text_hit)?;
    if require_text_hit && pos.x < row.content_left {
        return None;
    }
    Some(TextPosition {
        row: row.row,
        col: text_col_at(pos.x, row),
    })
}

fn row_at_position(pos: egui::Pos2, rows: &[TextRow], require_inside: bool) -> Option<TextRow> {
    if let Some(row) = rows.iter().find(|row| row.rect.contains(pos)) {
        return Some(*row);
    }
    if require_inside {
        return None;
    }
    rows.iter()
        .min_by(|a, b| y_distance(pos.y, a.rect).total_cmp(&y_distance(pos.y, b.rect)))
        .copied()
}

fn y_distance(y: f32, rect: egui::Rect) -> f32 {
    if y < rect.top() {
        rect.top() - y
    } else if y > rect.bottom() {
        y - rect.bottom()
    } else {
        0.0
    }
}

fn text_col_at(x: f32, row: TextRow) -> usize {
    if row.text_len == 0 {
        return 0;
    }
    let col = ((x - row.content_left) / row.char_w).floor().max(0.0) as usize;
    col.min(row.text_len - 1)
}

fn text_click_position(
    response: &egui::Response,
    content_left: f32,
    char_w: f32,
    row: usize,
    text_len: usize,
) -> Option<TextPosition> {
    if text_len == 0 {
        return None;
    }
    let pos = response.interact_pointer_pos()?;
    let content_right = content_left + text_len as f32 * char_w;
    if pos.x < content_left || pos.x > content_right {
        return None;
    }
    Some(TextPosition {
        row,
        col: text_col_at(
            pos.x,
            TextRow {
                row,
                rect: response.rect,
                content_left,
                char_w,
                text_len,
            },
        ),
    })
}

fn copy_requested(ui: &egui::Ui) -> bool {
    ui.ctx()
        .input(|input| input.events.iter().any(|e| matches!(e, egui::Event::Copy)))
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
            egui::Stroke::new(1.0, intent),
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
fn gutter_icon_button(
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
        let mut state = DiffViewState::default();
        assert!(!state.selected(0, 1));
        state.toggle(0, 1);
        assert!(state.selected(0, 1));
        assert_eq!(state.selected_lines(0), vec![1]);
        state.toggle(0, 1);
        assert!(!state.selected(0, 1));
        assert!(state.selected_lines(0).is_empty());
    }

    #[test]
    fn selected_lines_are_sorted_and_scoped_per_hunk() {
        let mut state = DiffViewState::default();
        state.toggle(0, 3);
        state.toggle(0, 1);
        state.toggle(1, 5);
        assert_eq!(state.selected_lines(0), vec![1, 3]);
        assert_eq!(state.selected_lines(1), vec![5]);
        assert!(state.selected_lines(2).is_empty());
    }

    use crate::git::diff::DiffLine;

    fn diff_with(lines: Vec<LineOrigin>) -> FileDiff {
        FileDiff {
            path: "f".into(),
            binary: false,
            oversize: false,
            hunks: vec![Hunk {
                header: String::new(),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 0,
                lines: lines
                    .into_iter()
                    .map(|origin| DiffLine {
                        origin,
                        content: String::new(),
                        old_lineno: None,
                        new_lineno: None,
                    })
                    .collect(),
            }],
            source_lines: Vec::new(),
            image: None,
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
        let diff = ext_diff(vec![(5, 3, 5, 3)], 10);
        assert!(can_extend(&diff, &HashMap::new(), 0));
        assert!(
            !can_extend(&diff, &amounts(&[(0, 10)]), 0),
            "the whole file is shown: nothing to extend"
        );

        let full = ext_diff(vec![(1, 4, 1, 4)], 4);
        assert!(
            !can_extend(&full, &HashMap::new(), 0),
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
        let mut state = DiffViewState::default();
        state.toggle(0, 1);
        let diff = diff_with(vec![LineOrigin::Context, LineOrigin::Addition]);

        assert!(!state.reconcile(&diff));
        assert!(!state.is_stale());
        assert_eq!(state.selected_lines(0), vec![1]);
    }

    #[test]
    fn reconcile_drops_an_out_of_range_selection_and_flags_stale() {
        let mut state = DiffViewState::default();
        state.toggle(2, 0);
        let diff = diff_with(vec![LineOrigin::Addition]);

        assert!(state.reconcile(&diff));
        assert!(state.is_stale());
        assert!(state.selected_lines(2).is_empty());
    }

    #[test]
    fn reconcile_drops_a_selection_that_became_context() {
        let mut state = DiffViewState::default();
        state.toggle(0, 0);
        let diff = diff_with(vec![LineOrigin::Context]);

        assert!(state.reconcile(&diff));
        assert!(state.is_stale());
    }

    #[test]
    fn note_on_a_deleted_row_does_not_show_on_an_added_row_with_the_same_number() {
        let mut store = FileComments::new();
        crate::review::add_comment(
            &mut store,
            "f",
            LineComment {
                old_lineno: Some(5),
                new_lineno: None,
                code: "removed".into(),
                note: "for claude".into(),
            },
        );

        assert_eq!(
            note_at(&store, "f", Some(5), None).as_deref(),
            Some("for claude")
        );
        assert_eq!(note_at(&store, "f", None, Some(5)), None);
    }
}
