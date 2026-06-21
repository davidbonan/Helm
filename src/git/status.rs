use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Untracked,
    Added,
    Modified,
    Deleted,
    Renamed,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub kind: ChangeKind,
    /// Lines added / removed in the section's delta (0/0 for a binary or a
    /// conflict) — the sidebar's `+N` / `−N` stats (git.md §3).
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoStatus {
    pub staged: Vec<FileEntry>,
    pub unstaged: Vec<FileEntry>,
}

impl RepoStatus {
    /// Number of touched files — union of staged/unstaged, deduplicated paths (a
    /// half-staged file counts once). Counter of the WIP row (M10-7).
    pub fn changed_file_count(&self) -> usize {
        self.staged
            .iter()
            .chain(&self.unstaged)
            .map(|f| f.path.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// Total additions / deletions over all deltas (staged + unstaged) — the
    /// sidebar's "N files changed" summary band (git.md §3). A half-staged file
    /// sums both of its deltas (WT→index and index→HEAD).
    pub fn total_line_stats(&self) -> (usize, usize) {
        self.staged
            .iter()
            .chain(&self.unstaged)
            .fold((0, 0), |(add, del), f| {
                (add + f.additions, del + f.deletions)
            })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sections {
    pub staged: bool,
    pub unstaged: bool,
}

pub fn classify(status: git2::Status) -> Sections {
    Sections {
        staged: staged_kind(status).is_some(),
        unstaged: unstaged_kind(status).is_some(),
    }
}

fn staged_kind(status: git2::Status) -> Option<ChangeKind> {
    use git2::Status as S;
    if status.contains(S::INDEX_NEW) {
        Some(ChangeKind::Added)
    } else if status.contains(S::INDEX_DELETED) {
        Some(ChangeKind::Deleted)
    } else if status.contains(S::INDEX_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if status.intersects(S::INDEX_MODIFIED | S::INDEX_TYPECHANGE) {
        Some(ChangeKind::Modified)
    } else {
        None
    }
}

fn unstaged_kind(status: git2::Status) -> Option<ChangeKind> {
    use git2::Status as S;
    if status.contains(S::CONFLICTED) {
        Some(ChangeKind::Conflicted)
    } else if status.contains(S::WT_NEW) {
        Some(ChangeKind::Untracked)
    } else if status.contains(S::WT_DELETED) {
        Some(ChangeKind::Deleted)
    } else if status.contains(S::WT_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if status.intersects(S::WT_MODIFIED | S::WT_TYPECHANGE) {
        Some(ChangeKind::Modified)
    } else {
        None
    }
}

/// Merge / rebase (or other git operation) in progress — `Repository::state()`
/// ≠ clean, including for an operation started from the terminal (git.md §10):
/// **Merge/Rebase in progress** banner in the status sidebar (M12-8).
pub fn op_in_progress(repo: &git2::Repository) -> bool {
    repo.state() != git2::RepositoryState::Clean
}

/// One-line summary of the operation in progress, for the conflict panel header
/// ("Merging `<source>` into `<target>`", conflicts.md §2). `source`/`target` are
/// best-effort: a branch name when one resolves, else `None` (the panel then
/// shows the bare verb).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpSummary {
    pub verb: &'static str,
    pub source: Option<String>,
    pub target: Option<String>,
}

pub fn op_summary(repo: &git2::Repository) -> Option<OpSummary> {
    use git2::RepositoryState as State;
    let verb = match repo.state() {
        State::Merge => "Merging",
        State::Rebase | State::RebaseInteractive | State::RebaseMerge => "Rebasing",
        State::CherryPick | State::CherryPickSequence => "Cherry-picking",
        State::Revert | State::RevertSequence => "Reverting",
        _ => return None,
    };
    let (source, target) = match repo.state() {
        State::Rebase | State::RebaseInteractive | State::RebaseMerge => {
            (rebase_head_name(repo, "head-name"), rebase_onto_name(repo))
        }
        _ => (incoming_name(repo), current_branch(repo)),
    };
    Some(OpSummary {
        verb,
        source,
        target,
    })
}

fn current_branch(repo: &git2::Repository) -> Option<String> {
    repo.head().ok()?.shorthand().ok().map(str::to_owned)
}

/// The merged/picked/reverted commit recorded in `*_HEAD`, resolved to a branch
/// name when one points at it, else its short oid.
fn incoming_name(repo: &git2::Repository) -> Option<String> {
    use git2::RepositoryState as State;
    let head_file = match repo.state() {
        State::Merge => "MERGE_HEAD",
        State::CherryPick | State::CherryPickSequence => "CHERRY_PICK_HEAD",
        State::Revert | State::RevertSequence => "REVERT_HEAD",
        _ => return None,
    };
    let raw = std::fs::read_to_string(repo.path().join(head_file)).ok()?;
    let oid = git2::Oid::from_str(raw.trim()).ok()?;
    Some(name_for_oid(repo, oid))
}

fn name_for_oid(repo: &git2::Repository, oid: git2::Oid) -> String {
    if let Ok(branches) = repo.branches(None) {
        for (branch, _) in branches.flatten() {
            if branch.get().target() == Some(oid) {
                if let Ok(Some(name)) = branch.name() {
                    return name.to_owned();
                }
            }
        }
    }
    let hex = oid.to_string();
    hex[..hex.len().min(7)].to_owned()
}

/// The branch being rebased, read from the sequencer's `head-name` file.
fn rebase_head_name(repo: &git2::Repository, file: &str) -> Option<String> {
    for dir in ["rebase-merge", "rebase-apply"] {
        if let Ok(raw) = std::fs::read_to_string(repo.path().join(dir).join(file)) {
            let name = raw.trim();
            let short = name.strip_prefix("refs/heads/").unwrap_or(name);
            if !short.is_empty() {
                return Some(short.to_owned());
            }
        }
    }
    None
}

/// The rebase base (`onto`), resolved to a branch name when one points at it.
fn rebase_onto_name(repo: &git2::Repository) -> Option<String> {
    for dir in ["rebase-merge", "rebase-apply"] {
        if let Ok(raw) = std::fs::read_to_string(repo.path().join(dir).join("onto")) {
            if let Ok(oid) = git2::Oid::from_str(raw.trim()) {
                return Some(name_for_oid(repo, oid));
            }
        }
    }
    None
}

pub fn load(repo_path: &Path) -> Result<RepoStatus, git2::Error> {
    let repo = git2::Repository::open(repo_path)?;
    load_repo(&repo)
}

/// At least one staged / unstaged / untracked change. Cheap dirty check for the
/// stash guards (`stash`, checkout auto-stash): no line stats, no untracked
/// recursion (a dirty directory entry is enough) — `load_repo` pays a `Patch`
/// per changed file just to answer yes/no.
pub fn is_dirty(repo: &git2::Repository) -> Result<bool, git2::Error> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).exclude_submodules(true);
    Ok(!repo.statuses(Some(&mut opts))?.is_empty())
}

/// Status pass with the same flags as `load_repo` but **without the line
/// stats** (no `Patch` per file): the cheap enumeration behind the batch
/// commands (stage all, unstage all, discard all) and the rename-pair lookup.
pub(crate) fn work_statuses(repo: &git2::Repository) -> Result<git2::Statuses<'_>, git2::Error> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true)
        .exclude_submodules(true);
    repo.statuses(Some(&mut opts))
}

/// Old path of the detected rename whose **new** path is `path`, on the
/// requested side (HEAD→index for the staged section, index→workdir for the
/// unstaged one). Same detection flags as `load_repo`, so it matches exactly
/// the entry the sidebar displays; `None` when the entry is not a rename.
pub(crate) fn rename_old_path(
    repo: &git2::Repository,
    path: &str,
    staged: bool,
) -> Result<Option<String>, git2::Error> {
    for entry in work_statuses(repo)?.iter() {
        let delta = if staged {
            entry.head_to_index()
        } else {
            entry.index_to_workdir()
        };
        let Some(delta) = delta else { continue };
        if delta.status() == git2::Delta::Renamed
            && delta.new_file().path().and_then(|p| p.to_str()) == Some(path)
        {
            return Ok(delta
                .old_file()
                .path()
                .and_then(|p| p.to_str())
                .map(str::to_owned));
        }
    }
    Ok(None)
}

pub fn load_repo(repo: &git2::Repository) -> Result<RepoStatus, git2::Error> {
    let statuses = work_statuses(repo)?;
    let mut out = RepoStatus::default();
    for entry in statuses.iter() {
        let status = entry.status();
        let path = entry_path(&entry);
        if let Some(kind) = staged_kind(status) {
            out.staged.push(FileEntry {
                path: path.clone(),
                kind,
                additions: 0,
                deletions: 0,
            });
        }
        if let Some(kind) = unstaged_kind(status) {
            out.unstaged.push(FileEntry {
                path,
                kind,
                additions: 0,
                deletions: 0,
            });
        }
    }
    // Line stats only for non-empty sections: the unstaged pass re-walks the
    // whole working tree (`statuses` already paid one walk) — on a clean or
    // staged-only repo the 1 s poll skips that second scan entirely.
    if !out.staged.is_empty() {
        let stats = staged_line_stats(repo)?;
        for file in &mut out.staged {
            (file.additions, file.deletions) = line_stats_for(&stats, &file.path);
        }
    }
    if !out.unstaged.is_empty() {
        let stats = unstaged_line_stats(repo)?;
        for file in &mut out.unstaged {
            if file.kind != ChangeKind::Conflicted {
                (file.additions, file.deletions) = line_stats_for(&stats, &file.path);
            }
        }
    }
    Ok(out)
}

type LineStats = std::collections::HashMap<String, (usize, usize)>;

fn line_stats_for(stats: &LineStats, path: &str) -> (usize, usize) {
    stats.get(path).copied().unwrap_or((0, 0))
}

/// Blobs above this size are auto-flagged binary by libgit2 (never loaded):
/// stats fall to 0/0 — hidden by the sidebar, like the diff view's oversize
/// rule (git.md §8). Without the cap, a poll re-read every huge file each
/// second just to count lines nobody would see.
const MAX_STAT_BLOB_BYTES: i64 = crate::git::diff::MAX_DIFF_BYTES as i64;

fn staged_line_stats(repo: &git2::Repository) -> Result<LineStats, git2::Error> {
    // Unborn HEAD ⇒ diff against the empty tree (the whole index is additions).
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let mut opts = git2::DiffOptions::new();
    opts.show_binary(true).max_size(MAX_STAT_BLOB_BYTES);
    let mut diff = repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))?;
    find_renames(&mut diff)?;
    diff_line_stats(&diff)
}

