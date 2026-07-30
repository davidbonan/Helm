use std::ops::Range;
#[cfg(test)]
use std::os::unix::fs::PermissionsExt;
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

/// Runs the entry checks without keeping the content, on the new-side bytes the caller
/// has already read: called while the diff is computed, so the caret appears on a click
/// without the UI thread touching the disk (architecture §3). The bytes come in rather
/// than being read here — the diff asks this question again on every poll, and the file
/// would be read a second time each round.
pub fn editable(repo: &git2::Repository, path: &str, bytes: &[u8]) -> Result<(), EditError> {
    entry(repo, path)?;
    text_of(bytes).map(|_| ())
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
    let content = spliced(&file, range, &request.replacement);
    write_atomic(&file.full, &content, file.perms).map_err(io)
}

struct Loaded {
    full: PathBuf,
    /// The file as it reads on disk, kept verbatim: everything outside the edited range
    /// goes back byte for byte.
    text: String,
    /// Its lines, terminators stripped — what the anchor's precondition compares.
    lines: Vec<String>,
    /// Byte offset each line starts at, so a splice can address the range in `text`.
    starts: Vec<usize>,
    eol: LineEnding,
    perms: std::fs::Permissions,
}

fn load(repo: &git2::Repository, path: &str) -> Result<Loaded, EditError> {
    let (full, perms) = entry(repo, path)?;
    let bytes = std::fs::read(&full).map_err(io)?;
    let eol = LineEnding::detect(&bytes);
    let text = text_of(&bytes)?.to_owned();
    let (lines, starts) = file_lines(&text);
    Ok(Loaded {
        full,
        text,
        lines,
        starts,
        eol,
        perms,
    })
}

/// The path checks and the metadata ones — everything decided before the content is
/// looked at. Returns the resolved path and the permissions a write must carry over.
fn entry(
    repo: &git2::Repository,
    path: &str,
) -> Result<(PathBuf, std::fs::Permissions), EditError> {
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
    Ok((full, perms))
}

/// Content that can back a buffer: text, and text alone. A NUL byte is valid UTF-8, so
/// it is ruled out on its own.
fn text_of(bytes: &[u8]) -> Result<&str, EditError> {
    if bytes.contains(&0) {
        return Err(EditError::Binary);
    }
    std::str::from_utf8(bytes).map_err(|_| EditError::Binary)
}

/// The file's lines with the byte offset each starts at, terminators stripped from the
/// lines themselves: the anchor's comparison and the buffer both stay LF, while the
/// offsets let a splice put the untouched part of the file back verbatim.
fn file_lines(text: &str) -> (Vec<String>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    let mut at = 0;
    while at < text.len() {
        starts.push(at);
        match text[at..].find('\n') {
            Some(offset) => {
                let line = &text[at..at + offset];
                lines.push(line.strip_suffix('\r').unwrap_or(line).to_owned());
                at += offset + 1;
            }
            None => {
                lines.push(text[at..].to_owned());
                at = text.len();
            }
        }
    }
    (lines, starts)
}

/// The editor buffer's lines. Unlike a file, a trailing `\n` here is a real empty
/// last line — the user typed it inside the hunk. An **emptied** buffer is no line at
/// all, not one empty line: selecting the whole range and deleting it deletes those
/// lines (git.md §4).
fn buffer_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

