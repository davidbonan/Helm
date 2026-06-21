use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::git::cli::{self, CliError, CliOutput};

/// IDE opened by a terminal Cmd+click on a file link (terminal.md §12,
/// preferences.md §4 Terminal). Each maps to the editor `template` fed to
/// `execute`; its CLI is expected on `PATH` (installed by the IDE's "shell
/// command" action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Editor {
    #[default]
    VsCode,
    Cursor,
    Zed,
}

impl Editor {
    pub const ALL: [Editor; 3] = [Editor::VsCode, Editor::Cursor, Editor::Zed];

    /// Preferences dropdown label: the product name.
    pub fn label(self) -> &'static str {
        match self {
            Editor::VsCode => "VS Code",
            Editor::Cursor => "Cursor",
            Editor::Zed => "Zed",
        }
    }

    /// Editor `template` (`{file}`/`{line}` substituted at click time): VS Code
    /// and its Cursor fork share the `-g` goto flag; Zed takes a `path:line` arg.
    pub fn template(self) -> &'static str {
        match self {
            Editor::VsCode => "code -g {file}:{line}",
            Editor::Cursor => "cursor -g {file}:{line}",
            Editor::Zed => "zed {file}:{line}",
        }
    }
}

/// What activating a link does. The domain resolves and validates everything
/// (path existence, line/column); the app only executes the intent (architecture §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    Url(String),
    File {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// A link under the pointer: the flat-column run to underline plus its action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<usize>,
    pub action: LinkAction,
}

const TRAILING_PUNCT: &[char] = &['.', ',', ';', ':', ')', ']', '}', '\'', '"', '>'];

/// Detect a link anchored at flat column `idx` of a logical line (soft-wrapped
/// rows already joined by the caller). `uris` carries the OSC 8 hyperlink of each
/// cell, parallel to `text`'s chars. Resolution order: OSC 8 span, then an
/// `http(s)://` URL token, then an existing file path. `None` if nothing matches.
pub fn link_at(text: &str, uris: &[Option<String>], idx: usize, cwd: &Path) -> Option<Link> {
    let chars: Vec<char> = text.chars().collect();
    if idx >= chars.len() {
        return None;
    }
    if let Some((range, uri)) = uri_span(uris, idx) {
        return osc8_action(uri).map(|action| Link { range, action });
    }
    if let Some((range, url)) = url_token_at(&chars, idx) {
        return Some(Link {
            range,
            action: LinkAction::Url(url),
        });
    }
    path_token_at(&chars, idx, cwd).map(|(range, action)| Link { range, action })
}

/// The contiguous run of cells around `idx` sharing the same OSC 8 URI.
fn uri_span(uris: &[Option<String>], idx: usize) -> Option<(Range<usize>, &str)> {
    let uri = uris.get(idx)?.as_deref()?;
    let mut start = idx;
    while start > 0 && uris[start - 1].as_deref() == Some(uri) {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < uris.len() && uris[end].as_deref() == Some(uri) {
        end += 1;
    }
    Some((start..end, uri))
}

/// Map an OSC 8 URI to an action: `file://` (percent-decoded) to a file, `http(s)`
/// to a URL, every other scheme ignored (v1, terminal.md §12).
fn osc8_action(uri: &str) -> Option<LinkAction> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let path = match rest.find('/') {
            Some(slash) => &rest[slash..],
            None => rest,
        };
        Some(LinkAction::File {
            path: PathBuf::from(percent_decode(path)),
            line: None,
            column: None,
        })
    } else if uri.starts_with("http://") || uri.starts_with("https://") {
        Some(LinkAction::Url(uri.to_string()))
    } else {
        None
    }
}

/// `[start, end)` of the whitespace-delimited token covering `idx`.
fn token_bounds(chars: &[char], idx: usize) -> Option<Range<usize>> {
    if idx >= chars.len() || chars[idx].is_whitespace() {
        return None;
    }
    let mut start = idx;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    Some(start..end)
}

