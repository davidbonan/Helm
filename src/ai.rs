use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};

use crate::agents::{CommitMessageRequest, PROMPT_ENV};
use crate::git::cli::{self, CliError};
use crate::terminal::pty::shell_program;

/// Message proposed by the AI, ready to fill the commit card's inputs — never
/// committed automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSuggestion {
    pub subject: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
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
            AiError::Failed(detail) => format!("Commit message generation failed — {detail}"),
            AiError::EmptyReply => {
                "Commit message generation failed — the command printed nothing".to_owned()
            }
            AiError::NoChanges => "Nothing to describe — stage your changes first".to_owned(),
        }
    }
}

/// Bounds the diff embedded in the prompt: the prompt is passed in the
/// environment, whose size is limited by the OS (~1 MB on macOS, shared with argv).
const MAX_DIFF_BYTES: usize = 80_000;
const TRUNCATION_MARKER: &str = "\n[diff truncated]";

/// Printed by the shell on both streams right before the command runs: whatever
/// an interactive shell's startup wrote sits ahead of it, the command's own
/// output after.
const REPLY_MARKER: &str = "__helm_reply__";

pub fn generate(
    workdir: &Path,
    request: &CommitMessageRequest,
) -> Result<CommitSuggestion, AiError> {
    generate_in_shell(Path::new(&shell_program()), workdir, request)
}

/// Seam: `generate` pins the shell to the user's; the parameter lets us exercise
/// the full pipeline under a plain `/bin/sh`.
pub fn generate_in_shell(
    shell: &Path,
    workdir: &Path,
    request: &CommitMessageRequest,
) -> Result<CommitSuggestion, AiError> {
    let changes = change_context(workdir);
    if changes.trim().is_empty() {
        return Err(AiError::NoChanges);
    }
    let prompt = request.prompt_for(&changes);
    let line = format!(
        "printf '%s\\n' {REPLY_MARKER}; printf '%s\\n' {REPLY_MARKER} >&2; {}",
        request.command
    );
    // Login + interactive, like a terminal tab: the user's PATH and aliases apply.
    let output = cli::run_program_with_timeout(
        shell,
        workdir,
        &["-lic", &line],
        cli::DEFAULT_TIMEOUT,
        &[(PROMPT_ENV, prompt)],
    )
    .map_err(|err| match err {
        CliError::NotFound => AiError::Failed(format!("shell '{}' not found", shell.display())),
        CliError::TimedOut(duration) => {
            AiError::Failed(format!("timed out after {}s", duration.as_secs()))
        }
        CliError::Io(err) => AiError::Failed(err.to_string()),
    })?;
    let reply = after_marker(&output.stdout);
    if !output.success() {
        return Err(AiError::Failed(failure_detail(&output, reply)));
    }
    parse_suggestion(reply).ok_or(AiError::EmptyReply)
}

fn after_marker(stream: &str) -> &str {
    stream
        .split_once(REPLY_MARKER)
        .map_or(stream, |(_, own)| own)
}

fn failure_detail(output: &cli::CliOutput, reply: &str) -> String {
    let stderr = after_marker(&output.stderr).trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    let reply = reply.trim();
    if !reply.is_empty() {
        return reply.to_owned();
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
    pub fn request(&mut self, request: CommitMessageRequest) -> bool {
        self.request_in_shell(PathBuf::from(shell_program()), request)
    }

    /// Seam: same execution under an explicit shell (`/bin/sh` in tests).
    pub fn request_in_shell(&mut self, shell: PathBuf, request: CommitMessageRequest) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        let path = self.repo_path.clone();
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = generate_in_shell(&shell, &path, &request);
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
}
