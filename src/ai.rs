use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::git::cli::{self, CliError};

/// AI CLI that writes the commit message, chosen in the preferences; invoked as
/// `<command> -p <prompt>` in the active repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProvider {
    #[default]
    Claude,
    Codex,
    Opencode,
}

impl AiProvider {
    pub const ALL: [AiProvider; 3] = [AiProvider::Claude, AiProvider::Codex, AiProvider::Opencode];

    pub fn command(self) -> &'static str {
        match self {
            AiProvider::Claude => "claude",
            AiProvider::Codex => "codex",
            AiProvider::Opencode => "opencode",
        }
    }

    /// Preferences dropdown label: the product name, identical for the
    /// commit-message and the AI-rebase provider. The invocation flavor (`-p`
    /// text vs agentic) differs internally but is not user-facing — each setting's
    /// description states which action it drives.
    pub fn display_name(self) -> &'static str {
        match self {
            AiProvider::Claude => "Claude Code",
            AiProvider::Codex => "Codex",
            AiProvider::Opencode => "opencode",
        }
    }

    /// Model-selection flags prepended to the commit-message invocation:
    /// summarizing a staged diff is cheap, so a small/fast model is plenty. For
    /// Claude that is Haiku; the others keep their own default (empty).
    pub fn commit_model_args(self) -> &'static [&'static str] {
        match self {
            AiProvider::Claude => &["--model", "haiku"],
            AiProvider::Codex | AiProvider::Opencode => &[],
        }
    }
}

/// Message proposed by the AI, ready to fill the commit card's inputs — never
/// committed automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSuggestion {
    pub subject: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// Provider binary missing from PATH.
    NotFound(String),
    /// Process failed (non-zero code or I/O error).
    Failed(String),
    /// Empty or unusable output.
    EmptyReply,
    /// Nothing staged to describe.
    NoChanges,
}

impl AiError {
    /// Toast message (git.md §10): the action spelled out + the useful detail.
    pub fn message(&self) -> String {
        match self {
            AiError::NotFound(command) => {
                format!("'{command}' not found — pick another AI provider in Preferences")
            }
            AiError::Failed(detail) => format!("Commit message generation failed — {detail}"),
            AiError::EmptyReply => {
                "Commit message generation failed — the provider returned nothing".to_owned()
            }
            AiError::NoChanges => "Nothing to describe — stage your changes first".to_owned(),
        }
    }
}

/// Bounds the diff embedded in the prompt: the prompt is passed in argv, whose
/// size is limited by the OS (~1 MB on macOS).
const MAX_DIFF_BYTES: usize = 80_000;
const TRUNCATION_MARKER: &str = "\n[diff truncated]";

pub fn generate(
    workdir: &Path,
    provider: AiProvider,
    instructions: &str,
) -> Result<CommitSuggestion, AiError> {
    generate_with(
        Path::new(provider.command()),
        provider.commit_model_args(),
        workdir,
        instructions,
    )
}

/// Seam: `generate` pins the program and model flags to the preferences
/// provider; the parameters let us exercise the full pipeline with a fake binary.
pub fn generate_with(
    program: &Path,
    model_args: &[&str],
    workdir: &Path,
    instructions: &str,
) -> Result<CommitSuggestion, AiError> {
    let context = change_context(workdir);
    if context.trim().is_empty() {
        return Err(AiError::NoChanges);
    }
    let prompt = build_prompt(instructions, &context);
    let mut args: Vec<&str> = Vec::with_capacity(model_args.len() + 2);
    args.extend_from_slice(model_args);
    args.push("-p");
    args.push(&prompt);
    let output = cli::run_program(program, workdir, &args).map_err(|err| match err {
        CliError::NotFound => AiError::NotFound(program.display().to_string()),
        CliError::TimedOut(duration) => {
            AiError::Failed(format!("timed out after {}s", duration.as_secs()))
        }
        CliError::Io(err) => AiError::Failed(err.to_string()),
    })?;
    if !output.success() {
        return Err(AiError::Failed(failure_detail(&output)));
    }
    parse_suggestion(&output.stdout).ok_or(AiError::EmptyReply)
}

pub(crate) fn failure_detail(output: &cli::CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_owned();
    }
    match output.code {
        Some(code) => format!("exit code {code}"),
        None => "killed by a signal".to_owned(),
    }
}

