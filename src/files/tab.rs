//! What the right sidebar keeps per worktree (specs/files.md §2, §7): the tab on
//! show, the folders unfolded in the tree, the row selected there.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

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