fn unstaged_line_stats(repo: &git2::Repository) -> Result<LineStats, git2::Error> {
    let mut opts = git2::DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        // Without this flag libgit2 does not load an untracked file's lines —
        // its delta would stay at 0/0 (cf. git::diff::untracked_file_diff).
        .show_untracked_content(true)
        .show_binary(true)
        .max_size(MAX_STAT_BLOB_BYTES);
    let mut diff = repo.diff_index_to_workdir(None, Some(&mut opts))?;
    find_renames(&mut diff)?;
    diff_line_stats(&diff)
}

/// Pairs renames the way `statuses` does (renames_*): without this a renamed file
/// would count as a full deletion + full addition instead of its real delta.
fn find_renames(diff: &mut git2::Diff) -> Result<(), git2::Error> {
    let mut find = git2::DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find))
}

fn diff_line_stats(diff: &git2::Diff) -> Result<LineStats, git2::Error> {
    let mut out = LineStats::new();
    for (idx, delta) in diff.deltas().enumerate() {
        let Some(path) = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
        else {
            continue;
        };
        // `from_diff` ⇒ None and empty stats for a binary: it stays at 0/0.
        let Some(patch) = git2::Patch::from_diff(diff, idx)? else {
            continue;
        };
        if patch.delta().flags().contains(git2::DiffFlags::BINARY) {
            continue;
        }
        let (_, additions, deletions) = patch.line_stats()?;
        out.insert(path.to_string(), (additions, deletions));
    }
    Ok(out)
}

