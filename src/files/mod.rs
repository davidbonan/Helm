//! The files of a worktree, as the phone browses them (specs/remote.md §7.3), as
//! the Files tab's tree lists them (specs/files.md §3) and its viewer reads them
//! (§4): read only, never outside the worktree, never its `.git`.

pub mod content;
pub mod file_type;
pub mod tab;
pub mod tint;
pub mod tree;

use std::cmp::Ordering;
use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

/// Entries a phone listing carries: the newest ones.
const NEWEST_LISTED: usize = 500;

/// Entries a tree folder carries, then a *N more not shown* row.
const TREE_LISTED: usize = 2_000;

const GIT_DIR: &str = ".git";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRow {
    pub name: String,
    pub dir: bool,
    #[serde(skip)]
    pub symlink: bool,
    pub size: u64,
    pub modified_ms: u64,
}

impl FileRow {
    /// `None` for what no listing shows: `.git`, a name that is not UTF-8.
    fn of(entry: &DirEntry) -> Option<Self> {
        let name = entry.file_name().into_string().ok()?;
        if name == GIT_DIR {
            return None;
        }
        let metadata = entry.metadata().ok()?;
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            name,
            dir: metadata.is_dir(),
            symlink: metadata.is_symlink(),
            size: metadata.len(),
            modified_ms: modified.as_millis() as u64,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub entries: Vec<FileRow>,
    /// Every entry of the directory, listed or not.
    pub total: usize,
}

impl Listing {
    fn capped(mut entries: Vec<FileRow>, max: usize) -> Self {
        let total = entries.len();
        entries.truncate(max);
        Self { entries, total }
    }
}

/// `relative` inside the worktree `root`, symlinks followed: `None` when it leaves
/// the worktree, enters `.git` or does not exist.
pub fn resolve(root: &Path, relative: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let path = root.join(relative).canonicalize().ok()?;
    let inside = path.strip_prefix(&root).ok()?;
    let in_git = inside.components().any(|part| part.as_os_str() == GIT_DIR);
    (!in_git).then_some(path)
}

/// The phone's listing: newest first, symlinks left out.
pub fn list_newest(dir: &Path) -> io::Result<Listing> {
    let mut entries: Vec<FileRow> = rows(dir)?.filter(|row| !row.symlink).collect();
    entries.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Listing::capped(entries, NEWEST_LISTED))
}

/// The tree's listing: folders first, then files, each by natural name order;
/// symlinks listed, flagged, never followed.
pub fn list_tree(dir: &Path) -> io::Result<Listing> {
    let mut entries: Vec<FileRow> = rows(dir)?.collect();
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| natural_cmp(&a.name, &b.name))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Listing::capped(entries, TREE_LISTED))
}

/// Whether the repo's ignore rules match `relative` (to the worktree root);
/// rules that cannot be read ignore nothing.
pub fn is_ignored(repo: &git2::Repository, relative: &Path) -> bool {
    repo.is_path_ignored(relative).unwrap_or(false)
}

/// `a/b/c.rs` ⇒ `a/b`; a top-level entry ⇒ `""`, the root.
pub(crate) fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// `name` inside `folder`, the root being `""`.
pub(crate) fn child_path(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// `a/b/c.rs` ⇒ `a`, `a/b`.
pub(crate) fn folders_above(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(|(slash, _)| &path[..slash])
}

fn rows(dir: &Path) -> io::Result<impl Iterator<Item = FileRow>> {
    Ok(std::fs::read_dir(dir)?.filter_map(|entry| FileRow::of(&entry.ok()?)))
}

/// Case-insensitive, digit runs compared as numbers: `file2` before `file10`.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chunks = chunks(a);
    let mut b_chunks = chunks(b);
    loop {
        let order = match (a_chunks.next(), b_chunks.next()) {
            (Some(a_chunk), Some(b_chunk)) => chunk_cmp(a_chunk, b_chunk),
            (a_chunk, b_chunk) => return a_chunk.is_some().cmp(&b_chunk.is_some()),
        };
        if order.is_ne() {
            return order;
        }
    }
}

/// Runs of ASCII digits and runs of anything else, in order.
fn chunks(name: &str) -> impl Iterator<Item = &str> {
    let mut rest = name;
    std::iter::from_fn(move || {
        let digits = rest.chars().next()?.is_ascii_digit();
        let end = rest
            .find(|c: char| c.is_ascii_digit() != digits)
            .unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(end);
        rest = tail;
        Some(chunk)
    })
}

fn chunk_cmp(a: &str, b: &str) -> Ordering {
    let is_number = |chunk: &str| chunk.starts_with(|c: char| c.is_ascii_digit());
    if is_number(a) && is_number(b) {
        let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
        return a.len().cmp(&b.len()).then_with(|| a.cmp(b));
    }
    let lowered = |chunk: &str| {
        chunk
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<Vec<_>>()
    };
    lowered(a).cmp(&lowered(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_names(dir: &Path) -> Vec<String> {
        let listing = list_tree(dir).unwrap();
        listing.entries.into_iter().map(|row| row.name).collect()
    }

    #[test]
    fn the_tree_lists_folders_first_then_files_in_natural_case_insensitive_order() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["file10", "File2", "b.txt", "a.txt", "file1"] {
            std::fs::write(dir.path().join(file), "").unwrap();
        }
        for folder in ["zeta", "Alpha", "src"] {
            std::fs::create_dir(dir.path().join(folder)).unwrap();
        }

        assert_eq!(
            tree_names(dir.path()),
            ["Alpha", "src", "zeta", "a.txt", "b.txt", "file1", "File2", "file10"]
        );
    }

    #[test]
    fn leading_zeros_and_long_numbers_keep_the_numeric_order() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["v100000000000000000000", "v9", "v010", "v02"] {
            std::fs::write(dir.path().join(file), "").unwrap();
        }

        assert_eq!(
            tree_names(dir.path()),
            ["v02", "v9", "v010", "v100000000000000000000"]
        );
    }

    #[test]
    fn the_git_entry_is_never_listed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "").unwrap();

        assert_eq!(tree_names(dir.path()), [".gitignore"]);
    }

    #[test]
    fn a_tree_folder_lists_two_thousand_entries_and_keeps_the_total() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..TREE_LISTED + 5 {
            std::fs::write(dir.path().join(format!("f{index}")), "").unwrap();
        }

        let listing = list_tree(dir.path()).unwrap();

        assert_eq!(listing.entries.len(), TREE_LISTED);
        assert_eq!(listing.total, TREE_LISTED + 5);
        assert_eq!(listing.entries.last().unwrap().name, "f1999");
    }
}
