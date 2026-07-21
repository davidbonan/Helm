use std::path::Path;

use crate::git::diff::{self, DiffSource, Hunk, LineOrigin};
use crate::git::status;

/// Reloads the repo's in-memory index from disk before a mutation. The worker
/// holds one long-lived handle and libgit2 only refreshes its cached index
/// inside status passes: an index written by another process since the last
/// poll (git in a terminal pane) would otherwise be silently clobbered by the
/// next write — cf. `stage_sees_index_changes_made_by_another_handle`. The read
/// is **forced**: a soft read is a no-op when the on-disk index has not moved,
/// which would let entries a failed mutation left in memory ride into the next
/// `write_tree` — cf. `a_failed_stage_all_never_leaks_into_the_next_commit`.
pub(crate) fn fresh_index(repo: &git2::Repository) -> Result<git2::Index, git2::Error> {
    let mut index = repo.index()?;
    index.read(true)?;
    Ok(index)
}

pub fn stage(repo: &git2::Repository, path: &str) -> Result<(), git2::Error> {
    let mut index = fresh_index(repo)?;
    let rel = Path::new(path);
    // `exists()` follows the link: a symlink repointed at a target that does
    // not exist yet would read as removed and stage its deletion, where
    // `stage_all` (switching on the delta status) stages the modification.
    let exists = repo
        .workdir()
        .map(|wd| wd.join(rel).symlink_metadata().is_ok())
        .unwrap_or(false);
    if exists {
        // New path of a detected rename (not yet in the index): the old path —
        // deleted from the working tree, still in the index — leaves with it.
        // Staging only the addition would leave the deletion behind as an
        // unstaged residue.
        if index.get_path(rel, 0).is_none() {
            if let Some(old) = status::rename_old_path(repo, path, false)? {
                index.remove_path(Path::new(&old))?;
            }
        }
        index.add_path(rel)?;
    } else {
        index.remove_path(rel)?;
    }
    index.write()
}

pub fn unstage(repo: &git2::Repository, path: &str) -> Result<(), git2::Error> {
    // `reset_default` mutates the cached index too: refresh it first.
    fresh_index(repo)?;
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.peel(git2::ObjectType::Commit).ok());
    // New path of a staged rename (absent from HEAD): both sides reset
    // together, otherwise the old path's deletion stays staged.
    let renamed_from = match &head {
        Some(commit) if head_lacks(commit, path) => status::rename_old_path(repo, path, true)?,
        _ => None,
    };
    match renamed_from {
        Some(old) => repo.reset_default(head.as_ref(), [path, old.as_str()]),
        None => repo.reset_default(head.as_ref(), [path]),
    }
}

fn head_lacks(head: &git2::Object, path: &str) -> bool {
    head.as_commit()
        .and_then(|commit| commit.tree().ok())
        .map(|tree| tree.get_path(Path::new(path)).is_err())
        .unwrap_or(false)
}

