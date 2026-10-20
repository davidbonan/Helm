//! The sections of the favorites page (browser.md §7): This project, Starred,
//! Frequently visited, and each import source's folder tree.

use super::import::{ImportSource, FOLDER_SEPARATOR};
use super::library::{mentions, Library};

/// The most visited pages Frequently visited lists.
const FREQUENT_LIMIT: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Section {
    /// The project's local favorites (the page adds the Dev server).
    Project,
    /// The shared favorites starred by hand.
    Starred,
    Frequent,
    /// An imported folder; `path: None` holds the favorites at the source's root.
    Folder {
        source: Option<ImportSource>,
        path: Option<String>,
    },
}

/// One page a section lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    pub title: &'a str,
    pub url: &'a str,
    /// A favorite (renamed, removed) rather than a history page.
    pub favorite: bool,
}

/// An import source's folders, in tree order.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceGroup {
    /// `None`: favorites imported before the source was kept.
    pub source: Option<ImportSource>,
    pub rows: Vec<FolderRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderRow {
    pub section: Section,
    /// The folder's own name; `None` for the source's root.
    pub name: Option<String>,
    pub depth: usize,
    /// The favorites right in it, not in its subfolders.
    pub count: usize,
}

/// A folder seen while walking the favorites, in first-seen order.
struct Tally {
    path: Option<String>,
    count: usize,
}

impl Library {
    /// What `section` lists in `project`, in the order kept.
    pub fn section(&self, section: &Section, project: &str) -> Vec<Entry<'_>> {
        if *section == Section::Frequent {
            return self
                .most_visited()
                .take(FREQUENT_LIMIT)
                .map(|visit| Entry {
                    title: &visit.title,
                    url: &visit.url,
                    favorite: false,
                })
                .collect();
        }
        self.favorites
            .iter()
            .filter(|favorite| match section {
                Section::Project => favorite.project.as_deref() == Some(project),
                Section::Starred => favorite.is_starred() && favorite.project.is_none(),
                Section::Frequent => false,
                Section::Folder { source, path } => {
                    !favorite.is_starred() && favorite.source == *source && favorite.folder == *path
                }
            })
            .map(|favorite| Entry {
                title: &favorite.title,
                url: &favorite.url,
                favorite: true,
            })
            .collect()
    }

    /// The favorites of `project` whose title or URL contains `query`.
    pub fn search<'a>(&'a self, query: &str, project: &'a str) -> Vec<Entry<'a>> {
        let query = query.trim().to_lowercase();
        self.favorites_in(project)
            .filter(|favorite| mentions(&query, &favorite.url, &favorite.title))
            .map(|favorite| Entry {
                title: &favorite.title,
                url: &favorite.url,
                favorite: true,
            })
            .collect()
    }

    /// Each import source's folder tree, sources in the order they came;
    /// a folder with only subfolders still has its row.
    pub fn imported_groups(&self) -> Vec<SourceGroup> {
        let mut tallies: Vec<(Option<ImportSource>, Vec<Tally>)> = Vec::new();
        for favorite in self
            .favorites
            .iter()
            .filter(|favorite| !favorite.is_starred())
        {
            let at = match tallies
                .iter()
                .position(|(source, _)| *source == favorite.source)
            {
                Some(at) => at,
                None => {
                    tallies.push((favorite.source, Vec::new()));
                    tallies.len() - 1
                }
            };
            count_in(&mut tallies[at].1, favorite.folder.as_deref());
        }
        tallies
            .into_iter()
            .map(|(source, tallies)| SourceGroup {
                source,
                rows: tree_rows(source, &tallies),
            })
            .collect()
    }
}

/// One more favorite in `path`, its ancestors seen before it.
fn count_in(tallies: &mut Vec<Tally>, path: Option<&str>) {
    if let Some(path) = path {
        let mut ancestor = String::new();
        for name in path.split(FOLDER_SEPARATOR) {
            if !ancestor.is_empty() {
                ancestor.push_str(FOLDER_SEPARATOR);
            }
            ancestor.push_str(name);
            if !tallies
                .iter()
                .any(|tally| tally.path.as_deref() == Some(&ancestor))
            {
                tallies.push(Tally {
                    path: Some(ancestor.clone()),
                    count: 0,
                });
            }
        }
    } else if !tallies.iter().any(|tally| tally.path.is_none()) {
        tallies.push(Tally {
            path: None,
            count: 0,
        });
    }
    if let Some(tally) = tallies
        .iter_mut()
        .find(|tally| tally.path.as_deref() == path)
    {
        tally.count += 1;
    }
}

/// Folders depth first, siblings in first-seen order; the root's own
/// favorites last.
fn tree_rows(source: Option<ImportSource>, tallies: &[Tally]) -> Vec<FolderRow> {
    let mut rows = Vec::new();
    push_children(source, tallies, None, &mut rows);
    if let Some(root) = tallies.iter().find(|tally| tally.path.is_none()) {
        rows.push(FolderRow {
            section: Section::Folder { source, path: None },
            name: None,
            depth: 0,
            count: root.count,
        });
    }
    rows
}

