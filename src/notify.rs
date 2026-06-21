//! macOS completion notifications (specs/agents.md): when a watched agent
//! finishes a turn, surface a native banner. We hand the text to `osascript`'s
//! `display notification` — no app bundle, entitlement, or Notification Center
//! authorization needed, so it works under `cargo run` and stays testable with a
//! fake `osascript` capturing its argv (mirrors `feedback.rs`). Pure domain: no
//! egui dependency.

use std::path::Path;

use crate::git::cli::{self, CliError, CliOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyError {
    /// `osascript` binary absent (not macOS, or a broken PATH).
    NotFound,
    /// `osascript` ran but failed to post the notification.
    Failed(String),
}

impl NotifyError {
    pub fn message(&self) -> String {
        match self {
            NotifyError::NotFound => "could not post a notification".to_owned(),
            NotifyError::Failed(detail) => detail.clone(),
        }
    }
}

/// Posts a native banner with `title` / `body`.
pub fn notify(title: &str, body: &str) -> Result<(), NotifyError> {
    notify_with(Path::new("osascript"), title, body)
}

/// Seam: explicit `osascript` path for e2e tests (a fake binary capturing its
/// argv, no banner posted).
pub fn notify_with(osascript: &Path, title: &str, body: &str) -> Result<(), NotifyError> {
    let script = display_notification_script(title, body);
    let output =
        cli::run_program(osascript, Path::new("/"), &["-e", &script]).map_err(|err| match err {
            CliError::NotFound => NotifyError::NotFound,
            CliError::TimedOut(duration) => {
                NotifyError::Failed(format!("osascript timed out after {}s", duration.as_secs()))
            }
            CliError::Io(err) => NotifyError::Failed(err.to_string()),
        })?;
    if !output.success() {
        return Err(NotifyError::Failed(failure_detail(&output)));
    }
    Ok(())
}

/// AppleScript one-liner: `display notification "body" with title "title"`. Both
/// operands are string literals — escaped so a quote or backslash in an agent /
/// repo name cannot break out of the literal.
fn display_notification_script(title: &str, body: &str) -> String {
    format!(
        "display notification \"{}\" with title \"{}\"",
        escape_applescript(body),
        escape_applescript(title),
    )
}

/// Escapes an AppleScript double-quoted string: backslash and quote are the only
/// metacharacters; a raw newline would terminate the `-e` line, so it is folded
/// to a space.
fn escape_applescript(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Title / body of an agent's completion banner: "Claude finished" + the project
/// (and its branch when known).
pub fn completion_message(agent: &str, repo: &str, branch: Option<&str>) -> (String, String) {
    let title = format!("{} finished", crate::agent_watch::display_name(agent));
    let body = match branch {
        Some(branch) if !branch.is_empty() => format!("{repo} · {branch}"),
        _ => repo.to_owned(),
    };
    (title, body)
}

fn failure_detail(output: &CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    match output.code {
        Some(code) => format!("osascript exit code {code}"),
        None => "osascript killed by a signal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_wraps_both_operands_as_applescript_literals() {
        assert_eq!(
            display_notification_script("Claude finished", "helm · main"),
            "display notification \"helm · main\" with title \"Claude finished\"",
        );
    }

    #[test]
    fn escape_neutralises_quotes_backslashes_and_newlines() {
        assert_eq!(escape_applescript("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(
            escape_applescript("line one\nline two"),
            "line one line two"
        );
        // The escaped script never carries a raw newline (would split `-e`).
        let script = display_notification_script("t\"itle", "bo\ndy");
        assert!(!script.contains('\n'), "{script}");
        assert!(
            !script.contains("t\"itle"),
            "unescaped quote leaks: {script}"
        );
    }

    #[test]
    fn completion_message_capitalises_and_appends_the_branch() {
        let (title, body) = completion_message("claude", "helm", Some("main"));
        assert_eq!(title, "Claude finished");
        assert_eq!(body, "helm · main");

        // No branch (or an empty one) ⇒ the repo name alone.
        let (_, body) = completion_message("codex", "api", None);
        assert_eq!(body, "api");
        let (_, body) = completion_message("codex", "api", Some(""));
        assert_eq!(body, "api");
    }

    #[test]
    fn an_error_renders_a_message() {
        assert_eq!(
            NotifyError::NotFound.message(),
            "could not post a notification"
        );
        assert_eq!(NotifyError::Failed("boom".into()).message(), "boom");
    }
}
