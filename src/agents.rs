//! Agent settings (specs/preferences.md §4 *Agents*): the agents table, source of
//! every command helm runs for an AI action, and the prompt of each action.
//! Nothing here is imposed — every command and prompt is free text, pre-filled
//! from a [`Preset`].

use serde::{Deserialize, Serialize};

use crate::agent_watch;

/// Env var every agent command reads its prompt from: expanded by the shell as
/// one argument, so a prompt is never escaped nor echoed into the terminal.
pub const PROMPT_ENV: &str = "HELM_PROMPT";

/// Reply contract of the commit message, appended to the user's prompt: helm
/// splits the reply into subject + description, so it is not editable.
const COMMIT_REPLY_FORMAT: &str = "Reply with the raw message only — no markdown, no code \
     fences, no commentary.\n\
     First line: a short imperative subject (at most 72 characters).\n\
     Optionally, after a blank line: a concise body explaining the why and the what.";

const DEFAULT_COMMIT_PROMPT: &str = "Write a git commit message for the changes below.\n\
     Follow the project's own commit conventions, inspecting the repository to \
     find them: first any documented ones (CONTRIBUTING, docs, *.md, contributor \
     or agent guidelines); if it documents none, the conventions of its recent \
     commit history. Match the subject prefixes or scopes, tense, capitalization, \
     and length you find there.\n\n{changes}";

const DEFAULT_COMMENTS_PROMPT: &str =
    "Please address the following code review comments and explain what you did.\n{comments}";

const DEFAULT_PR_PROMPT: &str = "Review the changes on this branch ({source}), which is the pull \
     request \"{title}\" (#{number}) targeting {dest}. Read the diff against {dest}, then \
     summarize the key changes and flag any bugs, risks, or improvements.";

/// An agent of the table: one free shell line per way helm uses it. A blank line
/// turns that use off for this agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    /// Opens a session, typed into a login shell: the phone's **+** (remote.md §7.2).
    pub command: String,
    /// Opens a session on `$HELM_PROMPT`: the review (pull-requests.md §11).
    #[serde(default)]
    pub prompt_command: String,
    /// Non-interactive: answers `$HELM_PROMPT` once on stdout and exits, ideally on
    /// a fast model — the commit message (git.md §5).
    #[serde(default)]
    pub headless_command: String,
}

impl Agent {
    pub fn new(name: &str, command: &str) -> Self {
        Self {
            name: name.to_owned(),
            command: command.to_owned(),
            prompt_command: String::new(),
            headless_command: String::new(),
        }
    }

    pub fn defaults() -> Vec<Self> {
        Preset::ALL.into_iter().map(Preset::agent).collect()
    }

    pub fn program(&self) -> Option<&str> {
        program_of(&self.command)
    }

    fn is_named(&self) -> bool {
        !self.name.trim().is_empty()
    }

    pub fn is_offered_on_phone(&self) -> bool {
        self.is_named() && self.program().is_some()
    }

    pub fn can_review(&self) -> bool {
        self.is_named() && !self.prompt_command.trim().is_empty()
    }

    pub fn can_write_commit_messages(&self) -> bool {
        self.is_named() && !self.headless_command.trim().is_empty()
    }

    /// `false` ⇒ the watcher never badges it, so the phone never lists it.
    pub fn is_detected(&self) -> bool {
        self.program().is_some_and(agent_watch::is_watched_program)
    }

    /// A row of the pre-agents phone list, which only held `command`: the other
    /// two follow what helm ran then (`<command> "<prompt>"`, the preset's headless command).
    pub fn from_legacy(name: &str, command: &str) -> Self {
        let command = command.trim();
        let preset = program_of(command).and_then(Preset::of_program);
        match preset {
            Some(preset) if preset.program() == command => Self {
                name: name.to_owned(),
                ..preset.agent()
            },
            _ => Self {
                name: name.to_owned(),
                command: command.to_owned(),
                prompt_command: prompt_command_of(command),
                headless_command: preset
                    .map(|preset| preset.agent().headless_command)
                    .unwrap_or_default(),
            },
        }
    }
}

fn program_of(command: &str) -> Option<&str> {
    command
        .split_whitespace()
        .find(|word| !is_env_assignment(word))
}

fn is_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.starts_with(|first: char| first.is_ascii_digit())
        && !name.is_empty()
        && name
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || letter == '_')
}

fn prompt_command_of(invocation: &str) -> String {
    if invocation.is_empty() {
        return String::new();
    }
    format!("{invocation} \"${PROMPT_ENV}\"")
}

pub fn agent_named<'a>(agents: &'a [Agent], name: &str) -> Option<&'a Agent> {
    agents.iter().find(|agent| agent.name == name)
}

