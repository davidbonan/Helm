//! What the right sidebar keeps per worktree (specs/files.md §2, §7): the tab on
//! show, the folders unfolded in the tree, the row selected there.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::files::folders_above;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarTab {
    #[default]
    Git,
    Files,
}

impl SidebarTab {
    pub fn other(self) -> Self {
        match self {
            Self::Git => Self::Files,
            Self::Files => Self::Git,
        }
    }
}

/// Paths relative to the worktree, `/`-separated as in the git status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    pub tab: SidebarTab,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub unfolded: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
}

/// One change to a worktree's [`TabState`], as the sidebar asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabEdit {
    Show(SidebarTab),
    Select(String),
    Unfold(String),
    Fold(String),
    CollapseAll,
}

impl TabState {
    pub fn apply(&mut self, edit: TabEdit) {
        match edit {
            TabEdit::Show(tab) => self.tab = tab,
            TabEdit::Select(path) => self.selected = Some(path),
            TabEdit::Unfold(folder) => {
                self.unfolded.insert(folder);
            }
            TabEdit::Fold(folder) => self.fold(&folder),
            TabEdit::CollapseAll => self.collapse_all(),
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

    fn state(unfolded: &[&str], selected: &str) -> TabState {
        TabState {
            tab: SidebarTab::Files,
            unfolded: unfolded.iter().map(|folder| folder.to_string()).collect(),
            selected: Some(selected.to_owned()),
        }
    }

    #[test]
    fn folding_a_folder_keeps_its_unfolded_children_and_lifts_a_selection_inside_it() {
        let mut tab = state(&["src", "src/ui"], "src/ui/mod.rs");

        tab.apply(TabEdit::Fold("src".to_owned()));

        assert_eq!(tab, state(&["src/ui"], "src"));
    }

    #[test]
    fn folding_a_folder_leaves_a_selection_outside_it() {
        let mut tab = state(&["src", "srcs"], "srcs/lib.rs");

        tab.apply(TabEdit::Fold("src".to_owned()));

        assert_eq!(tab, state(&["srcs"], "srcs/lib.rs"));
    }

    #[test]
    fn collapse_all_lifts_the_selection_to_its_top_level_folder() {
        let mut nested = state(&["src", "src/ui"], "src/ui/mod.rs");
        let mut top_level = state(&["src"], "README.md");

        nested.apply(TabEdit::CollapseAll);
        top_level.apply(TabEdit::CollapseAll);

        assert_eq!(nested, state(&[], "src"));
        assert_eq!(top_level, state(&[], "README.md"));
    }
}
