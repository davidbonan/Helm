//! The Files tab's tree (specs/files.md §3, §5, §6): the folders read so far, the
//! rows they show, the keyboard moves over those rows.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::files::tab::TabState;
use crate::files::tint::{StatusTints, Tint};
use crate::files::{child_path, folders_above, is_ignored, list_tree, parent_of, FileRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    File,
    Symlink,
}

impl EntryKind {
    fn of(row: &FileRow) -> Self {
        if row.symlink {
            Self::Symlink
        } else if row.dir {
            Self::Folder
        } else {
            Self::File
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub name: String,
    pub kind: EntryKind,
    pub ignored: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderListing {
    pub entries: Vec<TreeEntry>,
    /// Every entry of the folder, listed or not.
    pub total: usize,
}

/// A folder relative to the worktree (`""` for its root) and what it holds.
pub type ListedFolder = (String, FolderListing);

/// Reads each folder of the worktree for the tree. A folder that is gone or
/// unreadable lists empty; inside an ignored folder every entry is ignored,
/// without checking it.
pub fn list_folders(repo: &git2::Repository, folders: &[String]) -> Vec<ListedFolder> {
    folders
        .iter()
        .map(|folder| (folder.clone(), list_folder(repo, folder)))
        .collect()
}

fn list_folder(repo: &git2::Repository, folder: &str) -> FolderListing {
    let Some(listing) = repo
        .workdir()
        .and_then(|root| list_tree(&root.join(folder)).ok())
    else {
        return FolderListing::default();
    };
    let folder_ignored = !folder.is_empty() && is_ignored(repo, Path::new(folder));
    let entries = listing
        .entries
        .iter()
        .map(|row| TreeEntry {
            name: row.name.clone(),
            kind: EntryKind::of(row),
            ignored: folder_ignored || is_ignored(repo, Path::new(&child_path(folder, &row.name))),
        })
        .collect();
    FolderListing {
        entries,
        total: listing.total,
    }
}

/// The root plus each unfolded folder whose parents are all unfolded: the
/// folders the tree shows the content of.
pub fn shown_folders(unfolded: &BTreeSet<String>) -> Vec<String> {
    let shown = unfolded
        .iter()
        .filter(|folder| folders_above(folder).all(|above| unfolded.contains(above)));
    std::iter::once(String::new())
        .chain(shown.cloned())
        .collect()
}

/// What the listings tell of a path.
enum Presence<'a> {
    /// Its folder is not read, or capped before it.
    Unknown,
    Gone,
    Listed(&'a TreeEntry),
}

/// The folders read so far, keyed by path relative to the worktree (`""` = root).
#[derive(Debug, Default)]
pub struct Listings(HashMap<String, FolderListing>);

impl Listings {
    pub fn store(&mut self, listed: Vec<ListedFolder>) {
        self.0.extend(listed);
    }

    /// The shown folders (see [`shown_folders`]) not read yet.
    pub fn unread(&self, unfolded: &BTreeSet<String>) -> Vec<String> {
        shown_folders(unfolded)
            .into_iter()
            .filter(|folder| !self.0.contains_key(folder))
            .collect()
    }

    /// `state` without what the listings show is gone (files.md §6, §7): an
    /// unfolded folder drops out, the selection moves up to the nearest entry left.
    pub fn prune(&self, state: &TabState) -> TabState {
        let unfolded = state
            .unfolded
            .iter()
            .filter(|folder| match self.presence(folder) {
                Presence::Unknown => true,
                Presence::Gone => false,
                Presence::Listed(entry) => entry.kind == EntryKind::Folder,
            })
            .cloned()
            .collect();
        let mut selected = state.selected.as_deref();
        while let Some(path) = selected.filter(|path| matches!(self.presence(path), Presence::Gone))
        {
            selected = Some(parent_of(path)).filter(|parent| !parent.is_empty());
        }
        TabState {
            tab: state.tab,
            unfolded,
            selected: selected.map(str::to_owned),
        }
    }

    fn presence(&self, path: &str) -> Presence<'_> {
        let Some(listing) = self.0.get(parent_of(path)) else {
            return Presence::Unknown;
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        match listing.entries.iter().find(|entry| entry.name == name) {
            Some(entry) => Presence::Listed(entry),
            None if listing.total > listing.entries.len() => Presence::Unknown,
            None => Presence::Gone,
        }
    }

    /// The rows on show, depth first; `None` until the root is read.
    pub fn rows(&self, unfolded: &BTreeSet<String>, tints: &StatusTints) -> Option<Vec<TreeRow>> {
        self.0.get("")?;
        let shown = ShownTree {
            listings: self,
            unfolded,
            tints,
        };
        let mut rows = Vec::new();
        shown.push_rows("", 0, &mut rows);
        Some(rows)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Folder { unfolded: bool },
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRow {
    /// Relative to the worktree, `/`-separated.
    pub path: String,
    pub depth: usize,
    pub kind: RowKind,
    pub ignored: bool,
    /// A file's own change; a folder's strongest change below it.
    pub tint: Option<Tint>,
}

impl EntryRow {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    Entry(EntryRow),
    /// The entries of a capped folder left out (*N more not shown*).
    More {
        depth: usize,
        hidden: usize,
    },
}

impl TreeRow {
    pub fn entry(&self) -> Option<&EntryRow> {
        match self {
            Self::Entry(row) => Some(row),
            Self::More { .. } => None,
        }
    }
}

struct ShownTree<'a> {
    listings: &'a Listings,
    unfolded: &'a BTreeSet<String>,
    tints: &'a StatusTints,
}

impl ShownTree<'_> {
    fn push_rows(&self, folder: &str, depth: usize, rows: &mut Vec<TreeRow>) {
        let Some(listing) = self.listings.0.get(folder) else {
            return;
        };
        for entry in &listing.entries {
            let row = self.row(entry, child_path(folder, &entry.name), depth);
            let open = (row.kind == RowKind::Folder { unfolded: true }).then(|| row.path.clone());
            rows.push(TreeRow::Entry(row));
            if let Some(path) = open {
                self.push_rows(&path, depth + 1, rows);
            }
        }
        let hidden = listing.total.saturating_sub(listing.entries.len());
        if hidden > 0 {
            rows.push(TreeRow::More { depth, hidden });
        }
    }

    fn row(&self, entry: &TreeEntry, path: String, depth: usize) -> EntryRow {
        let (kind, tint) = match entry.kind {
            EntryKind::Folder => (
                RowKind::Folder {
                    unfolded: self.unfolded.contains(&path),
                },
                self.tints.folder(&path),
            ),
            EntryKind::File => (RowKind::File, self.tints.file(&path)),
            EntryKind::Symlink => (RowKind::Symlink, self.tints.file(&path)),
        };
        EntryRow {
            path,
            depth,
            kind,
            ignored: entry.ignored,
            tint,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeMove {
    Select(String),
    /// Selects the file and opens it in the viewer.
    Open(String),
    Fold(String),
    Unfold(String),
}

/// What `key` does from the `selected` row (files.md §5); `None` when it does
/// nothing, the selection off screen included.
pub fn key_move(rows: &[TreeRow], selected: &str, key: TreeKey) -> Option<TreeMove> {
    let entries: Vec<&EntryRow> = rows.iter().filter_map(TreeRow::entry).collect();
    let index = entries.iter().position(|row| row.path == selected)?;
    let row = entries[index];
    match key {
        TreeKey::Up => index.checked_sub(1).map(|above| arrive(entries[above])),
        TreeKey::Down => entries.get(index + 1).map(|below| arrive(below)),
        TreeKey::Right => step_in(row, entries.get(index + 1).copied()),
        TreeKey::Left => step_out(row),
        TreeKey::Enter => Some(activate(row)),
    }
}

/// ↑/↓ landing on a row: a file opens, a folder is only selected.
fn arrive(row: &EntryRow) -> TreeMove {
    match row.kind {
        RowKind::Folder { .. } => TreeMove::Select(row.path.clone()),
        RowKind::File | RowKind::Symlink => TreeMove::Open(row.path.clone()),
    }
}

fn step_in(row: &EntryRow, next: Option<&EntryRow>) -> Option<TreeMove> {
    match row.kind {
        RowKind::Folder { unfolded: false } => Some(TreeMove::Unfold(row.path.clone())),
        RowKind::Folder { unfolded: true } => next
            .filter(|child| child.depth == row.depth + 1)
            .map(|child| TreeMove::Select(child.path.clone())),
        RowKind::File | RowKind::Symlink => None,
    }
}

fn step_out(row: &EntryRow) -> Option<TreeMove> {
    if row.kind == (RowKind::Folder { unfolded: true }) {
        return Some(TreeMove::Fold(row.path.clone()));
    }
    let parent = parent_of(&row.path);
    (!parent.is_empty()).then(|| TreeMove::Select(parent.to_owned()))
}

fn activate(row: &EntryRow) -> TreeMove {
    match row.kind {
        RowKind::Folder { unfolded: true } => TreeMove::Fold(row.path.clone()),
        RowKind::Folder { unfolded: false } => TreeMove::Unfold(row.path.clone()),
        RowKind::File | RowKind::Symlink => TreeMove::Open(row.path.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::tab::SidebarTab;
    use crate::git::status::{ChangeKind, FileEntry, RepoStatus};

    fn folder(path: &str, entries: &[(&str, EntryKind)]) -> ListedFolder {
        let entries: Vec<TreeEntry> = entries
            .iter()
            .map(|(name, kind)| TreeEntry {
                name: (*name).to_owned(),
                kind: *kind,
                ignored: false,
            })
            .collect();
        let total = entries.len();
        (path.to_owned(), FolderListing { entries, total })
    }

    /// `src/` (`ui/` (`mod.rs`), `lib.rs`), `link`, `README.md`.
    fn sample() -> Listings {
        let mut listings = Listings::default();
        listings.store(vec![
            folder(
                "",
                &[
                    ("src", EntryKind::Folder),
                    ("link", EntryKind::Symlink),
                    ("README.md", EntryKind::File),
                ],
            ),
            folder(
                "src",
                &[("ui", EntryKind::Folder), ("lib.rs", EntryKind::File)],
            ),
            folder("src/ui", &[("mod.rs", EntryKind::File)]),
        ]);
        listings
    }

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    fn shown_paths(listings: &Listings, unfolded: &[&str]) -> Vec<String> {
        let rows = listings
            .rows(&set(unfolded), &StatusTints::default())
            .unwrap();
        rows.iter()
            .map(|row| match row {
                TreeRow::Entry(entry) => format!("{}{}", "  ".repeat(entry.depth), entry.path),
                TreeRow::More { depth, hidden } => format!("{}+{hidden}", "  ".repeat(*depth)),
            })
            .collect()
    }

    fn tab(unfolded: &[&str], selected: Option<&str>) -> TabState {
        TabState {
            tab: SidebarTab::Files,
            unfolded: set(unfolded),
            selected: selected.map(str::to_owned),
        }
    }

    #[test]
    fn rows_follow_the_unfolded_folders_depth_first() {
        let listings = sample();

        assert_eq!(shown_paths(&listings, &[]), ["src", "link", "README.md"]);
        assert_eq!(
            shown_paths(&listings, &["src", "src/ui"]),
            [
                "src",
                "  src/ui",
                "    src/ui/mod.rs",
                "  src/lib.rs",
                "link",
                "README.md"
            ]
        );
        assert_eq!(
            shown_paths(&listings, &["src/ui"]),
            ["src", "link", "README.md"],
            "a folder unfolded under a folded one stays hidden"
        );
    }

    #[test]
    fn no_rows_until_the_root_is_read_and_a_capped_folder_ends_on_its_more_row() {
        assert_eq!(
            Listings::default().rows(&BTreeSet::new(), &StatusTints::default()),
            None
        );
        let mut listings = Listings::default();
        let (_, mut root) = folder("", &[("a.txt", EntryKind::File)]);
        root.total = 5;
        listings.store(vec![(String::new(), root)]);

        assert_eq!(shown_paths(&listings, &[]), ["a.txt", "+4"]);
    }

    #[test]
    fn rows_carry_the_file_tint_and_the_folder_dot() {
        let status = RepoStatus {
            staged: Vec::new(),
            unstaged: vec![FileEntry {
                path: "src/ui/mod.rs".to_owned(),
                kind: ChangeKind::Modified,
                additions: 1,
                deletions: 0,
            }],
        };
        let rows = sample()
            .rows(&set(&["src", "src/ui"]), &StatusTints::of(&status))
            .unwrap();
        let tint = |path: &str| {
            rows.iter()
                .filter_map(TreeRow::entry)
                .find(|row| row.path == path)
                .unwrap()
                .tint
        };

        assert_eq!(tint("src"), Some(Tint::Modified));
        assert_eq!(tint("src/ui/mod.rs"), Some(Tint::Modified));
        assert_eq!(tint("src/lib.rs"), None);
    }

    #[test]
    fn the_shown_folders_still_unread_are_the_ones_to_list() {
        let mut listings = Listings::default();
        let unfolded = set(&["src", "src/ui", "docs/api"]);

        assert_eq!(shown_folders(&unfolded), ["", "src", "src/ui"]);
        assert_eq!(listings.unread(&unfolded), ["", "src", "src/ui"]);
        listings.store(vec![folder("", &[]), folder("src", &[])]);
        assert_eq!(listings.unread(&unfolded), ["src/ui"]);
    }

    #[test]
    fn prune_drops_unfolded_folders_gone_or_now_files_and_keeps_the_unknown() {
        let mut listings = sample();
        listings.store(vec![folder("src", &[("ui", EntryKind::File)])]);
        let state = tab(&["src", "src/ui", "src/old", "docs", "docs/api"], None);

        let pruned = listings.prune(&state);

        assert_eq!(pruned.unfolded, set(&["src", "docs/api"]));
    }

    #[test]
    fn a_vanished_selection_moves_up_to_the_nearest_entry_left() {
        let mut listings = sample();
        listings.store(vec![
            folder("src", &[("lib.rs", EntryKind::File)]),
            folder("src/ui", &[]),
        ]);

        let file_gone = listings.prune(&tab(&[], Some("src/ui/mod.rs")));
        let top_gone = listings.prune(&tab(&[], Some("old.txt")));
        let unknown = listings.prune(&tab(&[], Some("docs/a.md")));

        assert_eq!(file_gone.selected.as_deref(), Some("src"));
        assert_eq!(top_gone.selected, None);
        assert_eq!(unknown.selected.as_deref(), Some("docs/a.md"));
    }

    fn rows(unfolded: &[&str]) -> Vec<TreeRow> {
        sample()
            .rows(&set(unfolded), &StatusTints::default())
            .unwrap()
    }

    fn moved(unfolded: &[&str], selected: &str, key: TreeKey) -> Option<TreeMove> {
        key_move(&rows(unfolded), selected, key)
    }

    #[test]
    fn up_and_down_walk_the_visible_rows_without_wrapping_and_open_files() {
        let open = &["src"];

        assert_eq!(
            moved(open, "src", TreeKey::Down),
            Some(TreeMove::Select("src/ui".to_owned()))
        );
        assert_eq!(
            moved(open, "src/ui", TreeKey::Down),
            Some(TreeMove::Open("src/lib.rs".to_owned()))
        );
        assert_eq!(
            moved(open, "src/lib.rs", TreeKey::Down),
            Some(TreeMove::Open("link".to_owned()))
        );
        assert_eq!(moved(open, "README.md", TreeKey::Down), None);
        assert_eq!(moved(open, "src", TreeKey::Up), None);
        assert_eq!(
            moved(open, "link", TreeKey::Up),
            Some(TreeMove::Open("src/lib.rs".to_owned()))
        );
    }

    #[test]
    fn right_unfolds_then_steps_into_the_first_child() {
        assert_eq!(
            moved(&[], "src", TreeKey::Right),
            Some(TreeMove::Unfold("src".to_owned()))
        );
        assert_eq!(
            moved(&["src"], "src", TreeKey::Right),
            Some(TreeMove::Select("src/ui".to_owned()))
        );
        assert_eq!(moved(&["src"], "src/lib.rs", TreeKey::Right), None);
    }

    #[test]
    fn left_folds_then_steps_out_to_the_parent() {
        assert_eq!(
            moved(&["src"], "src", TreeKey::Left),
            Some(TreeMove::Fold("src".to_owned()))
        );
        assert_eq!(
            moved(&["src"], "src/lib.rs", TreeKey::Left),
            Some(TreeMove::Select("src".to_owned()))
        );
        assert_eq!(
            moved(&["src"], "src/ui", TreeKey::Left),
            Some(TreeMove::Select("src".to_owned()))
        );
        assert_eq!(moved(&[], "README.md", TreeKey::Left), None);
    }

    #[test]
    fn enter_toggles_a_folder_opens_a_file_and_an_unseen_selection_does_nothing() {
        assert_eq!(
            moved(&["src"], "src", TreeKey::Enter),
            Some(TreeMove::Fold("src".to_owned()))
        );
        assert_eq!(
            moved(&[], "src", TreeKey::Enter),
            Some(TreeMove::Unfold("src".to_owned()))
        );
        assert_eq!(
            moved(&[], "link", TreeKey::Enter),
            Some(TreeMove::Open("link".to_owned()))
        );
        assert_eq!(moved(&[], "src/lib.rs", TreeKey::Down), None);
    }
}
