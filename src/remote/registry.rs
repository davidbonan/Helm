//! What the phone may see (specs/remote.md §1, §4): the live panes the UI
//! publishes at its poll, filtered by the agent watcher's live readings — only a
//! pane with an agent is exposed, so the phone never reaches a plain shell. Plus
//! what it may launch (§7.2), and the panes it launched that the UI has not
//! adopted yet. And the projects, which the phone may hide like the sidebar's eye
//! (§7.4): an agent or entry of a hidden project is neither listed nor launchable.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent_watch::watcher::{PaneReading, WatchedPane, WatcherLink};
use crate::agent_watch::AgentBadge;
use crate::agents::Agent;
use crate::remote::launch::{LaunchTarget, LaunchTargets};
use crate::remote::protocol::{path_id, PageTheme};
use crate::terminal::palette::TermPalette;
use crate::terminal::pane::{PaneHandle, PaneUid};
use crate::theme::{self, ThemePreset};

#[derive(Clone)]
pub struct ExposedPane {
    pub uid: PaneUid,
    pub project: String,
    pub branch: Option<String>,
    pub tab: String,
    /// The workspace entry's directory: what the phone may browse (§7.3).
    pub worktree: PathBuf,
    pub handle: PaneHandle,
}

/// A workspace project as the UI publishes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneProject {
    pub root: PathBuf,
    pub name: String,
    pub hidden: bool,
    /// The root and its worktrees: where the project's panes and launch targets live.
    pub entries: Vec<PathBuf>,
}

/// The phone hid or showed a project; the UI applies it to its workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibilityRequest {
    pub root: PathBuf,
    pub hidden: bool,
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
    projects: Vec<PhoneProject>,
    /// Applied here at once — helm may draw no frame — until the UI takes them.
    visibility_requests: Vec<VisibilityRequest>,
}

impl Published {
    fn all_panes(&self) -> impl Iterator<Item = &ExposedPane> {
        self.panes.iter().chain(&self.launched)
    }

    fn is_in_hidden_project(&self, entry: &Path) -> bool {
        self.projects
            .iter()
            .any(|project| project.hidden && project.entries.iter().any(|e| e == entry))
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
            projects: Vec::new(),
            visibility_requests: Vec::new(),
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
        let published = self.lock();
        LaunchTargets {
            entries: published
                .targets
                .entries
                .iter()
                .filter(|target| !published.is_in_hidden_project(&target.path))
                .cloned()
                .collect(),
            agents: published.targets.agents.clone(),
        }
    }

    pub fn launch_choice(&self, entry: u64, agent: usize) -> Option<(LaunchTarget, Agent)> {
        let published = self.lock();
        let target = published
            .targets
            .entries
            .iter()
            .find(|t| t.id == entry && !published.is_in_hidden_project(&t.path))?;
        let agent = published.targets.agents.get(agent)?;
        Some((target.clone(), agent.clone()))
    }

    /// The phone's requests the UI has not taken yet stay applied on top.
    pub fn publish_projects(&self, mut projects: Vec<PhoneProject>) {
        let mut published = self.lock();
        for request in &published.visibility_requests {
            if let Some(project) = projects.iter_mut().find(|p| p.root == request.root) {
                project.hidden = request.hidden;
            }
        }
        published.projects = projects;
    }

    pub fn projects(&self) -> Vec<PhoneProject> {
        self.lock().projects.clone()
    }

    pub fn set_project_hidden(&self, id: u64, hidden: bool) {
        let mut published = self.lock();
        let Some(project) = published
            .projects
            .iter_mut()
            .find(|project| path_id(&project.root) == id)
        else {
            return;
        };
        project.hidden = hidden;
        let root = project.root.clone();
        published
            .visibility_requests
            .retain(|request| request.root != root);
        published
            .visibility_requests
            .push(VisibilityRequest { root, hidden });
    }

    pub fn take_visibility_requests(&self) -> Vec<VisibilityRequest> {
        std::mem::take(&mut self.lock().visibility_requests)
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
            .filter(|pane| !published.is_in_hidden_project(&pane.worktree))
            .filter_map(|pane| {
                let reading = published.watcher.as_ref()?.get(pane.uid)?;
                (reading.badge != AgentBadge::None).then(|| ExposedAgent {
                    pane: pane.clone(),
                    reading,
                })
            })
            .collect()
    }

    /// The worktree whose files the phone may read: an exposed agent's only.
    pub fn agent_worktree(&self, id: u64) -> Option<PathBuf> {
        self.agents()
            .into_iter()
            .find(|agent| agent.pane.uid.get() == id)
            .map(|agent| agent.pane.worktree)
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
            worktree: PathBuf::from("/tmp/api"),
            handle: pane.handle(),
        }
    }

    fn project(root: &str, hidden: bool) -> PhoneProject {
        PhoneProject {
            root: PathBuf::from(root),
            name: "api".to_owned(),
            hidden,
            entries: vec![PathBuf::from(root)],
        }
    }

    fn is_hidden(registry: &Registry, root: &str) -> bool {
        registry
            .projects()
            .iter()
            .any(|p| p.root == Path::new(root) && p.hidden)
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

    #[test]
    fn a_project_the_phone_hides_offers_no_launch_target() {
        let registry = Registry::default();
        let target = LaunchTarget::new(PathBuf::from("/tmp/api"), "api".to_owned(), None, false);
        let id = target.id;
        registry.publish_targets(LaunchTargets {
            entries: vec![target],
            agents: vec![Agent::new("Claude Code", "claude")],
        });
        registry.publish_projects(vec![project("/tmp/api", false)]);

        registry.set_project_hidden(path_id(Path::new("/tmp/api")), true);

        assert!(registry.targets().entries.is_empty());
        assert!(registry.launch_choice(id, 0).is_none());
    }

    #[test]
    fn a_phone_hide_outlives_ui_publishes_until_the_ui_takes_it() {
        let registry = Registry::default();
        registry.publish_projects(vec![project("/tmp/api", false)]);

        registry.set_project_hidden(path_id(Path::new("/tmp/api")), true);
        registry.publish_projects(vec![project("/tmp/api", false)]);
        let before_taken = is_hidden(&registry, "/tmp/api");
        let taken = registry.take_visibility_requests();
        registry.publish_projects(vec![project("/tmp/api", false)]);

        assert!(before_taken, "a stale publish does not undo the phone");
        assert_eq!(
            taken,
            vec![VisibilityRequest {
                root: PathBuf::from("/tmp/api"),
                hidden: true
            }]
        );
        assert!(!is_hidden(&registry, "/tmp/api"), "the UI owns it again");
    }
}
