use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::term::{Term, TermMode};
use anyhow::Result;
use portable_pty::{Child, CommandBuilder, PtySize};

use crate::terminal::activity::{now_ms, PaneActivity};
use crate::terminal::emu::{
    lock_writer, resize_term, Emulator, PtyWriter, ReplyListener, SharedTerm,
};
use crate::terminal::palette::TermPalette;
use crate::terminal::pty::Pty;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPos {
    pub line: i32,
    pub col: usize,
}

type OnChange = Arc<dyn Fn() + Send + Sync>;

pub struct Pane {
    pty: Pty,
    emu: Emulator,
    writer: PtyWriter,
    cwd: PathBuf,
    on_change: OnChange,
    /// Pane I/O timestamps (agent detection, specs/agents.md): input stamped by
    /// `feed`, output by the reader callback.
    activity: Arc<PaneActivity>,
    /// Set per frame from the paint site: true only while this pane is actually
    /// on screen. Gates the reader's repaint wakeup so a pane in a background
    /// repo/tab updates its grid in memory without pacing the whole event loop.
    visible: Arc<AtomicBool>,
    rows: u16,
    cols: u16,
    reply_palette: TermPalette,
}

impl Pane {
    pub fn open(
        cwd: &Path,
        rows: u16,
        cols: u16,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::from_pty(
            Pty::open_login_shell(cwd, pty_size(rows, cols))?,
            cwd,
            rows,
            cols,
            Arc::new(on_change),
        )
    }

    pub fn from_command(
        cmd: CommandBuilder,
        rows: u16,
        cols: u16,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let cwd = cmd
            .get_cwd()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        Self::from_pty(
            Pty::spawn(cmd, pty_size(rows, cols))?,
            &cwd,
            rows,
            cols,
            Arc::new(on_change),
        )
    }

    fn from_pty(pty: Pty, cwd: &Path, rows: u16, cols: u16, on_change: OnChange) -> Result<Self> {
        // Writer shared with the emulator: the `ReplyListener` writes there the
        // replies to the program's queries (kitty `CSI ? u`, DA, DSR, OSC colors).
        let writer: PtyWriter = Arc::new(Mutex::new(pty.take_writer()?));
        let activity = Arc::new(PaneActivity::default());
        let visible = Arc::new(AtomicBool::new(false));
        let reply_palette = TermPalette::dark();
        let emu = Emulator::spawn(
            pty.reader()?,
            rows,
            cols,
            Arc::clone(&writer),
            callback(&on_change, &activity, &visible),
        );
        Ok(Self {
            pty,
            emu,
            writer,
            cwd: cwd.to_path_buf(),
            on_change,
            activity,
            visible,
            rows,
            cols,
            reply_palette,
        })
    }

