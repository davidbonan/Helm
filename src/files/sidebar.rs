//! What the files sidebar keeps per worktree (specs/files.md §2, §7): whether it
//! shows, the folders unfolded in the tree, the row selected there.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::files::folders_above;

/// Paths relative to the worktree, `/`-separated as in the git status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesSidebar {
    pub shown: bool,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub unfolded: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
}

/// One change to a worktree's tree, as the sidebar asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEdit {
    Select(String),
    Unfold(String),
    Fold(String),
    CollapseAll,
}

impl FilesSidebar {
    pub fn apply(&mut self, edit: TreeEdit) {
        match edit {
            TreeEdit::Select(path) => self.selected = Some(path),
            TreeEdit::Unfold(folder) => {
                self.unfolded.insert(folder);
            }
            TreeEdit::Fold(folder) => self.fold(&folder),
            TreeEdit::CollapseAll => self.collapse_all(),
        }
    }

    /// Folders unfolded inside `folder` stay so; a selection hidden by the fold
    /// moves up to `folder`.
    fn fold(&mut self, folder: &str) {
        self.unfolded.remove(folder);
        let hidden = self
            .selected
            .as_deref()
            .is_some_and(|selected| folders_above(selected).any(|above| above == folder));
        if hidden {
            self.selected = Some(folder.to_owned());
        }
    }

    /// A selection hidden by the collapse moves up to its top-level folder.
    fn collapse_all(&mut self) {
        self.unfolded.clear();
        let top = self
            .selected
            .as_deref()
            .and_then(|selected| folders_above(selected).next());
        if let Some(top) = top {
            self.selected = Some(top.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(unfolded: &[&str], selected: &str) -> FilesSidebar {
        FilesSidebar {
            shown: true,
            unfolded: unfolded.iter().map(|folder| folder.to_string()).collect(),
            selected: Some(selected.to_owned()),
        }
    }

    #[test]
    fn folding_a_folder_keeps_its_unfolded_children_and_lifts_a_selection_inside_it() {
        let mut sidebar = state(&["src", "src/ui"], "src/ui/mod.rs");

        sidebar.apply(TreeEdit::Fold("src".to_owned()));

        assert_eq!(sidebar, state(&["src/ui"], "src"));
    }

    #[test]
    fn folding_a_folder_leaves_a_selection_outside_it() {
        let mut sidebar = state(&["src", "srcs"], "srcs/lib.rs");

        sidebar.apply(TreeEdit::Fold("src".to_owned()));

        assert_eq!(sidebar, state(&["srcs"], "srcs/lib.rs"));
    }

    #[test]
    fn collapse_all_lifts_the_selection_to_its_top_level_folder() {
        let mut nested = state(&["src", "src/ui"], "src/ui/mod.rs");
        let mut top_level = state(&["src"], "README.md");

        nested.apply(TreeEdit::CollapseAll);
        top_level.apply(TreeEdit::CollapseAll);

        assert_eq!(nested, state(&[], "src"));
        assert_eq!(top_level, state(&[], "README.md"));
    }
}
