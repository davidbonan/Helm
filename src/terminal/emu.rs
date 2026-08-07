use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Rgb as VteRgb, StdSyncHandler};

use crate::terminal::palette::TermPalette;

pub const SCROLLBACK_LINES: usize = 10_000;

pub const DEFAULT_FONT_SIZE: f32 = 13.0;
const MIN_FONT_SIZE: f32 = 6.0;
const MAX_FONT_SIZE: f32 = 40.0;
const FONT_ZOOM_STEP: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontZoom {
    size: f32,
}

impl Default for FontZoom {
    fn default() -> Self {
        Self {
            size: DEFAULT_FONT_SIZE,
        }
    }
}

impl FontZoom {
    pub fn point_size(&self) -> f32 {
        self.size
    }

    pub fn zoom_in(&mut self) {
        self.size = (self.size + FONT_ZOOM_STEP).min(MAX_FONT_SIZE);
    }

    pub fn zoom_out(&mut self) {
        self.size = (self.size - FONT_ZOOM_STEP).max(MIN_FONT_SIZE);
    }

    pub fn reset(&mut self) {
        self.size = DEFAULT_FONT_SIZE;
    }
}

type AnsiParser = Processor<StdSyncHandler>;

/// 64 KB: under a flood (cat of a big file, agent streaming), `read` fills the
/// buffer and the term lock is taken once per 64 KB instead of per 4 KB —
/// fewer contention windows with the UI thread, which takes the same lock every
/// frame. Latency is unchanged: `read` returns as soon as any bytes arrive.
const READ_CHUNK: usize = 64 * 1024;

/// PTY writer shared between the pane (input) and the listener (replies).
pub type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type ReplyPalette = Arc<Mutex<TermPalette>>;
/// Last OSC 0/1/2 title set by the program, shared from the reader thread (which
/// parses it) to the UI thread (tab auto-naming, terminal.md §4).
type TitleSlot = Arc<Mutex<Option<String>>>;

/// A panic in another thread while holding the writer poisons the lock; the
/// PTY fd has no invariant to protect, so recover the guard instead of
/// cascading the panic into every later keystroke (app crash).
pub(crate) fn lock_writer(writer: &PtyWriter) -> std::sync::MutexGuard<'_, Box<dyn Write + Send>> {
    writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_reply_palette(palette: &ReplyPalette) -> std::sync::MutexGuard<'_, TermPalette> {
    palette
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_title(slot: &TitleSlot) -> std::sync::MutexGuard<'_, Option<String>> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Relays back to the PTY the replies the emulation emits as `Event::PtyWrite`
/// (kitty `CSI ? u` query, device attributes, device status), plus dynamic color
/// responses (`OSC 10/11/12 ; ?`). Without this relay, programs that probe the
/// terminal (Claude Code, Codex) never get a reply and skip terminal capabilities
/// such as kitty keyboard or background-aware styling.
pub struct ReplyListener {
    writer: Option<PtyWriter>,
    palette: ReplyPalette,
    title: TitleSlot,
}

impl EventListener for ReplyListener {
    fn send_event(&self, event: Event) {
        let text = match event {
            Event::PtyWrite(text) => text,
            Event::ColorRequest(index, formatter) => {
                let Some(color) = lock_reply_palette(&self.palette).query_color(index) else {
                    return;
                };
                formatter(vte_rgb(color))
            }
            // OSC 0/1/2: the program names the window/tab; kept for auto-naming
            // (terminal.md §4), nothing to write back to the PTY.
            Event::Title(title) => {
                *lock_title(&self.title) = Some(title);
                return;
            }
            Event::ResetTitle => {
                *lock_title(&self.title) = None;
                return;
            }
            _ => return,
        };
        let Some(writer) = &self.writer else { return };
        let mut writer = lock_writer(writer);
        let _ = writer
            .write_all(text.as_bytes())
            .and_then(|()| writer.flush());
    }
}

pub type SharedTerm = Arc<FairMutex<Term<ReplyListener>>>;