    /// Marks whether this pane is painted in the current frame (driven by the
    /// render path). The reader's repaint wakeup is gated on it.
    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::Relaxed);
    }

    pub fn feed(&self, bytes: &[u8]) -> Result<()> {
        self.activity.stamp_input(now_ms());
        let mut writer = lock_writer(&self.writer);
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub fn input(&self, bytes: &[u8]) -> Result<()> {
        self.feed(bytes)
    }

    pub fn paste(&self, text: &str) -> Result<()> {
        if bracketed_paste(&self.emu.term().lock()) {
            self.feed(b"\x1b[200~")?;
            self.feed(text.as_bytes())?;
            self.feed(b"\x1b[201~")
        } else {
            self.feed(text.as_bytes())
        }
    }

    pub fn grid(&self) -> &SharedTerm {
        self.emu.term()
    }

    pub fn clear(&self) {
        crate::terminal::emu::clear(self.emu.term());
    }

    pub fn scroll(&self, kind: crate::terminal::emu::ScrollKind) {
        crate::terminal::emu::scroll(self.emu.term(), kind);
    }

    pub fn cursor(&self) -> CursorPos {
        cursor_pos(&self.emu.term().lock())
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn set_reply_palette(&mut self, palette: TermPalette) {
        if self.reply_palette == palette {
            return;
        }
        self.reply_palette = palette;
        self.emu.set_reply_palette(palette);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        // Stamp before the PTY resize so the window is open when the program's
        // SIGWINCH repaint lands: that burst is helm's doing, not agent work,
        // and must not re-arm the activity badge (specs/agents.md).
        self.activity.stamp_resize(now_ms());
        self.pty.resize(rows, cols)?;
        resize_term(&mut self.emu.term().lock(), rows, cols);
        self.rows = rows;
        self.cols = cols;
        Ok(())
    }

    pub fn activity(&self) -> &PaneActivity {
        &self.activity
    }

    /// The shell process id (PTY child), used to read its live cwd (terminal.md §12).
    pub fn shell_pid(&self) -> Option<u32> {
        self.pty.process_id()
    }

    /// The cwd the pane's shell was spawned in — the fallback when the live cwd
    /// is unavailable (terminal.md §12).
    pub fn spawn_cwd(&self) -> &Path {
        &self.cwd
    }

    /// Last OSC 0/1/2 title set by the program (tab auto-naming, terminal.md §4);
    /// `None` if it never set one.
    pub fn osc_title(&self) -> Option<String> {
        self.emu.title()
    }

    /// PTY's foreground process group (layer A of agent detection); `None` if the
    /// terminal no longer has a controlling process.
    pub fn foreground_pgid(&self) -> Option<i32> {
        self.pty.foreground_pgid()
    }

    pub fn child(&mut self) -> &mut (dyn Child + Send + Sync) {
        self.pty.child()
    }

    pub fn has_exited(&mut self) -> bool {
        matches!(self.pty.child().try_wait(), Ok(Some(_)))
    }

    pub fn relaunch(&mut self) -> Result<()> {
        let pty = Pty::open_login_shell(&self.cwd, pty_size(self.rows, self.cols))?;
        let writer: PtyWriter = Arc::new(Mutex::new(pty.take_writer()?));
        let emu = Emulator::spawn_with_palette(
            pty.reader()?,
            self.rows,
            self.cols,
            Arc::clone(&writer),
            self.reply_palette,
            callback(&self.on_change, &self.activity, &self.visible),
        );
        // The old emulator is **not** joined: its reader only returns on PTY EOF,
        // and a detached survivor (setsid, disowned job) still holding the slave
        // would block the UI thread indefinitely. Once replaced, the thread shuts
        // itself down on EOF (same tradeoff as dropping a pane, emu.rs).
        self.pty = pty;
        self.emu = emu;
        self.writer = writer;
        (self.on_change)();
        Ok(())
    }

    pub fn join(&mut self) {
        self.emu.join();
    }
}

fn bracketed_paste(term: &Term<ReplyListener>) -> bool {
    term.mode().contains(TermMode::BRACKETED_PASTE)
}

fn cursor_pos(term: &Term<ReplyListener>) -> CursorPos {
    let point = term.grid().cursor.point;
    CursorPos {
        line: point.line.0,
        col: point.column.0,
    }
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Coalesce the reader's repaint wakeups: a streaming TUI agent emits a chunk —
/// and would wake the UI — hundreds of times per second. At ~16 ms apart per pane
/// the terminal still redraws near the display refresh, but the background thread
/// stops pacing the whole event loop; without it, scrolling stutters while an
/// agent streams (measured: frames pinned to the wakeup cadence, not the display).
const REDRAW_THROTTLE_MS: u64 = 16;

fn callback(
    on_change: &OnChange,
    activity: &Arc<PaneActivity>,
    visible: &Arc<AtomicBool>,
) -> impl Fn(usize) + Send + 'static {
    let on_change = Arc::clone(on_change);
    let activity = Arc::clone(activity);
    let visible = Arc::clone(visible);
    let last_repaint = AtomicU64::new(0);
    move |bytes| {
        let now = now_ms();
        // Stamped unconditionally — agent detection (the 1 s poll) tracks panes in
        // background repos too; only the repaint wakeup below is visibility-gated.
        activity.stamp_output(now, bytes as u64);
        // An off-screen pane (other repo/tab, or a dashboard not mirroring it) keeps
        // reading into its grid but must not pace the event loop: its output would
        // repaint the whole window for pixels nobody can see.
        if !visible.load(Ordering::Relaxed) {
            return;
        }
        // Skipping a wakeup is lossless: a prior request within the window already
        // booked a paint that renders the now-updated grid. `last == 0` means no
        // wakeup yet (now_ms never returns 0): always fire so the first output —
        // shell prompt, keystroke echo — paints without waiting out the window.
        let last = last_repaint.load(Ordering::Relaxed);
        if last == 0 || now.saturating_sub(last) >= REDRAW_THROTTLE_MS {
            last_repaint.store(now, Ordering::Relaxed);
            on_change();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    #[test]
    fn cursor_pos_reads_grid_cursor() {
        let mut term = crate::terminal::emu::new_term(4, 10);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, b"abc");
        assert_eq!(cursor_pos(&term), CursorPos { line: 0, col: 3 });
    }

    #[test]
    fn bracketed_paste_tracks_terminal_mode() {
        let mut term = crate::terminal::emu::new_term(4, 10);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        assert!(!bracketed_paste(&term));
        parser.advance(&mut term, b"\x1b[?2004h");
        assert!(bracketed_paste(&term));
        parser.advance(&mut term, b"\x1b[?2004l");
        assert!(!bracketed_paste(&term));
    }
}
