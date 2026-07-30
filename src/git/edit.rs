use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use crate::git::conflict::LineEnding;
use crate::git::stage;

/// A file past this size never opens for inline editing (git.md §4): the buffer is
/// one hunk, but a splice re-reads and rewrites the whole file. Deliberately well
/// above `diff::MAX_DIFF_BYTES` — that threshold measures the *patch*, so a large
/// file with a three-line change still renders a normal diff and must stay editable.
pub const MAX_EDIT_BYTES: u64 = 8 * 1024 * 1024;

/// Why a working-tree file cannot take an inline edit. The variants are what the UI
/// arbitrates on: `Diverged` offers *Reload* / *Overwrite*, the others are entry
/// refusals that point at the external editor (git.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// Bare repository, or a path escaping the working tree.
    OutsideWorkdir,
    /// Symlink, directory, device… — a symlink's diff shows its target, not text.
    NotRegular,
    /// Not valid UTF-8, or holds a NUL byte.
    Binary,
    /// Above `MAX_EDIT_BYTES`.
    TooLarge,
    /// The file carries no write bit.
    ReadOnly,
    /// The anchored lines are no longer the ones that were read: the file moved on
    /// disk. Nothing is written — the typed buffer outlives the refusal.
    Diverged,
    /// The write could not be carried out: an I/O failure, or a git call around it
    /// (unreadable repository, another operation in progress). Carries the message
    /// verbatim — it is already user-facing.
    Io(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            EditError::OutsideWorkdir => "this path is outside the working tree",
            EditError::NotRegular => "only a regular file can be edited here",
            EditError::Binary => "this file is not UTF-8 text",
            EditError::TooLarge => "this file is too large to edit here",
            EditError::ReadOnly => "this file is not writable",
            EditError::Diverged => "the file changed on disk",
            EditError::Io(err) => return f.write_str(err),
        };
        f.write_str(text)
    }
}

/// Where an inline edit landed (git.md §4). The section it was made from decides, so
/// the reply is what the UI reports on — never a silent choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing {
    /// Written to the working tree and left unstaged: the edit came from **Unstaged**.
    Unstaged,
    /// Written then staged at file level: the edit came from **Staged**.
    Staged,
    /// Written but left unstaged although it came from **Staged**, with the reason to
    /// show. The text is on disk either way — the save succeeded.
    NotStaged(String),
}

/// One inline-editor write: the anchor read when the caret appeared plus the buffer to
/// splice in. Travels UI → worker and rides back on the reply — a refusal has to be
/// actionable, and only the request itself says what to retry (git.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    pub path: String,
    /// Working-tree lines the buffer replaces: the file's own numbering, 0-based and
    /// end-exclusive.
    pub range: Range<usize>,
    /// Those lines as they read when the editor opened — the write's precondition.
    pub original: Vec<String>,
    /// The buffer, LF separated, one line per `\n`.
    pub replacement: String,
    /// The edit came from **Staged**: the write is followed by a file-level stage.
    pub stage_after: bool,
    /// Skips the precondition — the **Overwrite** answer to a divergence notice.
    pub force: bool,
}

/// One inline-editor save: the write, then the file-level stage that keeps a **Staged**
/// edit in its section (git.md §4). Called on the worker under the mutation lock.
pub fn flush(repo: &git2::Repository, request: &EditRequest) -> Result<Landing, EditError> {
    let path = request.path.as_str();
    // Judged **before** the write: the write is itself an unstaged change, so
    // afterwards every file looks like one that must not be staged wholesale.
    let refusal = request
        .stage_after
        .then(|| stage_refusal(repo, path))
        .flatten();
    write_range(repo, request)?;
    if !request.stage_after {
        return Ok(Landing::Unstaged);
    }
    if let Some(reason) = refusal {
        return Ok(Landing::NotStaged(reason));
    }
    match stage::stage(repo, path) {
        Ok(()) => Ok(Landing::Staged),
        // The text is already on disk: a failed stage is reported, not raised —
        // raising it would send the editor into a retry whose anchor no longer
        // matches what it just wrote.
        Err(err) => Ok(Landing::NotStaged(err.message().to_owned())),
    }
}