/// Token at `idx`, trailing punctuation trimmed, kept only if it carries an
/// `http(s)://` scheme with a non-empty remainder.
fn url_token_at(chars: &[char], idx: usize) -> Option<(Range<usize>, String)> {
    let range = token_bounds(chars, idx)?;
    let mut end = range.end;
    while end > range.start && TRAILING_PUNCT.contains(&chars[end - 1]) {
        end -= 1;
    }
    let token: String = chars[range.start..end].iter().collect();
    let after = token
        .strip_prefix("https://")
        .or_else(|| token.strip_prefix("http://"))?;
    if after.is_empty() {
        return None;
    }
    Some((range.start..end, token))
}

/// Token at `idx` parsed as a file path with an optional `:line(:col)` suffix,
/// resolved against `cwd` and gated on `is_file()`.
fn path_token_at(chars: &[char], idx: usize, cwd: &Path) -> Option<(Range<usize>, LinkAction)> {
    let range = token_bounds(chars, idx)?;
    let mut end = range.end;
    while end > range.start && TRAILING_PUNCT.contains(&chars[end - 1]) {
        end -= 1;
    }
    let token: String = chars[range.start..end].iter().collect();
    let (path_part, line, column) = parse_line_col(&token);
    let path = resolve_path(path_part, cwd)?;
    Some((range.start..end, LinkAction::File { path, line, column }))
}

/// Split a trailing `:line` or `:line:col` off the path part.
fn parse_line_col(token: &str) -> (&str, Option<u32>, Option<u32>) {
    if let Some((rest, n1)) = split_trailing_number(token) {
        if let Some((path, n2)) = split_trailing_number(rest) {
            return (path, Some(n2), Some(n1));
        }
        return (rest, Some(n1), None);
    }
    (token, None, None)
}

fn split_trailing_number(s: &str) -> Option<(&str, u32)> {
    let (head, tail) = s.rsplit_once(':')?;
    let n: u32 = tail.parse().ok()?;
    Some((head, n))
}

/// Resolve `~`, relative, and absolute paths against `cwd`; `None` unless the
/// result is an existing file (directories are not links — terminal.md §12).
fn resolve_path(path: &str, cwd: &Path) -> Option<PathBuf> {
    let expanded = if path == "~" {
        PathBuf::from(std::env::var_os("HOME")?)
    } else if let Some(rest) = path.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME")?).join(rest)
    } else {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    };
    expanded.is_file().then_some(expanded)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Why opening a link failed, surfaced as a toast (terminal.md §12): a configured
/// editor that cannot run is reported, never silently swapped for `open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// The launcher/editor binary is absent (a typo'd command, or no `open`).
    NotFound(String),
    /// The command launched but reported a failure (non-zero exit, signal, I/O).
    Failed { command: String, detail: String },
}

impl LinkError {
    pub fn message(&self) -> String {
        match self {
            LinkError::NotFound(command) => format!("Couldn't run {command}"),
            LinkError::Failed { command, detail } => format!("{command}: {detail}"),
        }
    }
}

/// Argv for the editor `template` (terminal.md §12, preferences.md §4): split on
/// whitespace, then per-token substitute `{file}` and `{line}` (line defaults to
/// 1). An editor binary whose path contains spaces is the documented limit.
pub fn editor_argv(template: &str, file: &Path, line: Option<u32>) -> Vec<String> {
    let file = file.to_string_lossy();
    let line = line.unwrap_or(1).to_string();
    template
        .split_whitespace()
        .map(|token| token.replace("{file}", &file).replace("{line}", &line))
        .collect()
}

/// Execute a resolved link (terminal.md §12): a URL — or a file with an empty
/// `template` — opens through macOS `open` (default browser / app); a configured
/// `template` opens the file in that editor, detached.
pub fn execute(action: &LinkAction, template: &str) -> Result<(), LinkError> {
    execute_with(Path::new("open"), action, template)
}

