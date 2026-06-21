#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub line: i32,
    pub col: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionMode {
    Char,
    Word,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: Cell,
    pub head: Cell,
    pub mode: SelectionMode,
}

impl Selection {
    pub fn new(at: Cell, mode: SelectionMode) -> Self {
        Self {
            anchor: at,
            head: at,
            mode,
        }
    }

    /// `(start, end)` sorted in reading order (top→bottom, then left→right), with
    /// `end` inclusive.
    pub fn ordered(&self) -> (Cell, Cell) {
        if (self.head.line, self.head.col) < (self.anchor.line, self.anchor.col) {
            (self.head, self.anchor)
        } else {
            (self.anchor, self.head)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.mode == SelectionMode::Char && self.anchor == self.head
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~')
}

/// `[start, end)` bounds of the word covering `col` on line `row`; on a non-word
/// cell, selects that single character.
pub fn word_bounds(row: &[char], col: usize) -> (usize, usize) {
    if col >= row.len() {
        return (col, col + 1);
    }
    if !is_word_char(row[col]) {
        return (col, col + 1);
    }
    let mut start = col;
    while start > 0 && is_word_char(row[start - 1]) {
        start -= 1;
    }
    let mut end = col + 1;
    while end < row.len() && is_word_char(row[end]) {
        end += 1;
    }
    (start, end)
}

/// True if `(line, col)` falls within the selection (bounds included), accounting
/// for the mode (word/line widen the clicked cell).
pub fn covers(sel: &Selection, rows: &[Vec<char>], first_line: i32, line: i32, col: usize) -> bool {
    let (start, end) = resolved_bounds(sel, rows, first_line);
    let within_start = (line, col) >= (start.line, start.col);
    let within_end = (line, col) <= (end.line, end.col);
    within_start && within_end
}

/// Selected text, lines joined by `\n`, trailing spaces removed per line.
pub fn selected_text(sel: &Selection, rows: &[Vec<char>], first_line: i32) -> String {
    let (start, end) = resolved_bounds(sel, rows, first_line);
    let mut out = String::new();
    for line in start.line..=end.line {
        let Some(row) = row_at(rows, first_line, line) else {
            continue;
        };
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == end.line {
            (end.col + 1).min(row.len())
        } else {
            row.len()
        };
        if from < to {
            let segment: String = row[from..to].iter().collect();
            out.push_str(segment.trim_end());
        }
        if line != end.line {
            out.push('\n');
        }
    }
    out
}

/// Effective bounds `(start, inclusive end)` after applying the mode.
fn resolved_bounds(sel: &Selection, rows: &[Vec<char>], first_line: i32) -> (Cell, Cell) {
    let (mut start, mut end) = sel.ordered();
    match sel.mode {
        SelectionMode::Char => {}
        SelectionMode::Word => {
            if let Some(row) = row_at(rows, first_line, start.line) {
                start.col = word_bounds(row, start.col).0;
            }
            if let Some(row) = row_at(rows, first_line, end.line) {
                let (_, word_end) = word_bounds(row, end.col);
                end.col = word_end.saturating_sub(1);
            }
        }
        SelectionMode::Line => {
            start.col = 0;
            end.col = row_at(rows, first_line, end.line)
                .map(|row| row.len().saturating_sub(1))
                .unwrap_or(0);
        }
    }
    (start, end)
}

fn row_at(rows: &[Vec<char>], first_line: i32, line: i32) -> Option<&Vec<char>> {
    let idx = line - first_line;
    if idx < 0 {
        return None;
    }
    rows.get(idx as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(lines: &[&str]) -> Vec<Vec<char>> {
        lines.iter().map(|l| l.chars().collect()).collect()
    }

    fn cell(line: i32, col: usize) -> Cell {
        Cell { line, col }
    }

    #[test]
    fn ordered_sorts_reverse_drag() {
        let sel = Selection {
            anchor: cell(2, 5),
            head: cell(1, 0),
            mode: SelectionMode::Char,
        };
        let (start, end) = sel.ordered();
        assert_eq!(start, cell(1, 0));
        assert_eq!(end, cell(2, 5));
    }

    #[test]
    fn char_selection_extracts_inclusive_range() {
        let rows = grid(&["hello world"]);
        let sel = Selection {
            anchor: cell(0, 0),
            head: cell(0, 4),
            mode: SelectionMode::Char,
        };
        assert_eq!(selected_text(&sel, &rows, 0), "hello");
    }

    #[test]
    fn word_bounds_spans_the_full_word() {
        let row: Vec<char> = "foo bar-baz".chars().collect();
        assert_eq!(word_bounds(&row, 5), (4, 11));
        assert_eq!(word_bounds(&row, 1), (0, 3));
    }

    #[test]
    fn word_bounds_on_separator_picks_single_cell() {
        let row: Vec<char> = "foo bar".chars().collect();
        assert_eq!(word_bounds(&row, 3), (3, 4));
    }

    #[test]
    fn word_mode_extracts_whole_word_from_inner_click() {
        let rows = grid(&["foo bar-baz qux"]);
        let sel = Selection::new(cell(0, 6), SelectionMode::Word);
        assert_eq!(selected_text(&sel, &rows, 0), "bar-baz");
    }

    #[test]
    fn line_mode_extracts_trimmed_full_line() {
        let rows = grid(&["  hi there      "]);
        let sel = Selection::new(cell(0, 3), SelectionMode::Line);
        assert_eq!(selected_text(&sel, &rows, 0), "  hi there");
    }

    #[test]
    fn multi_line_char_selection_joins_with_newline() {
        let rows = grid(&["abc   ", "defg  "]);
        let sel = Selection {
            anchor: cell(0, 1),
            head: cell(1, 1),
            mode: SelectionMode::Char,
        };
        assert_eq!(selected_text(&sel, &rows, 0), "bc\nde");
    }

    #[test]
    fn covers_marks_cells_inside_char_range() {
        let rows = grid(&["hello world"]);
        let sel = Selection {
            anchor: cell(0, 2),
            head: cell(0, 4),
            mode: SelectionMode::Char,
        };
        assert!(covers(&sel, &rows, 0, 0, 3));
        assert!(!covers(&sel, &rows, 0, 0, 1));
        assert!(!covers(&sel, &rows, 0, 0, 5));
    }

    #[test]
    fn covers_uses_word_bounds_in_word_mode() {
        let rows = grid(&["foo bar baz"]);
        let sel = Selection::new(cell(0, 5), SelectionMode::Word);
        assert!(covers(&sel, &rows, 0, 0, 4));
        assert!(covers(&sel, &rows, 0, 0, 6));
        assert!(!covers(&sel, &rows, 0, 0, 7));
    }

    #[test]
    fn empty_char_selection_yields_nothing() {
        let sel = Selection::new(cell(0, 3), SelectionMode::Char);
        assert!(sel.is_empty());
        let word = Selection::new(cell(0, 3), SelectionMode::Word);
        assert!(!word.is_empty());
    }
}
