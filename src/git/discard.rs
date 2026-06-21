use std::path::Path;

use crate::git::status;

/// Reverts a file's **working-tree** change (unstaged delta): an untracked file is
/// removed from disk, a tracked file is restored from the index. Any already-staged
/// portion is **preserved** (unstaging remains the inverse of `stage`). Destructive:
/// the UI puts it behind a confirmation (git.md §3).
pub fn discard_file(repo: &git2::Repository, path: &str) -> Result<(), git2::Error> {
    let file_status = repo.status_file(Path::new(path))?;
    if file_status.contains(git2::Status::WT_NEW) {
        // New path of a detected rename: the old path is restored from the
        // index with the deletion — removing the new file alone would leave
        // the move half-discarded. Looked up before the file disappears.
        let renamed_from = status::rename_old_path(repo, path, false)?;
        remove_from_disk(repo, path)?;
        return checkout_paths(repo, renamed_from.as_deref());
    }
    checkout_paths(repo, [path])
}

pub fn discard_all(repo: &git2::Repository) -> Result<(), git2::Error> {
    // Enumeration without line stats: `load_repo` paid a `Patch` per changed
    // file just to list the paths.
    let statuses = status::work_statuses(repo)?;
    let mut restore: Vec<String> = Vec::new();
    for entry in statuses.iter() {
        if entry.status().contains(git2::Status::CONFLICTED) {
            continue;
        }
        let Some(delta) = entry.index_to_workdir() else {
            continue;
        };
        let new = delta.new_file().path().and_then(|p| p.to_str());
        let old = delta.old_file().path().and_then(|p| p.to_str());
        match delta.status() {
            git2::Delta::Untracked => {
                if let Some(path) = new.or(old) {
                    remove_from_disk(repo, path)?;
                }
            }
            git2::Delta::Renamed => {
                if let Some(path) = new {
                    remove_from_disk(repo, path)?;
                }
                restore.extend(old.map(str::to_owned));
            }
            git2::Delta::Modified | git2::Delta::Deleted | git2::Delta::Typechange => {
                restore.extend(new.or(old).map(str::to_owned));
            }
            _ => {}
        }
    }
    // A single checkout for every tracked restore (one pass instead of one
    // `checkout_index` per file).
    checkout_paths(repo, restore.iter().map(String::as_str))
}

/// Deletes an untracked file, reporting the failure (a silently kept file used
/// to leave the discard without feedback). Already gone ⇒ goal reached.
fn remove_from_disk(repo: &git2::Repository, path: &str) -> Result<(), git2::Error> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository has no workdir"))?;
    match std::fs::remove_file(workdir.join(path)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(git2::Error::from_str(&format!("delete {path}: {err}"))),
    }
}

/// Restores `paths` from the index in one checkout. Paths are literal, never
/// globs (git.md §3); no path ⇒ no-op.
fn checkout_paths<'a>(
    repo: &git2::Repository,
    paths: impl IntoIterator<Item = &'a str>,
) -> Result<(), git2::Error> {
    // The restore source is the cached index: refresh it so a stage done by
    // another process since the last poll is what gets restored, not a stale
    // snapshot (cf. stage::fresh_index).
    crate::git::stage::fresh_index(repo)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout
        .force()
        .update_index(false)
        .disable_pathspec_match(true);
    let mut any = false;
    for path in paths {
        checkout.path(path);
        any = true;
    }
    if !any {
        return Ok(());
    }
    repo.checkout_index(None, Some(&mut checkout))
}