/// Name of the agent of the table doing what `wanted` does (`same`), `wanted`
/// joining the table under a free name when none does.
fn adopt(agents: &mut Vec<Agent>, wanted: Agent, same: impl Fn(&Agent, &Agent) -> bool) -> String {
    if let Some(existing) = agents.iter().find(|agent| same(agent, &wanted)) {
        return existing.name.clone();
    }
    let name = (1..)
        .map(|rank| match rank {
            1 => wanted.name.clone(),
            _ => format!("{} {rank}", wanted.name),
        })
        .find(|name| agent_named(agents, name).is_none())
        .unwrap_or_default();
    agents.push(Agent {
        name: name.clone(),
        ..wanted
    });
    name
}

/// A known agent CLI: only a source of text for the table, never a constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    ClaudeCode,
    Codex,
    Opencode,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::ClaudeCode, Preset::Codex, Preset::Opencode];

    pub fn name(self) -> &'static str {
        match self {
            Preset::ClaudeCode => "Claude Code",
            Preset::Codex => "Codex",
            Preset::Opencode => "opencode",
        }
    }

    pub fn program(self) -> &'static str {
        match self {
            Preset::ClaudeCode => "claude",
            Preset::Codex => "codex",
            Preset::Opencode => "opencode",
        }
    }

    pub fn of_program(program: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.program() == program)
    }

    /// Claude writes commit messages with its small/fast model: summarizing a
    /// staged diff is cheap.
    pub fn agent(self) -> Agent {
        let (prompt_command, headless_command) = match self {
            Preset::ClaudeCode => (
                format!("claude \"${PROMPT_ENV}\""),
                format!("claude --model haiku -p \"${PROMPT_ENV}\""),
            ),
            Preset::Codex => (
                format!("codex \"${PROMPT_ENV}\""),
                format!("codex exec \"${PROMPT_ENV}\""),
            ),
            Preset::Opencode => (
                format!("opencode --prompt \"${PROMPT_ENV}\""),
                format!("opencode run \"${PROMPT_ENV}\""),
            ),
        };
        Agent {
            name: self.name().to_owned(),
            command: self.program().to_owned(),
            prompt_command,
            headless_command,
        }
    }
}

/// The commit card's "Generate commit message" (git.md §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitMessageSettings {
    /// Name of the agent of the table whose headless command writes the message.
    pub agent: String,
    /// `{changes}` ⇒ the staged file list and diff.
    pub prompt: String,
}

impl Default for CommitMessageSettings {
    fn default() -> Self {
        Self {
            agent: Preset::ClaudeCode.name().to_owned(),
            prompt: DEFAULT_COMMIT_PROMPT.to_owned(),
        }
    }
}

impl CommitMessageSettings {
    /// `None` when the chosen agent left the table or has no headless command.
    pub fn request_in(&self, agents: &[Agent]) -> Option<CommitMessageRequest> {
        agent_named(agents, &self.agent)
            .filter(|agent| agent.can_write_commit_messages())
            .map(|agent| self.request_running(&agent.headless_command))
    }

    pub fn request_running(&self, command: &str) -> CommitMessageRequest {
        CommitMessageRequest {
            command: command.to_owned(),
            prompt: self.prompt.clone(),
        }
    }

    /// The settings a pre-agents prefs file described: its provider, adopted into
    /// `agents`, and its instructions inlined ahead of the changes.
    pub fn from_legacy(
        provider: Option<&str>,
        instructions: &str,
        agents: &mut Vec<Agent>,
    ) -> Self {
        let preset = provider
            .and_then(Preset::of_program)
            .unwrap_or(Preset::ClaudeCode);
        let agent = adopt(agents, preset.agent(), |agent, wanted| {
            agent.headless_command == wanted.headless_command
        });
        let instructions = instructions.trim();
        let prompt = if instructions.is_empty() {
            DEFAULT_COMMIT_PROMPT.to_owned()
        } else {
            DEFAULT_COMMIT_PROMPT.replace(
                "{changes}",
                &format!("Additional instructions:\n{instructions}\n\n{{changes}}"),
            )
        };
        Self { agent, prompt }
    }
}

/// One commit message generation: the agent's headless command and the prompt
/// it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMessageRequest {
    /// Shell line run in the repo; its stdout is the message.
    pub command: String,
    pub prompt: String,
}

impl CommitMessageRequest {
    pub fn prompt_for(&self, changes: &str) -> String {
        let prompt = fill(&self.prompt, &[("changes", changes)]);
        format!("{}\n\n{COMMIT_REPLY_FORMAT}", prompt.trim_end())
    }
}