/// Why the file-level stage must be skipped, `None` when it may run: staging from the
/// Staged section is only safe while index == working tree (git.md §4), and that
/// precondition is re-checked here — the editor opened on a snapshot that is now old.
/// The same rule decides whether the Staged side offers a caret at all, so
/// `diff::file_diff` asks it too rather than restating it.
pub fn stage_refusal(repo: &git2::Repository, path: &str) -> Option<String> {
    match repo.status_file(Path::new(path)) {
        Ok(status) => status
            .intersects(
                git2::Status::WT_MODIFIED
                    | git2::Status::WT_NEW
                    | git2::Status::WT_DELETED
                    | git2::Status::WT_TYPECHANGE
                    | git2::Status::WT_RENAMED,
            )
            .then(|| "the file also has unstaged changes".to_owned()),
        // The user's text is never held hostage to a status read: an unreadable
        // status costs the automatic stage, not the save.
        Err(err) => Some(err.message().to_owned()),
    }
}

/// Runs the entry checks alone, without keeping the content: called while the diff
/// is computed so the caret can appear on click without touching the disk from the
/// UI thread (architecture §3).
pub fn editable(repo: &git2::Repository, path: &str) -> Result<(), EditError> {
    load(repo, path).map(|_| ())
}

/// Replaces the working-tree lines under the request's range with its buffer, provided
/// they still read exactly as its original lines (git.md §4) — unless `force`, the
/// **Overwrite** answer to a divergence notice, which keeps the range and drops the
/// comparison. The file's terminator and final-newline policy are re-applied on the way
/// out.
pub fn write_range(repo: &git2::Repository, request: &EditRequest) -> Result<(), EditError> {
    let file = load(repo, &request.path)?;
    let range = request.range.clone();
    if range.start > range.end || range.end > file.lines.len() {
        return Err(EditError::Diverged);
    }
    if !request.force && file.lines[range.clone()] != *request.original {
        return Err(EditError::Diverged);
    }
    let content = spliced(&file.lines, range, &request.replacement, &file.eol);
    write_atomic(&file.full, &content, file.perms).map_err(io)
}

struct Loaded {
    full: PathBuf,
    lines: Vec<String>,
    eol: LineEnding,
    perms: std::fs::Permissions,
}

fn load(repo: &git2::Repository, path: &str) -> Result<Loaded, EditError> {
    let workdir = repo.workdir().ok_or(EditError::OutsideWorkdir)?;
    let rel = Path::new(path);
    if rel.is_absolute() || rel.components().any(|c| c == Component::ParentDir) {
        return Err(EditError::OutsideWorkdir);
    }
    let full = workdir.join(rel);
    // Never `metadata`: it follows the link, and a symlink would then be judged on
    // whatever it points at before being overwritten through (cf. `conflict`'s
    // `remove_resolution`).
    let meta = std::fs::symlink_metadata(&full).map_err(io)?;
    if !meta.is_file() {
        return Err(EditError::NotRegular);
    }
    if meta.len() > MAX_EDIT_BYTES {
        return Err(EditError::TooLarge);
    }
    let perms = meta.permissions();
    if perms.readonly() {
        return Err(EditError::ReadOnly);
    }
    let bytes = std::fs::read(&full).map_err(io)?;
    if bytes.contains(&0) {
        return Err(EditError::Binary);
    }
    let eol = LineEnding::detect(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| EditError::Binary)?;
    Ok(Loaded {
        full,
        lines: file_lines(&text),
        eol,
        perms,
    })
}

/// The file's lines, terminators stripped. CRLF folds to LF here and comes back at
/// compose time, so the anchor's comparison and the buffer both stay LF.
fn file_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

