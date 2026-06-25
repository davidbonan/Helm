use std::path::Path;

use crate::git::diff::{self, DiffLine, DiffSource, Hunk, LineOrigin};
use crate::git::status;

/// Soft-reloads the repo's in-memory index from disk before a mutation. The
/// worker holds one long-lived handle and libgit2 only refreshes its cached
/// index inside status passes: an index written by another process since the
/// last poll (git in a terminal pane) would otherwise be silently clobbered by
/// the next write — cf. `stage_sees_index_changes_made_by_another_handle`.
pub(crate) fn fresh_index(repo: &git2::Repository) -> Result<git2::Index, git2::Error> {
    let mut index = repo.index()?;
    index.read(false)?;
    Ok(index)
}

pub fn stage(repo: &git2::Repository, path: &str) -> Result<(), git2::Error> {
    let mut index = fresh_index(repo)?;
    let rel = Path::new(path);
    let exists = repo
        .workdir()
        .map(|wd| wd.join(rel).exists())
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
/// (old path removed with the new one added).
pub fn stage_all(repo: &git2::Repository) -> Result<(), git2::Error> {
    let statuses = status::work_statuses(repo)?;
    let nested = crate::git::worktree::nested_in_workdir(repo);
    let mut index = fresh_index(repo)?;
    let mut touched = false;
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
        match delta.status() {
            git2::Delta::Untracked | git2::Delta::Modified | git2::Delta::Typechange => {
                let Some(path) = new.or(old) else { continue };
                index.add_path(path)?;
            }
            git2::Delta::Deleted => {
                let Some(path) = old.or(new) else { continue };
                index.remove_path(path)?;
            }
            git2::Delta::Renamed => {
                let (Some(old), Some(new)) = (old, new) else {
                    continue;
                };
                index.remove_path(old)?;
                index.add_path(new)?;
            }
            _ => continue,
        }
        touched = true;
    }
    if touched {
        index.write()?;
    }
    Ok(())
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
    let Some(rendered) = render_hunk_patch(&file.path, hunk, None, true, false) else {
        return Ok(());
    };
    let parsed = git2::Diff::from_buffer(rendered.as_bytes())?;
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
    // File absent from the index (untracked, or whole-file deletion staged):
    // the patch must create it, otherwise `apply` fails with "index does not
    // contain <path>". `fresh_index` also covers the `repo.apply` below, which
    // writes through the same cached index.
    let new_file = fresh_index(repo)?.get_path(Path::new(path), 0).is_none();
    let Some(rendered) = render_hunk_patch(&file.path, hunk, line_indices, reverse, new_file)
    else {
        return Ok(());
    };
    let parsed = git2::Diff::from_buffer(rendered.as_bytes())?;
    repo.apply(&parsed, git2::ApplyLocation::Index, None)
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

fn render_hunk_patch(
    path: &str,
    hunk: &Hunk,
    line_indices: Option<&[usize]>,
    reverse: bool,
    new_file: bool,
) -> Option<String> {
    let mut body = String::new();
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    let mut has_change = false;

    for (idx, line) in hunk.lines.iter().enumerate() {
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
        push_line(&mut body, patch_prefix(effective, reverse), line);
    }

    if !has_change {
        return None;
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
    patch.push_str(&format!("diff --git a/{path} b/{path}\n"));
    if new_file {
        patch.push_str("new file mode 100644\n");
        patch.push_str("--- /dev/null\n");
    } else {
        patch.push_str(&format!("--- a/{path}\n"));
    }
    patch.push_str(&format!("+++ b/{path}\n"));
    patch.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        old_start, old_count, new_start, new_count
    ));
    patch.push_str(&body);
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

fn patch_prefix(origin: LineOrigin, reverse: bool) -> char {
    let origin = if reverse { flip(origin) } else { origin };
    match origin {
        LineOrigin::Context => ' ',
        LineOrigin::Addition => '+',
        LineOrigin::Deletion => '-',
    }
}

fn flip(origin: LineOrigin) -> LineOrigin {
    match origin {
        LineOrigin::Context => LineOrigin::Context,
        LineOrigin::Addition => LineOrigin::Deletion,
        LineOrigin::Deletion => LineOrigin::Addition,
    }
}

fn push_line(body: &mut String, prefix: char, line: &DiffLine) {
    body.push(prefix);
    body.push_str(&line.content);
    if !line.content.ends_with('\n') {
        body.push('\n');
        body.push_str("\\ No newline at end of file\n");
    }
}
