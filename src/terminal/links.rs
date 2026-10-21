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
    Folder(PathBuf),
}

/// A link under the pointer: the flat-column run to underline plus its action.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    range: Range<usize>,
    action: LinkAction,
}

/// One visible terminal row as the link scanner reads it: a char and an OSC 8
/// URI per cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRow {
    pub chars: Vec<char>,
    pub uris: Vec<Option<String>>,
    /// The row soft-wraps into the next one (its last cell carries `WRAPLINE`).
    pub wrapped: bool,
}

/// A link found on screen: its action plus the `(row, col)` cells to underline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenLink {
    pub action: LinkAction,
    pub cells: Vec<(usize, usize)>,
}

/// A link can span at most this many visual rows — the join window walked
/// around the hovered cell (terminal.md §12).
pub const MAX_LOGICAL_ROWS: usize = 8;

const TRAILING_PUNCT: &[char] = &['.', ',', ';', ':', ')', ']', '}', '\'', '"', '>', '`'];

/// Chars that end a URL inside a token: quotes and angle brackets never belong to one.
const URL_DELIMITERS: &[char] = &['"', '\'', '<', '>', '`'];

/// Chars that wrap a path in agent and compiler output — `Update(src/a.rs)`,
/// `` `src/a.rs` ``, `[src/a.rs]` — and so bound it.
const PATH_DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '<', '>', '"', '\'', '`'];

/// Which row breaks the logical line crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Joins {
    /// Only the terminal's own soft wraps.
    Soft,
    /// Also the hard wraps TUIs (Ink: Claude Code, Codex) insert themselves.
    SoftAndHard,
}

/// Detect the link covering cell `(row, col)` of `rows` (terminal.md §12). The
/// logical line is first rebuilt across hard wraps too — a TUI breaks a long URL
/// with a newline and re-indents its tail — then, if that finds nothing, across
/// soft wraps only, so a full-width row never hides a link by gluing its
/// neighbour on.
pub fn link_on_screen(
    rows: &[ScreenRow],
    row: usize,
    col: usize,
    cwd: &Path,
) -> Option<ScreenLink> {
    [Joins::SoftAndHard, Joins::Soft]
        .into_iter()
        .find_map(|joins| {
            let line = LogicalLine::around(rows, row, col, joins)?;
            let link = link_at(&line.text, &line.uris, line.hover, cwd)?;
            Some(ScreenLink {
                action: link.action,
                cells: line.cells[link.range].to_vec(),
            })
        })
}

/// Rows joined into one line of text, each char mapped back to its cell.
struct LogicalLine {
    text: String,
    uris: Vec<Option<String>>,
    cells: Vec<(usize, usize)>,
    /// Flat index of the hovered cell.
    hover: usize,
}

