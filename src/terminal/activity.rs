//! Per-pane I/O activity counter (layer B of agent detection, specs/agents.md):
//! lock-free timestamps stamped by the reader thread (output) and the UI thread
//! (input). Output arriving right after a keystroke is an echo (keystroke, TUI
//! navigation): it does not count as spontaneous.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Output closer than this to the last input = echo, not spontaneous.
pub const ECHO_WINDOW_MS: u64 = 350;
/// Output closer than this to a viewport resize = the program redrawing on
/// SIGWINCH (helm changed the pane size: window resize, split, or switching
/// to/from the agents dashboard), not the agent doing new work. Suppressed like
/// a keystroke echo so a resize never re-arms Working/Done. Wider than the echo
/// window: a full-screen TUI repaint streams longer than a key echo.
pub const RESIZE_ECHO_MS: u64 = 1_000;
/// A gap longer than this between two spontaneous outputs ends the current run.
pub const RUN_GAP_MS: u64 = 2_500;
/// Byte-window width: throughput is measured over the current window + the
/// previous one so it does not drop to zero at each switch.
pub const BYTE_WINDOW_MS: u64 = 2_500;

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Milliseconds since the process epoch, never 0 (0 = "never" in the atomics).
pub fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64 + 1
}

/// Best-effort consistent snapshot of the counters (fields are read one by one:
/// millisecond precision without a lock, sufficient for a heuristic).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivitySnapshot {
    pub last_input_ms: u64,
    pub last_spont_output_ms: u64,
    pub spont_run_start_ms: u64,
    window_start_ms: u64,
    window_bytes: u64,
    prev_window_bytes: u64,
}

impl ActivitySnapshot {
    /// Spontaneous bytes received over the last two windows, 0 if the window is
    /// stale (no recent spontaneous output).
    pub fn recent_spont_bytes(&self, now_ms: u64) -> u64 {
        if self.window_start_ms == 0
            || now_ms.saturating_sub(self.window_start_ms) > 2 * BYTE_WINDOW_MS
        {
            0
        } else {
            self.window_bytes + self.prev_window_bytes
        }
    }
}

#[derive(Default)]
pub struct PaneActivity {
    last_input_ms: AtomicU64,
    last_resize_ms: AtomicU64,
    last_spont_output_ms: AtomicU64,
    spont_run_start_ms: AtomicU64,
    window_start_ms: AtomicU64,
    window_bytes: AtomicU64,
    prev_window_bytes: AtomicU64,
}

impl PaneActivity {
    pub fn stamp_input(&self, now_ms: u64) {
        self.last_input_ms.store(now_ms, Ordering::Relaxed);
    }

    /// A viewport resize: the program's SIGWINCH repaint that follows is helm's
    /// doing, not the agent's — suppressed like an echo. Does **not** count as a
    /// reply (no `last_input` stamp), so it never acknowledges a pending green.
    pub fn stamp_resize(&self, now_ms: u64) {
        self.last_resize_ms.store(now_ms, Ordering::Relaxed);
    }