/// Changes to describe: **staged only** — list of the index's files (covers
/// additions even when the diff is truncated) + the index diff. The working tree
/// never enters the prompt; nothing staged ⇒ empty context (`NoChanges`
/// upstream). git failure ⇒ empty section, never an error: the provider stays
/// useful with partial context.
fn change_context(workdir: &Path) -> String {
    let files = git_stdout(workdir, &["diff", "--cached", "--name-status"]);
    let diff = git_stdout(workdir, &["diff", "--cached"]);

    let mut sections = Vec::new();
    if !files.trim().is_empty() {
        sections.push(format!("Staged files:\n{files}"));
    }
    if !diff.trim().is_empty() {
        sections.push(format!(
            "Staged diff:\n{}",
            truncate_diff(&diff, MAX_DIFF_BYTES)
        ));
    }
    sections.join("\n")
}

fn git_stdout(workdir: &Path, args: &[&str]) -> String {
    cli::run(workdir, args)
        .ok()
        .filter(cli::CliOutput::success)
        .map(|output| output.stdout)
        .unwrap_or_default()
}

/// Truncates on a character boundary, with an explicit marker — never a silent
/// truncation.
pub fn truncate_diff(diff: &str, max_bytes: usize) -> String {
    if diff.len() <= max_bytes {
        return diff.to_owned();
    }
    let mut end = max_bytes;
    while !diff.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATION_MARKER}", &diff[..end])
}

pub fn build_prompt(instructions: &str, context: &str) -> String {
    let mut prompt = String::from(
        "Write a git commit message for the changes below.\n\
         Reply with the raw message only — no markdown, no code fences, no commentary.\n\
         First line: a short imperative subject (at most 72 characters).\n\
         Optionally, after a blank line: a concise body explaining the why and the what.\n\
         Follow the project's own commit conventions, inspecting the repository to \
         find them: first any documented ones (CONTRIBUTING, docs, *.md, contributor \
         or agent guidelines); if it documents none, the conventions of its recent \
         commit history. Match the subject prefixes or scopes, tense, capitalization, \
         and length you find there over the generic guidance above.",
    );
    let instructions = instructions.trim();
    if !instructions.is_empty() {
        prompt.push_str("\n\nAdditional instructions:\n");
        prompt.push_str(instructions);
    }
    prompt.push_str("\n\n");
    prompt.push_str(context);
    prompt
}

/// Splits the reply into (subject, body): first non-empty line ⇒ subject, the
/// rest ⇒ description. Markdown fences are tolerated despite the instruction.
pub fn parse_suggestion(output: &str) -> Option<CommitSuggestion> {
    let text = strip_fences(output);
    let mut lines = text.lines();
    let subject = lines
        .by_ref()
        .find(|line| !line.trim().is_empty())?
        .trim()
        .to_owned();
    let description = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
    Some(CommitSuggestion {
        subject,
        description,
    })
}

fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    // The opening line may carry a language: skip it entirely.
    let body = rest.split_once('\n').map_or("", |(_, body)| body);
    let body = body.trim_end();
    body.strip_suffix("```").unwrap_or(body).trim()
}

/// Runs generation on a **dedicated thread per request**: the UI thread and the
/// git worker are never blocked by the provider (several seconds). **One request
/// at a time**: `request` is ignored until the previous one has been drained
/// (`busy` ⇒ spinner on the button). The thread is not joined: abandoning the
/// session lets the subprocess finish, its reply discarded.
pub struct AiRunner {
    repo_path: PathBuf,
    on_event: Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<Result<CommitSuggestion, AiError>>,
    results_rx: Receiver<Result<CommitSuggestion, AiError>>,
    in_flight: bool,
}

impl AiRunner {
    pub fn new(repo_path: &Path, on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            repo_path: repo_path.to_path_buf(),
            on_event: Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: false,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight
    }

    /// Starts generation; returns `false` (request ignored) if one is in progress.
    pub fn request(&mut self, provider: AiProvider, instructions: String) -> bool {
        self.request_program(
            PathBuf::from(provider.command()),
            provider.commit_model_args(),
            instructions,
        )
    }

