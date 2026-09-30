//! Launching an agent from the phone (specs/remote.md §4, §7.2): the phone names
//! a workspace entry and an agent of the Mac's own list, never a path nor a
//! command line. The server thread spawns the pane — helm may draw no frame — and
//! hands it to the UI, which adopts it as a tab.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::agent_watch;
use crate::agent_watch::watcher::WatchedPane;
use crate::remote::registry::{ExposedPane, Registry};
use crate::terminal::pane::Pane;
use crate::terminal::pty::{login_shell_command, shell_program};
use crate::terminal::sizing::GridSize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchAgent {
    pub name: String,
    /// Typed into a login shell in the chosen entry's directory.
    pub command: String,
}

impl LaunchAgent {
    pub fn new(name: &str, command: &str) -> Self {
        Self {
            name: name.to_owned(),
            command: command.to_owned(),
        }
    }

    pub fn defaults() -> Vec<Self> {
        vec![
            Self::new("Claude Code", "claude"),
            Self::new("Codex", "codex"),
            Self::new("opencode", "opencode"),
        ]
    }

    pub fn program(&self) -> Option<&str> {
        self.command.split_whitespace().next()
    }

    /// A row with an empty name or command is kept in Preferences but never
    /// offered to the phone.
    pub fn is_offered(&self) -> bool {
        !self.name.trim().is_empty() && self.program().is_some()
    }

    /// `false` ⇒ the watcher never badges it, so the phone never lists it.
    pub fn is_detected(&self) -> bool {
        self.program().is_some_and(agent_watch::is_watched_program)
    }
}

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
    /// The id is derived from the path: stable across the UI's republishes.
    pub fn new(path: PathBuf, project: String, branch: Option<String>, worktree: bool) -> Self {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        Self {
            id: hasher.finish(),
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
    pub agents: Vec<LaunchAgent>,
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
        let pane = spawn_agent(&target.path, &agent.command, size.phone(), move || wake())
            .map_err(|err| format!("Could not start {} — {err}", agent.name))?;
        let uid = pane.uid();
        registry.add_launched(
            ExposedPane {
                uid,
                project: target.project,
                branch: target.branch,
                tab: agent.name,
                handle: pane.handle(),
            },
            WatchedPane::of(&pane),
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

/// A login shell in `cwd` into which `command` is typed, as the user would: the
/// shell's profile (PATH) applies, and quitting the agent leaves the shell.
fn spawn_agent(
    cwd: &Path,
    command: &str,
    size: GridSize,
    on_change: impl Fn() + Send + Sync + 'static,
) -> Result<Pane> {
    let pane = Pane::from_command(
        login_shell_command(shell_program(), cwd),
        size.rows,
        size.cols,
        on_change,
    )?;
    pane.feed(format!("{command}\n").as_bytes())?;
    Ok(pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_is_detected_by_the_invoked_name_of_its_first_word() {
        assert!(LaunchAgent::new("Claude", "claude --model opus").is_detected());
        assert!(LaunchAgent::new("Claude", "/opt/bin/claude-code").is_detected());
        assert!(!LaunchAgent::new("Cursor", "cursor-agent").is_detected());
        assert!(!LaunchAgent::new("Wrapped", "npx claude").is_detected());
    }

    #[test]
    fn a_row_missing_its_name_or_command_is_not_offered() {
        assert!(LaunchAgent::new("Codex", "codex").is_offered());
        assert!(!LaunchAgent::new("  ", "codex").is_offered());
        assert!(!LaunchAgent::new("Codex", "   ").is_offered());
    }
}