/// The agent the diff's review notes and the PR review surface hand work to
/// (pull-requests.md §11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewSettings {
    /// Name of the agent of the table; it names the buttons: "Send to {agent}",
    /// "Ask {agent}".
    pub agent: String,
    /// `{comments}` ⇒ the user's line notes or a posted thread, grouped by file.
    pub comments_prompt: String,
    /// `{source}` `{dest}` `{title}` `{number}` ⇒ the pull request.
    pub pr_prompt: String,
}

impl Default for ReviewSettings {
    fn default() -> Self {
        Self {
            agent: Preset::ClaudeCode.name().to_owned(),
            comments_prompt: DEFAULT_COMMENTS_PROMPT.to_owned(),
            pr_prompt: DEFAULT_PR_PROMPT.to_owned(),
        }
    }
}

impl ReviewSettings {
    /// The settings a pre-agents prefs file described: `invocation`, typed with
    /// the prompt appended as its last argument, adopted into `agents`.
    pub fn from_legacy(invocation: &str, agents: &mut Vec<Agent>) -> Self {
        let Some(program) = program_of(invocation) else {
            return Self::from_legacy(Preset::ClaudeCode.program(), agents);
        };
        let name = match Preset::of_program(program) {
            Some(preset) => preset.name().to_owned(),
            None => agent_watch::display_name(program),
        };
        let agent = adopt(
            agents,
            Agent::from_legacy(&name, invocation),
            |agent, wanted| agent.prompt_command == wanted.prompt_command,
        );
        Self {
            agent,
            ..Self::default()
        }
    }

    /// What the buttons, the tab and the toasts call the agent: a blank choice
    /// still reads as a sentence.
    pub fn label(&self) -> &str {
        match self.agent.trim() {
            "" => "agent",
            name => name,
        }
    }

    /// `None` when the chosen agent left the table or has no prompt command.
    pub fn command_in<'a>(&self, agents: &'a [Agent]) -> Option<&'a str> {
        agent_named(agents, &self.agent)
            .filter(|agent| agent.can_review())
            .map(|agent| agent.prompt_command.as_str())
    }

    pub fn comments_prompt_for(&self, comments: &str) -> String {
        fill(&self.comments_prompt, &[("comments", comments)])
    }

    pub fn pr_prompt_for(&self, pr: &PullRequestRef<'_>) -> String {
        fill(
            &self.pr_prompt,
            &[
                ("source", pr.source),
                ("dest", pr.dest),
                ("title", pr.title),
                ("number", &pr.number.to_string()),
            ],
        )
    }
}

/// What a PR prompt can name.
pub struct PullRequestRef<'a> {
    pub source: &'a str,
    pub dest: &'a str,
    pub title: &'a str,
    pub number: u64,
}

