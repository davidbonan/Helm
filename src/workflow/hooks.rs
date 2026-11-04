//! The Claude Code side of a run (workflows.md §6): the per-run settings file
//! declaring helm's hooks, the launch command, and the hook input parsed back.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::run::RunEvent;
use crate::terminal::pty::shell_single_quote;

/// Set on the Claude process, inherited by its hooks: which run an event is for.
pub const RUN_ENV: &str = "HELM_WORKFLOW_RUN";

pub const SUBCOMMAND: &str = "workflow-event";

/// Which hook invoked `helm workflow-event`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    Stop,
    Notification,
    SessionStart,
}

impl HookKind {
    pub fn arg(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Notification => "notification",
            Self::SessionStart => "session-start",
        }
    }

    pub fn from_arg(arg: &str) -> Option<Self> {
        [Self::Stop, Self::Notification, Self::SessionStart]
            .into_iter()
            .find(|kind| kind.arg() == arg)
    }
}

/// A hook event as it travels on the control socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "hook", rename_all = "snake_case")]
pub enum Hook {
    Stop {
        turn: String,
        last_message: Option<String>,
    },
    PermissionPrompt,
    /// `/compact` or `/clear` done: the session starts over.
    SessionReset,
}

impl Hook {
    pub fn into_event(self) -> RunEvent {
        match self {
            Self::Stop { turn, last_message } => RunEvent::TurnEnded { turn, last_message },
            Self::PermissionPrompt => RunEvent::PermissionPrompt,
            Self::SessionReset => RunEvent::SessionReset,
        }
    }
}

#[derive(Deserialize)]
struct HookInput {
    #[serde(default)]
    prompt_id: String,
    last_assistant_message: Option<String>,
    notification_type: Option<String>,
    source: Option<String>,
}

/// `None` for what the run does not track (a malformed input, a notification other
/// than a permission prompt, a session start other than a compact or a clear).
pub fn parse_hook_input(kind: HookKind, input: &str) -> Option<Hook> {
    let input: HookInput = serde_json::from_str(input).ok()?;
    match kind {
        HookKind::Stop => Some(Hook::Stop {
            turn: input.prompt_id,
            last_message: input.last_assistant_message,
        }),
        HookKind::Notification => (input.notification_type.as_deref() == Some("permission_prompt"))
            .then_some(Hook::PermissionPrompt),
        HookKind::SessionStart => matches!(input.source.as_deref(), Some("compact" | "clear"))
            .then_some(Hook::SessionReset),
    }
}

/// The `--settings` file: helm's own binary as the command of every hook, so no
/// `PATH` lookup and a dev build answers to itself.
pub fn settings_json(helm_exe: &Path) -> String {
    let command = |kind: HookKind| {
        let exe = shell_single_quote(&helm_exe.to_string_lossy());
        serde_json::json!([{ "hooks": [{
            "type": "command",
            "command": format!("{exe} {SUBCOMMAND} {}", kind.arg()),
        }]}])
    };
    serde_json::json!({ "hooks": {
        "Stop": command(HookKind::Stop),
        "Notification": command(HookKind::Notification),
        "SessionStart": command(HookKind::SessionStart),
    }})
    .to_string()
}

/// Run through the login shell; the first step travels as Claude's initial prompt.
pub fn launch_command(claude: &str, settings: &Path, prompt: Option<&str>) -> String {
    let mut command = format!(
        "{claude} --settings {}",
        shell_single_quote(&settings.to_string_lossy())
    );
    if let Some(prompt) = prompt {
        command.push(' ');
        command.push_str(&shell_single_quote(prompt));
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_input_carries_the_turn_and_the_last_message() {
        let input = r#"{"hook_event_name":"Stop","prompt_id":"p1",
            "last_assistant_message":"PONG\n\n[[STEP_DONE]]","stop_hook_active":false}"#;
        assert_eq!(
            parse_hook_input(HookKind::Stop, input),
            Some(Hook::Stop {
                turn: "p1".to_owned(),
                last_message: Some("PONG\n\n[[STEP_DONE]]".to_owned()),
            })
        );
    }

    #[test]
    fn only_a_permission_notification_is_tracked() {
        let permission = r#"{"notification_type":"permission_prompt","message":"…"}"#;
        let idle = r#"{"notification_type":"idle_prompt"}"#;
        assert_eq!(
            parse_hook_input(HookKind::Notification, permission),
            Some(Hook::PermissionPrompt)
        );
        assert_eq!(parse_hook_input(HookKind::Notification, idle), None);
        assert_eq!(parse_hook_input(HookKind::Stop, "not json"), None);
    }

    #[test]
    fn only_a_compact_or_clear_session_start_is_tracked() {
        let start = |source: &str| {
            parse_hook_input(
                HookKind::SessionStart,
                &format!(r#"{{"hook_event_name":"SessionStart","source":"{source}"}}"#),
            )
        };
        assert_eq!(start("compact"), Some(Hook::SessionReset));
        assert_eq!(start("clear"), Some(Hook::SessionReset));
        assert_eq!(start("startup"), None);
        assert_eq!(start("resume"), None);
    }

    #[test]
    fn settings_declare_every_hook_on_the_quoted_helm_binary() {
        let json: serde_json::Value =
            serde_json::from_str(&settings_json(Path::new("/Apps/my helm/helm"))).unwrap();
        let command = |event: &str| json["hooks"][event][0]["hooks"][0]["command"].clone();
        assert_eq!(command("Stop"), "'/Apps/my helm/helm' workflow-event stop");
        assert_eq!(
            command("Notification"),
            "'/Apps/my helm/helm' workflow-event notification"
        );
        assert_eq!(
            command("SessionStart"),
            "'/Apps/my helm/helm' workflow-event session-start"
        );
    }

    #[test]
    fn the_launch_command_quotes_the_prompt_and_omits_it_on_a_gate() {
        let settings = Path::new("/s/run 1.json");
        assert_eq!(
            launch_command("claude", settings, Some("Fix it's bug")),
            r"claude --settings '/s/run 1.json' 'Fix it'\''s bug'"
        );
        assert_eq!(
            launch_command("claude", settings, None),
            "claude --settings '/s/run 1.json'"
        );
    }
}
