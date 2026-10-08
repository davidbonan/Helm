//! Text selection over monospace rows a view paints itself — the diff view, the
//! file viewer: drag, double-click a word, triple-click a line, `Cmd+C`.

use crate::theme::Palette;
use crate::ui::with_alpha;

const TEXT_DRAG_THRESHOLD: f32 = 2.0;
const TEXT_SELECTION_ALPHA: u8 = 70;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextPosition {
    pub(crate) row: usize,
    pub(crate) col: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TextSelectionMode {
    Char,
    Word,
    Line,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TextSelection {
    pub(crate) anchor: TextPosition,
    pub(crate) head: TextPosition,
    pub(crate) mode: TextSelectionMode,
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

    pub(crate) fn range_for_row(self, row: usize, text: &str) -> Option<(usize, usize)> {
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

    pub(crate) fn clamped_to(self, lines: &[&str]) -> Option<Self> {
        if lines.is_empty() || self.anchor.row >= lines.len() || self.head.row >= lines.len() {
            return None;
        }
        Some(Self {
            anchor: clamp_text_position(self.anchor, lines),
            head: clamp_text_position(self.head, lines),
            mode: self.mode,
        })
    }

    /// The selected text of `lines`, one `\n` between rows.
    pub(crate) fn text_of(self, lines: &[&str]) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let selection = self.clamped_to(lines)?;
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
}

/// Where a painted row sits and what it holds, for hit-testing a drag.
#[derive(Debug, Copy, Clone)]
pub(crate) struct TextRow {
    pub(crate) row: usize,
    pub(crate) rect: egui::Rect,
    pub(crate) content_left: f32,
    pub(crate) char_w: f32,
    pub(crate) text_len: usize,
}

fn clamp_text_position(position: TextPosition, lines: &[&str]) -> TextPosition {
    let text_len = lines[position.row].chars().count();
    TextPosition {
        row: position.row,
        col: position.col.min(text_len.saturating_sub(1)),
    }
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

pub(crate) fn paint_text_selection(
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

/// The selection a press-and-drag over `rows` spans this frame; `None` while the
/// pointer is up, still or pressed outside the text.
pub(crate) fn dragged_selection(ui: &egui::Ui, rows: &[TextRow]) -> Option<TextSelection> {
    ui.input(|input| {
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
    })
}

/// A triple click selects the line, a double click the word under `at`.
pub(crate) fn clicked_selection(
    response: &egui::Response,
    at: TextPosition,
) -> Option<TextSelection> {
    let mode = if response.triple_clicked() {
        TextSelectionMode::Line
    } else if response.double_clicked() {
        TextSelectionMode::Word
    } else {
        return None;
    };
    Some(TextSelection {
        anchor: at,
        head: at,
        mode,
    })
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

pub(crate) fn text_click_position(
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

pub(crate) fn copy_requested(ui: &egui::Ui) -> bool {
    ui.ctx()
        .input(|input| input.events.iter().any(|e| matches!(e, egui::Event::Copy)))
}