#[derive(Clone, Copy)]
struct Size {
    cols: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

pub fn new_term(rows: u16, cols: u16) -> Term<ReplyListener> {
    new_term_replying(rows, cols, None, default_reply_palette(), default_title())
}

fn new_term_replying(
    rows: u16,
    cols: u16,
    writer: Option<PtyWriter>,
    palette: ReplyPalette,
    title: TitleSlot,
) -> Term<ReplyListener> {
    let config = Config {
        scrolling_history: SCROLLBACK_LINES,
        // Kitty keyboard protocol: without this flag, push/pop/report (CSI u)
        // are no-ops and Shift+Enter stays indistinguishable from Enter.
        kitty_keyboard: true,
        ..Config::default()
    };
    Term::new(
        config,
        &grid_size(rows, cols),
        ReplyListener {
            writer,
            palette,
            title,
        },
    )
}

pub fn shared_term(rows: u16, cols: u16) -> SharedTerm {
    Arc::new(FairMutex::new(new_term(rows, cols)))
}

pub fn feed(term: &SharedTerm, bytes: &[u8]) {
    let mut parser = AnsiParser::new();
    parser.advance(&mut *term.lock(), bytes);
}

pub fn clear(term: &SharedTerm) {
    let mut term = term.lock();
    let cursor_line = term.grid().cursor.point.line.0;
    if cursor_line > 0 {
        let lines = term.grid().screen_lines() as i32;
        let grid = term.grid_mut();
        grid.scroll_up(&(Line(0)..Line(lines)), cursor_line as usize);
        grid.cursor.point.line = Line(0);
    }
    term.grid_mut().clear_history();
}

/// Scrollback scrolling, expressed as egui-independent intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollKind {
    /// Shifts the view by `lines` rows toward history (>0) or down (<0).
    Lines(i32),
    PageUp,
    PageDown,
    Bottom,
}

pub fn scroll(term: &SharedTerm, kind: ScrollKind) {
    let scroll = match kind {
        ScrollKind::Lines(lines) => Scroll::Delta(lines),
        ScrollKind::PageUp => Scroll::PageUp,
        ScrollKind::PageDown => Scroll::PageDown,
        ScrollKind::Bottom => Scroll::Bottom,
    };
    term.lock().scroll_display(scroll);
}

pub fn display_offset(term: &Term<ReplyListener>) -> usize {
    term.grid().display_offset()
}

/// Translates a wheel notch according to the terminal's modes (terminal.md §8):
/// an app in **mouse reporting** receives mouse wheel events (SGR or normal
/// encoding), a full-screen TUI (**alt screen + alternate scroll**, e.g. Claude
/// Code) receives ↑/↓ arrows; otherwise `None` — scrolling stays local
/// (scrollback). `lines > 0` = upward; `line`/`col`: cell under the pointer
/// (0-based), clamped to the grid.
pub fn wheel_bytes(
    term: &Term<ReplyListener>,
    lines: i32,
    line: usize,
    col: usize,
) -> Option<Vec<u8>> {
    let mode = *term.mode();
    let count = lines.unsigned_abs() as usize;
    if count == 0 {
        return None;
    }
    if mode.intersects(TermMode::MOUSE_MODE) {
        let button = if lines > 0 { 64 } else { 65 };
        let row = line.min(term.grid().screen_lines().saturating_sub(1)) + 1;
        let col = col.min(term.grid().columns().saturating_sub(1)) + 1;
        let event = if mode.contains(TermMode::SGR_MOUSE) {
            format!("\x1b[<{button};{col};{row}M").into_bytes()
        } else {
            // Normal (X10) encoding: one byte per coordinate, capped at 223.
            vec![
                0x1b,
                b'[',
                b'M',
                32 + button,
                (32 + col.min(223)) as u8,
                (32 + row.min(223)) as u8,
            ]
        };
        return Some(event.repeat(count));
    }
    if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        let arrow: &[u8] = match (mode.contains(TermMode::APP_CURSOR), lines > 0) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1bOB",
            (false, true) => b"\x1b[A",
            (false, false) => b"\x1b[B",
        };
        return Some(arrow.repeat(count));
    }
    None
}