/// Stages every unstaged change in one pass: one status enumeration (no line
/// stats) and a **single index write**, where per-file staging paid both per
/// file. Conflicts are skipped (read only, git.md §2); a rename moves whole
/// (old path removed with the new one added). An entry libgit2 refuses (a plain
/// clone nested in the workdir, reported as one untracked directory) does not
/// abort the batch: the failure is reported once every other entry has been
/// staged and written.
pub fn stage_all(repo: &git2::Repository) -> Result<(), git2::Error> {
    let statuses = status::work_statuses(repo)?;
    let nested = crate::git::worktree::nested_in_workdir(repo);
    let mut index = fresh_index(repo)?;
    let mut touched = false;
    let mut failed: Option<git2::Error> = None;
    for entry in statuses.iter() {
        if entry.status().contains(git2::Status::CONFLICTED) {
            continue;
        }
        let Some(delta) = entry.index_to_workdir() else {
            continue;
        };
        let new = delta.new_file().path();
        let old = delta.old_file().path();
        if new.or(old).is_some_and(|p| nested.contains(p)) {
            continue;
        }
        let applied = match delta.status() {
            git2::Delta::Untracked | git2::Delta::Modified | git2::Delta::Typechange => {
                let Some(path) = new.or(old) else { continue };
                index.add_path(path)
            }
            git2::Delta::Deleted => {
                let Some(path) = old.or(new) else { continue };
                index.remove_path(path)
            }
            git2::Delta::Renamed => {
                let (Some(old), Some(new)) = (old, new) else {
                    continue;
                };
                match index.remove_path(old) {
                    Ok(()) => index.add_path(new),
                    Err(err) => Err(err),
                }
            }
            _ => continue,
        };
        match applied {
            Ok(()) => touched = true,
            Err(err) => {
                failed.get_or_insert(err);
            }
        }
    }
    if touched {
        index.write()?;
    }
    match failed {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Unstages every staged change in one `reset_default` (a single index write);
/// a rename resets both of its sides.
pub fn unstage_all(repo: &git2::Repository) -> Result<(), git2::Error> {
    let statuses = status::work_statuses(repo)?;
    let mut paths: Vec<String> = Vec::new();
    for entry in statuses.iter() {
        let Some(delta) = entry.head_to_index() else {
            continue;
        };
        let new = delta.new_file().path().and_then(|p| p.to_str());
        let old = delta.old_file().path().and_then(|p| p.to_str());
        match delta.status() {
            git2::Delta::Added
            | git2::Delta::Modified
            | git2::Delta::Deleted
            | git2::Delta::Typechange => {
                paths.extend(new.or(old).map(str::to_owned));
            }
            git2::Delta::Renamed => {
                paths.extend(old.map(str::to_owned));
                paths.extend(new.map(str::to_owned));
            }
            _ => {}
        }
    }
    if paths.is_empty() {
        return Ok(());
    }
    fresh_index(repo)?;
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.peel(git2::ObjectType::Commit).ok());
    repo.reset_default(head.as_ref(), paths)
}

/// Stages hunk `hunk_index` of a file without touching the rest of its changes.
/// Mechanism (git.md §4): compute the working tree vs index diff (Unstaged
/// section), extract from it a **filtered patch** containing only that hunk, then
/// apply it to the index via `Repository::apply(ApplyLocation::Index)`.
pub fn stage_hunk(
    repo: &git2::Repository,
    path: &str,
    hunk_index: usize,
) -> Result<(), git2::Error> {
    apply_filtered(repo, path, DiffSource::Unstaged, hunk_index, None, false)
}

/// Unstages hunk `hunk_index`. Inverse of `stage_hunk`: start from the index vs
/// HEAD diff (Staged section) and apply the hunk's **reversed** patch to the
/// index, which brings the index back toward HEAD for that hunk alone.
pub fn unstage_hunk(
    repo: &git2::Repository,
    path: &str,
    hunk_index: usize,
) -> Result<(), git2::Error> {
    apply_filtered(repo, path, DiffSource::Staged, hunk_index, None, true)
}

/// Discards hunk `hunk_index` of an unstaged file: reverts that hunk's working
/// tree change back to the index content, leaving the other hunks and any
/// already-staged portion untouched. Mechanism (git.md §4): render the hunk's
/// **reversed** Unstaged patch and apply it to the working tree
/// (`Repository::apply(ApplyLocation::WorkDir)`) — the working-dir twin of
/// `unstage_hunk`, which reverse-applies to the index. Destructive: the UI puts
/// it behind a confirmation. Offered for unstaged hunks only.
pub fn discard_hunk(
    repo: &git2::Repository,
    path: &str,
    hunk_index: usize,
) -> Result<(), git2::Error> {
    // The Unstaged diff is computed against the cached index: refresh it so a
    // stage done in a terminal pane since the last poll is the baseline we
    // revert to, not a stale snapshot (cf. `fresh_index`).
    fresh_index(repo)?;
    let file = diff::file_diff(repo, path, DiffSource::Unstaged)?;
    let hunk = file
        .hunks
        .get(hunk_index)
        .ok_or_else(|| git2::Error::from_str("hunk index out of range"))?;
    if is_whole_file_add_or_delete(hunk) {
        // A whole-file addition (untracked) or deletion has no index side to
        // reverse onto: discarding the hunk is discarding the file (delete from
        // disk / restore from the index), exactly the file-level Discard.
        return crate::git::discard::discard_file(repo, path);
    }
    let raw = raw_lines(repo, &file.path, DiffSource::Unstaged, hunk_index, hunk)?;
    // No rename header: the hunk is reverted **in place** in the working tree,
    // the move itself is what the file-level Discard undoes (git.md §4).
    let Some(rendered) = render_hunk_patch(&file.path, hunk, &raw, None, true, None, None) else {
        return Ok(());
    };
    let parsed = git2::Diff::from_buffer(&rendered)?;
    repo.apply(&parsed, git2::ApplyLocation::WorkDir, None)
}

/// Stages only the lines `line_indices` (indices into `hunk.lines`) of hunk
/// `hunk_index`. The hunk is re-split into a sub-hunk: only the chosen lines stay
/// as changes, unchosen additions are dropped and unchosen deletions revert to
/// context (git.md §4).
pub fn stage_lines(
    repo: &git2::Repository,
    path: &str,
    hunk_index: usize,
    line_indices: &[usize],
) -> Result<(), git2::Error> {
    apply_filtered(
        repo,
        path,
        DiffSource::Unstaged,
        hunk_index,
        Some(line_indices),
        false,
    )
}

pub fn unstage_lines(
    repo: &git2::Repository,
    path: &str,
    hunk_index: usize,
    line_indices: &[usize],
) -> Result<(), git2::Error> {
    apply_filtered(
        repo,
        path,
        DiffSource::Staged,
        hunk_index,
        Some(line_indices),
        true,
    )
}

fn apply_filtered(
    repo: &git2::Repository,
    path: &str,
    source: DiffSource,
    hunk_index: usize,
    line_indices: Option<&[usize]>,
    reverse: bool,
) -> Result<(), git2::Error> {
    // A symlink has no sub-file selection to speak of: its "content" is the target
    // path, and the diff shows what the link points **at** (`diff::workdir_bytes`
    // reads through it), so a filtered patch would record a regular blob holding
    // the target's lines. The link is staged or unstaged whole, as `stage` does.
    if is_symlink(repo, path) {
        return match source {
            DiffSource::Unstaged => stage(repo, path),
            DiffSource::Staged => unstage(repo, path),
        };
    }
    let file = diff::file_diff(repo, path, source)?;
    let hunk = file
        .hunks
        .get(hunk_index)
        .ok_or_else(|| git2::Error::from_str("hunk index out of range"))?;
    if covers_every_change(hunk, line_indices) && is_whole_file_add_or_delete(hunk) {
        // Without a /dev/null header, libgit2 fails on an untracked file or
        // leaves an empty blob staged (including when unstaging the last
        // remaining line of an added file).
        return match source {
            DiffSource::Unstaged => stage(repo, path),
            DiffSource::Staged => unstage(repo, path),
        };
    }
    let raw = raw_lines(repo, &file.path, source, hunk_index, hunk)?;
    // File absent from the index (untracked, or whole-file deletion staged):
    // the patch must create it, otherwise `apply` fails with "index does not
    // contain <path>". `fresh_index` also covers the `repo.apply` below, which
    // writes through the same cached index.
    let indexed = fresh_index(repo)?.get_path(Path::new(path), 0);
    let new_file = indexed.is_none();
    // …unless it is the new side of an unstaged rename: the preimage the hunk
    // applies to is the **old** path's index entry, so the patch must move it
    // (libgit2 applies a `RENAMED` delta by reading the old path and dropping it
    // from the index). Declaring a new file instead makes `apply` reject the
    // hunk's context lines.
    let renamed_from = match (source, new_file) {
        (DiffSource::Unstaged, true) => status::rename_old_path(repo, path, false)?,
        _ => None,
    };
    let mode = match &indexed {
        Some(entry) => PatchMode::Tracked(blob_mode(entry.mode)),
        None => PatchMode::New(worktree_file_mode(repo, path)),
    };
    let Some(rendered) = render_hunk_patch(
        &file.path,
        hunk,
        &raw,
        line_indices,
        reverse,
        Some(mode),
        renamed_from.as_deref(),
    ) else {
        return Ok(());
    };
    let parsed = git2::Diff::from_buffer(&rendered)?;
    repo.apply(&parsed, git2::ApplyLocation::Index, None)
}

/// The hunk's lines as the bytes they really are, one entry per `hunk.lines`
/// entry (`diff::hunk_line_bytes` applies the same `line_origin` filter). A
/// length mismatch means the diff moved between the two reads: refuse rather than
/// apply a patch whose selection indices have shifted.
fn raw_lines(
    repo: &git2::Repository,
    path: &str,
    source: DiffSource,
    hunk_index: usize,
    hunk: &Hunk,
) -> Result<Vec<Vec<u8>>, git2::Error> {
    let raw = diff::hunk_line_bytes(repo, path, source, hunk_index)?;
    if raw.len() != hunk.lines.len() {
        return Err(git2::Error::from_str(
            "the diff changed while it was being staged",
        ));
    }
    Ok(raw)
}

/// `true` if the selection covers every changed line of the hunk (or if there is
/// no selection): the filtered patch then equals the whole hunk.
fn covers_every_change(hunk: &Hunk, line_indices: Option<&[usize]>) -> bool {
    line_indices.is_none_or(|sel| {
        hunk.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.origin != LineOrigin::Context)
            .all(|(idx, _)| sel.contains(&idx))
    })
}

