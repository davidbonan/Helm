//! The grid as styled text runs, for a client that draws it itself — the phone
//! mirror (specs/remote.md §6). Reads the **live** screen, whatever the Mac's own
//! scroll position.

use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};

use crate::terminal::emu::wheel_goes_to_app;
use crate::terminal::palette::{Rgb, TermPalette};

/// Resolved look of a run: palette applied, dim and inverse folded into the colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

/// One grid row; trailing blank cells on the default background are dropped.
pub type ScreenLine = Vec<Run>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub cols: usize,
    pub lines: Vec<ScreenLine>,
    /// `(line, col)`, `None` while the program hides the cursor.
    pub cursor: Option<(usize, usize)>,
    /// The app scrolls its own view (full-screen TUI): a swipe goes to it as the wheel.
    pub app_scrolls: bool,
}

/// Scrollback lines `first..before` in grid coordinates (0 = top of the screen,
/// negative = history). `first` is the next page's `before`; it stops at the
/// oldest line kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    pub first: i32,
    pub lines: Vec<ScreenLine>,
}

pub fn screen<T>(term: &Term<T>, palette: &TermPalette) -> Screen {
    let grid = term.grid();
    let cols = grid.columns();
    let lines = (0..grid.screen_lines() as i32)
        .map(|line| line_runs(&grid[Line(line)], cols, palette))
        .collect();
    let cursor = term.mode().contains(TermMode::SHOW_CURSOR).then(|| {
        let point = grid.cursor.point;
        (point.line.0.max(0) as usize, point.column.0)
    });
    Screen {
        cols,
        lines,
        cursor,
        app_scrolls: wheel_goes_to_app(*term.mode()),
    }
}

pub fn history<T>(term: &Term<T>, palette: &TermPalette, before: i32, count: usize) -> HistoryPage {
    let grid = term.grid();
    let oldest = -(grid.history_size() as i32);
    let before = before.clamp(oldest, 0);
    let first = (before - count as i32).max(oldest);
    let lines = (first..before)
        .map(|line| line_runs(&grid[Line(line)], grid.columns(), palette))
        .collect();
    HistoryPage { first, lines }
}

fn line_runs(row: &Row<Cell>, cols: usize, palette: &TermPalette) -> ScreenLine {
    let cells: Vec<&Cell> = (0..cols).map(|col| &row[Column(col)]).collect();
    let end = cells
        .iter()
        .rposition(|cell| !is_blank(cell, palette))
        .map_or(0, |last| last + 1);
    let mut runs: ScreenLine = Vec::new();
    for cell in &cells[..end] {
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let style = style_of(cell, palette);
        let run = match runs.last_mut() {
            Some(run) if run.style == style => run,
            _ => {
                runs.push(Run {
                    text: String::new(),
                    style,
                });
                runs.last_mut().expect("just pushed")
            }
        };
        run.text.push(cell.c);
        run.text.extend(cell.zerowidth().unwrap_or_default());
    }
    runs
}

fn is_blank(cell: &Cell, palette: &TermPalette) -> bool {
    let style = style_of(cell, palette);
    cell.c == ' '
        && cell.zerowidth().is_none()
        && style.bg == palette.background
        && !style.underline
}

fn style_of(cell: &Cell, palette: &TermPalette) -> Style {
    let mut fg = palette.resolve(cell.fg);
    let mut bg = palette.resolve(cell.bg);
    if cell.flags.contains(Flags::DIM) {
        fg = palette.dim(fg);
    }
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    Style {
        fg,
        bg,
        bold: cell.flags.contains(Flags::BOLD),
        italic: cell.flags.contains(Flags::ITALIC),
        underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::emu::{feed, shared_term};

    const PALETTE: TermPalette = TermPalette::dark();

    fn screen_after(bytes: &[u8]) -> Screen {
        let term = shared_term(4, 20);
        feed(&term, bytes);
        let term = term.lock();
        screen(&term, &PALETTE)
    }

    fn texts(line: &ScreenLine) -> Vec<&str> {
        line.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn plain_text_is_one_run_without_trailing_blanks() {
        let screen = screen_after(b"hello world");

        assert_eq!(texts(&screen.lines[0]), ["hello world"]);
        assert_eq!(screen.lines[0][0].style.fg, PALETTE.foreground);
        assert!(screen.lines[1].is_empty(), "a blank row carries no run");
    }

    #[test]
    fn a_color_change_splits_the_run_with_the_palette_color() {
        let screen = screen_after(b"\x1b[31mred\x1b[0m ok");

        assert_eq!(texts(&screen.lines[0]), ["red", " ok"]);
        assert_eq!(screen.lines[0][0].style.fg, PALETTE.ansi(1));
        assert_eq!(screen.lines[0][1].style.fg, PALETTE.foreground);
    }

    #[test]
    fn inverse_is_folded_into_the_colors() {
        let screen = screen_after(b"\x1b[7msel");

        let style = screen.lines[0][0].style;
        assert_eq!(
            (style.fg, style.bg),
            (PALETTE.background, PALETTE.foreground)
        );
    }

    #[test]
    fn a_wide_character_is_sent_once() {
        let screen = screen_after("界x".as_bytes());

        assert_eq!(texts(&screen.lines[0]), ["界x"]);
    }

    #[test]
    fn the_cursor_follows_the_program_visibility() {
        assert_eq!(screen_after(b"ab").cursor, Some((0, 2)));
        assert_eq!(screen_after(b"ab\x1b[?25l").cursor, None);
    }

    #[test]
    fn history_pages_walk_up_to_the_oldest_line() {
        let term = shared_term(2, 10);
        feed(&term, b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
        let term = term.lock();

        let page = history(&term, &PALETTE, 0, 2);
        assert_eq!(page.first, -2);
        assert_eq!(
            page.lines.iter().map(texts).collect::<Vec<_>>(),
            [["l2"], ["l3"]]
        );

        let last = history(&term, &PALETTE, page.first, 5);
        assert_eq!(last.first, -3, "stops at the oldest kept line");
        assert_eq!(last.lines.iter().map(texts).collect::<Vec<_>>(), [["l1"]]);
    }
}
