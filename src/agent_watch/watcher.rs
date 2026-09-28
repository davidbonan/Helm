//! The agent state machine on its own thread (specs/remote.md §4): a hidden app
//! gets no frame, yet the badges must keep moving — the phone reads them precisely
//! when helm is out of sight. The UI hands over the live panes and the focused
//! ones, then reads the published readings.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use super::{probe, AgentBadge, PaneAgentState};
use crate::terminal::activity::{now_ms, PaneActivity};
use crate::terminal::pane::{Pane, PaneUid};
use crate::terminal::pty::PgidProbe;

/// Cadence of the state machine (specs/agents.md §2).
pub const TICK: Duration = Duration::from_secs(1);

/// What the watcher needs of a pane: shared handles only, so it never borrows
/// the `Pane` the UI thread owns.
pub struct WatchedPane {
    uid: PaneUid,
    activity: Arc<PaneActivity>,
    pgid_probe: Arc<PgidProbe>,
}

impl WatchedPane {
    pub fn of(pane: &Pane) -> Self {
        Self {
            uid: pane.uid(),
            activity: pane.shared_activity(),
            pgid_probe: pane.shared_pgid_probe(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneReading {
    /// Watchlist name of the agent in the foreground, `None` when absent.
    pub agent: Option<&'static str>,
    pub badge: AgentBadge,
}

/// The last tick's readings; `generation` moves only when a reading changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readings {
    pub generation: u64,
    pub panes: HashMap<PaneUid, PaneReading>,
}

struct Track {
    panes: Vec<WatchedPane>,
    focused: HashSet<PaneUid>,
}

/// Joined on drop (closing the channel ends the loop), like the git worker.
pub struct AgentWatcher {
    track: Option<Sender<Track>>,
    readings: Arc<Mutex<Readings>>,
    thread: Option<JoinHandle<()>>,
}

impl AgentWatcher {
    /// `on_change` runs on the watcher thread whenever a reading changes.
    pub fn spawn(on_change: impl Fn() + Send + 'static) -> Self {
        let (track, commands) = crossbeam_channel::unbounded();
        let readings = Arc::new(Mutex::new(Readings::default()));
        let published = Arc::clone(&readings);
        let thread = std::thread::Builder::new()
            .name("agent-watch".into())
            .spawn(move || run(&commands, &published, &on_change))
            .expect("spawn the agent watcher thread");
        Self {
            track: Some(track),
            readings,
            thread: Some(thread),
        }
    }

    /// Replaces the watched set. `focused`: panes the user is looking at —
    /// seeing acknowledges a green (specs/agents.md §1).
    pub fn track(&self, panes: Vec<WatchedPane>, focused: HashSet<PaneUid>) {
        if let Some(track) = &self.track {
            let _ = track.send(Track { panes, focused });
        }
    }

    /// The readings, when their generation differs from `generation`.
    pub fn changed_since(&self, generation: u64) -> Option<Readings> {
        let readings = lock(&self.readings);
        (readings.generation != generation).then(|| readings.clone())
    }
}

impl Drop for AgentWatcher {
    fn drop(&mut self) {
        self.track.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(readings: &Mutex<Readings>) -> MutexGuard<'_, Readings> {
    readings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run(commands: &Receiver<Track>, published: &Mutex<Readings>, on_change: &dyn Fn()) {
    let mut watched: Vec<WatchedPane> = Vec::new();
    let mut focused: HashSet<PaneUid> = HashSet::new();
    let mut states: HashMap<PaneUid, PaneAgentState> = HashMap::new();
    let mut next_tick = Instant::now();
    loop {
        match commands.recv_deadline(next_tick) {
            Ok(track) => {
                let focus_moved = track.focused != focused;
                watched = track.panes;
                focused = track.focused;
                states.retain(|uid, _| watched.iter().any(|pane| pane.uid == *uid));
                // A focus change acknowledges a green now rather than at the next tick.
                if !focus_moved {
                    continue;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let panes = tick(&watched, &focused, &mut states, now_ms());
        next_tick = Instant::now() + TICK;
        let mut readings = lock(published);
        if readings.panes != panes {
            readings.generation += 1;
            readings.panes = panes;
            drop(readings);
            on_change();
        }
    }
}

fn tick(
    watched: &[WatchedPane],
    focused: &HashSet<PaneUid>,
    states: &mut HashMap<PaneUid, PaneAgentState>,
    now_ms: u64,
) -> HashMap<PaneUid, PaneReading> {
    watched
        .iter()
        .map(|pane| {
            let agent = pane
                .pgid_probe
                .foreground_pgid()
                .and_then(probe::foreground_agent);
            let badge = states.entry(pane.uid).or_default().tick(
                agent.is_some(),
                &pane.activity.snapshot(),
                focused.contains(&pane.uid),
                now_ms,
            );
            (pane.uid, PaneReading { agent, badge })
        })
        .collect()
}
