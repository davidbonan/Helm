//! What the phone may see (specs/remote.md §1, §4): the live panes the UI
//! publishes at its poll, filtered by the agent watcher's live readings — only a
//! pane with an agent is exposed, so the phone never reaches a plain shell.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent_watch::watcher::{PaneReading, WatcherLink};
use crate::agent_watch::AgentBadge;
use crate::terminal::palette::TermPalette;
use crate::terminal::pane::{PaneHandle, PaneUid};

#[derive(Clone)]
pub struct ExposedPane {
    pub uid: PaneUid,
    pub project: String,
    pub branch: Option<String>,
    pub tab: String,
    pub handle: PaneHandle,
}

/// An exposed pane with an agent, as the phone lists it.
pub struct ExposedAgent {
    pub pane: ExposedPane,
    pub reading: PaneReading,
}

/// Shared between the UI (writer) and the server threads (readers).
#[derive(Clone)]
pub struct Registry(Arc<Mutex<Published>>);

struct Published {
    panes: Vec<ExposedPane>,
    watcher: Option<WatcherLink>,
    palette: TermPalette,
}

impl Default for Registry {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Published {
            panes: Vec::new(),
            watcher: None,
            palette: TermPalette::dark(),
        })))
    }
}

impl Registry {
    pub fn publish(
        &self,
        panes: Vec<ExposedPane>,
        watcher: Option<WatcherLink>,
        palette: TermPalette,
    ) {
        *self.lock() = Published {
            panes,
            watcher,
            palette,
        };
    }

    pub fn agents(&self) -> Vec<ExposedAgent> {
        let published = self.lock();
        published
            .panes
            .iter()
            .filter_map(|pane| {
                let reading = published.watcher.as_ref()?.get(pane.uid)?;
                (reading.badge != AgentBadge::None).then(|| ExposedAgent {
                    pane: pane.clone(),
                    reading,
                })
            })
            .collect()
    }

    /// A pane still published, whatever its agent; `None` once the UI dropped it.
    pub fn pane(&self, id: u64) -> Option<ExposedPane> {
        self.lock()
            .panes
            .iter()
            .find(|pane| pane.uid.get() == id)
            .cloned()
    }

    /// The phone types only while an agent is in the pane's foreground.
    pub fn is_writable(&self, id: u64) -> bool {
        self.agents()
            .iter()
            .any(|agent| agent.pane.uid.get() == id && agent.reading.agent.is_some())
    }

    /// Seeing an agent on the phone acknowledges its green (specs/agents.md §1).
    pub fn phone_sees(&self, uid: PaneUid) {
        if let Some(watcher) = &self.lock().watcher {
            watcher.phone_sees(uid);
        }
    }

    pub fn phone_leaves(&self, uid: PaneUid) {
        if let Some(watcher) = &self.lock().watcher {
            watcher.phone_leaves(uid);
        }
    }

    pub fn palette(&self) -> TermPalette {
        self.lock().palette
    }

    fn lock(&self) -> MutexGuard<'_, Published> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