    pub fn stamp_output(&self, now_ms: u64, bytes: u64) {
        let last_input = self.last_input_ms.load(Ordering::Relaxed);
        if last_input != 0 && now_ms.saturating_sub(last_input) <= ECHO_WINDOW_MS {
            return;
        }
        let last_resize = self.last_resize_ms.load(Ordering::Relaxed);
        if last_resize != 0 && now_ms.saturating_sub(last_resize) <= RESIZE_ECHO_MS {
            return;
        }
        let last_spont = self.last_spont_output_ms.load(Ordering::Relaxed);
        if last_spont == 0 || now_ms.saturating_sub(last_spont) > RUN_GAP_MS {
            self.spont_run_start_ms.store(now_ms, Ordering::Relaxed);
        }
        self.last_spont_output_ms.store(now_ms, Ordering::Relaxed);

        let window_start = self.window_start_ms.load(Ordering::Relaxed);
        if window_start == 0 || now_ms.saturating_sub(window_start) > BYTE_WINDOW_MS {
            // Next window: the old one becomes "previous" if it is contiguous,
            // otherwise throughput restarts from zero.
            let stale =
                window_start == 0 || now_ms.saturating_sub(window_start) > 2 * BYTE_WINDOW_MS;
            let prev = if stale {
                0
            } else {
                self.window_bytes.load(Ordering::Relaxed)
            };
            self.prev_window_bytes.store(prev, Ordering::Relaxed);
            self.window_start_ms.store(now_ms, Ordering::Relaxed);
            self.window_bytes.store(bytes, Ordering::Relaxed);
        } else {
            self.window_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> ActivitySnapshot {
        ActivitySnapshot {
            last_input_ms: self.last_input_ms.load(Ordering::Relaxed),
            last_spont_output_ms: self.last_spont_output_ms.load(Ordering::Relaxed),
            spont_run_start_ms: self.spont_run_start_ms.load(Ordering::Relaxed),
            window_start_ms: self.window_start_ms.load(Ordering::Relaxed),
            window_bytes: self.window_bytes.load(Ordering::Relaxed),
            prev_window_bytes: self.prev_window_bytes.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_right_after_input_is_an_echo() {
        let a = PaneActivity::default();
        a.stamp_input(1_000);
        a.stamp_output(1_100, 64);
        let s = a.snapshot();
        assert_eq!(s.last_spont_output_ms, 0, "echo is not spontaneous output");
        assert_eq!(s.recent_spont_bytes(1_100), 0);
        assert_eq!(s.last_input_ms, 1_000);
    }

    #[test]
    fn output_right_after_a_resize_is_an_echo() {
        let a = PaneActivity::default();
        a.stamp_resize(1_000);
        a.stamp_output(1_300, 4_096);
        let s = a.snapshot();
        assert_eq!(
            s.last_spont_output_ms, 0,
            "a SIGWINCH repaint is not spontaneous output"
        );
        assert_eq!(s.recent_spont_bytes(1_300), 0);
    }

    #[test]
    fn output_past_the_resize_window_is_spontaneous() {
        let a = PaneActivity::default();
        a.stamp_resize(1_000);
        a.stamp_output(1_000 + RESIZE_ECHO_MS + 1, 256);
        assert_eq!(
            a.snapshot().last_spont_output_ms,
            1_000 + RESIZE_ECHO_MS + 1
        );
    }

    #[test]
    fn output_past_the_echo_window_is_spontaneous() {
        let a = PaneActivity::default();
        a.stamp_input(1_000);
        a.stamp_output(1_000 + ECHO_WINDOW_MS + 1, 64);
        let s = a.snapshot();
        assert_eq!(s.last_spont_output_ms, 1_000 + ECHO_WINDOW_MS + 1);
        assert_eq!(s.spont_run_start_ms, 1_000 + ECHO_WINDOW_MS + 1);
    }

    #[test]
    fn output_without_any_input_is_spontaneous() {
        let a = PaneActivity::default();
        a.stamp_output(500, 10);
        assert_eq!(a.snapshot().last_spont_output_ms, 500);
    }

    #[test]
    fn a_run_survives_gaps_shorter_than_run_gap() {
        let a = PaneActivity::default();
        a.stamp_output(1_000, 10);
        a.stamp_output(1_000 + RUN_GAP_MS, 10);
        let s = a.snapshot();
        assert_eq!(s.spont_run_start_ms, 1_000, "the run keeps its start");
        assert_eq!(s.last_spont_output_ms, 1_000 + RUN_GAP_MS);
    }

    #[test]
    fn a_gap_longer_than_run_gap_starts_a_new_run() {
        let a = PaneActivity::default();
        a.stamp_output(1_000, 10);
        a.stamp_output(1_000 + RUN_GAP_MS + 1, 10);
        assert_eq!(a.snapshot().spont_run_start_ms, 1_000 + RUN_GAP_MS + 1);
    }

    #[test]
    fn byte_rate_spans_current_and_previous_window() {
        let a = PaneActivity::default();
        a.stamp_output(1_000, 150);
        // Window switch: the previous bytes stay counted.
        let t2 = 1_000 + BYTE_WINDOW_MS + 1;
        a.stamp_output(t2, 100);
        assert_eq!(a.snapshot().recent_spont_bytes(t2), 250);
    }

    #[test]
    fn stale_windows_report_zero_bytes() {
        let a = PaneActivity::default();
        a.stamp_output(1_000, 500);
        let later = 1_000 + 2 * BYTE_WINDOW_MS + 1;
        assert_eq!(a.snapshot().recent_spont_bytes(later), 0);
    }

    #[test]
    fn a_long_silence_resets_the_previous_window_too() {
        let a = PaneActivity::default();
        a.stamp_output(1_000, 500);
        let later = 1_000 + 3 * BYTE_WINDOW_MS;
        a.stamp_output(later, 10);
        assert_eq!(
            a.snapshot().recent_spont_bytes(later),
            10,
            "bytes from before the silence are not carried over"
        );
    }

    #[test]
    fn now_ms_is_never_zero() {
        assert!(now_ms() > 0);
    }
}