/// Seam: explicit `open` binary for e2e tests (a fake `open` capturing its argv).
/// The editor path takes its binary from `template`, so the template is its seam.
pub fn execute_with(open: &Path, action: &LinkAction, template: &str) -> Result<(), LinkError> {
    match action {
        LinkAction::Url(url) => run_open(open, url),
        LinkAction::File { path, line, .. } => {
            if template.trim().is_empty() {
                run_open(open, &path.to_string_lossy())
            } else {
                spawn_editor(template, path, *line)
            }
        }
    }
}

/// Open a URL in the default browser (git.md §9 Create pull request, feedback.rs
/// pattern) — the create-PR seam, distinct from the [`execute`] link path.
pub fn open_url(url: &str) -> Result<(), LinkError> {
    open_url_with(Path::new("open"), url)
}

/// Seam: explicit `open` binary for e2e tests (a fake `open` capturing its argv).
pub fn open_url_with(open: &Path, url: &str) -> Result<(), LinkError> {
    run_open(open, url)
}

/// Hand `arg` to macOS `open` and wait for the hand-off (feedback.rs pattern):
/// LaunchServices returns at once, so this never blocks on the opened app.
fn run_open(open: &Path, arg: &str) -> Result<(), LinkError> {
    let command = open.to_string_lossy().into_owned();
    let output = cli::run_program(open, Path::new("/"), &[arg]).map_err(|err| match err {
        CliError::NotFound => LinkError::NotFound(command.clone()),
        CliError::TimedOut(duration) => LinkError::Failed {
            command: command.clone(),
            detail: format!("timed out after {}s", duration.as_secs()),
        },
        CliError::Io(err) => LinkError::Failed {
            command: command.clone(),
            detail: err.to_string(),
        },
    })?;
    if !output.success() {
        return Err(LinkError::Failed {
            command,
            detail: open_failure_detail(&output),
        });
    }
    Ok(())
}

/// Spawn the configured editor on `file`, detached: a reaper thread reaps it so a
/// GUI editor that lingers leaves no zombie. Only a spawn failure (missing or
/// unrunnable binary) is surfaced — a detached process' exit code is unobservable.
fn spawn_editor(template: &str, file: &Path, line: Option<u32>) -> Result<(), LinkError> {
    let argv = editor_argv(template, file, line);
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| LinkError::NotFound(template.to_owned()))?;
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => LinkError::NotFound(program.clone()),
            _ => LinkError::Failed {
                command: program.clone(),
                detail: err.to_string(),
            },
        })?;
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

