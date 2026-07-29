use std::path::Path;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;

use crate::keybindings::Shortcut;
use crate::terminal::emu::{
    mouse_protocol, mouse_report, wheel_bytes, MouseButton, MouseKind, MouseMods, MouseProtocol,
    ScrollKind, SharedTerm,
};
use crate::terminal::layout::{
    first_leaf, split_rects, Dir, Layout, Node, Orient, PaneId, Rect as PaneRect, MIN_COLS,
    MIN_LINES,
};
use crate::terminal::links::{link_at, LinkAction};
use crate::terminal::palette::{Rgb, TermPalette};
use crate::terminal::selection::{covers, selected_text, Cell, Selection, SelectionMode};
use crate::theme::Palette;
use crate::ui::{paint_icon, with_alpha};

const SELECTION_ALPHA: u8 = 110;

const SEPARATOR_THICKNESS: f32 = 1.0;
const SEPARATOR_HANDLE: f32 = 6.0;

/// Drag-grip pill revealed at the top of each pane on hover (terminal.md §5):
/// grabbing it starts a drag-and-drop reorg of the splits.
const GRIP_W: f32 = 26.0;
const GRIP_H: f32 = 14.0;
const GRIP_TOP: f32 = 3.0;
const GRIP_ICON: f32 = 13.0;
/// Half-extent (per axis, as a fraction of the target pane) of the central
/// "swap" zone of a drop target; outside it the nearest edge picks the re-split
/// side.
const DROP_SWAP_HALF: f32 = 0.18;
/// Fill translucency of the drop-zone highlight overlay.
const DROP_OVERLAY_ALPHA: u8 = 64;

/// Breathing room below the last line: the grid is sized to the area minus this
/// margin so the bottom doesn't touch the window edge.
const BOTTOM_PAD: f32 = 6.0;

/// Dim laid over an unfocused split pane to spotlight the focused one, blending
/// its content toward the terminal background (Ghostty's `unfocused-split-opacity`
/// 0.7 ⇒ a background fill at 1 − 0.7 alpha). A lone pane is always focused, so
/// only splits are ever dimmed.
const UNFOCUSED_DIM_ALPHA: u8 = 76;

/// Max lines forwarded to a full-screen TUI (alt-screen / mouse reporting) per
/// frame. Each line is an arrow keypress the app (e.g. Claude Code) redraws on,
/// so a peak scroll frame would otherwise emit a burst that scrolls many lines in
/// one redraw — the visible jerk. The overflow drains over the next frames (same
/// total, even cadence). Local scrollback renders instantly and is never capped.
const SCROLL_STEP_CAP: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Outline,
}

pub fn cursor_shape(focused: bool) -> CursorShape {
    if focused {
        CursorShape::Block
    } else {
        CursorShape::Outline
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub rows: u16,
    pub cols: u16,
}

pub struct TerminalInput {
    pub bytes: Vec<u8>,
    pub paste: Option<String>,
    pub size: GridSize,
    pub relaunch: bool,
    pub clicked: bool,
    pub clear: bool,
    pub scroll: Option<ScrollKind>,
    /// Wheel translated for the application (mouse reporting / alternate scroll,
    /// terminal.md §8): forward to the PTY without snapping the view to the bottom.
    pub scroll_bytes: Vec<u8>,
    /// Mouse button events (press / release / drag) for an app in mouse reporting
    /// (terminal.md §7): forwarded to the PTY without snapping the view.
    pub mouse_bytes: Vec<u8>,
    /// Link activated by a Cmd+click this frame (terminal.md §12); the app resolves
    /// it to `open` / the editor.
    pub open_link: Option<LinkAction>,
}

pub const PROCESS_ENDED_BANNER: &str = "[process exited]";

/// Normalized modifier signature, compared exactly by the chords. `cmd` merges
/// egui's `command`/`mac_cmd` into a single flag.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Mods {
    cmd: bool,
    alt: bool,
    ctrl: bool,
    shift: bool,
}

impl Mods {
    const CMD: Self = Self {
        cmd: true,
        alt: false,
        ctrl: false,
        shift: false,
    };
    const ALT: Self = Self {
        cmd: false,
        alt: true,
        ctrl: false,
        shift: false,
    };
    const SHIFT: Self = Self {
        cmd: false,
        alt: false,
        ctrl: false,
        shift: true,
    };

    fn of(m: egui::Modifiers) -> Self {
        Self {
            cmd: m.command || m.mac_cmd,
            alt: m.alt,
            ctrl: m.ctrl,
            shift: m.shift,
        }
    }
}

/// Chords forwarded to the PTY, selected by exact modifier match. Adding a line
/// is enough to bind a new shortcut.
const CHORDS: &[(Mods, egui::Key, &[u8])] = &[
    (Mods::SHIFT, egui::Key::Tab, b"\x1b[Z"), // backtab (CSI Z)
    // Shift+Enter: CSI u **without negotiation** (kitty/Ghostty convention for
    // combos without a legacy encoding). Claude Code never pushes the kitty
    // protocol: it parses `CSI 13;2u` unconditionally and relies on the terminal
    // to emit it by default — gating on the push breaks it.
    (Mods::SHIFT, egui::Key::Enter, b"\x1b[13;2u"),
    (Mods::ALT, egui::Key::Enter, b"\x1b\r"), // meta+enter: Claude Code newline (/terminal-setup)
    (Mods::ALT, egui::Key::ArrowLeft, b"\x1bb"), // previous word
    (Mods::ALT, egui::Key::ArrowRight, b"\x1bf"), // next word
    (Mods::ALT, egui::Key::Backspace, b"\x1b\x7f"), // delete previous word
    (Mods::CMD, egui::Key::ArrowLeft, b"\x01"), // start of line
    (Mods::CMD, egui::Key::ArrowRight, b"\x05"), // end of line
    (Mods::CMD, egui::Key::Backspace, b"\x15"), // delete to start of line
];

/// Special keys without a chord; residual modifiers are ignored.
const SPECIAL: &[(egui::Key, &[u8])] = &[
    (egui::Key::Enter, b"\r"),
    (egui::Key::Tab, b"\t"),
    (egui::Key::Backspace, b"\x7f"),
    (egui::Key::Escape, b"\x1b"),
    (egui::Key::Delete, b"\x1b[3~"),
    (egui::Key::ArrowUp, b"\x1b[A"),
    (egui::Key::ArrowDown, b"\x1b[B"),
    (egui::Key::ArrowRight, b"\x1b[C"),
    (egui::Key::ArrowLeft, b"\x1b[D"),
];

pub fn key_bytes(key: egui::Key, modifiers: egui::Modifiers) -> Option<Vec<u8>> {
    let mods = Mods::of(modifiers);
    if let Some((_, _, seq)) = CHORDS.iter().find(|(m, k, _)| *m == mods && *k == key) {
        return Some(seq.to_vec());
    }
    // Other Cmd combinations belong to the app (split, zoom, sidebar).
    if mods.cmd {
        return None;
    }
    if mods.ctrl {
        return ctrl_byte(key).map(|b| vec![b]);
    }
    SPECIAL
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, seq)| seq.to_vec())
}

fn ctrl_byte(key: egui::Key) -> Option<u8> {
    let name = key.name();
    let bytes = name.as_bytes();
    if bytes.len() == 1 {
        let c = bytes[0].to_ascii_uppercase();
        if c.is_ascii_uppercase() {
            return Some(c - b'A' + 1);
        }
    }
    None
}

struct CellView {
    c: char,
    zerowidth: Vec<char>,
    fg: egui::Color32,
    bg: egui::Color32,
    italic: bool,
    underline: bool,
    /// Double-width character (CJK, emoji): occupies 2 cells.
    wide: bool,
    /// Ghost cell behind a wide character: background painted, glyph not.
    spacer: bool,
    /// OSC 8 hyperlink target carried by the cell, if any (terminal.md §12).
    link: Option<String>,
}

impl CellView {
    fn push_text(&self, out: &mut String) {
        out.push(self.c);
        out.extend(self.zerowidth.iter().copied());
    }

    fn text(&self) -> String {
        let mut text = String::new();
        self.push_text(&mut text);
        text
    }

    fn has_ink(&self) -> bool {
        self.c != ' ' || !self.zerowidth.is_empty()
    }
}

struct GridSnapshot {
    rows: Vec<Vec<CellView>>,
    /// `wrapped[line]` is set when the row soft-wraps into the next one (its last
    /// cell carries `WRAPLINE`): the link scanner joins such rows into one logical
    /// line (terminal.md §12).
    wrapped: Vec<bool>,
    cursor_line: usize,
    cursor_col: usize,
    /// The app's mouse-tracking state this frame (terminal.md §7): drives whether a
    /// click is forwarded to the PTY or kept as a local gesture.
    mouse: MouseProtocol,
}

impl GridSnapshot {
    fn char_rows(&self) -> Vec<Vec<char>> {
        self.rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.c).collect())
            .collect()
    }
}

fn rgb(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.r, c.g, c.b)
}

fn snapshot(grid: &SharedTerm, palette: &TermPalette) -> GridSnapshot {
    let waited = std::time::Instant::now();
    let term = grid.lock();
    crate::frame_log::add_lock_wait(waited.elapsed());
    let mouse = mouse_protocol(&term);
    let offset = term.grid().display_offset() as i32;
    let inner = term.grid();
    let lines = inner.screen_lines();
    let cols = inner.columns();
    let mut rows = Vec::with_capacity(lines);
    let mut wrapped = Vec::with_capacity(lines);
    for line in 0..lines {
        let row = &inner[Line(line as i32 - offset)];
        let mut cells = Vec::with_capacity(cols);
        for col in 0..cols {
            let cell = &row[Column(col)];
            let inverse = cell.flags.contains(Flags::INVERSE);
            let mut fg = palette.resolve(cell.fg);
            let mut bg = palette.resolve(cell.bg);
            if cell.flags.contains(Flags::DIM) {
                fg = palette.dim(fg);
            }
            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            cells.push(CellView {
                c: cell.c,
                zerowidth: cell.zerowidth().unwrap_or_default().to_vec(),
                fg: rgb(fg),
                bg: rgb(bg),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                wide: cell.flags.contains(Flags::WIDE_CHAR),
                spacer: cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                link: cell.hyperlink().map(|h| h.uri().to_string()),
            });
        }
        wrapped.push(cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE));
        rows.push(cells);
    }
    let cursor = inner.cursor.point;
    GridSnapshot {
        rows,
        wrapped,
        cursor_line: (cursor.line.0 + offset).max(0) as usize,
        cursor_col: cursor.column.0,
        mouse,
    }
}

fn line_job(cells: &[CellView], font_size: f32, line_height: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(font_size);
    for cell in cells {
        let underline = if cell.underline {
            egui::Stroke::new(1.0_f32, cell.fg)
        } else {
            egui::Stroke::NONE
        };
        job.append(
            &cell.text(),
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: cell.fg,
                background: cell.bg,
                italics: cell.italic,
                underline,
                line_height: Some(line_height),
                ..Default::default()
            },
        );
    }
    job
}

/// Glyph advance comparison tolerance: a glyph served by a fallback font
/// (advance ≠ mono cell) would shift the whole rest of the line if it stayed
/// in a run — it is instead centered alone in its cell.
const ADVANCE_EPSILON: f32 = 0.01;

/// Flat edge of powerline triangles stretched under the neighboring band:
/// without this overlap, antialiasing feathering leaves a seam of the background
/// between two flats of the same color.
const POWERLINE_OVERLAP: f32 = 0.5;

const Q_UL: (f32, f32, f32, f32) = (0.0, 0.0, 0.5, 0.5);
const Q_UR: (f32, f32, f32, f32) = (0.5, 0.0, 1.0, 0.5);
const Q_LL: (f32, f32, f32, f32) = (0.0, 0.5, 0.5, 1.0);
const Q_LR: (f32, f32, f32, f32) = (0.5, 0.5, 1.0, 1.0);
const B_FULL: (f32, f32, f32, f32) = (0.0, 0.0, 1.0, 1.0);