fn is_whole_file_add_or_delete(hunk: &Hunk) -> bool {
    !hunk.lines.is_empty()
        && ((hunk.old_lines == 0 && hunk.lines.iter().all(|l| l.origin == LineOrigin::Addition))
            || (hunk.new_lines == 0 && hunk.lines.iter().all(|l| l.origin == LineOrigin::Deletion)))
}

/// Mode the `new file mode` header must declare, mirroring what `index.add_path`
/// would record for the same file: the working tree's exec bit, unless
/// `core.filemode` is off (a filesystem that cannot carry it), where git records
/// `100644` regardless. `symlink_metadata` so a link is not read through to its
/// target (cf. `stage`); a link staged this way keeps the previous `100644`, its
/// own mode being out of this path's reach.
fn worktree_file_mode(repo: &git2::Repository, path: &str) -> &'static str {
    use std::os::unix::fs::PermissionsExt;
    let honored = repo
        .config()
        .and_then(|c| c.get_bool("core.filemode"))
        .unwrap_or(true);
    let executable = repo
        .workdir()
        .and_then(|wd| wd.join(path).symlink_metadata().ok())
        .is_some_and(|md| md.is_file() && md.permissions().mode() & 0o111 != 0);
    if honored && executable {
        "100755"
    } else {
        "100644"
    }
}

