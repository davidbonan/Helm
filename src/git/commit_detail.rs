use crate::git::status::ChangeKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMeta {
    pub oid: git2::Oid,
    pub short_id: String,
    pub author: String,
    pub email: String,
    /// Author time in seconds since the Unix epoch (UTC).
    pub time: i64,
    /// Author timezone offset in minutes — `time + offset` renders the author's
    /// wall-clock time, like `git log`.
    pub offset_minutes: i32,
    pub committer: String,
    /// First paragraph of the message, folded on one line (git "subject").
    pub summary: String,
    /// Message after the first blank line, trimmed (empty if none).
    pub body: String,
    pub parents: Vec<git2::Oid>,
}

/// One file changed by a commit, relative to its first parent (root commit ⇒ vs
/// the empty tree). The unified diff of the file is loaded on demand (M9-3).
/// Line stats follow the status rule (M13-2): binary ⇒ 0/0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    pub path: String,
    pub kind: ChangeKind,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetail {
    pub meta: CommitMeta,
    pub files: Vec<CommitFile>,
}

impl CommitDetail {
    /// Total additions / deletions over all files in the commit.
    pub fn total_line_stats(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(add, del), f| {
            (add + f.additions, del + f.deletions)
        })
    }
}

pub fn load(repo_path: &std::path::Path, oid: git2::Oid) -> Result<CommitDetail, git2::Error> {
    let repo = git2::Repository::open(repo_path)?;
    load_repo(&repo, oid)
}

pub fn load_repo(repo: &git2::Repository, oid: git2::Oid) -> Result<CommitDetail, git2::Error> {
    let commit = repo.find_commit(oid)?;
    let meta = CommitMeta {
        oid,
        short_id: commit
            .as_object()
            .short_id()?
            .as_str()
            .unwrap_or_default()
            .to_string(),
        author: commit.author().name().unwrap_or("").to_string(),
        email: commit.author().email().unwrap_or("").to_string(),
        time: commit.author().when().seconds(),
        offset_minutes: commit.author().when().offset_minutes(),
        committer: commit.committer().name().unwrap_or("").to_string(),
        summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
        body: commit
            .body()
            .ok()
            .flatten()
            .unwrap_or("")
            .trim()
            .to_string(),
        parents: commit.parent_ids().collect(),
    };

    let new_tree = commit.tree()?;
    // First parent only: a merge is shown against its mainline (git.md §9); a root
    // commit has no parent, so it diffs against the empty tree (everything added).
    let parent_tree = match commit.parent(0) {
        Ok(parent) => Some(parent.tree()?),
        Err(_) => None,
    };

    let mut opts = git2::DiffOptions::new();
    let mut diff =
        repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), Some(&mut opts))?;
    let mut find = git2::DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find))?;

    let mut files = Vec::with_capacity(diff.deltas().len());
    for (idx, delta) in diff.deltas().enumerate() {
        let Some(kind) = delta_kind(delta.status()) else {
            continue;
        };
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or_default()
            .to_string();
        let (additions, deletions) = delta_line_stats(&diff, idx)?;
        files.push(CommitFile {
            path,
            kind,
            additions,
            deletions,
        });
    }

    // Stash row (git.md §9): untracked files live in the stash's 3rd parent
    // commit, not in the stash tree — without them a stash holding only
    // untracked files would show as empty.
    if let Some(untracked) = crate::git::stash::untracked_tree(repo, &commit)? {
        let diff = repo.diff_tree_to_tree(None, Some(&untracked), None)?;
        for (idx, delta) in diff.deltas().enumerate() {
            let path = delta
                .new_file()
                .path()
                .and_then(|p| p.to_str())
                .unwrap_or_default()
                .to_string();
            let (additions, deletions) = delta_line_stats(&diff, idx)?;
            files.push(CommitFile {
                path,
                kind: ChangeKind::Added,
                additions,
                deletions,
            });
        }
    }

    Ok(CommitDetail { meta, files })
}

/// Lines (added, removed) of delta `idx` — `from_diff` ⇒ None and a binary stays
/// at 0/0, same rule as `status::diff_line_stats` (M13-2).
pub(crate) fn delta_line_stats(
    diff: &git2::Diff,
    idx: usize,
) -> Result<(usize, usize), git2::Error> {
    let Some(patch) = git2::Patch::from_diff(diff, idx)? else {
        return Ok((0, 0));
    };
    if patch.delta().flags().contains(git2::DiffFlags::BINARY) {
        return Ok((0, 0));
    }
    let (_, additions, deletions) = patch.line_stats()?;
    Ok((additions, deletions))
}

pub(crate) fn delta_kind(status: git2::Delta) -> Option<ChangeKind> {
    use git2::Delta as D;
    match status {
        D::Added | D::Copied | D::Untracked => Some(ChangeKind::Added),
        D::Deleted => Some(ChangeKind::Deleted),
        D::Renamed => Some(ChangeKind::Renamed),
        D::Modified | D::Typechange => Some(ChangeKind::Modified),
        D::Conflicted => Some(ChangeKind::Conflicted),
        D::Unmodified | D::Ignored | D::Unreadable => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_line_stats_sums_every_file() {
        let file = |a: usize, d: usize| CommitFile {
            path: String::new(),
            kind: ChangeKind::Modified,
            additions: a,
            deletions: d,
        };
        let detail = CommitDetail {
            meta: CommitMeta {
                oid: git2::Oid::ZERO_SHA1,
                short_id: String::new(),
                author: String::new(),
                email: String::new(),
                time: 0,
                offset_minutes: 0,
                committer: String::new(),
                summary: String::new(),
                body: String::new(),
                parents: vec![],
            },
            files: vec![file(62, 5), file(64, 7), file(0, 0)],
        };
        assert_eq!(detail.total_line_stats(), (126, 12));
    }

    #[test]
    fn delta_status_maps_to_change_kinds() {
        assert_eq!(delta_kind(git2::Delta::Added), Some(ChangeKind::Added));
        assert_eq!(delta_kind(git2::Delta::Copied), Some(ChangeKind::Added));
        assert_eq!(delta_kind(git2::Delta::Deleted), Some(ChangeKind::Deleted));
        assert_eq!(delta_kind(git2::Delta::Renamed), Some(ChangeKind::Renamed));
        assert_eq!(
            delta_kind(git2::Delta::Modified),
            Some(ChangeKind::Modified)
        );
        assert_eq!(
            delta_kind(git2::Delta::Typechange),
            Some(ChangeKind::Modified)
        );
        assert_eq!(delta_kind(git2::Delta::Unmodified), None);
    }
}