/// Glyph drawn by the painter rather than the font stack: font glyphs never fill
/// the cell exactly (`cell_h` = rounded font height + 1) — seams between blocks,
/// dotted progress bars, powerline triangles detached from their band. Like
/// Ghostty/Kitty, these characters are rendered as full-cell shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CellShape {
    /// Block elements: rects in cell fractions (x0, y0, x1, y1), top-left origin;
    /// the ░▒▓ shades are a fill at `alpha`.
    Blocks {
        rects: &'static [(f32, f32, f32, f32)],
        alpha: f32,
    },
    /// Solid triangle E0B0 (pointing right) / E0B2 (pointing left).
    PowerlineSolid { left: bool },
    /// Hollow chevron E0B1 / E0B3.
    PowerlineChevron { left: bool },
}

fn cell_shape(c: char) -> Option<CellShape> {
    let blocks = |rects: &'static [(f32, f32, f32, f32)]| CellShape::Blocks { rects, alpha: 1.0 };
    let shape = match c {
        '\u{E0B0}' => CellShape::PowerlineSolid { left: false },
        '\u{E0B1}' => CellShape::PowerlineChevron { left: false },
        '\u{E0B2}' => CellShape::PowerlineSolid { left: true },
        '\u{E0B3}' => CellShape::PowerlineChevron { left: true },
        '\u{2580}' => blocks(&[(0.0, 0.0, 1.0, 0.5)]),
        '\u{2581}' => blocks(&[(0.0, 0.875, 1.0, 1.0)]),
        '\u{2582}' => blocks(&[(0.0, 0.75, 1.0, 1.0)]),
        '\u{2583}' => blocks(&[(0.0, 0.625, 1.0, 1.0)]),
        '\u{2584}' => blocks(&[(0.0, 0.5, 1.0, 1.0)]),
        '\u{2585}' => blocks(&[(0.0, 0.375, 1.0, 1.0)]),
        '\u{2586}' => blocks(&[(0.0, 0.25, 1.0, 1.0)]),
        '\u{2587}' => blocks(&[(0.0, 0.125, 1.0, 1.0)]),
        '\u{2588}' => blocks(&[B_FULL]),
        '\u{2589}' => blocks(&[(0.0, 0.0, 0.875, 1.0)]),
        '\u{258A}' => blocks(&[(0.0, 0.0, 0.75, 1.0)]),
        '\u{258B}' => blocks(&[(0.0, 0.0, 0.625, 1.0)]),
        '\u{258C}' => blocks(&[(0.0, 0.0, 0.5, 1.0)]),
        '\u{258D}' => blocks(&[(0.0, 0.0, 0.375, 1.0)]),
        '\u{258E}' => blocks(&[(0.0, 0.0, 0.25, 1.0)]),
        '\u{258F}' => blocks(&[(0.0, 0.0, 0.125, 1.0)]),
        '\u{2590}' => blocks(&[(0.5, 0.0, 1.0, 1.0)]),
        '\u{2591}' => CellShape::Blocks {
            rects: &[B_FULL],
            alpha: 0.25,
        },
        '\u{2592}' => CellShape::Blocks {
            rects: &[B_FULL],
            alpha: 0.5,
        },
        '\u{2593}' => CellShape::Blocks {
            rects: &[B_FULL],
            alpha: 0.75,
        },
        '\u{2594}' => blocks(&[(0.0, 0.0, 1.0, 0.125)]),
        '\u{2595}' => blocks(&[(0.875, 0.0, 1.0, 1.0)]),
        '\u{2596}' => blocks(&[Q_LL]),
        '\u{2597}' => blocks(&[Q_LR]),
        '\u{2598}' => blocks(&[Q_UL]),
        '\u{2599}' => blocks(&[Q_UL, Q_LL, Q_LR]),
        '\u{259A}' => blocks(&[Q_UL, Q_LR]),
        '\u{259B}' => blocks(&[Q_UL, Q_UR, Q_LL]),
        '\u{259C}' => blocks(&[Q_UL, Q_UR, Q_LR]),
        '\u{259D}' => blocks(&[Q_UR]),
        '\u{259E}' => blocks(&[Q_UR, Q_LL]),
        '\u{259F}' => blocks(&[Q_UR, Q_LL, Q_LR]),
        _ => return None,
    };
    Some(shape)
}

/// Shapes spanning the full cell width (█, ░▒▓, ▀▄…) merge with identical
/// neighbors into a single rect: no antialiasing seam in the middle of a
/// progress bar.
fn full_width_blocks(shape: &CellShape) -> bool {
    matches!(shape, CellShape::Blocks { rects, .. }
        if rects.iter().all(|r| r.0 == 0.0 && r.2 == 1.0))
}

/// A line paint operation, in grid columns — produced under the fonts lock,
/// then painted at the exact `col × char_w` rects.
#[derive(Debug)]
enum PaintOp {
    Bg {
        col: usize,
        span: usize,
        color: egui::Color32,
    },
    /// Run of standard-advance glyphs, anchored at its starting column.
    Run {
        col: usize,
        galley: std::sync::Arc<egui::Galley>,
    },
    /// Fallback glyph (advance ≠ cell) or wide: centered over `span` cells.
    Loose {
        col: usize,
        span: usize,
        galley: std::sync::Arc<egui::Galley>,
    },
    Shape {
        col: usize,
        span: usize,
        shape: CellShape,
        color: egui::Color32,
    },
}

fn cell_format(cell: &CellView, font: egui::FontId, row_h: f32) -> egui::TextFormat {
    let underline = if cell.underline {
        egui::Stroke::new(1.0_f32, cell.fg)
    } else {
        egui::Stroke::NONE
    };
    egui::TextFormat {
        font_id: font,
        color: cell.fg,
        italics: cell.italic,
        underline,
        line_height: Some(row_h),
        ..Default::default()
    }
}

fn single_glyph_job(cell: &CellView, font: egui::FontId, row_h: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(&cell.text(), 0.0, cell_format(cell, font, row_h));
    job
}

/// Splits a line into grid-aligned operations: backgrounds merged by color
/// (full-cell rects, seamless), standard-advance text runs anchored at their
/// column, fallback glyphs centered in their cell(s), procedural shapes. This
/// alignment is what keeps a fallback glyph from shifting the rest of the line.
fn row_paint_ops(
    fonts: &mut egui::epaint::FontsView<'_>,
    cells: &[CellView],
    font_size: f32,
    char_w: f32,
    row_h: f32,
    bg_default: egui::Color32,
) -> Vec<PaintOp> {
    let font = egui::FontId::monospace(font_size);
    let mut ops = Vec::new();

    let mut col = 0;
    while col < cells.len() {
        let color = cells[col].bg;
        let start = col;
        while col < cells.len() && cells[col].bg == color {
            col += 1;
        }
        if color != bg_default {
            ops.push(PaintOp::Bg {
                col: start,
                span: col - start,
                color,
            });
        }
    }

    let mut col = 0;
    while col < cells.len() {
        let cell = &cells[col];
        if cell.spacer {
            col += 1;
            continue;
        }
        let span = if cell.wide { 2 } else { 1 };
        if cell.zerowidth.is_empty() {
            if let Some(shape) = cell_shape(cell.c) {
                let mut end = col + span;
                if full_width_blocks(&shape) {
                    while end < cells.len()
                        && cells[end].c == cell.c
                        && cells[end].zerowidth.is_empty()
                        && cells[end].fg == cell.fg
                    {
                        end += 1;
                    }
                }
                ops.push(PaintOp::Shape {
                    col,
                    span: end - col,
                    shape,
                    color: cell.fg,
                });
                col = end;
                continue;
            }
        }
        let advance = fonts.glyph_width(&font, cell.c);
        if cell.wide || (advance - char_w).abs() > ADVANCE_EPSILON {
            let galley = fonts.layout_job(single_glyph_job(cell, font.clone(), row_h));
            ops.push(PaintOp::Loose { col, span, galley });
            col += span;
            continue;
        }
        let key = (cell.fg, cell.italic, cell.underline);
        let start = col;
        let mut text = String::new();
        let mut has_ink = false;
        while col < cells.len() {
            let next = &cells[col];
            if next.spacer
                || next.wide
                || (next.fg, next.italic, next.underline) != key
                || (next.zerowidth.is_empty() && cell_shape(next.c).is_some())
                || (fonts.glyph_width(&font, next.c) - char_w).abs() > ADVANCE_EPSILON
            {
                break;
            }
            has_ink |= next.has_ink();
            next.push_text(&mut text);
            col += 1;
        }
        // A run of bare spaces has nothing to paint: the background is already laid down.
        if has_ink || cell.underline {
            let mut job = egui::text::LayoutJob::default();
            job.append(&text, 0.0, cell_format(cell, font.clone(), row_h));
            ops.push(PaintOp::Run {
                col: start,
                galley: fonts.layout_job(job),
            });
        }
    }
    ops
}

fn grid_cell_rect(
    origin: egui::Pos2,
    col: usize,
    span: usize,
    char_w: f32,
    row_h: f32,
) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(origin.x + col as f32 * char_w, origin.y),
        egui::vec2(span as f32 * char_w, row_h),
    )
}

fn paint_row_ops(
    painter: &egui::Painter,
    origin: egui::Pos2,
    ops: &[PaintOp],
    char_w: f32,
    row_h: f32,
) {
    for op in ops {
        match op {
            PaintOp::Bg { col, span, color } => {
                painter.rect_filled(
                    grid_cell_rect(origin, *col, *span, char_w, row_h),
                    0.0,
                    *color,
                );
            }
            PaintOp::Run { col, galley } => {
                painter.galley(
                    egui::pos2(origin.x + *col as f32 * char_w, origin.y),
                    galley.clone(),
                    egui::Color32::WHITE,
                );
            }
            PaintOp::Loose { col, span, galley } => {
                let x = origin.x
                    + *col as f32 * char_w
                    + (*span as f32 * char_w - galley.size().x) / 2.0;
                painter.galley(
                    egui::pos2(x, origin.y),
                    galley.clone(),
                    egui::Color32::WHITE,
                );
            }
            PaintOp::Shape {
                col,
                span,
                shape,
                color,
            } => {
                paint_cell_shape(
                    painter,
                    grid_cell_rect(origin, *col, *span, char_w, row_h),
                    *shape,
                    *color,
                );
            }
        }
    }
}

fn paint_cell_shape(
    painter: &egui::Painter,
    rect: egui::Rect,
    shape: CellShape,
    color: egui::Color32,
) {
    match shape {
        CellShape::Blocks { rects, alpha } => {
            let fill = if alpha < 1.0 {
                color.gamma_multiply(alpha)
            } else {
                color
            };
            for (x0, y0, x1, y1) in rects {
                let r = egui::Rect::from_min_max(
                    egui::pos2(
                        rect.min.x + rect.width() * x0,
                        rect.min.y + rect.height() * y0,
                    ),
                    egui::pos2(
                        rect.min.x + rect.width() * x1,
                        rect.min.y + rect.height() * y1,
                    ),
                );
                painter.rect_filled(r, 0.0, fill);
            }
        }
        CellShape::PowerlineSolid { left } => {
            let (flat, apex) = if left {
                (rect.right() + POWERLINE_OVERLAP, rect.left())
            } else {
                (rect.left() - POWERLINE_OVERLAP, rect.right())
            };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(flat, rect.top()),
                    egui::pos2(apex, rect.center().y),
                    egui::pos2(flat, rect.bottom()),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        CellShape::PowerlineChevron { left } => {
            let (flat, apex) = if left {
                (rect.right(), rect.left())
            } else {
                (rect.left(), rect.right())
            };
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(flat, rect.top()),
                    egui::pos2(apex, rect.center().y),
                    egui::pos2(flat, rect.bottom()),
                ],
                egui::Stroke::new(1.0_f32, color),
            ));
        }
    }
}