impl LogicalLine {
    /// The line through `(row, col)`, capped at [`MAX_LOGICAL_ROWS`]; `None` when
    /// the cell is out of the grid or in a hard-wrapped row's indent.
    fn around(rows: &[ScreenRow], row: usize, col: usize, joins: Joins) -> Option<Self> {
        if col >= rows.get(row)?.chars.len() {
            return None;
        }
        let mut first = row;
        while first > 0 && row - first + 1 < MAX_LOGICAL_ROWS && continues(rows, first - 1, joins) {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < rows.len()
            && last - first + 1 < MAX_LOGICAL_ROWS
            && continues(rows, last, joins)
        {
            last += 1;
        }
        let mut line = LogicalLine {
            text: String::new(),
            uris: Vec::new(),
            cells: Vec::new(),
            hover: 0,
        };
        let mut hover = None;
        for r in first..=last {
            let screen_row = &rows[r];
            let indent = if r > first && !rows[r - 1].wrapped {
                screen_row
                    .chars
                    .iter()
                    .take_while(|c| c.is_whitespace())
                    .count()
            } else {
                0
            };
            for c in indent..screen_row.chars.len() {
                if (r, c) == (row, col) {
                    hover = Some(line.cells.len());
                }
                line.text.push(screen_row.chars[c]);
                line.uris.push(screen_row.uris.get(c).cloned().flatten());
                line.cells.push((r, c));
            }
        }
        line.hover = hover?;
        Some(line)
    }
}

/// Whether row `upper` runs on into the next one: a soft wrap, or — for
/// [`Joins::SoftAndHard`] — a row inked up to the right edge followed by a
/// non-blank row, the shape of a TUI breaking a word too long for its box.
fn continues(rows: &[ScreenRow], upper: usize, joins: Joins) -> bool {
    if rows[upper].wrapped {
        return true;
    }
    joins == Joins::SoftAndHard
        && rows[upper].chars.last().is_some_and(|c| !c.is_whitespace())
        && rows
            .get(upper + 1)
            .is_some_and(|lower| lower.chars.iter().any(|c| !c.is_whitespace()))
}

/// Detect a link anchored at flat column `idx` of a logical line. `uris` carries
/// the OSC 8 hyperlink of each cell, parallel to `text`'s chars. Resolution
/// order: OSC 8 span, then an `http(s)://` URL, then an existing file or folder
/// path. `None` if nothing matches.
fn link_at(text: &str, uris: &[Option<String>], idx: usize, cwd: &Path) -> Option<Link> {
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

/// `[start, end)` of the run covering `idx` bounded by whitespace or `delimiters`.
fn token_bounds(chars: &[char], idx: usize, delimiters: &[char]) -> Option<Range<usize>> {
    let bounds = |c: &char| c.is_whitespace() || delimiters.contains(c);
    if bounds(chars.get(idx)?) {
        return None;
    }
    let mut start = idx;
    while start > 0 && !bounds(&chars[start - 1]) {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < chars.len() && !bounds(&chars[end]) {
        end += 1;
    }
    Some(start..end)
}

/// `range` with its trailing punctuation trimmed.
fn trim_trailing_punct(chars: &[char], range: Range<usize>) -> Range<usize> {
    let mut end = range.end;
    while end > range.start && TRAILING_PUNCT.contains(&chars[end - 1]) {
        end -= 1;
    }
    range.start..end
}

/// The `http(s)://` URL covering `idx`: it starts at the scheme, wherever it sits
/// in the token (`(https://…)`, `[docs](https://…)`), and ends at the token's end,
/// trailing punctuation trimmed. A scheme with nothing after it is not a URL.
fn url_token_at(chars: &[char], idx: usize) -> Option<(Range<usize>, String)> {
    let token = token_bounds(chars, idx, URL_DELIMITERS)?;
    let text: String = chars[token.clone()].iter().collect();
    let start = token.start
        + ["https://", "http://"]
            .iter()
            .flat_map(|scheme| text.match_indices(scheme))
            .map(|(byte, _)| text[..byte].chars().count())
            .filter(|&at| token.start + at <= idx)
            .max()?;
    let range = trim_trailing_punct(chars, start..token.end);
    let url: String = chars[range.clone()].iter().collect();
    let scheme_len = if url.starts_with("https://") { 8 } else { 7 };
    (idx < range.end && url.len() > scheme_len).then_some((range, url))
}

/// The run at `idx` parsed as a path with an optional `:line(:col)` suffix,
/// resolved against `cwd`: an existing file opens at its line, an existing
/// folder as a folder.
fn path_token_at(chars: &[char], idx: usize, cwd: &Path) -> Option<(Range<usize>, LinkAction)> {
    let range = trim_trailing_punct(chars, token_bounds(chars, idx, PATH_DELIMITERS)?);
    if idx >= range.end {
        return None;
    }
    let token: String = chars[range.clone()].iter().collect();
    let (path_part, line, column) = parse_line_col(&token);
    let path = resolve_path(path_part, cwd)?;
    let action = if path.is_dir() {
        LinkAction::Folder(path)
    } else {
        LinkAction::File { path, line, column }
    };
    Some((range, action))
}

/// Split a trailing `:line`, `:line:col` or `:line-line` (a range, opened at its
/// first line) off the path part.
fn parse_line_col(token: &str) -> (&str, Option<u32>, Option<u32>) {
    if let Some((rest, n1)) = split_trailing_number(token) {
        if let Some((path, n2)) = split_trailing_number(rest) {
            return (path, Some(n2), Some(n1));
        }
        return (rest, Some(n1), None);
    }
    if let Some((rest, line)) = split_trailing_range(token) {
        return (rest, Some(line), None);
    }
    (token, None, None)
}

fn split_trailing_number(s: &str) -> Option<(&str, u32)> {
    let (head, tail) = s.rsplit_once(':')?;
    let n: u32 = tail.parse().ok()?;
    Some((head, n))
}

fn split_trailing_range(s: &str) -> Option<(&str, u32)> {
    let (head, tail) = s.rsplit_once(':')?;
    let (from, to) = tail.split_once('-')?;
    to.parse::<u32>().ok()?;
    Some((head, from.parse().ok()?))
}

/// Resolve `~`, relative, and absolute paths against `cwd`; `None` unless the
/// result exists (a file or a folder).
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
    expanded.exists().then_some(expanded)
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

/// Argv opening `folder` in the editor of `template`: its binary, given the
/// folder alone — every supported IDE opens a folder argument as a workspace.
fn folder_argv(template: &str, folder: &Path) -> Vec<String> {
    template
        .split_whitespace()
        .take(1)
        .map(str::to_owned)
        .chain([folder.to_string_lossy().into_owned()])
        .collect()
}

/// Execute a resolved link (terminal.md §12): a URL — or a file or folder with an
/// empty `template` — opens through macOS `open` (default browser / app); a
/// configured `template` opens the file or folder in that editor, detached.
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
                spawn_editor(editor_argv(template, path, *line), template)
            }
        }
        LinkAction::Folder(path) => {
            if template.trim().is_empty() {
                run_open(open, &path.to_string_lossy())
            } else {
                spawn_editor(folder_argv(template, path), template)
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

/// Spawn the configured editor's `argv`, detached: a reaper thread reaps it so a
/// GUI editor that lingers leaves no zombie. Only a spawn failure (missing or
/// unrunnable binary) is surfaced — a detached process' exit code is unobservable.
fn spawn_editor(argv: Vec<String>, template: &str) -> Result<(), LinkError> {
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

    const COLS: usize = 40;

    /// Rows as a TUI draws them: each line padded to `COLS`, no soft wrap.
    fn screen(lines: &[&str]) -> Vec<ScreenRow> {
        lines
            .iter()
            .map(|line| {
                let mut chars: Vec<char> = line.chars().collect();
                chars.resize(COLS, ' ');
                ScreenRow {
                    uris: vec![None; COLS],
                    chars,
                    wrapped: false,
                }
            })
            .collect()
    }

    fn col_of(line: &str, needle: &str) -> usize {
        line[..line.find(needle).unwrap()].chars().count()
    }

    #[test]
    fn a_url_hard_wrapped_by_a_tui_is_joined_across_its_indent() {
        let rows = screen(&[
            "  See (https://example.com/pull/1/files#",
            "  diff-0123) here.",
        ]);
        let link = link_on_screen(&rows, 1, 3, Path::new("/")).unwrap();
        assert_eq!(
            link.action,
            LinkAction::Url("https://example.com/pull/1/files#diff-0123".to_string())
        );
        assert_eq!(link.cells.first(), Some(&(0, 7)));
        assert_eq!(link.cells.last(), Some(&(1, 10)));
    }

    #[test]
    fn a_full_width_row_above_never_hides_a_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        let rule = "─".repeat(COLS);
        let rows = screen(&[&rule, "a.rs:3 ok"]);
        let link = link_on_screen(&rows, 1, 1, dir.path()).unwrap();
        assert_eq!(
            link.action,
            LinkAction::File {
                path: dir.path().join("a.rs"),
                line: Some(3),
                column: None,
            }
        );
    }

    #[test]
    fn a_url_rendered_as_a_markdown_link_is_found_past_its_bracket() {
        for line in [
            "docs (https://example.com/a).",
            "[docs](https://example.com/a)",
        ] {
            let rows = screen(&[line]);
            let col = col_of(line, "example");
            let link = link_on_screen(&rows, 0, col, Path::new("/")).unwrap();
            assert_eq!(
                link.action,
                LinkAction::Url("https://example.com/a".to_string())
            );
        }
    }

    #[test]
    fn a_path_wrapped_in_a_tool_call_or_backticks_is_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        for line in ["Update(a.rs)", "edited `a.rs:12-30`."] {
            let rows = screen(&[line]);
            let link = link_on_screen(&rows, 0, col_of(line, "a.rs"), dir.path()).unwrap();
            assert!(
                matches!(&link.action, LinkAction::File { path, .. } if path == &dir.path().join("a.rs")),
                "{line}: {:?}",
                link.action
            );
        }
    }

    #[test]
    fn a_line_range_suffix_opens_at_its_first_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        let link = link_on_screen(&screen(&["a.rs:12-30"]), 0, 0, dir.path()).unwrap();
        assert_eq!(
            link.action,
            LinkAction::File {
                path: dir.path().join("a.rs"),
                line: Some(12),
                column: None,
            }
        );
    }

    #[test]
    fn an_existing_folder_is_a_folder_link() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        let link = link_on_screen(&screen(&["folder src/ ok"]), 0, 8, dir.path()).unwrap();
        assert_eq!(link.action, LinkAction::Folder(dir.path().join("src")));
        assert_eq!(link.cells, [(0, 7), (0, 8), (0, 9), (0, 10)]);
    }

    #[test]
    fn each_editor_opens_a_folder_with_its_binary_alone() {
        let folder = Path::new("/src");
        assert_eq!(
            folder_argv(Editor::VsCode.template(), folder),
            ["code", "/src"]
        );
        assert_eq!(
            folder_argv(Editor::Cursor.template(), folder),
            ["cursor", "/src"]
        );
        assert_eq!(folder_argv(Editor::Zed.template(), folder), ["zed", "/src"]);
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
