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

enum Command {
    Track(Track),
    /// A phone shows the pane: seeing it there acknowledges, like the Mac focus.
    PhoneSees(PaneUid),
    PhoneLeaves(PaneUid),
    /// A pane the phone launched, watched until a `Track` lists it (the UI adopted
    /// it) or it is forgotten (dropped unadopted).
    Launched(WatchedPane),
    Forget(PaneUid),
}

/// Joined on drop (closing the channel ends the loop), like the git worker.
pub struct AgentWatcher {
    commands: Option<Sender<Command>>,
    readings: Arc<Mutex<Readings>>,
    thread: Option<JoinHandle<()>>,
}

impl AgentWatcher {
    /// `on_change` runs on the watcher thread whenever a reading changes.
    pub fn spawn(on_change: impl Fn() + Send + 'static) -> Self {
        let (sender, commands) = crossbeam_channel::unbounded();
        let readings = Arc::new(Mutex::new(Readings::default()));
        let published = Arc::clone(&readings);
        let thread = std::thread::Builder::new()
            .name("agent-watch".into())
            .spawn(move || run(&commands, &published, &on_change))
            .expect("spawn the agent watcher thread");
        Self {
            commands: Some(sender),
            readings,
            thread: Some(thread),
        }
    }

    /// Replaces the watched set. `focused`: panes the user is looking at —
    /// seeing acknowledges a green (specs/agents.md §1).
    pub fn track(&self, panes: Vec<WatchedPane>, focused: HashSet<PaneUid>) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Track(Track { panes, focused }));
        }
    }

    pub fn link(&self) -> WatcherLink {
        WatcherLink {
            readings: Arc::clone(&self.readings),
            commands: self.commands.clone().expect("the channel lives until drop"),
        }
    }

    /// The readings, when their generation differs from `generation`.
    pub fn changed_since(&self, generation: u64) -> Option<Readings> {
        let readings = lock(&self.readings);
        (readings.generation != generation).then(|| readings.clone())
    }
}

/// The watcher as another thread (the phone server) reaches it: the published
/// readings, and which panes a phone shows.
#[derive(Clone)]
pub struct WatcherLink {
    readings: Arc<Mutex<Readings>>,
    commands: Sender<Command>,
}

impl WatcherLink {
    pub fn get(&self, uid: PaneUid) -> Option<PaneReading> {
        lock(&self.readings).panes.get(&uid).copied()
    }

    pub fn phone_sees(&self, uid: PaneUid) {
        let _ = self.commands.send(Command::PhoneSees(uid));
    }

    pub fn phone_leaves(&self, uid: PaneUid) {
        let _ = self.commands.send(Command::PhoneLeaves(uid));
    }

    pub fn launched(&self, pane: WatchedPane) {
        let _ = self.commands.send(Command::Launched(pane));
    }

    pub fn forget(&self, uid: PaneUid) {
        let _ = self.commands.send(Command::Forget(uid));
    }
}

impl Drop for AgentWatcher {
    fn drop(&mut self) {
        self.commands.take();
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

fn run(commands: &Receiver<Command>, published: &Mutex<Readings>, on_change: &dyn Fn()) {
    let mut watched: Vec<WatchedPane> = Vec::new();
    let mut launched: Vec<WatchedPane> = Vec::new();
    let mut focused: HashSet<PaneUid> = HashSet::new();
    // Phones showing each pane: two phones on one agent both count.
    let mut phone_seen: HashMap<PaneUid, usize> = HashMap::new();
    let mut states: HashMap<PaneUid, PaneAgentState> = HashMap::new();
    let mut next_tick = Instant::now();
    loop {
        let sight_moved = match commands.recv_deadline(next_tick) {
            Ok(Command::Track(track)) => {
                let focus_moved = track.focused != focused;
                watched = track.panes;
                focused = track.focused;
                launched.retain(|pane| !watched.iter().any(|w| w.uid == pane.uid));
                states
                    .retain(|uid, _| watched.iter().chain(&launched).any(|pane| pane.uid == *uid));
                focus_moved
            }
            Ok(Command::PhoneSees(uid)) => {
                *phone_seen.entry(uid).or_default() += 1;
                true
            }
            Ok(Command::PhoneLeaves(uid)) => {
                if let Some(count) = phone_seen.get_mut(&uid) {
                    *count -= 1;
                    if *count == 0 {
                        phone_seen.remove(&uid);
                    }
                }
                false
            }
            Ok(Command::Launched(pane)) => {
                launched.push(pane);
                true
            }
            Ok(Command::Forget(uid)) => {
                launched.retain(|pane| pane.uid != uid);
                states.remove(&uid);
                true
            }
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // A new sight acknowledges a green now rather than at the next tick.
        if !sight_moved {
            continue;
        }
        let seen: HashSet<PaneUid> = focused.iter().chain(phone_seen.keys()).copied().collect();
        let panes = tick(
            watched.iter().chain(&launched),
            &seen,
            &mut states,
            now_ms(),
        );
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

fn tick<'a>(
    watched: impl Iterator<Item = &'a WatchedPane>,
    focused: &HashSet<PaneUid>,
    states: &mut HashMap<PaneUid, PaneAgentState>,
    now_ms: u64,
) -> HashMap<PaneUid, PaneReading> {
    watched
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