/// `Fonts::row_height` doesn't reflect the physical-pixel rounding applied at
/// render time (`galley_from_rows`): the discrepancy accumulates over the pane
/// height and truncates the last line. So we measure a one-line galley, which
/// gives the height actually allocated by `ui.label`.
///
/// The cell is raised to `ceil(ascent − descent + gap) + 1`: the font's bare
/// height makes descenders (p, g, y) stick to the bottom of the cell. The `ceil`
/// alone (= `NSLayoutManager::defaultLineHeight`) is fully
/// absorbed by baseline rounding and hinting — measured at the pixel, descender
/// ink still reaches the cell's last line. The +1 pt guarantees visible room
/// below the descent (~1.31 em at 13 pt, within terminal norms). The space goes
/// below the baseline (epaint keeps the baseline at `ascent`).
pub fn cell_metrics(ctx: &egui::Context, font_size: f32) -> (f32, f32) {
    let font = egui::FontId::monospace(font_size);
    ctx.fonts_mut(|f| {
        let plain = f.layout_no_wrap(" ".to_owned(), font.clone(), egui::Color32::WHITE);
        let cell_h = plain.rows[0].glyphs[0].font_height.ceil() + 1.0;
        let row = f.layout_job(line_job(
            &[CellView {
                c: ' ',
                zerowidth: Vec::new(),
                fg: egui::Color32::WHITE,
                bg: egui::Color32::TRANSPARENT,
                italic: false,
                underline: false,
                wide: false,
                spacer: false,
                link: None,
            }],
            font_size,
            cell_h,
        ));
        (f.glyph_width(&font, ' ').max(1.0), row.size().y.max(1.0))
    })
}

fn grid_size_for(area: egui::Vec2, char_w: f32, row_h: f32) -> GridSize {
    let cols = (area.x / char_w).floor() as u16;
    let rows = (area.y / row_h).floor() as u16;
    GridSize {
        rows: rows.max(MIN_LINES),
        cols: cols.max(MIN_COLS),
    }
}

fn collect_input(ui: &egui::Ui) -> Vec<u8> {
    ui.ctx()
        .input(|input| input_bytes_from_events(&input.events))
}

fn input_bytes_from_events(events: &[egui::Event]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, event) in events.iter().enumerate() {
        match event {
            egui::Event::Text(text) => bytes.extend_from_slice(text.as_bytes()),
            egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                bytes.extend_from_slice(text.as_bytes())
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if let Some(seq) = key_bytes(*key, *modifiers) {
                    bytes.extend_from_slice(&seq);
                } else if next_event_has_no_textual_input(events, index) {
                    bytes.extend_from_slice(printable_key_fallback(*key, *modifiers));
                }
            }
            _ => {}
        }
    }
    bytes
}

fn next_event_has_no_textual_input(events: &[egui::Event], index: usize) -> bool {
    !matches!(
        events.get(index + 1),
        Some(egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_)))
    )
}

fn printable_key_fallback(key: egui::Key, modifiers: egui::Modifiers) -> &'static [u8] {
    if modifiers.command || modifiers.mac_cmd || modifiers.ctrl {
        return b"";
    }
    match (key, modifiers.shift) {
        (egui::Key::Backtick, false) => b"`",
        (egui::Key::Backtick, true) => b"~",
        (egui::Key::Quote, false) => b"'",
        (egui::Key::Quote, true) => b"\"",
        (egui::Key::Num6, true) => b"^",
        _ => b"",
    }
}

/// Paints the grid background + rows + cursor + exited banner into `ui`, returning
/// the painted region. Shared by the interactive [`terminal_view`] and the
/// read-only [`terminal_view_readonly`]; the dim, selection, focus and input
/// handling stay in the callers.
#[allow(clippy::too_many_arguments)]
fn paint_grid(
    ui: &mut egui::Ui,
    snap: &GridSnapshot,
    area: egui::Vec2,
    char_w: f32,
    row_h: f32,
    font_size: f32,
    palette: &TermPalette,
    focused: bool,
    exited: bool,
) -> egui::Rect {
    let inner = egui::Frame::new()
        .fill(rgb(palette.background))
        .show(ui, |ui| {
            ui.set_min_size(area);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

            let bg_default = rgb(palette.background);
            let row_ops: Vec<Vec<PaintOp>> = ui.ctx().fonts_mut(|f| {
                snap.rows
                    .iter()
                    .map(|cells| row_paint_ops(f, cells, font_size, char_w, row_h, bg_default))
                    .collect()
            });

            for (line, (cells, ops)) in snap.rows.iter().zip(&row_ops).enumerate() {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(area.x, row_h), egui::Sense::hover());
                // Line text exposed to accessibility (VoiceOver, kittest).
                resp.widget_info(|| {
                    let mut text = String::new();
                    for cell in cells {
                        cell.push_text(&mut text);
                    }
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text)
                });
                paint_row_ops(ui.painter(), rect.min, ops, char_w, row_h);
                if !exited && line == snap.cursor_line {
                    paint_cursor(
                        ui.painter(),
                        grid_cell_rect(rect.min, snap.cursor_col, 1, char_w, row_h),
                        rgb(palette.foreground),
                        cursor_shape(focused),
                    );
                }
            }

            if exited {
                ui.label(
                    egui::RichText::new(PROCESS_ENDED_BANNER)
                        .monospace()
                        .color(rgb(palette.foreground)),
                );
            }
        });
    inner.response.rect
}

/// Wheel-driven scroll + the grid size to fit, for the read-only Run terminal
/// (git.md §3): the panel mirrors a server's output with no keyboard input.
pub struct ReadonlyTerminalOutput {
    pub size: GridSize,
    /// Local scrollback walk this frame (wheel only — never forwarded to the PTY).
    pub scroll: Option<ScrollKind>,
}

/// Read-only terminal viewer: paints the grid and walks the scrollback on wheel,
/// but takes no keyboard focus and forwards nothing to the PTY — the Run panel
/// drives the process through its own buttons (git.md §3).
pub fn terminal_view_readonly(
    ui: &mut egui::Ui,
    grid: &SharedTerm,
    palette: &TermPalette,
    font_size: f32,
    exited: bool,
) -> ReadonlyTerminalOutput {
    let snap = snapshot(grid, palette);
    let (char_w, row_h) = cell_metrics(ui.ctx(), font_size);
    let area = ui.available_size();
    let grid_area = egui::vec2(area.x, (area.y - BOTTOM_PAD).max(0.0));
    let region = paint_grid(
        ui, &snap, area, char_w, row_h, font_size, palette, false, exited,
    );
    let id = ui.id().with("run_terminal_scroll");
    let response = ui.interact(region, id, egui::Sense::hover());
    let scroll = response
        .hovered()
        .then(|| wheel_scroll(ui, id, row_h).map(ScrollKind::Lines))
        .flatten();
    if scroll.is_some() {
        ui.ctx().request_repaint();
    }
    ReadonlyTerminalOutput {
        size: grid_size_for(grid_area, char_w, row_h),
        scroll,
    }
}

/// Glanceable progress preview for the agents dashboard's collapsed cards: the
/// agent's last few **conversation** rows at readable native size, left-aligned and
/// clipped to the card width with a soft right-edge fade. The agent's chrome is
/// dropped (see [`condense_preview_rows`]) — its bottom composer / status block and
/// any box-framed startup banner left on screen — so a condensed glance shows what
/// the agent is *doing*, not its input UI; the header badge already carries the live
/// state. The pane is never resized; takes no focus, draws no cursor, forwards
/// nothing; hugs the rows it shows.
pub fn terminal_view_preview(
    ui: &mut egui::Ui,
    grid: &SharedTerm,
    palette: &TermPalette,
    font_size: f32,
    lines: usize,
) {
    let mut snap = snapshot(grid, palette);
    let rows = std::mem::take(&mut snap.rows);
    snap.rows = condense_preview_rows(rows, snap.cursor_line, lines);
    snap.cursor_line = usize::MAX; // static preview, no cursor

    let (char_w, row_h) = cell_metrics(ui.ctx(), font_size);
    let width = ui.available_width().max(1.0);
    let area = egui::vec2(width, snap.rows.len() as f32 * row_h);
    let (rect, _) = ui.allocate_exact_size(area, egui::Sense::hover());
    if snap.rows.is_empty() {
        return;
    }
    // Clip to the card width: a transcript line wider than the card is cut at the
    // right edge (left-aligned content stays readable) rather than forcing the
    // column wider or bleeding over the next card. Intersect with the inherited clip
    // so a card scrolled under the sidebar stays clipped to the scroll viewport
    // instead of painting over it.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    paint_grid(
        &mut child, &snap, area, char_w, row_h, font_size, palette, false, false,
    );
    paint_right_fade(ui.painter(), rect, rgb(palette.background));
    // A collapsed card is never the keyboard target, so its preview carries the same
    // dim an unfocused split does: the wall then recedes uniformly, collapsed previews
    // and mirrored terminals alike, behind the one active terminal.
    let bg = palette.background;
    ui.painter().rect_filled(
        rect,
        0.0,
        egui::Color32::from_rgba_unmultiplied(bg.r, bg.g, bg.b, UNFOCUSED_DIM_ALPHA),
    );
}

/// Reduces a snapshot's rows to the condensed preview a collapsed card shows: drop
/// the bottom chrome block from the composer down, then keep only **conversation**
/// — blank lines and box-framed chrome (the composer itself, and any top startup
/// banner still on screen) fall out — and finally keep the last `lines` of it.
fn condense_preview_rows(
    mut rows: Vec<Vec<CellView>>,
    cursor_line: usize,
    lines: usize,
) -> Vec<Vec<CellView>> {
    rows.truncate(composer_block_top(&rows, cursor_line));
    rows.retain(|row| row.iter().any(CellView::has_ink) && !is_box_part(row));
    if rows.len() > lines {
        rows.drain(..rows.len() - lines);
    }
    rows
}

/// First row of an agent TUI's bottom **chrome block** — its composer (boxed for
/// Claude Code, a bare prompt line for Codex) plus the status / hint lines under it
/// — so a condensed preview can drop everything from there down and keep only the
/// conversation above. The **cursor** is the generic anchor (no per-agent parsing):
/// it lives in the input composer, always pinned to the bottom of the screen. We
/// trust it only when it actually sits in the lower half — a cursor parked high
/// (fresh banner, no input yet) is not a composer — and then walk up over any
/// box-walled rows above it to clear a multi-line boxed composer's top edge.
/// Returns `rows.len()` (cut nothing here) when the cursor is unusable.
fn composer_block_top(rows: &[Vec<CellView>], cursor_line: usize) -> usize {
    if cursor_line >= rows.len() || cursor_line * 2 < rows.len() {
        return rows.len();
    }
    let mut top = cursor_line;
    while top > 0 && is_box_part(&rows[top - 1]) {
        top -= 1;
    }
    top
}

/// A row whose inked content is at least half box-drawing glyphs — a composer's
/// `╭─╮` / `╰─╯` border, or the `│ … │` walls of a near-empty input line.
fn is_box_dominated(row: &[CellView]) -> bool {
    let mut inked = 0usize;
    let mut box_glyphs = 0usize;
    for cell in row {
        if !cell.has_ink() {
            continue;
        }
        inked += 1;
        if is_box_drawing(cell.c) {
            box_glyphs += 1;
        }
    }
    inked > 0 && box_glyphs * 2 >= inked
}

/// Part of the composer box: a box-dominated border, or a **box-walled** input row
/// whose first and last inked cells are both box-drawing (`│ > … │`).
fn is_box_part(row: &[CellView]) -> bool {
    if is_box_dominated(row) {
        return true;
    }
    let first = row.iter().find(|c| c.has_ink());
    let last = row.iter().rev().find(|c| c.has_ink());
    matches!((first, last), (Some(f), Some(l)) if is_box_drawing(f.c) && is_box_drawing(l.c))
}