fn entry_path(entry: &git2::StatusEntry) -> String {
    let renamed = entry
        .head_to_index()
        .or_else(|| entry.index_to_workdir())
        .and_then(|diff| diff.new_file().path().or_else(|| diff.old_file().path()))
        .and_then(|p| p.to_str());
    renamed
        .or_else(|| entry.path().ok())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, kind: ChangeKind, additions: usize, deletions: usize) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            kind,
            additions,
            deletions,
        }
    }

    fn commit_blob(
        repo: &git2::Repository,
        content: &str,
        msg: &str,
        update_ref: Option<&str>,
        parents: &[git2::Oid],
    ) -> git2::Oid {
        let sig = git2::Signature::now("T", "t@e").unwrap();
        std::fs::write(repo.workdir().unwrap().join("f.txt"), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("f.txt")).unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let parents: Vec<git2::Commit> = parents
            .iter()
            .map(|oid| repo.find_commit(*oid).unwrap())
            .collect();
        let refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(update_ref, &sig, &sig, msg, &tree, &refs)
            .unwrap()
    }

    #[test]
    fn op_summary_reports_a_merge_in_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let base = commit_blob(&repo, "base\n", "base", Some("HEAD"), &[]);
        // "theirs" is a dangling commit a branch points at; "ours" advances main.
        let theirs = commit_blob(&repo, "theirs\n", "theirs", None, &[base]);
        repo.branch("theirs", &repo.find_commit(theirs).unwrap(), false)
            .unwrap();
        commit_blob(&repo, "ours\n", "ours", Some("HEAD"), &[base]);

        let annotated = repo.find_annotated_commit(theirs).unwrap();
        // conflicting merge → repo stays in the Merge state with MERGE_HEAD set.
        let _ = repo.merge(&[&annotated], None, None);

        let summary = op_summary(&repo).expect("a merge is in progress");
        assert_eq!(summary.verb, "Merging");
        assert_eq!(summary.source.as_deref(), Some("theirs"));
        assert_eq!(summary.target.as_deref(), Some("main"));
        assert!(
            op_summary(&git2::Repository::init(tempfile::tempdir().unwrap().path()).unwrap())
                .is_none()
        );
    }

    #[test]
    fn changed_file_count_dedupes_half_staged_paths() {
        let status = RepoStatus {
            staged: vec![entry("a.txt", ChangeKind::Modified, 0, 0)],
            unstaged: vec![
                entry("a.txt", ChangeKind::Modified, 0, 0),
                entry("b.txt", ChangeKind::Untracked, 0, 0),
            ],
        };
        assert_eq!(status.changed_file_count(), 2);
        assert_eq!(RepoStatus::default().changed_file_count(), 0);
    }

    #[test]
    fn total_line_stats_sums_both_sections() {
        let status = RepoStatus {
            staged: vec![
                entry("a.txt", ChangeKind::Modified, 12, 3),
                entry("b.txt", ChangeKind::Added, 7, 0),
            ],
            unstaged: vec![entry("a.txt", ChangeKind::Modified, 2, 1)],
        };
        assert_eq!(status.total_line_stats(), (21, 4));
        assert_eq!(RepoStatus::default().total_line_stats(), (0, 0));
    }

    #[test]
    fn index_change_is_staged_only() {
        let s = classify(git2::Status::INDEX_MODIFIED);
        assert_eq!(
            s,
            Sections {
                staged: true,
                unstaged: false
            }
        );
    }

    #[test]
    fn worktree_change_is_unstaged_only() {
        let s = classify(git2::Status::WT_NEW);
        assert_eq!(
            s,
            Sections {
                staged: false,
                unstaged: true
            }
        );
    }

    #[test]
    fn partially_staged_file_is_in_both_sections() {
        let s = classify(git2::Status::INDEX_MODIFIED | git2::Status::WT_MODIFIED);
        assert_eq!(
            s,
            Sections {
                staged: true,
                unstaged: true
            }
        );
    }

    #[test]
    fn untracked_file_is_classified_as_untracked() {
        assert_eq!(
            unstaged_kind(git2::Status::WT_NEW),
            Some(ChangeKind::Untracked)
        );
        assert_eq!(staged_kind(git2::Status::WT_NEW), None);
    }

    #[test]
    fn index_new_is_added_in_staged() {
        assert_eq!(
            staged_kind(git2::Status::INDEX_NEW),
            Some(ChangeKind::Added)
        );
    }

    #[test]
    fn renamed_flags_map_to_renamed_kind() {
        assert_eq!(
            staged_kind(git2::Status::INDEX_RENAMED),
            Some(ChangeKind::Renamed)
        );
        assert_eq!(
            unstaged_kind(git2::Status::WT_RENAMED),
            Some(ChangeKind::Renamed)
        );
    }

    #[test]
    fn deleted_flags_map_to_deleted_kind() {
        assert_eq!(
            staged_kind(git2::Status::INDEX_DELETED),
            Some(ChangeKind::Deleted)
        );
        assert_eq!(
            unstaged_kind(git2::Status::WT_DELETED),
            Some(ChangeKind::Deleted)
        );
    }

    #[test]
    fn conflicted_file_is_unstaged_conflict_only() {
        let s = git2::Status::CONFLICTED;
        assert_eq!(unstaged_kind(s), Some(ChangeKind::Conflicted));
        assert_eq!(staged_kind(s), None);
    }

    #[test]
    fn partially_staged_keeps_distinct_kinds_per_section() {
        let s = git2::Status::INDEX_NEW | git2::Status::WT_MODIFIED;
        assert_eq!(staged_kind(s), Some(ChangeKind::Added));
        assert_eq!(unstaged_kind(s), Some(ChangeKind::Modified));
    }
}
