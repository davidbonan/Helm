//! The git status as the tree shows it (specs/files.md §3): a tint on each changed
//! file, a dot on each folder holding one.

use std::collections::HashMap;

use crate::git::status::{ChangeKind, RepoStatus};

/// Weakest first: a folder's dot takes the strongest tint below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tint {
    Added,
    Modified,
    Conflicted,
}

impl Tint {
    /// `None` for a deleted file: it is not in the tree.
    fn of(kind: ChangeKind) -> Option<Self> {
        match kind {
            ChangeKind::Untracked | ChangeKind::Added | ChangeKind::Renamed => Some(Self::Added),
            ChangeKind::Modified => Some(Self::Modified),
            ChangeKind::Conflicted => Some(Self::Conflicted),
            ChangeKind::Deleted => None,
        }
    }
}

/// Keyed by path relative to the worktree, `/`-separated as in [`RepoStatus`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusTints {
    files: HashMap<String, Tint>,
    folders: HashMap<String, Tint>,
}

impl StatusTints {
    pub fn of(status: &RepoStatus) -> Self {
        let mut tints = Self::default();
        for entry in status.staged.iter().chain(&status.unstaged) {
            let Some(tint) = Tint::of(entry.kind) else {
                continue;
            };
            raise(&mut tints.files, &entry.path, tint);
            for folder in folders_above(&entry.path) {
                raise(&mut tints.folders, folder, tint);
            }
        }
        tints
    }

    pub fn file(&self, path: &str) -> Option<Tint> {
        self.files.get(path).copied()
    }

    pub fn folder(&self, path: &str) -> Option<Tint> {
        self.folders.get(path).copied()
    }
}

fn raise(tints: &mut HashMap<String, Tint>, path: &str, tint: Tint) {
    let slot = tints.entry(path.to_owned()).or_insert(tint);
    *slot = (*slot).max(tint);
}

/// `a/b/c.rs` ⇒ `a`, `a/b`.
fn folders_above(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(|(slash, _)| &path[..slash])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::status::FileEntry;

    fn entry(path: &str, kind: ChangeKind) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            kind,
            additions: 0,
            deletions: 0,
        }
    }

    fn unstaged(entries: &[(&str, ChangeKind)]) -> RepoStatus {
        RepoStatus {
            staged: Vec::new(),
            unstaged: entries
                .iter()
                .map(|(path, kind)| entry(path, *kind))
                .collect(),
        }
    }

    #[test]
    fn each_change_kind_takes_its_tint_and_a_deleted_file_none() {
        let tints = StatusTints::of(&unstaged(&[
            ("new.rs", ChangeKind::Untracked),
            ("moved.rs", ChangeKind::Renamed),
            ("edited.rs", ChangeKind::Modified),
            ("clash.rs", ChangeKind::Conflicted),
            ("gone.rs", ChangeKind::Deleted),
        ]));

        assert_eq!(tints.file("new.rs"), Some(Tint::Added));
        assert_eq!(tints.file("moved.rs"), Some(Tint::Added));
        assert_eq!(tints.file("edited.rs"), Some(Tint::Modified));
        assert_eq!(tints.file("clash.rs"), Some(Tint::Conflicted));
        assert_eq!(tints.file("gone.rs"), None);
    }

    #[test]
    fn a_folder_dot_takes_the_strongest_change_below_it() {
        let tints = StatusTints::of(&unstaged(&[
            ("src/new.rs", ChangeKind::Untracked),
            ("src/ui/edited.rs", ChangeKind::Modified),
            ("src/git/new.rs", ChangeKind::Untracked),
            ("src/git/deep/clash.rs", ChangeKind::Conflicted),
            ("docs/gone.md", ChangeKind::Deleted),
        ]));

        assert_eq!(tints.folder("src"), Some(Tint::Conflicted));
        assert_eq!(tints.folder("src/git"), Some(Tint::Conflicted));
        assert_eq!(tints.folder("src/ui"), Some(Tint::Modified));
        assert_eq!(tints.folder("docs"), None);
        assert_eq!(tints.folder("src/new.rs"), None);
    }

    #[test]
    fn a_file_both_staged_and_unstaged_takes_the_stronger_tint() {
        let mut status = unstaged(&[("lib.rs", ChangeKind::Modified)]);
        status.staged.push(entry("lib.rs", ChangeKind::Added));

        assert_eq!(
            StatusTints::of(&status).file("lib.rs"),
            Some(Tint::Modified)
        );
    }
}
