//! Feedback submission (specs/feedback.md): a Suggestion/Bug report filed as a
//! GitHub issue on the helm repo. We hand a pre-filled `issues/new` URL to macOS
//! `open` — no HTTP, no embedded token; the user reviews and submits the issue
//! in their browser (signed in to GitHub). Synchronous and instant
//! (LaunchServices hands off at once), so no worker thread. Pure domain: no egui
//! dependency.

use std::fmt::Write as _;
use std::path::Path;

use crate::git::cli::{self, CliError, CliOutput};

/// The helm repo that receives feedback issues (same slug as `update.rs`).
const REPO: &str = "davidbonan/Helm";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackKind {
    Suggestion,
    Bug,
}

impl FeedbackKind {
    /// Dropdown order: Suggestion first, Bug second (the modal defaults to Bug).
    pub const ALL: [FeedbackKind; 2] = [FeedbackKind::Suggestion, FeedbackKind::Bug];

    pub fn label(self) -> &'static str {
        match self {
            FeedbackKind::Suggestion => "Suggestion",
            FeedbackKind::Bug => "Bug",
        }
    }

    /// GitHub label pre-applied to the issue (default repo labels).
    fn issue_label(self) -> &'static str {
        match self {
            FeedbackKind::Suggestion => "enhancement",
            FeedbackKind::Bug => "bug",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackError {
    /// `open` binary absent (not macOS, or a broken PATH).
    OpenNotFound,
    /// `open` ran but could not launch a mail client.
    OpenFailed(String),
}

impl FeedbackError {
    pub fn message(&self) -> String {
        match self {
            FeedbackError::OpenNotFound => "could not open your browser".to_owned(),
            FeedbackError::OpenFailed(detail) => detail.clone(),
        }
    }
}

/// Opens the GitHub "new issue" form on the helm repo, pre-filled with the
/// description and the app + macOS metadata footer.
pub fn open_issue(kind: FeedbackKind, description: &str) -> Result<(), FeedbackError> {
    open_with(Path::new("open"), REPO, kind, description, &metadata())
}

/// Seam: explicit open/repo/metadata for e2e tests (a fake `open` capturing its
/// argv, no browser launched).
pub fn open_with(
    open: &Path,
    repo: &str,
    kind: FeedbackKind,
    description: &str,
    metadata: &str,
) -> Result<(), FeedbackError> {
    let url = issue_url(repo, kind, description, metadata);
    let output = cli::run_program(open, Path::new("/"), &[&url]).map_err(|err| match err {
        CliError::NotFound => FeedbackError::OpenNotFound,
        CliError::TimedOut(duration) => {
            FeedbackError::OpenFailed(format!("open timed out after {}s", duration.as_secs()))
        }
        CliError::Io(err) => FeedbackError::OpenFailed(err.to_string()),
    })?;
    if !output.success() {
        return Err(FeedbackError::OpenFailed(open_failure_detail(&output)));
    }
    Ok(())
}

/// GitHub `issues/new` URL with percent-encoded `title` / `body` / `labels`. The
/// title is the first non-blank line of the description; the body carries the
/// full description plus the metadata footer (newlines encode as `%0A`).
fn issue_url(repo: &str, kind: FeedbackKind, description: &str, metadata: &str) -> String {
    let title = description
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("feedback");
    let body = format!("{description}\n\n— {metadata}");
    format!(
        "https://github.com/{repo}/issues/new?title={}&body={}&labels={}",
        encode(title),
        encode(&body),
        encode(kind.issue_label())
    )
}

/// Percent-encodes everything outside RFC 3986 unreserved set — the conservative
/// choice for a `mailto` query (spaces as `%20`, not `+`, which is literal here).
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

fn metadata() -> String {
    format!(
        "helm {} · macOS {}",
        crate::update::current_version(),
        macos_version()
    )
}

fn macos_version() -> String {
    cli::run_program(Path::new("sw_vers"), Path::new("/"), &["-productVersion"])
        .ok()
        .filter(|output| output.success())
        .map(|output| output.stdout.trim().to_owned())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn open_failure_detail(output: &CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    match output.code {
        Some(code) => format!("open exit code {code}"),
        None => "open killed by a signal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_query(url: &str) -> (String, String, String) {
        let query = url
            .strip_prefix("https://github.com/davidbonan/Helm/issues/new?")
            .unwrap();
        let mut title = None;
        let mut body = None;
        let mut labels = None;
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap();
            match key {
                "title" => title = Some(value.to_owned()),
                "body" => body = Some(value.to_owned()),
                "labels" => labels = Some(value.to_owned()),
                _ => panic!("unexpected query field {key}"),
            }
        }
        (title.unwrap(), body.unwrap(), labels.unwrap())
    }

    #[test]
    fn issue_carries_the_title_body_and_label_per_kind() {
        let url = issue_url(
            REPO,
            FeedbackKind::Bug,
            "it crashes on launch",
            "helm 0.2.0 · macOS 15",
        );
        let (title, body, labels) = parse_query(&url);
        assert_eq!(title, "it%20crashes%20on%20launch");
        // The full description with spaces encoded, then the footer.
        assert!(body.starts_with("it%20crashes%20on%20launch"), "{body}");
        assert!(body.contains("%E2%80%94%20helm%200.2.0"), "{body}");
        assert_eq!(labels, "bug");

        let (_, _, labels) = parse_query(&issue_url(REPO, FeedbackKind::Suggestion, "x", "m"));
        assert_eq!(labels, "enhancement");
    }

    #[test]
    fn the_title_is_the_first_non_blank_line() {
        let url = issue_url(
            REPO,
            FeedbackKind::Bug,
            "  \nreal title\nmore detail",
            "meta",
        );
        let (title, body, _) = parse_query(&url);
        assert_eq!(title, "real%20title");
        // The body still keeps the whole description, leading blank line included.
        assert!(
            body.starts_with("%20%20%0Areal%20title%0Amore%20detail"),
            "{body}"
        );
    }

    #[test]
    fn newlines_encode_so_the_url_stays_one_token() {
        let url = issue_url(REPO, FeedbackKind::Bug, "line one\nline two", "meta");
        assert!(
            !url.contains('\n'),
            "newlines must be percent-encoded: {url}"
        );
        let (_, body, _) = parse_query(&url);
        assert!(body.contains("line%20one%0Aline%20two"), "{body}");
    }

    #[test]
    fn encode_leaves_unreserved_bytes_and_escapes_the_rest() {
        assert_eq!(encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(encode("a b/c?d"), "a%20b%2Fc%3Fd");
    }

    #[test]
    fn an_error_renders_a_message() {
        assert_eq!(
            FeedbackError::OpenNotFound.message(),
            "could not open your browser"
        );
        assert_eq!(
            FeedbackError::OpenFailed("no handler".into()).message(),
            "no handler"
        );
    }
}