fn is_box_drawing(c: char) -> bool {
    matches!(c, '\u{2500}'..='\u{259F}')
}

/// Fades the right edge of `rect` to `bg` so a transcript line clipped at the card
/// edge trails off instead of cutting hard.
fn paint_right_fade(painter: &egui::Painter, rect: egui::Rect, bg: egui::Color32) {
    let fade_w = (rect.width() * 0.18).clamp(12.0, 32.0);
    let x0 = rect.right() - fade_w;
    let transparent = egui::Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(egui::pos2(x0, rect.top()), transparent);
    mesh.colored_vertex(egui::pos2(rect.right(), rect.top()), bg);
    mesh.colored_vertex(egui::pos2(rect.right(), rect.bottom()), bg);
    mesh.colored_vertex(egui::pos2(x0, rect.bottom()), transparent);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

#[allow(clippy::too_many_arguments)]
pub fn terminal_view(
    ui: &mut egui::Ui,
    grid: &SharedTerm,
    palette: &TermPalette,
    font_size: f32,
    focused: bool,
    exited: bool,
    clear_shortcut: Option<Shortcut>,
    // Resolution root for relative file links when Cmd is held (the pane's live
    // shell cwd); `None` disables link detection — every other gesture is then
    // byte-identical (terminal.md §12).
    link_cwd: Option<&Path>,
) -> TerminalInput {
    let snap = snapshot(grid, palette);
    let (char_w, row_h) = cell_metrics(ui.ctx(), font_size);
    let area = ui.available_size();
    // The background fills the whole area, but the grid stops BOTTOM_PAD above the
    // edge so the last line doesn't touch the bottom of the window.
    let grid_area = egui::vec2(area.x, (area.y - BOTTOM_PAD).max(0.0));

    let region = paint_grid(
        ui, &snap, area, char_w, row_h, font_size, palette, focused, exited,
    );
    let id = ui.id().with("terminal_focus");
    let response = ui.interact(region, id, egui::Sense::click_and_drag());
    let clicked = response.clicked();
    if response.drag_started() || clicked {
        response.request_focus();
    } else if focused && !response.has_focus() {
        // Only claim focus if no widget holds it: otherwise the active pane would
        // steal it from the commit fields the moment they're clicked. The outgoing
        // pane surrenders it below, letting keyboard navigation switch focus.
        if ui.memory(|m| m.focused()).is_none() {
            response.request_focus();
        }
    } else if !focused && response.has_focus() {
        response.surrender_focus();
        ui.ctx().request_repaint();
    }
    if response.has_focus() {
        // Tab/arrows/Esc belong to the PTY: without this lock, egui moves focus
        // (Shift+Tab notably) instead of forwarding the key.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }

    let cmd_held = ui
        .ctx()
        .input(|i| i.modifiers.command || i.modifiers.mac_cmd);
    let shift_held = ui.ctx().input(|i| i.modifiers.shift);
    // An app in mouse reporting (e.g. Claude Code) owns the click — forward it to the
    // PTY as a mouse report rather than selecting locally (terminal.md §7). Shift
    // (force-local selection) and Cmd (link affordance, §12) keep their gestures.
    let forward_mouse = !exited && snap.mouse.reporting && !cmd_held && !shift_held;

    let selection = (!forward_mouse)
        .then(|| update_selection(ui, id, &response, region, char_w, row_h))
        .flatten();
    let mut char_rows = selection.map(|_| snap.char_rows());
    if let (Some(sel), Some(char_rows)) = (selection, &char_rows) {
        paint_selection(
            ui.painter(),
            region,
            char_w,
            row_h,
            char_rows,
            &sel,
            palette,
        );
    }

    if !focused {
        let bg = palette.background;
        ui.painter().rect_filled(
            region,
            0.0,
            egui::Color32::from_rgba_unmultiplied(bg.r, bg.g, bg.b, UNFOCUSED_DIM_ALPHA),
        );
    }

    // Cmd+hover link affordance (terminal.md §12): a single hit-test per frame,
    // gated on `link_cwd` so the non-Cmd path above stays untouched. Painted last —
    // over the dim — and fg-colored, with no LayoutJob re-layout.
    let mut open_link = None;
    if let Some(cwd) = link_cwd {
        let hover = (cmd_held && response.hovered())
            .then(|| ui.input(|i| i.pointer.interact_pos()))
            .flatten()
            .map(|pos| point_to_cell(pos, region, char_w, row_h));
        if let Some(link) = hover.and_then(|cell| hovered_link(&snap, cell, cwd)) {
            paint_link_underline(ui.painter(), region, char_w, row_h, &snap, &link.cells);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if clicked {
                open_link = Some(link.action);
            }
        }
    }

    let listening = focused && response.has_focus();
    sync_terminal_ime_purpose(ui.ctx(), id, listening);
    if listening {
        request_terminal_ime(ui, region, &snap, char_w, row_h);
    }
    let mut bytes = Vec::new();
    let mut paste = None;
    let mut relaunch = false;
    let mut clear = false;
    let mut scroll = None;
    let mut scroll_bytes = Vec::new();
    let mut mouse_bytes = Vec::new();
    if forward_mouse {
        let cols = snap.rows.first().map_or(0, Vec::len);
        let rows = snap.rows.len();
        for ev in collect_mouse_events(ui, region, char_w, row_h, snap.mouse.motion) {
            let line = (ev.cell.line.max(0) as usize).min(rows.saturating_sub(1));
            let col = ev.cell.col.min(cols.saturating_sub(1));
            if let Some(b) = mouse_report(snap.mouse, ev.button, ev.kind, ev.mods, line, col) {
                mouse_bytes.extend(b);
            }
        }
    }
    if response.hovered() {
        let pending_id = id.with("wheel_pending");
        let pending = ui
            .ctx()
            .data_mut(|d| d.get_temp::<i32>(pending_id))
            .unwrap_or(0);
        let (mut pending, step) = pending_step(pending, wheel_scroll(ui, id, row_h));
        if pending != 0 {
            // Shift+wheel forces local scrollback (terminal convention).
            let shift = ui.ctx().input(|i| i.modifiers.shift);
            let cell = ui
                .input(|i| i.pointer.hover_pos())
                .map(|pos| point_to_cell(pos, region, char_w, row_h))
                .unwrap_or(Cell { line: 0, col: 0 });
            // Forwarded to a TUI, cap the per-frame burst; the rest drains on the
            // next frames (the scroll_bytes repaint below re-enters), turning a peak
            // frame into even steps. Local scrollback renders at once: no cap.
            let forwarded = (!shift)
                .then(|| {
                    let waited = std::time::Instant::now();
                    let term = grid.lock();
                    crate::frame_log::add_lock_wait(waited.elapsed());
                    wheel_bytes(&term, step, cell.line.max(0) as usize, cell.col)
                })
                .flatten();
            match forwarded {
                Some(b) => {
                    scroll_bytes = b;
                    pending -= step;
                }
                None => {
                    scroll = Some(ScrollKind::Lines(pending));
                    pending = 0;
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(pending_id, pending));
    }
    if listening {
        if exited {
            relaunch = enter_pressed(ui);
        } else {
            bytes = collect_input(ui);
            paste = collect_paste(ui);
            clear = clear_pressed(ui, clear_shortcut);
            if clear {
                ui.ctx().request_repaint();
            }
            if copy_requested(ui) {
                if let Some(sel) = selection.filter(|s| !s.is_empty()) {
                    let char_rows = char_rows.get_or_insert_with(|| snap.char_rows());
                    let text = selected_text(&sel, char_rows, 0);
                    if !text.is_empty() {
                        ui.ctx().copy_text(text);
                    }
                }
            }
            if let Some(page) = page_scroll(ui) {
                scroll = Some(page);
            }
            // Any keystroke forwarded to the PTY snaps the view to the bottom (terminal.md §8).
            if !bytes.is_empty() {
                scroll = Some(ScrollKind::Bottom);
            }
        }
    }
    // winit surfaces no drop position (`draggingLocation` ignored on macOS) and
    // the egui pointer is stale during an external drag: the mouse is read from
    // CoreGraphics at drop time to target the hovered pane.
    if !exited {
        if let Some(dropped) = collect_dropped_paths(ui) {
            if drop_targets_pane(drop_pointer_pos(ui.ctx()), region, focused) {
                paste = Some(paste.unwrap_or_default() + &dropped);
            }
        }
    }
    if scroll.is_some() || !scroll_bytes.is_empty() {
        // Deadline, not an immediate request: the drain re-enters at ~60 FPS
        // while eframe stays in `ControlFlow::WaitUntil`, where macOS delivers
        // wheel events evenly. 33 ms because egui subtracts a constant 1/60 s
        // `predicted_dt` from every deadline — 16 ms would saturate to zero and
        // become an immediate request again (see TERMINAL_REDRAW_INTERVAL).
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }

    TerminalInput {
        bytes,
        paste,
        size: grid_size_for(grid_area, char_w, row_h),
        relaunch,
        clicked,
        clear,
        scroll,
        scroll_bytes,
        mouse_bytes,
        open_link,
    }
}

/// Per-frame scroll amount in lines (>0 = back in history). Reads egui's
/// **smoothed** scroll delta — the same low-pass its own `ScrollArea` uses —
/// rather than the raw wheel events: a single mouse-wheel notch arrives as one
/// large delta, which egui then spreads across several frames. Without this, the
/// notch would land as one jump and, forwarded to a full-screen TUI as a burst of
/// ↑/↓ arrows (terminal.md §8), scroll in jerks. `Shift` is not required to walk
/// the scrollback, but egui reroutes a Shift gesture onto the horizontal axis, so
/// that axis is read back here.
fn wheel_scroll(ui: &egui::Ui, id: egui::Id, row_h: f32) -> Option<i32> {
    let points = ui.ctx().input(|i| {
        let d = i.smooth_scroll_delta;
        if i.modifiers.shift {
            d.x
        } else {
            d.y
        }
    });
    let delta = points / row_h.max(1.0);
    if delta == 0.0 {
        return None;
    }
    let acc_id = id.with("wheel_acc");
    let acc = ui
        .ctx()
        .data_mut(|d| d.get_temp::<f32>(acc_id))
        .unwrap_or(0.0);
    let (lines, rest) = accumulate_wheel(acc, delta);
    ui.ctx().data_mut(|d| d.insert_temp(acc_id, rest));
    (lines != 0).then_some(lines)
}

/// Carries the fractional remainder across frames: a slow trackpad scroll
/// delivers sub-row deltas each frame that would otherwise all round to 0.
/// A direction change drops the leftover.
fn accumulate_wheel(acc: f32, delta: f32) -> (i32, f32) {
    let total = if acc * delta < 0.0 {
        delta
    } else {
        acc + delta
    };
    let lines = total.trunc();
    (lines as i32, total - lines)
}

/// Folds the frame's whole-line delta into the carried `pending` count (a
/// direction change drops the leftover, like `accumulate_wheel`), then splits it
/// into the `step` forwarded to a TUI this frame — capped at `SCROLL_STEP_CAP` so
/// a peak frame doesn't scroll many lines in one redraw (terminal.md §8) — and the
/// remainder. Returns `(pending_after_add, step)`: the forwarded path keeps
/// `pending_after_add - step` for the next frames, the uncapped local-scrollback
/// path scrolls `pending_after_add` at once.
fn pending_step(pending: i32, new_lines: Option<i32>) -> (i32, i32) {
    let pending = match new_lines {
        Some(lines) if pending.signum() * lines.signum() < 0 => lines,
        Some(lines) => pending + lines,
        None => pending,
    };
    let step = pending.signum() * pending.abs().min(SCROLL_STEP_CAP);
    (pending, step)
}

/// `Shift+PageUp` / `Shift+PageDown` walk the scrollback by page
/// (keybindings.md §2).
fn page_scroll(ui: &egui::Ui) -> Option<ScrollKind> {
    ui.ctx().input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key: egui::Key::PageUp,
                pressed: true,
                modifiers,
                ..
            } if is_page_scroll_mods(*modifiers) => Some(ScrollKind::PageUp),
            egui::Event::Key {
                key: egui::Key::PageDown,
                pressed: true,
                modifiers,
                ..
            } if is_page_scroll_mods(*modifiers) => Some(ScrollKind::PageDown),
            _ => None,
        })
    })
}

