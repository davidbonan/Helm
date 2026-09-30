//! Agents the phone can launch (specs/remote.md §7.2): the Mac's own list, set in
//! Preferences — the phone names one, never a command line.

use serde::{Deserialize, Serialize};

use crate::agent_watch;

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