/// What the filtered patch has to say about the file's mode.
#[derive(Debug, Clone, Copy)]
enum PatchMode<'a> {
    /// Absent from the index: the patch must create it, with the working tree's
    /// mode (`worktree_file_mode`).
    New(&'a str),
    /// Already indexed: the entry's mode is restated on an `index` line, because a
    /// patch that says nothing about the mode makes libgit2 re-record the entry
    /// with the default blob mode — an exec bit silently dropped on every
    /// hunk-by-hunk stage of a tracked script.
    Tracked(&'a str),
}

/// The mode an index entry declares, as the patch header spells it.
fn blob_mode(mode: u32) -> &'static str {
    match mode {
        0o100755 => "100755",
        0o120000 => "120000",
        _ => "100644",
    }
}

fn is_symlink(repo: &git2::Repository, path: &str) -> bool {
    repo.workdir()
        .and_then(|wd| wd.join(path).symlink_metadata().ok())
        .is_some_and(|md| md.is_symlink())
}

fn render_hunk_patch(
    path: &str,
    hunk: &Hunk,
    raw: &[Vec<u8>],
    line_indices: Option<&[usize]>,
    reverse: bool,
    mode: Option<PatchMode>,
    renamed_from: Option<&str>,
) -> Option<Vec<u8>> {
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    let mut has_change = false;
    let mut emitted: Vec<(LineOrigin, &[u8])> = Vec::new();

    for (idx, (line, bytes)) in hunk.lines.iter().zip(raw).enumerate() {
        let selected = line_indices.is_none_or(|sel| sel.contains(&idx));
        let Some(effective) = effective_origin(line.origin, selected, reverse) else {
            continue;
        };
        match effective {
            LineOrigin::Context => {
                old_count += 1;
                new_count += 1;
            }
            LineOrigin::Deletion => {
                old_count += 1;
                has_change = true;
            }
            LineOrigin::Addition => {
                new_count += 1;
                has_change = true;
            }
        }
        emitted.push((effective, bytes.as_slice()));
    }

    if !has_change {
        return None;
    }

    if reverse {
        old_side_first(&mut emitted);
    }
    let mut body: Vec<u8> = Vec::new();
    for (origin, bytes) in &emitted {
        push_line(&mut body, patch_prefix(*origin, reverse), bytes);
    }

    let old_start = if reverse {
        hunk.new_start
    } else {
        hunk.old_start
    };
    let (old_count, new_count) = if reverse {
        (new_count, old_count)
    } else {
        (old_count, new_count)
    };
    let new_start = isolated_new_start(old_start, old_count, new_count);

    let mut patch = String::new();
    match renamed_from {
        // libgit2's parser only reaches `rename from` through a `similarity
        // index` line and then takes the two rename paths verbatim, with no
        // `---`/`+++` pair. The percentage is metadata `apply` never reads.
        Some(old) => {
            patch.push_str(&format!("diff --git a/{old} b/{path}\n"));
            patch.push_str("similarity index 100%\n");
            patch.push_str(&format!("rename from {old}\n"));
            patch.push_str(&format!("rename to {path}\n"));
        }
        None => {
            patch.push_str(&format!("diff --git a/{path} b/{path}\n"));
            match mode {
                Some(PatchMode::New(mode)) => {
                    patch.push_str(&format!("new file mode {mode}\n"));
                    patch.push_str("--- /dev/null\n");
                }
                Some(PatchMode::Tracked(mode)) => {
                    // The oids are metadata `apply` never reads — only the mode is.
                    patch.push_str(&format!("index 0000000..0000000 {mode}\n"));
                    patch.push_str(&format!("--- a/{path}\n"));
                }
                None => patch.push_str(&format!("--- a/{path}\n")),
            }
            patch.push_str(&format!("+++ b/{path}\n"));
        }
    }
    patch.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        old_start, old_count, new_start, new_count
    ));
    let mut patch = patch.into_bytes();
    patch.extend_from_slice(&body);
    Some(patch)
}