fn is_page_scroll_mods(modifiers: egui::Modifiers) -> bool {
    modifiers.shift && !modifiers.command && !modifiers.mac_cmd && !modifiers.alt && !modifiers.ctrl
}

/// Pointer position → grid cell (line/column, clamped to the grid).
fn point_to_cell(pos: egui::Pos2, region: egui::Rect, char_w: f32, row_h: f32) -> Cell {
    let line = ((pos.y - region.top()) / row_h).floor().max(0.0) as i32;
    let col = ((pos.x - region.left()) / char_w).floor().max(0.0) as usize;
    Cell { line, col }
}

fn request_terminal_ime(
    ui: &mut egui::Ui,
    region: egui::Rect,
    snap: &GridSnapshot,
    char_w: f32,
    row_h: f32,
) {
    let cursor_line = snap.cursor_line.min(snap.rows.len().saturating_sub(1));
    let cursor_col = snap
        .rows
        .get(cursor_line)
        .map(|row| snap.cursor_col.min(row.len().saturating_sub(1)))
        .unwrap_or(0);
    let cursor_rect = grid_cell_rect(
        egui::pos2(region.left(), region.top() + cursor_line as f32 * row_h),
        cursor_col,
        1,
        char_w,
        row_h,
    );
    let to_global = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or_default();
    ui.output_mut(|output| {
        output.ime = Some(egui::output::IMEOutput {
            rect: to_global * region,
            cursor_rect: to_global * cursor_rect,
        });
    });
}

/// `send_viewport_cmd` requests a repaint on every call, so emitting the IME
/// purpose each frame pins egui at a continuous repaint (and overruns the
/// kittest step budget). Fire it only on the focus edge, tracking the last
/// applied state per pane.
fn sync_terminal_ime_purpose(ctx: &egui::Context, id: egui::Id, listening: bool) {
    let key = id.with("ime_purpose_active");
    let was_active = ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if listening == was_active {
        return;
    }
    let purpose = if listening {
        egui::viewport::IMEPurpose::Terminal
    } else {
        egui::viewport::IMEPurpose::Normal
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::IMEPurpose(purpose));
    ctx.data_mut(|d| d.insert_temp(key, listening));
}

/// A link under the pointer: the action plus the visible cells (line, col) to
/// underline.
struct HoverLink {
    action: LinkAction,
    cells: Vec<(usize, usize)>,
}

/// A link can span at most this many soft-wrapped visual rows — the join window
/// the scanner walks around the hovered cell (terminal.md §12).
const MAX_LOGICAL_ROWS: usize = 8;

/// Detect the link under `hover`: rebuild the logical (soft-wrapped) line by
/// joining contiguous `WRAPLINE` rows around it — capped at `MAX_LOGICAL_ROWS` —
/// then hand the flat text + per-cell OSC 8 URIs to the domain scanner. The
/// returned range is mapped back to grid cells for underlining.
fn hovered_link(snap: &GridSnapshot, hover: Cell, cwd: &Path) -> Option<HoverLink> {
    let line = hover.line.max(0) as usize;
    let cols = snap.rows.get(line)?.len();
    if cols == 0 || hover.col >= cols {
        return None;
    }
    let mut start = line;
    while start > 0 && snap.wrapped[start - 1] && (line - start + 1) < MAX_LOGICAL_ROWS {
        start -= 1;
    }
    let mut end = line;
    while end + 1 < snap.rows.len() && snap.wrapped[end] && (end - start + 1) < MAX_LOGICAL_ROWS {
        end += 1;
    }
    let mut text = String::new();
    let mut uris = Vec::with_capacity((end - start + 1) * cols);
    for row in &snap.rows[start..=end] {
        for cell in row {
            text.push(cell.c);
            uris.push(cell.link.clone());
        }
    }
    let idx = (line - start) * cols + hover.col;
    let link = link_at(&text, &uris, idx, cwd)?;
    let cells = link.range.map(|f| (start + f / cols, f % cols)).collect();
    Some(HoverLink {
        action: link.action,
        cells,
    })
}

/// Underlines the hovered link's cells with each cell's own foreground color, a
/// 1px line at the bottom of the row — a paint pass over the rendered grid, no
/// LayoutJob re-layout.
fn paint_link_underline(
    painter: &egui::Painter,
    region: egui::Rect,
    char_w: f32,
    row_h: f32,
    snap: &GridSnapshot,
    cells: &[(usize, usize)],
) {
    for &(line, col) in cells {
        let color = snap.rows[line][col].fg;
        let y = region.top() + (line as f32 + 1.0) * row_h - 1.0;
        let x0 = region.left() + col as f32 * char_w;
        painter.line_segment(
            [egui::pos2(x0, y), egui::pos2(x0 + char_w, y)],
            egui::Stroke::new(1.0_f32, color),
        );
    }
}

/// A pointer event to relay to an app in mouse reporting (terminal.md §7).
struct PointerReport {
    button: MouseButton,
    kind: MouseKind,
    mods: MouseMods,
    cell: Cell,
}

/// Pointer presses, releases and (under modes 1002/1003) button-held drags that
/// fall inside the pane, in event order, ready to encode as PTY mouse reports.
/// Cmd/Shift gestures never reach here — they stay link / local-selection upstream.
fn collect_mouse_events(
    ui: &egui::Ui,
    region: egui::Rect,
    char_w: f32,
    row_h: f32,
    motion: bool,
) -> Vec<PointerReport> {
    ui.input(|i| {
        let mut out = Vec::new();
        for event in &i.events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } if region.contains(*pos) => {
                    let Some(button) = mouse_button(*button) else {
                        continue;
                    };
                    out.push(PointerReport {
                        button,
                        kind: if *pressed {
                            MouseKind::Press
                        } else {
                            MouseKind::Release
                        },
                        mods: MouseMods {
                            alt: modifiers.alt,
                            ctrl: modifiers.ctrl,
                        },
                        cell: point_to_cell(*pos, region, char_w, row_h),
                    });
                }
                egui::Event::PointerMoved(pos) if motion && region.contains(*pos) => {
                    let Some(button) = held_button(&i.pointer) else {
                        continue;
                    };
                    out.push(PointerReport {
                        button,
                        kind: MouseKind::Drag,
                        mods: MouseMods {
                            alt: i.modifiers.alt,
                            ctrl: i.modifiers.ctrl,
                        },
                        cell: point_to_cell(*pos, region, char_w, row_h),
                    });
                }
                _ => {}
            }
        }
        out
    })
}

fn mouse_button(button: egui::PointerButton) -> Option<MouseButton> {
    match button {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        egui::PointerButton::Extra1 | egui::PointerButton::Extra2 => None,
    }
}

/// The button held during a drag (left wins over middle over right) — its code goes
/// into the motion report.
fn held_button(pointer: &egui::PointerState) -> Option<MouseButton> {
    if pointer.button_down(egui::PointerButton::Primary) {
        Some(MouseButton::Left)
    } else if pointer.button_down(egui::PointerButton::Middle) {
        Some(MouseButton::Middle)
    } else if pointer.button_down(egui::PointerButton::Secondary) {
        Some(MouseButton::Right)
    } else {
        None
    }
}

/// Updates the persisted selection (egui temp memory) from the mouse gesture,
/// then returns it. Char on drag, word on double-click, line on triple. The
/// anchor is the press origin (not the current position) so the drag extends
/// from the starting cell.
fn update_selection(
    ui: &egui::Ui,
    id: egui::Id,
    response: &egui::Response,
    region: egui::Rect,
    char_w: f32,
    row_h: f32,
) -> Option<Selection> {
    let sel_id = id.with("selection");
    let mut selection: Option<Selection> = ui.data(|d| d.get_temp(sel_id));

    let cell_of = |pos: egui::Pos2| point_to_cell(pos, region, char_w, row_h);
    let press_cell = ui.input(|i| i.pointer.press_origin()).map(cell_of);
    let current_cell = ui.input(|i| i.pointer.interact_pos()).map(cell_of);

    if response.triple_clicked() {
        if let Some(at) = press_cell.or(current_cell) {
            selection = Some(Selection::new(at, SelectionMode::Line));
        }
    } else if response.double_clicked() {
        if let Some(at) = press_cell.or(current_cell) {
            selection = Some(Selection::new(at, SelectionMode::Word));
        }
    } else if response.drag_started() {
        if let Some(at) = press_cell {
            selection = Some(Selection::new(at, SelectionMode::Char));
        }
    } else if response.dragged() {
        if let (Some(at), Some(anchor)) = (current_cell, press_cell) {
            selection = Some(Selection {
                anchor,
                head: at,
                mode: SelectionMode::Char,
            });
        }
    } else if response.clicked() {
        selection = None;
    }

    ui.data_mut(|d| match selection {
        Some(sel) => {
            d.insert_temp(sel_id, sel);
        }
        None => d.remove::<Selection>(sel_id),
    });
    selection
}

fn paint_selection(
    painter: &egui::Painter,
    region: egui::Rect,
    char_w: f32,
    row_h: f32,
    char_rows: &[Vec<char>],
    sel: &Selection,
    palette: &TermPalette,
) {
    if sel.is_empty() {
        return;
    }
    let sel_rgb = palette.selection;
    let fill =
        egui::Color32::from_rgba_unmultiplied(sel_rgb.r, sel_rgb.g, sel_rgb.b, SELECTION_ALPHA);
    for (line, row) in char_rows.iter().enumerate() {
        for col in 0..row.len() {
            if covers(sel, char_rows, 0, line as i32, col) {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(
                        region.left() + col as f32 * char_w,
                        region.top() + line as f32 * row_h,
                    ),
                    egui::vec2(char_w, row_h),
                );
                painter.rect_filled(cell, 0.0, fill);
            }
        }
    }
}

/// `Cmd+V` is translated by the window layer into `Event::Paste`. We concatenate
/// the pasted content to forward it to the PTY (bracketed paste handled by `Pane`).
fn collect_paste(ui: &egui::Ui) -> Option<String> {
    ui.ctx().input(|input| {
        let mut pasted = String::new();
        for event in &input.events {
            if let egui::Event::Paste(text) = event {
                pasted.push_str(text);
            }
        }
        (!pasted.is_empty()).then_some(pasted)
    })
}

/// Files dropped onto the window (Finder): each path shell-escaped then followed
/// by a space, ready to insert into a command line. Goes through the paste path
/// so bracketed paste applies — TUIs (Claude Code) see the drop as a paste.
fn collect_dropped_paths(ui: &egui::Ui) -> Option<String> {
    ui.ctx().input(|input| {
        let text: String = input
            .raw
            .dropped_files
            .iter()
            .filter_map(|file| file.path.as_ref())
            .map(|path| shell_escape_path(&path.to_string_lossy()) + " ")
            .collect();
        (!text.is_empty()).then_some(text)
    })
}

/// The drop lands in the pane under the pointer; without a usable position
/// (headless run, query failure), it falls back to the focused pane.
fn drop_targets_pane(pointer: Option<egui::Pos2>, region: egui::Rect, focused: bool) -> bool {
    pointer.map_or(focused, |pos| region.contains(pos))
}

/// Drop position in window coordinates: global mouse re-anchored to the
/// viewport — both in points with a top-left screen origin.
fn drop_pointer_pos(ctx: &egui::Context) -> Option<egui::Pos2> {
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    let mouse = global_mouse_pos()?;
    Some(mouse - inner.min.to_vec2())
}