/// Replaces each `{key}` of `template` by its value, in one pass: a value that
/// itself holds braces is never expanded again, and an unknown key stays as typed.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let key = &after[..close];
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (close, *value))
        });
        match value {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(prompt: &str) -> CommitMessageRequest {
        CommitMessageSettings {
            prompt: prompt.to_owned(),
            ..CommitMessageSettings::default()
        }
        .request_running("cc")
    }

    #[test]
    fn fill_replaces_known_keys_once_and_keeps_the_rest() {
        let filled = fill(
            "{a} then {b}, {unknown} and a lone { brace",
            &[("a", "{b}"), ("b", "two")],
        );
        assert_eq!(filled, "{b} then two, {unknown} and a lone { brace");
    }

    #[test]
    fn the_commit_prompt_embeds_the_changes_then_the_reply_format() {
        let prompt = request(DEFAULT_COMMIT_PROMPT).prompt_for("Staged diff:\n+fn main() {}");
        let changes = prompt.find("Staged diff:\n+fn main() {}").unwrap();
        let format = prompt.find("imperative subject").unwrap();
        assert!(prompt.contains("project's own commit conventions"));
        assert!(changes < format, "the reply format closes the prompt");
    }

    #[test]
    fn an_edited_commit_prompt_still_ends_with_the_reply_format() {
        let prompt = request("En français.\n{changes}").prompt_for("diff");
        assert!(prompt.starts_with("En français.\ndiff\n\nReply with the raw message only"));
    }

    #[test]
    fn the_commit_message_runs_the_headless_command_of_its_agent() {
        let agents = Agent::defaults();
        let settings = CommitMessageSettings {
            agent: "Codex".to_owned(),
            ..CommitMessageSettings::default()
        };
        let request = settings.request_in(&agents).unwrap();
        assert_eq!(request.command, "codex exec \"$HELM_PROMPT\"");

        let start_only = [Agent::new("Codex", "codex")];
        assert_eq!(settings.request_in(&start_only), None);
        assert_eq!(settings.request_in(&[]), None);
    }

    #[test]
    fn the_review_types_the_prompt_command_of_its_agent() {
        let review = ReviewSettings {
            agent: "opencode".to_owned(),
            ..ReviewSettings::default()
        };
        assert_eq!(
            review.command_in(&Agent::defaults()),
            Some("opencode --prompt \"$HELM_PROMPT\"")
        );
        assert_eq!(
            review.command_in(&[Agent::new("opencode", "opencode")]),
            None
        );
        assert_eq!(review.label(), "opencode");
    }

    #[test]
    fn a_review_without_an_agent_still_labels_the_buttons() {
        let unset = ReviewSettings {
            agent: "  ".to_owned(),
            ..ReviewSettings::default()
        };
        assert_eq!(unset.label(), "agent");
    }

    #[test]
    fn a_legacy_phone_row_gains_the_commands_helm_ran_for_it() {
        assert_eq!(
            Agent::from_legacy("Mine", "opencode"),
            Agent {
                name: "Mine".to_owned(),
                ..Preset::Opencode.agent()
            }
        );

        let flagged = Agent::from_legacy("Opus", "claude --model opus");
        assert_eq!(
            flagged.prompt_command,
            "claude --model opus \"$HELM_PROMPT\""
        );
        assert_eq!(
            flagged.headless_command,
            Preset::ClaudeCode.agent().headless_command
        );

        let alias = Agent::from_legacy("Alias", "cc");
        assert_eq!(alias.prompt_command, "cc \"$HELM_PROMPT\"");
        assert_eq!(alias.headless_command, "");
    }

    #[test]
    fn a_legacy_provider_reuses_its_agent_of_the_table_and_keeps_its_instructions() {
        let mut agents = Agent::defaults();
        let settings =
            CommitMessageSettings::from_legacy(Some("codex"), " Write in French. ", &mut agents);
        assert_eq!(settings.agent, "Codex");
        assert_eq!(agents, Agent::defaults());
        assert!(settings
            .prompt
            .contains("Additional instructions:\nWrite in French.\n\n{changes}"));
    }

    #[test]
    fn a_legacy_provider_missing_from_the_table_joins_it_under_a_free_name() {
        let mut agents = vec![Agent::new("Claude Code", "cc")];
        let settings = CommitMessageSettings::from_legacy(None, "", &mut agents);
        assert_eq!(settings.agent, "Claude Code 2");
        assert_eq!(
            settings.request_in(&agents).unwrap().command,
            "claude --model haiku -p \"$HELM_PROMPT\""
        );
        assert_eq!(settings.prompt, DEFAULT_COMMIT_PROMPT);
    }

    #[test]
    fn a_legacy_review_command_becomes_an_agent_of_the_table() {
        let mut agents = Agent::defaults();
        let known = ReviewSettings::from_legacy("claude", &mut agents);
        assert_eq!(known.agent, "Claude Code");
        assert_eq!(agents, Agent::defaults());

        let alias = ReviewSettings::from_legacy("cc --model opus", &mut agents);
        assert_eq!(alias.agent, "Cc");
        assert_eq!(
            alias.command_in(&agents),
            Some("cc --model opus \"$HELM_PROMPT\"")
        );

        let blank = ReviewSettings::from_legacy("  ", &mut agents);
        assert_eq!(blank.agent, "Claude Code");
    }

    #[test]
    fn the_review_prompts_fill_their_placeholders() {
        let settings = ReviewSettings::default();
        assert_eq!(
            settings.comments_prompt_for("\n## a.rs\n"),
            "Please address the following code review comments and explain what you did.\n\n## a.rs\n"
        );
        let pr = settings.pr_prompt_for(&PullRequestRef {
            source: "feat/x",
            dest: "main",
            title: "Add x",
            number: 12,
        });
        assert!(
            pr.contains("this branch (feat/x)") && pr.contains("\"Add x\" (#12) targeting main")
        );
    }

    #[test]
    fn an_agent_is_detected_by_the_invoked_name_of_its_program() {
        assert!(Agent::new("Claude", "claude --model opus").is_detected());
        assert!(Agent::new("Perso", "CLAUDE_CONFIG_DIR=~/.claude-perso claude").is_detected());
        assert!(Agent::new("Claude", "/opt/bin/claude-code").is_detected());
        assert!(!Agent::new("Cursor", "cursor-agent").is_detected());
        assert!(!Agent::new("Wrapped", "npx claude").is_detected());
    }

    #[test]
    fn an_agent_without_a_name_or_a_start_command_stays_off_the_phone() {
        assert!(Agent::new("Codex", "codex").is_offered_on_phone());
        assert!(!Agent::new("  ", "codex").is_offered_on_phone());
        assert!(!Agent::new("Codex", "   ").is_offered_on_phone());
    }
}