/// The file with `range` replaced by the buffer. Only the replaced span is rewritten:
/// the rest of the file is copied verbatim, so a file with **mixed** endings keeps every
/// line the edit did not touch (rewriting them all would turn a three-line edit into a
/// whole-file diff). The buffer itself takes the terminator the lines it replaces used.
fn spliced(file: &Loaded, range: Range<usize>, replacement: &str) -> String {
    let end = file.text.len();
    let head = file.starts.get(range.start).copied().unwrap_or(end);
    let tail = file.starts.get(range.end).copied().unwrap_or(end);
    let replaced = &file.text[head..tail];
    let crlf = if replaced.contains('\n') {
        replaced.contains("\r\n")
    } else {
        // An unterminated last line, or a pure insertion: nothing local to read it off.
        file.eol.crlf
    };
    let eol = if crlf { "\r\n" } else { "\n" };
    let lines = buffer_lines(replacement);
    let mut out = String::with_capacity(end + replacement.len());
    out.push_str(&file.text[..head]);
    if !lines.is_empty() {
        out.push_str(&lines.join(eol));
        // The tail starts at a line, so the buffer owes it a terminator; reaching the
        // end of the file instead, it follows the file's own final-newline policy.
        if tail < end || file.eol.final_newline {
            out.push_str(eol);
        }
    }
    out.push_str(&file.text[tail..]);
    out
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

    /// A `Loaded` over `text`, as `load` would build it — without a repo on disk.
    fn loaded(text: &str) -> Loaded {
        let (lines, starts) = file_lines(text);
        Loaded {
            full: PathBuf::from("f"),
            text: text.to_owned(),
            lines,
            starts,
            eol: LineEnding::detect(text.as_bytes()),
            perms: std::fs::Permissions::from_mode(0o644),
        }
    }

    #[test]
    fn file_lines_strips_the_final_newline_but_not_an_empty_last_line() {
        assert!(file_lines("").0.is_empty());
        assert_eq!(file_lines("a").0, lines("a"));
        assert_eq!(file_lines("a\n").0, lines("a"));
        assert_eq!(file_lines("a\nb").0, lines("a\nb"));
        assert_eq!(file_lines("a\n\n").0, lines("a\n"));
        assert_eq!(file_lines("\n").0, vec![String::new()]);
    }

    #[test]
    fn file_lines_folds_crlf_and_reports_where_each_line_starts() {
        let (lines, starts) = file_lines("a\r\nbb\r\nc");
        assert_eq!(lines, vec!["a", "bb", "c"]);
        assert_eq!(starts, vec![0, 3, 7]);
    }

    #[test]
    fn buffer_lines_counts_a_trailing_newline_as_an_empty_line() {
        assert_eq!(buffer_lines("a"), lines("a"));
        assert_eq!(buffer_lines("a\n"), lines("a\n"));
        assert!(
            buffer_lines("").is_empty(),
            "an emptied buffer is no line at all"
        );
    }

    #[test]
    fn spliced_replaces_the_range_in_place() {
        let file = loaded("one\ntwo\nthree\n");
        assert_eq!(spliced(&file, 1..2, "TWO"), "one\nTWO\nthree\n");
        assert_eq!(spliced(&file, 1..2, "a\nb"), "one\na\nb\nthree\n");
        assert_eq!(spliced(&file, 0..3, "only"), "only\n");
    }

    #[test]
    fn spliced_keeps_crlf_and_a_missing_final_newline() {
        assert_eq!(
            spliced(&loaded("one\r\ntwo\r\n"), 1..2, "TWO"),
            "one\r\nTWO\r\n"
        );
        assert_eq!(
            spliced(&loaded("one\r\ntwo\r\nthree\r\n"), 1..2, "a\nb"),
            "one\r\na\r\nb\r\nthree\r\n",
            "the buffer's own new lines take the terminator of what they replace"
        );
        assert_eq!(spliced(&loaded("one\ntwo"), 1..2, "TWO"), "one\nTWO");
        assert_eq!(spliced(&loaded("one\r\ntwo"), 1..2, "TWO"), "one\r\nTWO");
    }

    #[test]
    fn spliced_leaves_a_mixed_ending_file_alone_outside_the_range() {
        // `LineEnding::detect` samples the first newline only, so re-applying it to the
        // whole file would rewrite every other line — a one-line edit would land as a
        // whole-file diff.
        let file = loaded("a\r\nb\nc\r\n");
        assert_eq!(spliced(&file, 0..1, "A"), "A\r\nb\nc\r\n");
        assert_eq!(
            spliced(&file, 1..2, "B"),
            "a\r\nB\nc\r\n",
            "the replaced line's own terminator is what comes back"
        );
    }

    #[test]
    fn an_emptied_buffer_deletes_the_range() {
        let file = loaded("one\ntwo\nthree\n");
        assert_eq!(spliced(&file, 1..2, ""), "one\nthree\n");
        assert_eq!(
            spliced(&file, 0..3, ""),
            "",
            "emptying every line empties the file"
        );
        assert_eq!(spliced(&loaded("a\r\nb\r\n"), 0..1, ""), "b\r\n");
    }

    #[test]
    fn spliced_writes_into_an_empty_file() {
        assert_eq!(spliced(&loaded(""), 0..0, "new"), "new\n");
    }

    #[test]
    fn text_of_refuses_a_nul_byte_and_invalid_utf8() {
        assert_eq!(text_of(b"a\0b").unwrap_err(), EditError::Binary);
        assert_eq!(text_of(&[0xff, 0xfe]).unwrap_err(), EditError::Binary);
        assert_eq!(text_of(b"ok\n").unwrap(), "ok\n");
    }
}