/// Current mouse position in global display coordinates (CoreGraphics).
/// `CGEventCreate(NULL)` snapshots the mouse without any permission; confined
/// here like `agent_watch::probe`, the rest of the module is portable.
fn global_mouse_pos() -> Option<egui::Pos2> {
    use std::ffi::c_void;
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *const c_void;
        fn CGEventGetLocation(event: *const c_void) -> CGPoint;
        fn CFRelease(cf: *const c_void);
    }
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return None;
        }
        let loc = CGEventGetLocation(event);
        CFRelease(event);
        Some(egui::pos2(loc.x as f32, loc.y as f32))
    }
}

/// Backslash escaping (Terminal.app convention): special ASCII chars escaped one
/// by one, non-ASCII (accents…) passes through. `~` and `=` are escaped too:
/// at word start zsh expands them (`~x`, `=cmd`).
fn shell_escape_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        let safe = !c.is_ascii()
            || c.is_ascii_alphanumeric()
            || matches!(c, '/' | '.' | '-' | '_' | '+' | ',' | ':' | '@' | '%');
        if !safe {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `Cmd+C` is translated by the window layer into `Event::Copy` (raw `Ctrl+C`
/// stays a `Key` forwarded to the PTY).
fn copy_requested(ui: &egui::Ui) -> bool {
    ui.ctx()
        .input(|input| input.events.iter().any(|e| matches!(e, egui::Event::Copy)))
}

/// The Clear binding (keybindings §2, `Cmd+K` by default); unbound ⇒ inert.
fn clear_pressed(ui: &egui::Ui, shortcut: Option<Shortcut>) -> bool {
    let Some(shortcut) = shortcut else {
        return false;
    };
    ui.ctx().input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if shortcut.matches(*key, *modifiers)
            )
        })
    })
}

fn enter_pressed(ui: &egui::Ui) -> bool {
    ui.ctx().input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                }
            )
        })
    })
}

/// A live drag on a split seam. The split is pinned by the leftmost/topmost
/// leaf on each side — a pair unique to one split node — so a drag adjusts the
/// exact seam grabbed, never the nearest same-orientation split (that "nearest"
/// rule is the keyboard semantics, wrong for a pointed seam). `delta` is the
/// signed per-frame ratio change in that split's local extent.
pub struct ResizeDrag {
    pub first: PaneId,
    pub second: PaneId,
    pub delta: f32,
}

/// Drag payload carried while a pane grip is dragged: the pane being relocated.
#[derive(Clone, Copy)]
struct DragPane(PaneId);

/// Where a pane drag was released over the target pane (terminal.md §5): a
/// directional edge re-splits the target, the center swaps the two panes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DropZone {
    Side(Dir),
    Swap,
}

/// A completed pane drag-and-drop: relocate `src` onto `target` per `zone`.
#[derive(Clone, Copy)]
pub struct PaneDrop {
    pub src: PaneId,
    pub target: PaneId,
    pub zone: DropZone,
}

#[derive(Default)]
pub struct TreeOutput {
    pub focus: Option<PaneId>,
    pub resize: Option<ResizeDrag>,
    pub drop: Option<PaneDrop>,
}

pub fn terminal_tree(
    ui: &mut egui::Ui,
    layout: &Layout,
    chrome: &Palette,
    mut leaf: impl FnMut(&mut egui::Ui, PaneId, bool) -> bool,
) -> TreeOutput {
    let area = ui.available_rect_before_wrap();
    let pane_area = PaneRect {
        x: area.min.x,
        y: area.min.y,
        w: area.width(),
        h: area.height(),
    };

    let mut output = TreeOutput::default();
    let rects = layout.rects(pane_area);
    for (id, rect) in &rects {
        let egui_rect = to_egui_rect(*rect);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(egui_rect).id_salt(id.0));
        let focused = *id == layout.focus();
        if leaf(&mut child, *id, focused) {
            output.focus = Some(*id);
        }
    }

    // Pane reorg drag-and-drop (terminal.md §5): a hover grip starts the drag,
    // the hovered pane shows a drop overlay, release emits the reorg. Only
    // meaningful with more than one pane.
    if rects.len() > 1 {
        pane_drag_grips(ui, &rects, chrome);
        detect_pane_drop(ui, &rects, chrome, &mut output);
    }

    paint_separators(ui, layout.root(), pane_area, chrome, &mut output);
    output
}

/// Reveals a drag grip at the top of each hovered pane and arms it as a
/// drag-and-drop source. Suppressed mid-drag — the drop overlays carry the
/// feedback from there on, and the payload persists in egui regardless.
fn pane_drag_grips(ui: &mut egui::Ui, rects: &[(PaneId, PaneRect)], chrome: &Palette) {
    if egui::DragAndDrop::has_payload_of_type::<DragPane>(ui.ctx()) {
        return;
    }
    for (id, rect) in rects {
        let egui_rect = to_egui_rect(*rect);
        if !ui.rect_contains_pointer(egui_rect) {
            continue;
        }
        // Top-right corner — clear of the left-aligned terminal content and of a
        // pane click (which still focuses the pane underneath the grip).
        let grip = egui::Rect::from_min_size(
            egui::pos2(
                egui_rect.right() - GRIP_W - GRIP_TOP,
                egui_rect.top() + GRIP_TOP,
            ),
            egui::vec2(GRIP_W, GRIP_H),
        );
        // Drag-only: a click on the grip falls through to the terminal below so
        // it still focuses the pane; only a drag is captured here.
        let resp = ui
            .interact(grip, ui.id().with(("pane_grip", id.0)), egui::Sense::drag())
            .on_hover_cursor(egui::CursorIcon::Grab);
        resp.dnd_set_drag_payload(DragPane(*id));
        let active = resp.hovered() || resp.dragged();
        if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        let bg = if active {
            chrome.bg_surface_hover
        } else {
            with_alpha(chrome.bg_surface, 200)
        };
        ui.painter()
            .rect_filled(grip, egui::CornerRadius::same(4), bg);
        let ink = if active {
            chrome.text_primary
        } else {
            chrome.text_muted
        };
        paint_icon(
            ui.painter(),
            grip.center(),
            GRIP_ICON,
            lucide_icons::Icon::GripVertical,
            ink,
        );
    }
}

/// While a pane is being dragged, highlights the drop zone under the pointer and,
/// on release, records the reorg in `output.drop`. Gated on an active `DragPane`
/// payload so the hover sensing never steals the terminal's pointer otherwise.
fn detect_pane_drop(
    ui: &mut egui::Ui,
    rects: &[(PaneId, PaneRect)],
    chrome: &Palette,
    output: &mut TreeOutput,
) {
    if !egui::DragAndDrop::has_payload_of_type::<DragPane>(ui.ctx()) {
        return;
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    for (id, rect) in rects {
        let egui_rect = to_egui_rect(*rect);
        let resp = ui.interact(
            egui_rect,
            ui.id().with(("pane_drop", id.0)),
            egui::Sense::hover(),
        );
        let Some(p) = pointer else { continue };
        if let Some(drag) = resp.dnd_release_payload::<DragPane>() {
            if drag.0 != *id {
                output.drop = Some(PaneDrop {
                    src: drag.0,
                    target: *id,
                    zone: drop_zone(egui_rect, p),
                });
            }
        } else if let Some(drag) = resp.dnd_hover_payload::<DragPane>() {
            if drag.0 != *id {
                paint_drop_overlay(ui.painter(), egui_rect, drop_zone(egui_rect, p), chrome);
            }
        }
    }
}

/// Maps the pointer position within a target pane to a drop zone: the central
/// band swaps, otherwise the nearest edge selects the re-split side.
fn drop_zone(rect: egui::Rect, p: egui::Pos2) -> DropZone {
    let fx = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    let fy = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    if (fx - 0.5).abs() < DROP_SWAP_HALF && (fy - 0.5).abs() < DROP_SWAP_HALF {
        return DropZone::Swap;
    }
    let (left, right, top, bottom) = (fx, 1.0 - fx, fy, 1.0 - fy);
    let min = left.min(right).min(top).min(bottom);
    if min == left {
        DropZone::Side(Dir::Left)
    } else if min == right {
        DropZone::Side(Dir::Right)
    } else if min == top {
        DropZone::Side(Dir::Up)
    } else {
        DropZone::Side(Dir::Down)
    }
}

fn paint_drop_overlay(painter: &egui::Painter, rect: egui::Rect, zone: DropZone, chrome: &Palette) {
    let half_w = egui::vec2(rect.width() / 2.0, rect.height());
    let half_h = egui::vec2(rect.width(), rect.height() / 2.0);
    let target = match zone {
        DropZone::Swap => rect,
        DropZone::Side(Dir::Left) => egui::Rect::from_min_size(rect.min, half_w),
        DropZone::Side(Dir::Right) => {
            egui::Rect::from_min_size(egui::pos2(rect.center().x, rect.top()), half_w)
        }
        DropZone::Side(Dir::Up) => egui::Rect::from_min_size(rect.min, half_h),
        DropZone::Side(Dir::Down) => {
            egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y), half_h)
        }
    };
    let target = target.shrink(2.0);
    painter.rect_filled(
        target,
        egui::CornerRadius::same(4),
        with_alpha(chrome.accent, DROP_OVERLAY_ALPHA),
    );
    painter.rect_stroke(
        target,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.5_f32, chrome.accent),
        egui::StrokeKind::Inside,
    );
}

fn paint_separators(
    ui: &mut egui::Ui,
    node: &Node,
    area: PaneRect,
    chrome: &Palette,
    output: &mut TreeOutput,
) {
    let Node::Split {
        orient,
        ratio,
        first,
        second,
    } = node
    else {
        return;
    };
    let seam = seam_rect(*orient, *ratio, area);
    ui.painter()
        .rect_filled(to_egui_rect(seam), 0.0, chrome.border_subtle);

    let handle = handle_rect(*orient, *ratio, area);
    // Identify the handle by the leaves it borders, not its pixel position: the
    // seam moves as the ratio changes mid-drag, and a position-derived id would
    // change every frame, so egui would drop the in-progress drag after ~1px.
    let id = ui
        .id()
        .with(("split", first_leaf(first).0, first_leaf(second).0));
    let response = ui.interact(to_egui_rect(handle), id, egui::Sense::drag());
    let cursor = match orient {
        Orient::Vertical => egui::CursorIcon::ResizeHorizontal,
        Orient::Horizontal => egui::CursorIcon::ResizeVertical,
    };
    let response = response.on_hover_cursor(cursor);
    if response.dragged() {
        let delta = response.drag_delta();
        let (pixels, extent) = match orient {
            Orient::Vertical => (delta.x, area.w),
            Orient::Horizontal => (delta.y, area.h),
        };
        if pixels.abs() > f32::EPSILON && extent > 0.0 {
            output.resize = Some(ResizeDrag {
                first: first_leaf(first),
                second: first_leaf(second),
                delta: pixels / extent,
            });
        }
    }

    let (first_area, second_area) = split_rects(*orient, *ratio, area);
    paint_separators(ui, first, first_area, chrome, output);
    paint_separators(ui, second, second_area, chrome, output);
}

fn seam_rect(orient: Orient, ratio: f32, area: PaneRect) -> PaneRect {
    match orient {
        Orient::Vertical => PaneRect {
            x: area.x + area.w * ratio - SEPARATOR_THICKNESS / 2.0,
            y: area.y,
            w: SEPARATOR_THICKNESS,
            h: area.h,
        },
        Orient::Horizontal => PaneRect {
            x: area.x,
            y: area.y + area.h * ratio - SEPARATOR_THICKNESS / 2.0,
            w: area.w,
            h: SEPARATOR_THICKNESS,
        },
    }
}

