//! What the phone may see (specs/remote.md §1, §4): the live panes the UI
//! publishes at its poll, filtered by the agent watcher's live readings — only a
//! pane with an agent is exposed, so the phone never reaches a plain shell. Plus
//! what it may launch (§7.2), and the panes it launched that the UI has not
//! adopted yet.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent_watch::watcher::{PaneReading, WatchedPane, WatcherLink};
use crate::agent_watch::AgentBadge;
use crate::agents::Agent;
use crate::remote::launch::{LaunchTarget, LaunchTargets};
use crate::remote::protocol::PageTheme;
use crate::terminal::palette::TermPalette;
use crate::terminal::pane::{PaneHandle, PaneUid};
use crate::theme::{self, ThemePreset};

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
    /// Launched from the phone, kept until a `publish` lists them.
    launched: Vec<ExposedPane>,
    watcher: Option<WatcherLink>,
    theme: &'static ThemePreset,
    targets: LaunchTargets,
}

impl Published {
    fn all_panes(&self) -> impl Iterator<Item = &ExposedPane> {
        self.panes.iter().chain(&self.launched)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Published {
            panes: Vec::new(),
            launched: Vec::new(),
            watcher: None,
            theme: theme::preset("helm", true),
            targets: LaunchTargets::default(),
        })))
    }
}

impl Registry {
    pub fn publish(
        &self,
        panes: Vec<ExposedPane>,
        watcher: Option<WatcherLink>,
        theme: &'static ThemePreset,
    ) {
        let mut published = self.lock();
        published
            .launched
            .retain(|launched| !panes.iter().any(|pane| pane.uid == launched.uid));
        published.panes = panes;
        published.watcher = watcher;
        published.theme = theme;
    }

    pub fn publish_targets(&self, targets: LaunchTargets) {
        self.lock().targets = targets;
    }

    pub fn targets(&self) -> LaunchTargets {
        self.lock().targets.clone()
    }

    pub fn launch_choice(&self, entry: u64, agent: usize) -> Option<(LaunchTarget, Agent)> {
        let published = self.lock();
        let target = published.targets.entries.iter().find(|t| t.id == entry)?;
        let agent = published.targets.agents.get(agent)?;
        Some((target.clone(), agent.clone()))
    }

    /// Exposed and watched at once, before the UI adopts it.
    pub fn add_launched(&self, pane: ExposedPane, watched: WatchedPane) {
        let mut published = self.lock();
        if let Some(watcher) = &published.watcher {
            watcher.launched(watched);
        }
        published.launched.push(pane);
    }

    /// A launched pane the UI dropped unadopted (its entry went away).
    pub fn forget(&self, uid: PaneUid) {
        let mut published = self.lock();
        published.launched.retain(|pane| pane.uid != uid);
        if let Some(watcher) = &published.watcher {
            watcher.forget(uid);
        }
    }

    pub fn agents(&self) -> Vec<ExposedAgent> {
        let published = self.lock();
        published
            .all_panes()
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
            .all_panes()
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
        self.lock().theme.term
    }

    pub fn page_theme(&self) -> PageTheme {
        PageTheme::of(&self.lock().theme.palette)
    }

    fn lock(&self) -> MutexGuard<'_, Published> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_watch::watcher::WatchedPane;
    use crate::terminal::pane::Pane;

    fn cat() -> Pane {
        Pane::from_command(portable_pty::CommandBuilder::new("cat"), 24, 80, || {}).unwrap()
    }

    fn exposed(pane: &Pane, tab: &str) -> ExposedPane {
        ExposedPane {
            uid: pane.uid(),
            project: "api".to_owned(),
            branch: None,
            tab: tab.to_owned(),
            handle: pane.handle(),
        }
    }

    fn tab_of(registry: &Registry, pane: &Pane) -> Option<String> {
        registry.pane(pane.uid().get()).map(|exposed| exposed.tab)
    }

    #[test]
    fn a_launched_pane_stays_exposed_until_a_publish_lists_it() {
        let registry = Registry::default();
        let pane = cat();
        let theme = theme::preset("helm", true);
        registry.add_launched(exposed(&pane, "Claude Code"), WatchedPane::of(&pane));

        registry.publish(Vec::new(), None, theme);
        let before_adoption = tab_of(&registry, &pane);
        registry.publish(vec![exposed(&pane, "claude")], None, theme);
        let adopted = tab_of(&registry, &pane);
        registry.publish(Vec::new(), None, theme);

        assert_eq!(before_adoption.as_deref(), Some("Claude Code"));
        assert_eq!(adopted.as_deref(), Some("claude"));
        assert_eq!(tab_of(&registry, &pane), None, "no longer held as launched");
    }

    #[test]
    fn a_forgotten_launched_pane_is_no_longer_exposed() {
        let registry = Registry::default();
        let pane = cat();
        registry.add_launched(exposed(&pane, "Claude Code"), WatchedPane::of(&pane));

        registry.forget(pane.uid());

        assert_eq!(tab_of(&registry, &pane), None);
    }
}