fn push_children(
    source: Option<ImportSource>,
    tallies: &[Tally],
    parent: Option<&str>,
    rows: &mut Vec<FolderRow>,
) {
    for tally in tallies {
        let Some(path) = tally.path.as_deref() else {
            continue;
        };
        let (own_parent, name) = match path.rsplit_once(FOLDER_SEPARATOR) {
            Some((own_parent, name)) => (Some(own_parent), name),
            None => (None, path),
        };
        if own_parent != parent {
            continue;
        }
        rows.push(FolderRow {
            section: Section::Folder {
                source,
                path: Some(path.to_owned()),
            },
            name: Some(name.to_owned()),
            depth: path.matches(FOLDER_SEPARATOR).count(),
            count: tally.count,
        });
        push_children(source, tallies, Some(path), rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::library::{Favorite, Visit};

    const PROJECT: &str = "/dev/helm";

    fn imported(source: ImportSource, folder: Option<&str>, url: &str) -> Favorite {
        Favorite {
            title: url.to_owned(),
            url: url.to_owned(),
            folder: folder.map(str::to_owned),
            source: Some(source),
            project: None,
        }
    }

    fn row(name: Option<&str>, depth: usize, count: usize) -> (Option<String>, usize, usize) {
        (name.map(str::to_owned), depth, count)
    }

    #[test]
    fn imported_folders_form_a_tree_with_the_root_favorites_last() {
        let mut library = Library::default();
        library.add_favorites(vec![
            imported(ImportSource::Safari, None, "https://loose.dev/"),
            imported(
                ImportSource::Safari,
                Some("Bar › Work › Infra"),
                "https://a.dev/",
            ),
            imported(ImportSource::Safari, Some("Bar"), "https://b.dev/"),
            imported(ImportSource::Safari, Some("Other"), "https://c.dev/"),
            imported(ImportSource::Safari, Some("Bar › Perso"), "https://d.dev/"),
        ]);
        let groups = library.imported_groups();
        assert_eq!(groups.len(), 1);
        let rows: Vec<_> = groups[0]
            .rows
            .iter()
            .map(|r| (r.name.clone(), r.depth, r.count))
            .collect();
        assert_eq!(
            rows,
            [
                row(Some("Bar"), 0, 1),
                row(Some("Work"), 1, 0),
                row(Some("Infra"), 2, 1),
                row(Some("Perso"), 1, 1),
                row(Some("Other"), 0, 1),
                row(None, 0, 1),
            ]
        );
    }

    #[test]
    fn each_source_has_its_own_group_and_starred_pages_none() {
        let mut library = Library::default();
        library.toggle_favorite("https://mine.dev/", "Mine", PROJECT);
        library.add_favorites(vec![
            imported(
                ImportSource::Chrome,
                Some("Bookmarks Bar"),
                "https://a.dev/",
            ),
            imported(
                ImportSource::Safari,
                Some("Favorites Bar"),
                "https://b.dev/",
            ),
        ]);
        let sources: Vec<_> = library
            .imported_groups()
            .into_iter()
            .map(|group| group.source)
            .collect();
        assert_eq!(
            sources,
            [Some(ImportSource::Chrome), Some(ImportSource::Safari)]
        );
        let section = Section::Folder {
            source: Some(ImportSource::Chrome),
            path: Some("Bookmarks Bar".to_owned()),
        };
        let urls: Vec<_> = library
            .section(&section, PROJECT)
            .iter()
            .map(|e| e.url)
            .collect();
        assert_eq!(urls, ["https://a.dev/"]);
    }

    #[test]
    fn local_pages_starred_in_a_project_stay_in_it() {
        let mut library = Library::default();
        library.toggle_favorite("http://localhost:3000/admin", "Admin", PROJECT);
        library.toggle_favorite("https://docs.rs/", "Docs", PROJECT);
        let urls = |section: Section, project: &str| -> Vec<String> {
            library
                .section(&section, project)
                .iter()
                .map(|entry| entry.url.to_owned())
                .collect()
        };
        assert_eq!(
            urls(Section::Project, PROJECT),
            ["http://localhost:3000/admin"]
        );
        assert_eq!(urls(Section::Starred, PROJECT), ["https://docs.rs/"]);
        assert!(urls(Section::Project, "/dev/other").is_empty());
        assert!(!library.is_favorite("http://localhost:3000/admin", "/dev/other"));
        assert!(library.is_favorite("https://docs.rs/", "/dev/other"));
    }

    #[test]
    fn frequently_visited_lists_the_most_visited_pages() {
        let mut library = Library::default();
        library.merge_history(
            (0..20)
                .map(|at| Visit {
                    url: format!("https://site{at}.dev/"),
                    title: String::new(),
                    count: at,
                    last: 0,
                })
                .collect(),
        );
        let entries = library.section(&Section::Frequent, PROJECT);
        assert_eq!(entries.len(), FREQUENT_LIMIT);
        assert_eq!(entries[0].url, "https://site19.dev/");
        assert!(!entries[0].favorite);
    }

    #[test]
    fn the_filter_searches_every_favorite_the_project_sees() {
        let mut library = Library::default();
        library.toggle_favorite("http://localhost:3000/", "Admin docs", "/dev/other");
        library.toggle_favorite("https://docs.rs/", "Docs", PROJECT);
        library.add_favorites(vec![imported(
            ImportSource::Chrome,
            Some("Bar"),
            "https://jira.dev/docs",
        )]);
        let urls: Vec<_> = library
            .search("DOCS", PROJECT)
            .iter()
            .map(|e| e.url)
            .collect();
        assert_eq!(urls, ["https://docs.rs/", "https://jira.dev/docs"]);
    }
}