fn open_failure_detail(output: &CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    match output.code {
        Some(code) => format!("exit code {code}"),
        None => "killed by a signal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_uris(text: &str) -> Vec<Option<String>> {
        vec![None; text.chars().count()]
    }

    #[test]
    fn url_detected_from_mid_token() {
        let text = "see https://example.com/path now";
        let url = "https://example.com/path";
        let start = text.find(url).unwrap();
        let link = link_at(text, &no_uris(text), start + 10, Path::new("/")).unwrap();
        assert_eq!(link.action, LinkAction::Url(url.to_string()));
        assert_eq!(link.range, start..start + url.chars().count());
    }

    #[test]
    fn url_trailing_punctuation_trimmed() {
        let text = "go https://example.com. ok";
        let link = link_at(text, &no_uris(text), 6, Path::new("/")).unwrap();
        assert_eq!(
            link.action,
            LinkAction::Url("https://example.com".to_string())
        );
    }

    #[test]
    fn bare_domain_is_not_a_link() {
        let dir = tempfile::tempdir().unwrap();
        assert!(link_at("example.com", &no_uris("example.com"), 3, dir.path()).is_none());
    }

    #[test]
    fn url_spanning_a_wrapped_join_keeps_the_full_range() {
        let row1 = "x".repeat(70);
        let url = "https://example.com/some/long/resource/path";
        let text = format!("{row1} {url}");
        let chars = text.chars().count();
        let link = link_at(&text, &no_uris(&text), chars - 3, Path::new("/")).unwrap();
        assert_eq!(link.action, LinkAction::Url(url.to_string()));
        let start = text.find(url).unwrap();
        assert_eq!(link.range, start..start + url.chars().count());
    }

    #[test]
    fn relative_path_resolves_against_cwd_with_existence_gate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/b.rs"), "").unwrap();
        let link = link_at("a/b.rs:42", &no_uris("a/b.rs:42"), 1, dir.path()).unwrap();
        assert_eq!(
            link.action,
            LinkAction::File {
                path: dir.path().join("a/b.rs"),
                line: Some(42),
                column: None,
            }
        );
        assert!(link_at("a/missing.rs", &no_uris("a/missing.rs"), 1, dir.path()).is_none());
    }

    #[test]
    fn line_and_column_suffix_parsed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        let link = link_at("b.rs:42:5", &no_uris("b.rs:42:5"), 0, dir.path()).unwrap();
        assert_eq!(
            link.action,
            LinkAction::File {
                path: dir.path().join("b.rs"),
                line: Some(42),
                column: Some(5),
            }
        );
    }

    #[test]
    fn absolute_path_resolves_and_keeps_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {}").unwrap();
        let typed = format!("{}:10", file.display());
        let link = link_at(&typed, &no_uris(&typed), 2, Path::new("/")).unwrap();
        assert_eq!(
            link.action,
            LinkAction::File {
                path: file,
                line: Some(10),
                column: None,
            }
        );
    }

    #[test]
    fn tilde_path_resolves_against_home() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let tmp = tempfile::Builder::new()
            .prefix("helm_link_test_")
            .tempfile_in(&home)
            .unwrap();
        let name = tmp.path().file_name().unwrap().to_str().unwrap();
        let typed = format!("~/{name}");
        let link = link_at(&typed, &no_uris(&typed), 2, Path::new("/")).unwrap();
        assert_eq!(
            link.action,
            LinkAction::File {
                path: tmp.path().to_path_buf(),
                line: None,
                column: None,
            }
        );
    }

    #[test]
    fn osc8_file_uri_decoded_to_file_route() {
        let text = "click here";
        let mut uris = no_uris(text);
        for u in uris.iter_mut().skip(6) {
            *u = Some("file:///tmp/a%20b.rs".to_string());
        }
        let link = link_at(text, &uris, 7, Path::new("/")).unwrap();
        assert_eq!(link.range, 6..10);
        assert_eq!(
            link.action,
            LinkAction::File {
                path: PathBuf::from("/tmp/a b.rs"),
                line: None,
                column: None,
            }
        );
    }

    #[test]
    fn osc8_http_uri_yields_url() {
        let text = "label";
        let uris = vec![Some("https://example.com".to_string()); 5];
        let link = link_at(text, &uris, 2, Path::new("/")).unwrap();
        assert_eq!(link.range, 0..5);
        assert_eq!(
            link.action,
            LinkAction::Url("https://example.com".to_string())
        );
    }

    #[test]
    fn osc8_non_supported_scheme_ignored() {
        let text = "mail me";
        let uris = vec![Some("mailto:foo@bar.com".to_string()); text.chars().count()];
        assert!(link_at(text, &uris, 0, Path::new("/")).is_none());
    }

    #[test]
    fn editor_argv_substitutes_file_and_line() {
        let argv = editor_argv("code -g {file}:{line}", Path::new("/src/main.rs"), Some(42));
        assert_eq!(argv, ["code", "-g", "/src/main.rs:42"]);
    }

    #[test]
    fn editor_argv_defaults_the_line_to_one() {
        let argv = editor_argv("vim +{line} {file}", Path::new("/a/b.txt"), None);
        assert_eq!(argv, ["vim", "+1", "/a/b.txt"]);
    }

    #[test]
    fn each_editor_template_builds_its_argv() {
        let file = Path::new("/src/main.rs");
        assert_eq!(
            editor_argv(Editor::VsCode.template(), file, Some(42)),
            ["code", "-g", "/src/main.rs:42"]
        );
        assert_eq!(
            editor_argv(Editor::Cursor.template(), file, Some(42)),
            ["cursor", "-g", "/src/main.rs:42"]
        );
        assert_eq!(
            editor_argv(Editor::Zed.template(), file, Some(42)),
            ["zed", "/src/main.rs:42"]
        );
    }
}
