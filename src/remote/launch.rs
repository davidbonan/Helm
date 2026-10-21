//! Launching an agent from the phone (specs/remote.md §4, §7.2): the phone names
//! a workspace entry and an agent of the Mac's own list, never a path nor a
//! command line. The server thread spawns the pane — helm may draw no frame — and
//! hands it to the UI, which adopts it as a tab.

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};

use crate::agent_watch::watcher::{PaneOrigin, WatchedPane};
use crate::agents::Agent;
use crate::remote::protocol::path_id;
use crate::remote::registry::{ExposedPane, Registry};
use crate::terminal::pane::{Pane, TypedCommand};
use crate::terminal::sizing::GridSize;

/// A workspace entry the phone may launch into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    pub id: u64,
    pub path: PathBuf,
    pub project: String,
    pub branch: Option<String>,
    pub worktree: bool,
}

impl LaunchTarget {
    pub fn new(path: PathBuf, project: String, branch: Option<String>, worktree: bool) -> Self {
        Self {
            id: path_id(&path),
            path,
            project,
            branch,
            worktree,
        }
    }
}

/// What the phone may pick from, as the UI last published it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchTargets {
    pub entries: Vec<LaunchTarget>,
    /// Offered agents only; the phone picks one by its index here.
    pub agents: Vec<Agent>,
}

/// A pane launched off the UI thread, on its way to become a tab of `entry`.
pub struct LaunchedPane {
    pub entry: PathBuf,
    pub pane: Pane,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// The server side of a launch: spawns the pane, exposes it at once, sends it to
/// the UI and wakes it.
#[derive(Clone)]
pub struct Launcher {
    adopt: Sender<LaunchedPane>,
    wake: Wake,
}

impl Launcher {
    /// The launcher and the UI's end: `wake` also paces the pane's redraws once
    /// the UI shows it.
    pub fn channel(wake: impl Fn() + Send + Sync + 'static) -> (Self, Receiver<LaunchedPane>) {
        let (adopt, launches) = crossbeam_channel::unbounded();
        let launcher = Self {
            adopt,
            wake: Arc::new(wake),
        };
        (launcher, launches)
    }

    /// The new pane's id, or what the phone should read.
    pub fn launch(
        &self,
        registry: &Registry,
        entry: u64,
        agent: usize,
        size: GridSize,
    ) -> Result<u64, String> {
        let (target, agent) = registry
            .launch_choice(entry, agent)
            .ok_or_else(|| "That project or agent is no longer offered".to_owned())?;
        let wake = Arc::clone(&self.wake);
        let typed = TypedCommand {
            cwd: &target.path,
            line: &agent.command,
            prompt: None,
        };
        let pane = Pane::typing(&typed, size.phone(), move || wake())
            .map_err(|err| format!("Could not start {} — {err}", agent.name))?;
        let uid = pane.uid();
        let origin = PaneOrigin {
            repo: target.project.clone(),
            branch: target.branch.clone(),
        };
        registry.add_launched(
            ExposedPane {
                uid,
                project: target.project,
                branch: target.branch,
                tab: agent.name,
                worktree: target.path.clone(),
                handle: pane.handle(),
            },
            WatchedPane::of(&pane, origin),
        );
        let launched = LaunchedPane {
            entry: target.path,
            pane,
        };
        if self.adopt.send(launched).is_err() {
            registry.forget(uid);
            return Err("helm is closing".to_owned());
        }
        (self.wake)();
        Ok(uid.get())
    }
}