    /// Seam: same execution with an explicit program and model flags (fake binary
    /// in tests).
    pub fn request_program(
        &mut self,
        program: PathBuf,
        model_args: &'static [&'static str],
        instructions: String,
    ) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        let path = self.repo_path.clone();
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = generate_with(&program, model_args, &path, &instructions);
            let _ = tx.send(result);
            on_event();
        });
        true
    }

    pub fn try_recv(&mut self) -> Option<Result<CommitSuggestion, AiError>> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.in_flight = false;
        }
        reply
    }

    pub fn recv(&mut self) -> Option<Result<CommitSuggestion, AiError>> {
        let reply = self.results_rx.recv().ok();
        if reply.is_some() {
            self.in_flight = false;
        }
        reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_map_to_their_cli_command() {
        assert_eq!(AiProvider::Claude.command(), "claude");
        assert_eq!(AiProvider::Codex.command(), "codex");
        assert_eq!(AiProvider::Opencode.command(), "opencode");
        assert_eq!(AiProvider::default(), AiProvider::Claude);
        let names: Vec<&str> = AiProvider::ALL.iter().map(|p| p.display_name()).collect();
        assert_eq!(names, ["Claude Code", "Codex", "opencode"]);
    }

    #[test]
    fn claude_pins_the_small_haiku_model_others_keep_their_default() {
        assert_eq!(AiProvider::Claude.commit_model_args(), ["--model", "haiku"]);
        assert!(AiProvider::Codex.commit_model_args().is_empty());
        assert!(AiProvider::Opencode.commit_model_args().is_empty());
    }

    #[test]
    fn the_prompt_embeds_the_context_and_the_format_contract() {
        let prompt = build_prompt("", "Staged diff:\n+fn main() {}");
        assert!(prompt.contains("imperative subject"));
        assert!(prompt.contains("project's own commit conventions"));
        assert!(prompt.contains("Staged diff:\n+fn main() {}"));
        assert!(
            !prompt.contains("Additional instructions"),
            "no instructions block when the preference is blank"
        );
    }

    #[test]
    fn the_prompt_appends_the_user_instructions() {
        let prompt = build_prompt("  Always write in French.  ", "diff");
        assert!(prompt.contains("Additional instructions:\nAlways write in French."));
    }

    #[test]
    fn parse_splits_subject_and_description() {
        let suggestion = parse_suggestion("Add login form\n\nWire the auth flow.\nSecond line.");
        assert_eq!(
            suggestion,
            Some(CommitSuggestion {
                subject: "Add login form".to_owned(),
                description: "Wire the auth flow.\nSecond line.".to_owned(),
            })
        );
    }

    #[test]
    fn parse_accepts_a_subject_only_reply_with_leading_noise() {
        let suggestion = parse_suggestion("\n\n  Fix typo in README  \n").unwrap();
        assert_eq!(suggestion.subject, "Fix typo in README");
        assert_eq!(suggestion.description, "");
    }

    #[test]
    fn parse_strips_markdown_fences_despite_the_contract() {
        let suggestion = parse_suggestion("```text\nAdd parser\n\nBody here.\n```").unwrap();
        assert_eq!(suggestion.subject, "Add parser");
        assert_eq!(suggestion.description, "Body here.");
    }

    #[test]
    fn parse_rejects_an_empty_reply() {
        assert_eq!(parse_suggestion(""), None);
        assert_eq!(parse_suggestion("   \n  \n"), None);
        assert_eq!(parse_suggestion("```\n\n```"), None);
    }

    #[test]
    fn truncate_keeps_short_diffs_and_marks_long_ones() {
        assert_eq!(truncate_diff("short", 100), "short");
        let truncated = truncate_diff(&"x".repeat(200), 100);
        assert!(truncated.starts_with(&"x".repeat(100)));
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        // Multi-byte boundary: does not panic in the middle of a character.
        let truncated = truncate_diff(&"é".repeat(100), 99);
        assert!(truncated.ends_with(TRUNCATION_MARKER));
    }

    #[test]
    fn provider_serializes_in_kebab_case() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            provider: AiProvider,
        }
        for (provider, expected) in [
            (AiProvider::Claude, "provider = \"claude\""),
            (AiProvider::Codex, "provider = \"codex\""),
            (AiProvider::Opencode, "provider = \"opencode\""),
        ] {
            let text = toml::to_string(&Wrap { provider }).unwrap();
            assert!(text.contains(expected), "unexpected format:\n{text}");
            let back: Wrap = toml::from_str(&text).unwrap();
            assert_eq!(back.provider, provider);
        }
    }
}
