//! File tree builder — turns a flat list of file paths (git status / commit
//! detail entries) into IDE-style tree rows: one row per directory level (no
//! single-child compaction), directories before files, alphabetical. Pure
//! domain, `pub` from the lib; the UI owns the collapsed set and the rendering.

use std::collections::{BTreeMap, HashSet};

/// One visible row of the file tree. [`TreeRow::Dir`] groups a directory level
/// (its `full_path` keys the collapsed set); [`TreeRow::File`] is a leaf
/// carrying the index of its entry in the input `paths` slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    Dir {
        name: String,
        full_path: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        index: usize,
        depth: usize,
    },
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: BTreeMap<String, usize>,
}

impl Node {
    fn insert(&mut self, path: &str, index: usize) {
        match path.split_once('/') {
            Some((dir, rest)) => self
                .dirs
                .entry(dir.to_owned())
                .or_default()
                .insert(rest, index),
            None => {
                self.files.insert(path.to_owned(), index);
            }
        }
    }

    fn walk(
        &self,
        prefix: &str,
        depth: usize,
        collapsed: &HashSet<String>,
        out: &mut Vec<TreeRow>,
    ) {
        for (name, child) in &self.dirs {
            let full_path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let is_collapsed = collapsed.contains(&full_path);
            out.push(TreeRow::Dir {
                name: name.clone(),
                full_path: full_path.clone(),
                depth,
                collapsed: is_collapsed,
            });
            if !is_collapsed {
                child.walk(&full_path, depth + 1, collapsed, out);
            }
        }
        for &index in self.files.values() {
            out.push(TreeRow::File { index, depth });
        }
    }
}

/// Builds the visible tree rows for `paths` (caller order; each
/// [`TreeRow::File`] carries the index back into this slice), hiding the
/// subtree of every directory whose full path is in `collapsed`.
pub fn tree_rows(paths: &[&str], collapsed: &HashSet<String>) -> Vec<TreeRow> {
    let mut root = Node::default();
    for (index, path) in paths.iter().enumerate() {
        root.insert(path, index);
    }
    let mut out = Vec::new();
    root.walk("", 0, collapsed, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, full_path: &str, depth: usize, collapsed: bool) -> TreeRow {
        TreeRow::Dir {
            name: name.to_owned(),
            full_path: full_path.to_owned(),
            depth,
            collapsed,
        }
    }

    fn collapsed(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn root_files_sit_at_depth_zero_sorted() {
        let rows = tree_rows(&["README.md", "Cargo.toml"], &HashSet::new());
        assert_eq!(
            rows,
            vec![
                TreeRow::File { index: 1, depth: 0 },
                TreeRow::File { index: 0, depth: 0 },
            ]
        );
    }

    #[test]
    fn dirs_group_before_files_alphabetically() {
        let paths = ["zeta.txt", "src/main.rs", "alpha.txt", "src/lib.rs"];
        let rows = tree_rows(&paths, &HashSet::new());
        assert_eq!(
            rows,
            vec![
                dir("src", "src", 0, false),
                TreeRow::File { index: 3, depth: 1 },
                TreeRow::File { index: 1, depth: 1 },
                TreeRow::File { index: 2, depth: 0 },
                TreeRow::File { index: 0, depth: 0 },
            ]
        );
    }

    #[test]
    fn nested_dirs_emit_one_row_per_level() {
        let rows = tree_rows(&["src/git/file_tree.rs"], &HashSet::new());
        assert_eq!(
            rows,
            vec![
                dir("src", "src", 0, false),
                dir("git", "src/git", 1, false),
                TreeRow::File { index: 0, depth: 2 },
            ]
        );
    }

    #[test]
    fn collapsed_dir_hides_its_subtree() {
        let paths = ["src/git/mod.rs", "src/lib.rs", "README.md"];
        let rows = tree_rows(&paths, &collapsed(&["src"]));
        assert_eq!(
            rows,
            vec![
                dir("src", "src", 0, true),
                TreeRow::File { index: 2, depth: 0 },
            ]
        );
    }

    #[test]
    fn collapse_applies_per_directory_path() {
        let paths = ["src/git/mod.rs", "src/git/diff.rs", "src/main.rs"];
        let rows = tree_rows(&paths, &collapsed(&["src/git"]));
        assert_eq!(
            rows,
            vec![
                dir("src", "src", 0, false),
                dir("git", "src/git", 1, true),
                TreeRow::File { index: 2, depth: 1 },
            ]
        );
    }

    #[test]
    fn file_rows_carry_their_input_index() {
        let paths = ["b/two.rs", "a/one.rs", "root.rs"];
        let rows = tree_rows(&paths, &HashSet::new());
        let files: Vec<(usize, usize)> = rows
            .iter()
            .filter_map(|r| match r {
                TreeRow::File { index, depth } => Some((*index, *depth)),
                TreeRow::Dir { .. } => None,
            })
            .collect();
        assert_eq!(files, vec![(1, 1), (0, 1), (2, 0)]);
    }
}