fn handle_rect(orient: Orient, ratio: f32, area: PaneRect) -> PaneRect {
    match orient {
        Orient::Vertical => PaneRect {
            x: area.x + area.w * ratio - SEPARATOR_HANDLE / 2.0,
            y: area.y,
            w: SEPARATOR_HANDLE,
            h: area.h,
        },
        Orient::Horizontal => PaneRect {
            x: area.x,
            y: area.y + area.h * ratio - SEPARATOR_HANDLE / 2.0,
            w: area.w,
            h: SEPARATOR_HANDLE,
        },
    }
}

fn to_egui_rect(rect: PaneRect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(rect.x, rect.y), egui::vec2(rect.w, rect.h))
}

fn paint_cursor(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    shape: CursorShape,
) {
    match shape {
        CursorShape::Block => {
            painter.rect_filled(rect, 0.0, color);
        }
        CursorShape::Outline => {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0_f32, color),
                egui::StrokeKind::Inside,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cv(c: char) -> CellView {
        CellView {
            c,
            zerowidth: Vec::new(),
            fg: egui::Color32::WHITE,
            bg: egui::Color32::TRANSPARENT,
            italic: false,
            underline: false,
            wide: false,
            spacer: false,
            link: None,
        }
    }

    /// Context with the default fonts initialized, for `row_paint_ops`.
    fn fonts_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |_| {});
        ctx
    }

    fn ops_for(cells: &[CellView]) -> (Vec<PaintOp>, f32) {
        let ctx = fonts_ctx();
        ctx.fonts_mut(|f| {
            let font = egui::FontId::monospace(13.0);
            let char_w = f.glyph_width(&font, ' ').max(1.0);
            (
                row_paint_ops(f, cells, 13.0, char_w, 16.0, egui::Color32::TRANSPARENT),
                char_w,
            )
        })
    }

    fn row(s: &str) -> Vec<CellView> {
        s.chars().map(cv).collect()
    }

    fn preview_texts(rows: Vec<Vec<CellView>>, cursor: usize, lines: usize) -> Vec<String> {
        condense_preview_rows(rows, cursor, lines)
            .iter()
            .map(|r| r.iter().map(|c| c.c).collect())
            .collect()
    }

    #[test]
    fn preview_drops_a_boxed_composer_anchored_on_the_cursor() {
        // Claude Code: conversation, then a boxed composer holding the cursor, then
        // status / hint lines below it — all of it must fall away.
        let rows = vec![
            row("> fix the failing billing test"),
            row(""),
            row("\u{23fa} Update src/billing/proration.rs (+18 -7)"),
            row("\u{256d}\u{2500}\u{2500}\u{2500}\u{256e}"),
            row("\u{2502} > _ \u{2502}"),
            row("\u{2570}\u{2500}\u{2500}\u{2500}\u{256f}"),
            row("  ? for shortcuts"),
            row("  \u{273b} Crunching..."),
        ];
        assert_eq!(
            preview_texts(rows, 4, 9),
            [
                "> fix the failing billing test",
                "\u{23fa} Update src/billing/proration.rs (+18 -7)",
            ]
        );
    }

    #[test]
    fn preview_strips_a_top_banner_and_a_non_boxed_composer() {
        // Codex: a top startup banner box, conversation, then a bare prompt composer
        // holding the cursor with a status line under it (no box around the input).
        let rows = vec![
            row("\u{256d}\u{2500}\u{2500}\u{2500}\u{256e}"),
            row("\u{2502} Update available! 0.139 -> 0.141 \u{2502}"),
            row("\u{2570}\u{2500}\u{2500}\u{2500}\u{256f}"),
            row(""),
            row("\u{2022} explain the worktree grouping"),
            row("  helm groups a root with its worktrees"),
            row(""),
            row("\u{203a} summarize recent commits"),
            row("  gpt-5.5 xhigh - ~/dev/helm-studio"),
        ];
        assert_eq!(
            preview_texts(rows, 7, 9),
            [
                "\u{2022} explain the worktree grouping",
                "  helm groups a root with its worktrees",
            ]
        );
    }

    #[test]
    fn preview_keeps_the_transcript_when_the_cursor_is_parked_high() {
        // Fresh session: a banner is on screen and the cursor is still up in it (no
        // input yet). The cursor is not a composer anchor, so nothing is cut from
        // the bottom — only box chrome and blanks drop.
        let rows = vec![
            row("\u{256d}\u{2500}\u{2500}\u{2500}\u{256e}"),
            row("\u{2502} welcome \u{2502}"),
            row("\u{2570}\u{2500}\u{2500}\u{2500}\u{256f}"),
            row(""),
            row("ready when you are"),
        ];
        assert_eq!(preview_texts(rows, 1, 9), ["ready when you are"]);
    }

    #[test]
    fn powerline_and_blocks_are_procedural_but_text_is_not() {
        assert!(cell_shape('\u{E0B0}').is_some());
        assert!(cell_shape('\u{E0B2}').is_some());
        assert!(cell_shape('\u{2588}').is_some());
        assert!(cell_shape('\u{2591}').is_some());
        assert!(cell_shape('\u{259F}').is_some());
        assert!(cell_shape('a').is_none());
        assert!(cell_shape('\u{25CF}').is_none()); // ● stays a font glyph
        assert!(cell_shape('\u{E0B4}').is_none()); // rounded variants: NF font
    }

    #[test]
    fn shades_fill_the_whole_cell_with_alpha() {
        for (c, expected) in [('\u{2591}', 0.25), ('\u{2592}', 0.5), ('\u{2593}', 0.75)] {
            match cell_shape(c) {
                Some(CellShape::Blocks { rects, alpha }) => {
                    assert_eq!(rects, &[B_FULL], "U+{:04X}", c as u32);
                    assert_eq!(alpha, expected, "U+{:04X}", c as u32);
                }
                other => panic!("U+{:04X}: unexpected {other:?}", c as u32),
            }
        }
    }

    #[test]
    fn ascii_cells_form_a_single_run_anchored_at_their_column() {
        let cells: Vec<CellView> = "hello".chars().map(cv).collect();
        let (ops, _) = ops_for(&cells);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PaintOp::Run { col, galley } => {
                assert_eq!(*col, 0);
                assert_eq!(galley.text(), "hello");
            }
            other => panic!("expected a Run, got {other:?}"),
        }
    }

    #[test]
    fn markdown_backticks_stay_in_the_text_run() {
        let cells: Vec<CellView> = "```rust".chars().map(cv).collect();
        let (ops, _) = ops_for(&cells);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PaintOp::Run { col, galley } => {
                assert_eq!(*col, 0);
                assert_eq!(galley.text(), "```rust");
            }
            other => panic!("expected a Run, got {other:?}"),
        }
    }

    #[test]
    fn combining_marks_are_rendered_with_their_base_cell() {
        let mut cell = cv('e');
        cell.zerowidth.push('\u{0302}');
        let (ops, _) = ops_for(&[cell]);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PaintOp::Run { col, galley } => {
                assert_eq!(*col, 0);
                assert_eq!(galley.text(), "e\u{0302}");
            }
            other => panic!("expected a Run, got {other:?}"),
        }
    }

    #[test]
    fn standalone_combining_marks_are_not_dropped_as_empty_spaces() {
        let mut cell = cv(' ');
        cell.zerowidth.push('\u{0300}');
        let (ops, _) = ops_for(&[cell]);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PaintOp::Run { galley, .. } => assert_eq!(galley.text(), " \u{0300}"),
            other => panic!("expected a Run, got {other:?}"),
        }
    }

    #[test]
    fn a_fallback_glyph_splits_the_run_and_is_centered_alone() {
        // ⚡ comes from a fallback font (advance ≠ mono cell): it must be isolated
        // so it doesn't shift the rest of the line.
        let cells: Vec<CellView> = "ab\u{26A1}cd".chars().map(cv).collect();
        let (ops, _) = ops_for(&cells);
        let kinds: Vec<&str> = ops
            .iter()
            .map(|op| match op {
                PaintOp::Run { .. } => "run",
                PaintOp::Loose { .. } => "loose",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["run", "loose", "run"]);
        match &ops[2] {
            PaintOp::Run { col, galley } => {
                assert_eq!(*col, 3, "the next run stays anchored to its column");
                assert_eq!(galley.text(), "cd");
            }
            other => panic!("expected a Run, got {other:?}"),
        }
    }

    #[test]
    fn full_width_blocks_merge_into_one_shape() {
        let cells: Vec<CellView> = "\u{2588}\u{2588}\u{2588}".chars().map(cv).collect();
        let (ops, _) = ops_for(&cells);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PaintOp::Shape { col, span, .. } => {
                assert_eq!((*col, *span), (0, 3), "a progress bar = a single rect");
            }
            other => panic!("expected a Shape, got {other:?}"),
        }
    }

    #[test]
    fn partial_width_blocks_stay_one_shape_per_cell() {
        let cells: Vec<CellView> = "\u{258C}\u{258C}".chars().map(cv).collect();
        let (ops, _) = ops_for(&cells);
        assert_eq!(ops.len(), 2, "▌ does not cover the full width: no merge");
    }

    #[test]
    fn backgrounds_merge_by_color_and_skip_the_default() {
        let mut cells: Vec<CellView> = "abcd".chars().map(cv).collect();
        cells[1].bg = egui::Color32::RED;
        cells[2].bg = egui::Color32::RED;
        let (ops, _) = ops_for(&cells);
        let bgs: Vec<(usize, usize)> = ops
            .iter()
            .filter_map(|op| match op {
                PaintOp::Bg { col, span, .. } => Some((*col, *span)),
                _ => None,
            })
            .collect();
        assert_eq!(bgs, [(1, 2)], "background merged, default not painted");
    }

    #[test]
    fn snapshot_applies_dim_sgr_to_the_foreground() {
        let term = crate::terminal::emu::shared_term(2, 12);
        let palette = TermPalette::variant(crate::terminal::palette::TermTheme::Dark);
        crate::terminal::emu::feed(&term, b"\x1b[2mplaceholder");

        let snap = snapshot(&term, &palette);

        assert_eq!(snap.rows[0][0].c, 'p');
        assert_eq!(
            snap.rows[0][0].fg,
            rgb(palette.dim(palette.foreground)),
            "SGR 2/faint text should render as muted ink, not the bright default foreground"
        );
    }

    #[test]
    fn snapshot_keeps_zero_width_circumflex_with_the_base_cell() {
        let term = crate::terminal::emu::shared_term(2, 12);
        let palette = TermPalette::variant(crate::terminal::palette::TermTheme::Dark);
        crate::terminal::emu::feed(&term, "e\u{0302}".as_bytes());

        let snap = snapshot(&term, &palette);

        assert_eq!(snap.rows[0][0].c, 'e');
        assert_eq!(snap.rows[0][0].zerowidth, ['\u{0302}']);
        assert_eq!(snap.rows[0][0].text(), "e\u{0302}");
    }

    #[test]
    fn snapshot_preserves_background_for_ansi_line_erase() {
        let term = crate::terminal::emu::shared_term(2, 12);
        let palette = TermPalette::variant(crate::terminal::palette::TermTheme::Dark);
        crate::terminal::emu::feed(&term, b"\x1b[48;2;50;51;52m>\x1b[K");

        let snap = snapshot(&term, &palette);
        let bg = egui::Color32::from_rgb(50, 51, 52);

        assert_eq!(
            snap.rows[0][0].bg, bg,
            "typed cells keep the input background"
        );
        assert_eq!(
            snap.rows[0][11].bg, bg,
            "CSI K fills erased cells with the current background, as TUIs use for input bars"
        );
    }

    #[test]
    fn a_wide_char_is_centered_over_two_cells_and_its_spacer_skipped() {
        let mut cells: Vec<CellView> = "\u{4E2D}x".chars().map(cv).collect();
        cells[0].wide = true;
        cells.insert(1, {
            let mut spacer = cv(' ');
            spacer.spacer = true;
            spacer
        });
        let (ops, _) = ops_for(&cells);
        match &ops[0] {
            PaintOp::Loose { col, span, .. } => assert_eq!((*col, *span), (0, 2)),
            other => panic!("expected a Loose, got {other:?}"),
        }
        match &ops[1] {
            PaintOp::Run { col, galley } => {
                assert_eq!(*col, 2, "x stays anchored after the wide char's 2 cells");
                assert_eq!(galley.text(), "x");
            }
            other => panic!("expected a Run, got {other:?}"),
        }
    }

    #[test]
    fn cursor_shape_follows_focus() {
        assert_eq!(cursor_shape(true), CursorShape::Block);
        assert_eq!(cursor_shape(false), CursorShape::Outline);
    }

    #[test]
    fn ctrl_letters_map_to_control_bytes() {
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(key_bytes(egui::Key::C, ctrl), Some(vec![0x03]));
        assert_eq!(key_bytes(egui::Key::D, ctrl), Some(vec![0x04]));
        assert_eq!(key_bytes(egui::Key::Z, ctrl), Some(vec![0x1a]));
    }

    #[test]
    fn shift_tab_sends_backtab_and_plain_tab_a_tab() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(key_bytes(egui::Key::Tab, shift), Some(b"\x1b[Z".to_vec()));
        assert_eq!(
            key_bytes(egui::Key::Tab, egui::Modifiers::default()),
            Some(b"\t".to_vec())
        );
    }

    #[test]
    fn special_keys_map_to_terminal_sequences() {
        let none = egui::Modifiers::default();
        assert_eq!(key_bytes(egui::Key::Enter, none), Some(b"\r".to_vec()));
        assert_eq!(
            key_bytes(egui::Key::Backspace, none),
            Some(b"\x7f".to_vec())
        );
        assert_eq!(
            key_bytes(egui::Key::ArrowUp, none),
            Some(b"\x1b[A".to_vec())
        );
    }

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn printable_key_fallback_for_backtick_quote_and_caret() {
        let none = egui::Modifiers::default();
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };

        assert_eq!(
            input_bytes_from_events(&[key_event(egui::Key::Backtick, none)]),
            b"`"
        );
        assert_eq!(
            input_bytes_from_events(&[key_event(egui::Key::Quote, none)]),
            b"'"
        );
        assert_eq!(
            input_bytes_from_events(&[key_event(egui::Key::Num6, shift)]),
            b"^"
        );
    }

    #[test]
    fn printable_key_fallback_does_not_duplicate_text_events() {
        let none = egui::Modifiers::default();
        let events = [
            key_event(egui::Key::Backtick, none),
            egui::Event::Text("`".to_owned()),
        ];

        assert_eq!(input_bytes_from_events(&events), b"`");
    }

    #[test]
    fn ime_commit_is_forwarded_to_the_pty() {
        let events = [egui::Event::Ime(egui::ImeEvent::Commit("ê`".to_owned()))];

        assert_eq!(input_bytes_from_events(&events), "ê`".as_bytes());
    }

    #[test]
    fn printable_key_fallback_does_not_duplicate_ime_commits() {
        let none = egui::Modifiers::default();
        let events = [
            key_event(egui::Key::Backtick, none),
            egui::Event::Ime(egui::ImeEvent::Commit("`".to_owned())),
        ];

        assert_eq!(input_bytes_from_events(&events), b"`");
    }

    #[test]
    fn printable_key_fallback_ignores_command_chords_but_allows_option_literals() {
        let cmd = egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };

        assert!(input_bytes_from_events(&[key_event(egui::Key::Backtick, cmd)]).is_empty());
        assert_eq!(
            input_bytes_from_events(&[key_event(egui::Key::Backtick, alt)]),
            b"`"
        );
    }

    #[test]
    fn shift_enter_sends_csi_u_and_alt_enter_meta_cr() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        // kitty/Ghostty convention: combo without a legacy encoding → CSI u,
        // without negotiation (Claude Code parses it unconditionally).
        assert_eq!(
            key_bytes(egui::Key::Enter, shift),
            Some(b"\x1b[13;2u".to_vec())
        );
        // Option+Enter: meta+enter, Claude Code newline.
        assert_eq!(key_bytes(egui::Key::Enter, alt), Some(b"\x1b\r".to_vec()));
        // Bare Enter keeps the legacy encoding.
        assert_eq!(
            key_bytes(egui::Key::Enter, egui::Modifiers::default()),
            Some(b"\r".to_vec())
        );
    }

    #[test]
    fn command_combinations_are_not_forwarded() {
        let cmd = egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        assert_eq!(key_bytes(egui::Key::C, cmd), None);
        assert_eq!(key_bytes(egui::Key::D, cmd), None);
    }

    #[test]
    fn cmd_backspace_kills_the_edit_line() {
        let cmd = egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        assert_eq!(key_bytes(egui::Key::Backspace, cmd), Some(vec![0x15]));

        let cmd_shift = egui::Modifiers { shift: true, ..cmd };
        assert_eq!(key_bytes(egui::Key::Backspace, cmd_shift), None);
    }

    #[test]
    fn alt_arrows_jump_by_word_and_alt_backspace_kills_word() {
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            key_bytes(egui::Key::ArrowLeft, alt),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(
            key_bytes(egui::Key::ArrowRight, alt),
            Some(b"\x1bf".to_vec())
        );
        assert_eq!(
            key_bytes(egui::Key::Backspace, alt),
            Some(b"\x1b\x7f".to_vec())
        );
    }

    #[test]
    fn cmd_arrows_jump_to_line_ends() {
        let cmd = egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        assert_eq!(key_bytes(egui::Key::ArrowLeft, cmd), Some(vec![0x01]));
        assert_eq!(key_bytes(egui::Key::ArrowRight, cmd), Some(vec![0x05]));
    }

    #[test]
    fn cmd_alt_arrows_stay_with_the_app_focus_navigation() {
        let cmd_alt = egui::Modifiers {
            command: true,
            mac_cmd: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(key_bytes(egui::Key::ArrowLeft, cmd_alt), None);
        assert_eq!(key_bytes(egui::Key::ArrowRight, cmd_alt), None);
    }

    #[test]
    fn slow_wheel_accumulates_fractions_across_frames() {
        let (lines, acc) = accumulate_wheel(0.0, 0.4);
        assert_eq!(lines, 0);
        let (lines, acc) = accumulate_wheel(acc, 0.4);
        assert_eq!(lines, 0);
        let (lines, acc) = accumulate_wheel(acc, 0.4);
        assert_eq!(lines, 1);
        assert!((acc - 0.2).abs() < 1e-5);
    }

    #[test]
    fn wheel_direction_change_drops_the_leftover() {
        let (_, acc) = accumulate_wheel(0.0, 0.9);
        let (lines, acc) = accumulate_wheel(acc, -0.4);
        assert_eq!(lines, 0);
        assert!((acc + 0.4).abs() < 1e-5);
    }

    #[test]
    fn whole_wheel_notches_pass_through_unchanged() {
        assert_eq!(accumulate_wheel(0.0, 3.0), (3, 0.0));
        assert_eq!(accumulate_wheel(0.0, -2.0), (-2, 0.0));
    }

    #[test]
    fn moderate_frame_forwards_in_full_under_the_cap() {
        assert_eq!(pending_step(0, Some(2)), (2, 2));
        assert_eq!(
            pending_step(0, Some(SCROLL_STEP_CAP)),
            (SCROLL_STEP_CAP, SCROLL_STEP_CAP)
        );
        assert_eq!(pending_step(0, Some(-1)), (-1, -1));
    }

    #[test]
    fn peak_frame_caps_the_step_and_drains_the_rest() {
        // 8 lines in one frame: cap this frame, carry the rest...
        let (pending, step) = pending_step(0, Some(8));
        assert_eq!((pending, step), (8, SCROLL_STEP_CAP));
        // ...and the caller drains `pending - step` over the next frames, same total.
        let mut carried = pending - step;
        let mut emitted = step;
        while carried != 0 {
            let (pending, step) = pending_step(carried, None);
            carried = pending - step;
            emitted += step;
        }
        assert_eq!(emitted, 8);
    }

    #[test]
    fn pending_step_resets_on_direction_change() {
        assert_eq!(pending_step(5, Some(-2)), (-2, -2));
        assert_eq!(pending_step(-4, Some(1)), (1, 1));
    }

    #[test]
    fn page_scroll_requires_shift_alone() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        assert!(is_page_scroll_mods(shift));

        let cmd_shift = egui::Modifiers {
            shift: true,
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        assert!(!is_page_scroll_mods(cmd_shift));
        assert!(!is_page_scroll_mods(egui::Modifiers::default()));
    }

    #[test]
    fn shell_escape_keeps_plain_paths_and_unicode() {
        assert_eq!(shell_escape_path("/tmp/img.png"), "/tmp/img.png");
        assert_eq!(
            shell_escape_path("/tmp/Capture d'écran 2026-06-04 à 10.23.45.png"),
            "/tmp/Capture\\ d\\'écran\\ 2026-06-04\\ à\\ 10.23.45.png"
        );
    }

    #[test]
    fn shell_escape_neutralizes_shell_specials() {
        assert_eq!(
            shell_escape_path("/tmp/a (1) & $x.png"),
            "/tmp/a\\ \\(1\\)\\ \\&\\ \\$x.png"
        );
        assert_eq!(
            shell_escape_path("/tmp/~tilde=eq.png"),
            "/tmp/\\~tilde\\=eq.png"
        );
    }

    #[test]
    fn dropped_files_become_space_separated_escaped_paths() {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        for p in ["/tmp/a b.png", "/tmp/c.png"] {
            raw.dropped_files.push(egui::DroppedFile {
                path: Some(p.into()),
                ..Default::default()
            });
        }
        let mut got = None;
        let _ = ctx.run_ui(raw, |ui| {
            got = collect_dropped_paths(ui);
        });
        assert_eq!(got.as_deref(), Some("/tmp/a\\ b.png /tmp/c.png "));
    }

    #[test]
    fn drop_goes_to_the_pane_under_the_pointer_else_focused() {
        let region = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 50.0));
        // Position known: the hovered pane wins, focus is irrelevant.
        assert!(drop_targets_pane(
            Some(egui::pos2(10.0, 10.0)),
            region,
            false
        ));
        assert!(!drop_targets_pane(
            Some(egui::pos2(200.0, 10.0)),
            region,
            true
        ));
        // Position unavailable: focused-pane fallback.
        assert!(drop_targets_pane(None, region, true));
        assert!(!drop_targets_pane(None, region, false));
    }

    #[test]
    fn no_dropped_files_means_no_paste() {
        let ctx = egui::Context::default();
        let mut got = Some(String::new());
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            got = collect_dropped_paths(ui);
        });
        assert_eq!(got, None);
    }

    #[test]
    fn grid_size_floors_to_cells_and_respects_minimums() {
        let big = grid_size_for(egui::vec2(800.0, 600.0), 8.0, 16.0);
        assert_eq!(
            big,
            GridSize {
                rows: 37,
                cols: 100
            }
        );
        let tiny = grid_size_for(egui::vec2(4.0, 4.0), 8.0, 16.0);
        assert_eq!(
            tiny,
            GridSize {
                rows: MIN_LINES,
                cols: MIN_COLS
            }
        );
    }

    #[test]
    fn drop_zone_swaps_at_center_and_picks_the_nearest_edge_otherwise() {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        assert_eq!(drop_zone(r, egui::pos2(50.0, 50.0)), DropZone::Swap);
        assert_eq!(
            drop_zone(r, egui::pos2(5.0, 50.0)),
            DropZone::Side(Dir::Left)
        );
        assert_eq!(
            drop_zone(r, egui::pos2(95.0, 50.0)),
            DropZone::Side(Dir::Right)
        );
        assert_eq!(drop_zone(r, egui::pos2(50.0, 5.0)), DropZone::Side(Dir::Up));
        assert_eq!(
            drop_zone(r, egui::pos2(50.0, 95.0)),
            DropZone::Side(Dir::Down)
        );
    }
}