/// Origin of a line **in the filtered patch**: an unselected line must stay as-is
/// on the side we modify (the index). In the stage direction (`!reverse`), an
/// unchosen deletion is still in the index → context; an unchosen addition is not
/// → dropped. In the unstage direction (`reverse`), it is the opposite: the
/// unchosen addition is in the index → context; the unchosen deletion (HEAD's
/// content) is not → dropped.
fn effective_origin(origin: LineOrigin, selected: bool, reverse: bool) -> Option<LineOrigin> {
    match origin {
        LineOrigin::Context => Some(LineOrigin::Context),
        LineOrigin::Addition if selected => Some(LineOrigin::Addition),
        LineOrigin::Deletion if selected => Some(LineOrigin::Deletion),
        LineOrigin::Addition if reverse => Some(LineOrigin::Context),
        LineOrigin::Addition => None,
        LineOrigin::Deletion if reverse => None,
        LineOrigin::Deletion => Some(LineOrigin::Context),
    }
}

fn isolated_new_start(old_start: u32, old_count: u32, new_count: u32) -> u32 {
    if old_count == 0 && new_count > 0 {
        old_start.saturating_add(1)
    } else if new_count == 0 {
        old_start.saturating_sub(1)
    } else {
        old_start
    }
}

fn patch_prefix(origin: LineOrigin, reverse: bool) -> u8 {
    let origin = if reverse { flip(origin) } else { origin };
    match origin {
        LineOrigin::Context => b' ',
        LineOrigin::Addition => b'+',
        LineOrigin::Deletion => b'-',
    }
}

fn flip(origin: LineOrigin) -> LineOrigin {
    match origin {
        LineOrigin::Context => LineOrigin::Context,
        LineOrigin::Addition => LineOrigin::Deletion,
        LineOrigin::Deletion => LineOrigin::Addition,
    }
}

/// Reorders each change block so the patch's **old** side comes first. Reversing
/// only flips the prefixes, so a block keeps the forward hunk's order — additions
/// (which become the reversed patch's deletions) after the deletions. That is
/// harmless until a side ends without a newline: `push_line` closes that line with
/// a `\ No newline at end of file` marker, which then sits between the two sides
/// instead of ending the body, and libgit2 rejects the whole hunk. Emitting the
/// forward additions first restores the shape `git diff -R` produces. Context
/// lines separate blocks and never move.
fn old_side_first(lines: &mut [(LineOrigin, &[u8])]) {
    let mut start = 0;
    while start < lines.len() {
        if lines[start].0 == LineOrigin::Context {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < lines.len() && lines[end].0 != LineOrigin::Context {
            end += 1;
        }
        lines[start..end].sort_by_key(|(origin, _)| match origin {
            LineOrigin::Addition => 0,
            _ => 1,
        });
        start = end;
    }
}

fn push_line(body: &mut Vec<u8>, prefix: u8, line: &[u8]) {
    body.push(prefix);
    body.extend_from_slice(line);
    if !line.ends_with(b"\n") {
        body.push(b'\n');
        body.extend_from_slice(b"\\ No newline at end of file\n");
    }
}