/// The editor buffer's lines. Unlike a file, a trailing `\n` here is a real empty
/// last line — the user typed it inside the hunk.
fn buffer_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

fn compose(lines: &[String], eol: &LineEnding) -> String {
    let mut text = lines.join("\n");
    if eol.final_newline && !lines.is_empty() {
        text.push('\n');
    }
    eol.apply(&text)
}

fn spliced(lines: &[String], range: Range<usize>, replacement: &str, eol: &LineEnding) -> String {
    let mut out = lines[..range.start].to_vec();
    out.extend(buffer_lines(replacement));
    out.extend_from_slice(&lines[range.end..]);
    compose(&out, eol)
}

/// Lands `content` through a sibling temporary file: a write that fails halfway
/// (full disk, killed process) must never leave the user's file truncated. The
/// rename brings a new inode, so the original permissions ride along explicitly.
fn write_atomic(full: &Path, content: &str, perms: std::fs::Permissions) -> std::io::Result<()> {
    let dir = full.parent().unwrap_or_else(|| Path::new("."));
    let name = full
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.helm-{}.tmp", std::process::id()));
    let landed = std::fs::write(&tmp, content)
        .and_then(|()| std::fs::set_permissions(&tmp, perms))
        .and_then(|()| std::fs::rename(&tmp, full));
    if landed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    landed
}

fn io(err: std::io::Error) -> EditError {
    EditError::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    #[test]
    fn file_lines_strips_the_final_newline_but_not_an_empty_last_line() {
        assert!(file_lines("").is_empty());
        assert_eq!(file_lines("a"), lines("a"));
        assert_eq!(file_lines("a\n"), lines("a"));
        assert_eq!(file_lines("a\nb"), lines("a\nb"));
        assert_eq!(file_lines("a\n\n"), lines("a\n"));
        assert_eq!(file_lines("\n"), vec![String::new()]);
    }

    #[test]
    fn file_lines_folds_crlf() {
        assert_eq!(file_lines("a\r\nb\r\n"), lines("a\nb"));
    }

    #[test]
    fn buffer_lines_counts_a_trailing_newline_as_an_empty_line() {
        assert_eq!(buffer_lines("a"), lines("a"));
        assert_eq!(buffer_lines("a\n"), lines("a\n"));
        assert_eq!(buffer_lines(""), vec![String::new()]);
    }

    #[test]
    fn spliced_replaces_the_range_in_place() {
        let file = lines("one\ntwo\nthree");
        let eol = LineEnding {
            crlf: false,
            final_newline: true,
        };
        assert_eq!(spliced(&file, 1..2, "TWO", &eol), "one\nTWO\nthree\n");
        assert_eq!(spliced(&file, 1..2, "a\nb", &eol), "one\na\nb\nthree\n");
        assert_eq!(spliced(&file, 0..3, "only", &eol), "only\n");
    }

    #[test]
    fn spliced_keeps_crlf_and_a_missing_final_newline() {
        let file = lines("one\ntwo");
        let crlf = LineEnding {
            crlf: true,
            final_newline: false,
        };
        assert_eq!(spliced(&file, 1..2, "TWO", &crlf), "one\r\nTWO");
        let lf_unterminated = LineEnding {
            crlf: false,
            final_newline: false,
        };
        assert_eq!(spliced(&file, 1..2, "TWO", &lf_unterminated), "one\nTWO");
    }

    #[test]
    fn spliced_turns_an_emptied_buffer_into_one_empty_line() {
        let file = lines("one\ntwo\nthree");
        let eol = LineEnding {
            crlf: false,
            final_newline: true,
        };
        assert_eq!(spliced(&file, 1..2, "", &eol), "one\n\nthree\n");
    }

    #[test]
    fn compose_leaves_an_empty_file_empty() {
        assert_eq!(compose(&[], &LineEnding::default()), "");
    }
}