/// The app's mouse-tracking state for a frame (terminal.md §7), read from the
/// terminal mode so the UI can translate clicks without re-locking the grid.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct MouseProtocol {
    /// Any button reporting is on (modes 1000 / 1002 / 1003).
    pub reporting: bool,
    /// Button-held drag is reported (modes 1002 / 1003).
    pub motion: bool,
    /// SGR encoding (mode 1006); otherwise legacy X10.
    pub sgr: bool,
}

pub fn mouse_protocol(term: &Term<ReplyListener>) -> MouseProtocol {
    let mode = *term.mode();
    MouseProtocol {
        reporting: mode.intersects(TermMode::MOUSE_MODE),
        motion: mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        sgr: mode.contains(TermMode::SGR_MOUSE),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseKind {
    Press,
    Release,
    Drag,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct MouseMods {
    pub alt: bool,
    pub ctrl: bool,
}

/// Encodes a mouse button event for an app in mouse reporting (terminal.md §7),
/// SGR (mode 1006) or legacy X10. Returns `None` when reporting is off — the click
/// stays a local gesture (focus / selection) — or for a drag the app didn't ask to
/// track. `line`/`col` are 0-based cells, clamped to the grid by the caller.
pub fn mouse_report(
    proto: MouseProtocol,
    button: MouseButton,
    kind: MouseKind,
    mods: MouseMods,
    line: usize,
    col: usize,
) -> Option<Vec<u8>> {
    if !proto.reporting {
        return None;
    }
    if kind == MouseKind::Drag && !proto.motion {
        return None;
    }
    let base = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    // Legacy X10 cannot name the released button, so it reports button 3; SGR keeps
    // the real button and marks the release with a trailing `m`.
    let mut cb = if kind == MouseKind::Release && !proto.sgr {
        3
    } else {
        base
    };
    if kind == MouseKind::Drag {
        cb += 32;
    }
    if mods.alt {
        cb += 8;
    }
    if mods.ctrl {
        cb += 16;
    }
    let row = line + 1;
    let col = col + 1;
    let bytes = if proto.sgr {
        let tail = if kind == MouseKind::Release { 'm' } else { 'M' };
        format!("\x1b[<{cb};{col};{row}{tail}").into_bytes()
    } else {
        vec![
            0x1b,
            b'[',
            b'M',
            (32 + cb) as u8,
            (32 + col.min(223)) as u8,
            (32 + row.min(223)) as u8,
        ]
    };
    Some(bytes)
}

fn grid_size(rows: u16, cols: u16) -> Size {
    Size {
        cols: cols as usize,
        lines: rows as usize,
    }
}

fn default_reply_palette() -> ReplyPalette {
    Arc::new(Mutex::new(TermPalette::dark()))
}

fn default_title() -> TitleSlot {
    Arc::new(Mutex::new(None))
}

fn vte_rgb(color: crate::terminal::palette::Rgb) -> VteRgb {
    VteRgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

pub fn resize_term(term: &mut Term<ReplyListener>, rows: u16, cols: u16) {
    term.resize(grid_size(rows, cols));
}

/// No `Drop` joining `reader`: the thread only exits on PTY EOF — a survivor
/// (setsid) still holding the slave would block the join, hence the UI thread.
/// Detached at drop, it shuts down on its own at EOF; `join()` stays available
/// for teardowns that have already killed the tree.
pub struct Emulator {
    term: SharedTerm,
    reader: Option<JoinHandle<()>>,
    reply_palette: ReplyPalette,
    title: TitleSlot,
}

impl Emulator {
    /// `on_change(n)` is called after each chunk read, with its size in bytes
    /// (activity counter, `terminal::activity`). `writer` receives the replies to
    /// the program's queries (`ReplyListener`).
    pub fn spawn(
        reader: Box<dyn Read + Send>,
        rows: u16,
        cols: u16,
        writer: PtyWriter,
        on_change: impl Fn(usize) + Send + 'static,
    ) -> Self {
        Self::spawn_with_palette(reader, rows, cols, writer, TermPalette::dark(), on_change)
    }

    pub fn spawn_with_palette(
        reader: Box<dyn Read + Send>,
        rows: u16,
        cols: u16,
        writer: PtyWriter,
        palette: TermPalette,
        on_change: impl Fn(usize) + Send + 'static,
    ) -> Self {
        let reply_palette = Arc::new(Mutex::new(palette));
        let title: TitleSlot = Arc::new(Mutex::new(None));
        let term = Arc::new(FairMutex::new(new_term_replying(
            rows,
            cols,
            Some(writer),
            Arc::clone(&reply_palette),
            Arc::clone(&title),
        )));
        let reader_term = Arc::clone(&term);
        let handle = std::thread::spawn(move || read_loop(reader, reader_term, on_change));
        Self {
            term,
            reader: Some(handle),
            reply_palette,
            title,
        }
    }

    pub fn term(&self) -> &SharedTerm {
        &self.term
    }

    /// Last OSC 0/1/2 title the program set, `None` if it never set one or reset
    /// it (tab auto-naming, terminal.md §4).
    pub fn title(&self) -> Option<String> {
        lock_title(&self.title).clone()
    }

    pub fn set_reply_palette(&self, palette: TermPalette) {
        *lock_reply_palette(&self.reply_palette) = palette;
    }

    pub fn join(&mut self) {
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

/// The reader holds the term's `FairMutex` while parsing each chunk, and the UI
/// thread (QOS_CLASS_USER_INTERACTIVE) takes that same lock every frame to
/// snapshot the grid. parking_lot does no priority donation: left at the default
/// QoS, the reader gets preempted mid-parse when a long task saturates the cores
/// and the UI thread stalls on the lock for the whole preemption — the priority
/// inversion behind app-wide stutter while terminals stream. Same class as the
/// thread that waits on it.
#[cfg(target_os = "macos")]
fn raise_reader_qos() {
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    term: SharedTerm,
    on_change: impl Fn(usize) + Send + 'static,
) {
    #[cfg(target_os = "macos")]
    raise_reader_qos();
    let mut parser = AnsiParser::new();
    let mut buf = [0u8; READ_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                {
                    let mut term = term.lock();
                    parser.advance(&mut *term, &buf[..n]);
                }
                on_change(n);
            }
        }
    }
}

pub fn scrollback_len(term: &Term<ReplyListener>) -> usize {
    let grid = term.grid();
    grid.total_lines() - grid.screen_lines()
}

/// Last `lines` **logical** lines of output, scrollback included — the Run strip's
/// viewer as text (specs/cli.md §9).
///
/// Rows the terminal wrapped are joined back (`WRAPLINE` on the last cell of a
/// wrapped row), so what comes out does not depend on how wide the strip happens
/// to be on screen. Walks upward from the bottom and stops at `lines`: the
/// scrollback holds 10 000 rows, and a tail must not pay for all of them. Grid
/// indexing is absolute (`Storage::compute_index` ignores the display offset), so
/// a scrolled viewer reads the same thing.
pub fn tail_text(term: &Term<ReplyListener>, lines: usize) -> Vec<String> {
    let first = -(scrollback_len(term) as i32);
    let last = term.grid().screen_lines() as i32 - 1;
    let mut out: Vec<String> = Vec::new();
    // Rows of the logical line being assembled, bottom-up.
    let mut wrapped: Vec<String> = Vec::new();
    for line in (first..=last).rev() {
        wrapped.push(line_text(term, line));
        // The row above continues into this one ⇒ the logical line is not complete.
        if line > first && row_wraps(term, line - 1) {
            continue;
        }
        wrapped.reverse();
        let text = wrapped.concat().trim_end().to_owned();
        wrapped.clear();
        // The grid is a fixed rectangle: everything below the cursor is padding.
        // Blank lines *inside* the output are kept — only the tail is dropped.
        if text.is_empty() && out.is_empty() {
            continue;
        }
        out.push(text);
        if out.len() == lines {
            break;
        }
    }
    out.reverse();
    out
}

/// Whether the terminal wrapped this row into the next one.
fn row_wraps(term: &Term<ReplyListener>, line: i32) -> bool {
    let grid = term.grid();
    let last = Column(grid.columns() - 1);
    grid[Line(line)][last].flags.contains(Flags::WRAPLINE)
}

pub fn line_text(term: &Term<ReplyListener>, line: i32) -> String {
    let grid = term.grid();
    let cols = grid.columns();
    let row = &grid[Line(line)];
    (0..cols).map(|col| row[Column(col)].c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_writer_recovers_instead_of_panicking() {
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(Vec::new())));
        let clone = Arc::clone(&writer);
        let _ = std::thread::spawn(move || {
            let _guard = clone.lock().unwrap();
            panic!("poison the lock");
        })
        .join();
        assert!(writer.lock().is_err());

        lock_writer(&writer).write_all(b"still alive").unwrap();
    }

    #[test]
    fn scrollback_saturates_at_ten_thousand_lines() {
        let mut term = new_term(4, 10);
        let mut parser = AnsiParser::new();
        let feed = vec![b'\n'; SCROLLBACK_LINES + 5_000];
        parser.advance(&mut term, &feed);
        assert_eq!(scrollback_len(&term), SCROLLBACK_LINES);
    }

    #[test]
    fn line_text_reads_trimmed_row_content() {
        let mut term = new_term(4, 10);
        let mut parser = AnsiParser::new();
        parser.advance(&mut term, b"hi");
        assert_eq!(line_text(&term, 0).trim_end(), "hi");
    }

    #[test]
    fn tail_text_reads_the_last_lines_across_the_scrollback() {
        let term = shared_term(3, 10);
        for line in 0..8 {
            feed(&term, format!("line{line}\r\n").as_bytes());
        }
        let term = term.lock();
        assert!(
            scrollback_len(&term) > 0,
            "the grid holds 3 rows for 8 lines"
        );

        assert_eq!(
            tail_text(&term, 3),
            vec!["line5".to_owned(), "line6".to_owned(), "line7".to_owned()],
            "the tail crosses the viewport into the scrollback, blank padding dropped"
        );
        assert_eq!(tail_text(&term, 100).len(), 8, "asking for more yields all");
    }

    #[test]
    fn tail_text_rejoins_lines_the_terminal_wrapped() {
        // 10 columns: the line below is written over three rows.
        let term = shared_term(4, 10);
        feed(&term, b"0123456789abcdefghijklmno\r\nshort\r\n");
        let term = term.lock();

        assert_eq!(
            tail_text(&term, 10),
            vec!["0123456789abcdefghijklmno".to_owned(), "short".to_owned()],
            "a wrapped line comes back whole — the strip's width is not the log's"
        );
    }

    #[test]
    fn tail_text_keeps_blank_lines_inside_the_output() {
        let term = shared_term(4, 10);
        feed(&term, b"top\r\n\r\nbottom\r\n");
        let term = term.lock();

        assert_eq!(
            tail_text(&term, 10),
            vec!["top".to_owned(), String::new(), "bottom".to_owned()]
        );
    }

    #[test]
    fn clear_keeps_prompt_line_drops_scrollback_and_lines_above() {
        let term = shared_term(4, 10);
        feed(&term, b"abc");
        for _ in 0..10 {
            feed(&term, b"\r\nx");
        }
        feed(&term, b"\r\nprompt %");
        assert!(scrollback_len(&term.lock()) > 0);

        clear(&term);

        let term = term.lock();
        assert_eq!(scrollback_len(&term), 0);
        assert_eq!(line_text(&term, 0).trim_end(), "prompt %");
        assert_eq!(line_text(&term, 1).trim_end(), "");
        assert_eq!(line_text(&term, 3).trim_end(), "");
        let cursor = term.grid().cursor.point;
        assert_eq!((cursor.line.0, cursor.column.0), (0, 8));
    }

    #[test]
    fn scroll_moves_display_offset_through_history_and_back() {
        let term = shared_term(4, 10);
        feed(&term, b"top");
        for _ in 0..20 {
            feed(&term, b"\r\nx");
        }
        assert_eq!(display_offset(&term.lock()), 0);
        assert!(scrollback_len(&term.lock()) > 0);

        scroll(&term, ScrollKind::PageUp);
        assert_eq!(display_offset(&term.lock()), 4);
        scroll(&term, ScrollKind::Lines(3));
        assert_eq!(display_offset(&term.lock()), 7);
        scroll(&term, ScrollKind::PageDown);
        assert_eq!(display_offset(&term.lock()), 3);

        scroll(&term, ScrollKind::Bottom);
        assert_eq!(display_offset(&term.lock()), 0);
    }

    #[test]
    fn scroll_is_bounded_by_history_and_bottom() {
        let term = shared_term(4, 10);
        for _ in 0..6 {
            feed(&term, b"\r\nx");
        }
        let history = scrollback_len(&term.lock());

        for _ in 0..50 {
            scroll(&term, ScrollKind::PageUp);
        }
        assert_eq!(display_offset(&term.lock()), history);

        scroll(&term, ScrollKind::Lines(-1000));
        assert_eq!(display_offset(&term.lock()), 0);
    }

    #[test]
    fn clear_returns_view_to_bottom() {
        let term = shared_term(4, 10);
        for _ in 0..20 {
            feed(&term, b"\r\nx");
        }
        scroll(&term, ScrollKind::PageUp);
        assert!(display_offset(&term.lock()) > 0);

        clear(&term);
        assert_eq!(display_offset(&term.lock()), 0);
    }

    fn term_with(seq: &[u8]) -> Term<ReplyListener> {
        let mut term = new_term(4, 10);
        let mut parser = AnsiParser::new();
        parser.advance(&mut term, seq);
        term
    }

    #[test]
    fn wheel_on_primary_screen_stays_local() {
        let term = term_with(b"");
        assert_eq!(wheel_bytes(&term, 3, 0, 0), None);
        assert_eq!(wheel_bytes(&term, -3, 0, 0), None);
    }

    #[test]
    fn wheel_in_alt_screen_sends_arrows() {
        let term = term_with(b"\x1b[?1049h");
        assert_eq!(wheel_bytes(&term, 2, 0, 0), Some(b"\x1b[A\x1b[A".to_vec()));
        assert_eq!(wheel_bytes(&term, -1, 0, 0), Some(b"\x1b[B".to_vec()));
    }

    #[test]
    fn wheel_in_alt_screen_honors_app_cursor_mode() {
        let term = term_with(b"\x1b[?1049h\x1b[?1h");
        assert_eq!(wheel_bytes(&term, 1, 0, 0), Some(b"\x1bOA".to_vec()));
        assert_eq!(wheel_bytes(&term, -1, 0, 0), Some(b"\x1bOB".to_vec()));
    }

    #[test]
    fn wheel_respects_alternate_scroll_opt_out() {
        let term = term_with(b"\x1b[?1049h\x1b[?1007l");
        assert_eq!(wheel_bytes(&term, 1, 0, 0), None);
    }

    #[test]
    fn wheel_under_sgr_mouse_reporting_sends_wheel_events() {
        let term = term_with(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(
            wheel_bytes(&term, 1, 2, 5),
            Some(b"\x1b[<64;6;3M".to_vec()),
            "1-based coordinates in the SGR event"
        );
        assert_eq!(
            wheel_bytes(&term, -2, 0, 0),
            Some(b"\x1b[<65;1;1M\x1b[<65;1;1M".to_vec())
        );
    }

    #[test]
    fn wheel_under_normal_mouse_reporting_uses_byte_encoding() {
        let term = term_with(b"\x1b[?1000h");
        assert_eq!(
            wheel_bytes(&term, 1, 1, 2),
            Some(vec![0x1b, b'[', b'M', 32 + 64, 32 + 3, 32 + 2])
        );
    }

    #[test]
    fn wheel_mouse_reporting_wins_over_alternate_scroll() {
        let term = term_with(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h");
        assert_eq!(wheel_bytes(&term, 1, 0, 0), Some(b"\x1b[<64;1;1M".to_vec()));
    }

    #[test]
    fn wheel_clamps_pointer_cell_to_the_grid() {
        let term = term_with(b"\x1b[?1000h\x1b[?1006h");
        // 4×10 grid: a cell off the board is clamped to the edge.
        assert_eq!(
            wheel_bytes(&term, 1, 99, 99),
            Some(b"\x1b[<64;10;4M".to_vec())
        );
    }

    #[test]
    fn mouse_protocol_tracks_reporting_motion_and_sgr() {
        assert_eq!(mouse_protocol(&term_with(b"")), MouseProtocol::default());
        assert_eq!(
            mouse_protocol(&term_with(b"\x1b[?1000h")),
            MouseProtocol {
                reporting: true,
                motion: false,
                sgr: false,
            }
        );
        assert_eq!(
            mouse_protocol(&term_with(b"\x1b[?1002h\x1b[?1006h")),
            MouseProtocol {
                reporting: true,
                motion: true,
                sgr: true,
            }
        );
    }

    const SGR: MouseProtocol = MouseProtocol {
        reporting: true,
        motion: true,
        sgr: true,
    };

    #[test]
    fn mouse_report_off_when_reporting_disabled() {
        let off = MouseProtocol::default();
        assert_eq!(
            mouse_report(
                off,
                MouseButton::Left,
                MouseKind::Press,
                MouseMods::default(),
                0,
                0
            ),
            None
        );
    }

    #[test]
    fn mouse_report_sgr_press_and_release_are_1_based() {
        // Left press at cell (row 2, col 5) → SGR `M`, release → lowercase `m`.
        assert_eq!(
            mouse_report(
                SGR,
                MouseButton::Left,
                MouseKind::Press,
                MouseMods::default(),
                2,
                5
            ),
            Some(b"\x1b[<0;6;3M".to_vec())
        );
        assert_eq!(
            mouse_report(
                SGR,
                MouseButton::Left,
                MouseKind::Release,
                MouseMods::default(),
                2,
                5
            ),
            Some(b"\x1b[<0;6;3m".to_vec())
        );
    }

    #[test]
    fn mouse_report_sgr_encodes_button_and_modifiers() {
        assert_eq!(
            mouse_report(
                SGR,
                MouseButton::Right,
                MouseKind::Press,
                MouseMods {
                    alt: false,
                    ctrl: true,
                },
                0,
                0
            ),
            Some(b"\x1b[<18;1;1M".to_vec()) // right(2) + ctrl(16)
        );
        assert_eq!(
            mouse_report(
                SGR,
                MouseButton::Left,
                MouseKind::Drag,
                MouseMods::default(),
                0,
                0
            ),
            Some(b"\x1b[<32;1;1M".to_vec()) // motion bit
        );
    }

    #[test]
    fn mouse_report_drag_needs_motion_tracking() {
        let click_only = MouseProtocol {
            reporting: true,
            motion: false,
            sgr: true,
        };
        assert_eq!(
            mouse_report(
                click_only,
                MouseButton::Left,
                MouseKind::Drag,
                MouseMods::default(),
                0,
                0
            ),
            None
        );
    }

    #[test]
    fn mouse_report_x10_uses_button_3_on_release() {
        let x10 = MouseProtocol {
            reporting: true,
            motion: false,
            sgr: false,
        };
        assert_eq!(
            mouse_report(
                x10,
                MouseButton::Left,
                MouseKind::Press,
                MouseMods::default(),
                1,
                2
            ),
            Some(vec![0x1b, b'[', b'M', 32, 32 + 3, 32 + 2])
        );
        assert_eq!(
            mouse_report(
                x10,
                MouseButton::Left,
                MouseKind::Release,
                MouseMods::default(),
                1,
                2
            ),
            Some(vec![0x1b, b'[', b'M', 32 + 3, 32 + 3, 32 + 2])
        );
    }

    #[test]
    fn kitty_keyboard_push_and_pop_toggle_disambiguate_mode() {
        let term = shared_term(4, 10);
        assert!(!term
            .lock()
            .mode()
            .contains(TermMode::DISAMBIGUATE_ESC_CODES));

        feed(&term, b"\x1b[>1u");
        assert!(term
            .lock()
            .mode()
            .contains(TermMode::DISAMBIGUATE_ESC_CODES));

        feed(&term, b"\x1b[<u");
        assert!(!term
            .lock()
            .mode()
            .contains(TermMode::DISAMBIGUATE_ESC_CODES));
    }

    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn kitty_keyboard_query_is_answered_through_the_writer() {
        let replies = Arc::new(Mutex::new(Vec::new()));
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(SharedBuf(Arc::clone(&replies)))));
        let reader = Box::new(std::io::Cursor::new(b"\x1b[?u".to_vec()));

        let mut emu = Emulator::spawn(reader, 4, 10, writer, |_| {});
        emu.join();

        assert_eq!(replies.lock().unwrap().as_slice(), b"\x1b[?0u");
    }

    #[test]
    fn dynamic_background_query_is_answered_through_the_writer() {
        let replies = Arc::new(Mutex::new(Vec::new()));
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(SharedBuf(Arc::clone(&replies)))));
        let reader = Box::new(std::io::Cursor::new(b"\x1b]11;?\x1b\\".to_vec()));

        let mut emu =
            Emulator::spawn_with_palette(reader, 4, 10, writer, TermPalette::dark(), |_| {});
        emu.join();

        assert_eq!(
            replies.lock().unwrap().as_slice(),
            b"\x1b]11;rgb:1919/2222/2d2d\x1b\\"
        );
    }

    #[test]
    fn osc_title_is_captured_then_cleared_by_reset() {
        let writer: PtyWriter = Arc::new(Mutex::new(Box::new(Vec::new())));
        // OSC 2 sets the title; OSC 2 with an empty string resets it.
        let reader = Box::new(std::io::Cursor::new(b"\x1b]2;build\x07".to_vec()));
        let mut emu = Emulator::spawn(reader, 4, 10, Arc::clone(&writer), |_| {});
        emu.join();
        assert_eq!(emu.title(), Some("build".to_string()));

        let reader = Box::new(std::io::Cursor::new(b"\x1b]0;deploy\x07".to_vec()));
        let mut emu = Emulator::spawn(reader, 4, 10, writer, |_| {});
        emu.join();
        assert_eq!(
            emu.title(),
            Some("deploy".to_string()),
            "OSC 0 also sets it"
        );
    }

    #[test]
    fn font_zoom_starts_at_default_and_resets_to_it() {
        let mut zoom = FontZoom::default();
        assert_eq!(zoom.point_size(), DEFAULT_FONT_SIZE);
        zoom.zoom_in();
        zoom.zoom_in();
        assert_ne!(zoom.point_size(), DEFAULT_FONT_SIZE);
        zoom.reset();
        assert_eq!(zoom.point_size(), DEFAULT_FONT_SIZE);
    }

    #[test]
    fn font_zoom_in_and_out_step_by_one_point() {
        let mut zoom = FontZoom::default();
        zoom.zoom_in();
        assert_eq!(zoom.point_size(), DEFAULT_FONT_SIZE + FONT_ZOOM_STEP);
        zoom.zoom_out();
        assert_eq!(zoom.point_size(), DEFAULT_FONT_SIZE);
    }

    #[test]
    fn font_zoom_is_bounded() {
        let mut zoom = FontZoom::default();
        for _ in 0..200 {
            zoom.zoom_out();
        }
        assert_eq!(zoom.point_size(), MIN_FONT_SIZE);
        for _ in 0..200 {
            zoom.zoom_in();
        }
        assert_eq!(zoom.point_size(), MAX_FONT_SIZE);
    }
}
