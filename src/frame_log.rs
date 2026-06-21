//! Frame-pacing instrumentation, enabled with `HELM_FRAME_LOG=1` (works in
//! release builds). Every ~2 s one summary line is appended to
//! `/tmp/helm-frames.log` with percentiles over the window:
//!
//! - `gap`  — time between consecutive `ui()` starts: the real on-screen
//!   cadence; spikes here are the visible stutter.
//! - `ui`   — duration of the `ui()` pass (widget building on the UI thread).
//! - `cpu`  — eframe's full previous-frame CPU time (tessellation and paint
//!   included), which `ui` alone does not cover.
//! - `lock` — UI-thread wait acquiring the terminal `FairMutex` (PTY reader
//!   contention; see `add_lock_wait` call sites in `terminal_view`).
//!
//! Reading the line: a `gap` spike with a matching `cpu` spike means the frame
//! itself was expensive (then `ui` vs `cpu` says whether building or painting);
//! a `gap` spike with flat `cpu` means the runloop slept or was scheduled out
//! (repaint pacing, QoS starvation, event delivery).

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static LOCK_WAIT_NS: AtomicU64 = AtomicU64::new(0);
static WHEEL_EVENTS: AtomicU64 = AtomicU64::new(0);
static PTY_WAKEUPS: AtomicU64 = AtomicU64::new(0);

/// Accumulates the UI thread's wait on a terminal lock for the current frame.
/// Unconditional at call sites: two `Instant::now()` per pane per frame.
pub fn add_lock_wait(elapsed: std::time::Duration) {
    LOCK_WAIT_NS.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
}

/// Wheel events delivered with this frame's raw input (`raw_input_hook`):
/// a stall whose closing frame carries wheel events happened mid-scroll —
/// that is the user-felt hitch, as opposed to a healthy idle gap.
pub fn note_wheel_events(n: u64) {
    WHEEL_EVENTS.fetch_add(n, Ordering::Relaxed);
}

/// A PTY reader booked a repaint (`repaint_when_visible`). Wakeups counted
/// during a stall prove the loop had a pending paint and still did not run.
pub fn note_pty_wakeup() {
    PTY_WAKEUPS.fetch_add(1, Ordering::Relaxed);
}

/// Above this, a gap between frames is logged individually with its context.
/// 50 ms leaves room for the 33 ms terminal redraw deadline plus a vsync.
const STALL_MS: f32 = 50.0;

pub struct FrameLog {
    file: std::fs::File,
    started: Instant,
    last_flush: Instant,
    frame_start: Option<Instant>,
    gaps_ms: Vec<f32>,
    uis_ms: Vec<f32>,
    cpus_ms: Vec<f32>,
    locks_ms: Vec<f32>,
    wheel_total: u64,
    pty_total: u64,
}

pub const PATH: &str = "/tmp/helm-frames.log";

impl FrameLog {
    /// `None` unless `HELM_FRAME_LOG` is set (truncates the log file when set).
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var_os("HELM_FRAME_LOG").is_some_and(|v| v != "0");
        let file = enabled.then(|| std::fs::File::create(PATH).ok())??;
        let now = Instant::now();
        Some(Self {
            file,
            started: now,
            last_flush: now,
            frame_start: None,
            gaps_ms: Vec::new(),
            uis_ms: Vec::new(),
            cpus_ms: Vec::new(),
            locks_ms: Vec::new(),
            wheel_total: 0,
            pty_total: 0,
        })
    }

    pub fn begin_frame(&mut self, prev_frame_cpu: Option<f32>) {
        let now = Instant::now();
        // Wheel events came with this frame's input; PTY wakeups accumulated
        // since the previous frame began — i.e. during the gap being closed.
        let wheel = WHEEL_EVENTS.swap(0, Ordering::Relaxed);
        let pty = PTY_WAKEUPS.swap(0, Ordering::Relaxed);
        self.wheel_total += wheel;
        self.pty_total += pty;
        if let Some(prev) = self.frame_start {
            let gap_ms = (now - prev).as_secs_f32() * 1e3;
            self.gaps_ms.push(gap_ms);
            if gap_ms > STALL_MS {
                let _ = writeln!(
                    self.file,
                    "stall +{:7.1}s gap={gap_ms:6.1}ms wheel={wheel} pty={pty}",
                    (now - self.started).as_secs_f32(),
                );
            }
        }
        self.frame_start = Some(now);
        if let Some(cpu) = prev_frame_cpu {
            self.cpus_ms.push(cpu * 1e3);
        }
        LOCK_WAIT_NS.store(0, Ordering::Relaxed);
    }

    pub fn end_frame(&mut self, mode: &str) {
        let Some(start) = self.frame_start else {
            return;
        };
        self.uis_ms.push(start.elapsed().as_secs_f32() * 1e3);
        self.locks_ms
            .push(LOCK_WAIT_NS.load(Ordering::Relaxed) as f32 / 1e6);
        if self.last_flush.elapsed().as_secs_f32() >= 2.0 {
            self.flush(mode);
        }
    }

    fn flush(&mut self, mode: &str) {
        let line = format!(
            "+{:7.1}s mode={mode} n={} | gap {} | ui {} | cpu {} | lock {} | wheel={} pty={}",
            self.started.elapsed().as_secs_f32(),
            self.gaps_ms.len(),
            series(&mut self.gaps_ms),
            series(&mut self.uis_ms),
            series(&mut self.cpus_ms),
            series(&mut self.locks_ms),
            self.wheel_total,
            self.pty_total,
        );
        let _ = writeln!(self.file, "{line}");
        self.gaps_ms.clear();
        self.uis_ms.clear();
        self.cpus_ms.clear();
        self.locks_ms.clear();
        self.wheel_total = 0;
        self.pty_total = 0;
        self.last_flush = Instant::now();
    }
}

fn series(values: &mut [f32]) -> String {
    values.sort_by(|a, b| a.total_cmp(b));
    format!(
        "p50={:5.1} p95={:5.1} max={:5.1}",
        pct(values, 0.50),
        pct(values, 0.95),
        values.last().copied().unwrap_or(0.0),
    )
}

fn pct(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f32 - 1.0) * p).round() as usize;
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::pct;

    #[test]
    fn pct_handles_empty_and_picks_rank() {
        assert_eq!(pct(&[], 0.5), 0.0);
        let sorted = [1.0, 2.0, 3.0, 4.0, 100.0];
        assert_eq!(pct(&sorted, 0.5), 3.0);
        assert_eq!(pct(&sorted, 0.95), 100.0);
    }
}
