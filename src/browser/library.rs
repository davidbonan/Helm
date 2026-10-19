//! Favorites and history of the browser panes (browser.md §7, §8), one library
//! for every project, kept in helm's support dir.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::import::ImportSource;
use super::url;

const LIBRARY_FILE: &str = "browser.json";
const HISTORY_LIMIT: usize = 5000;
const SUGGESTION_LIMIT: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub title: String,
    pub url: String,
    /// The bookmark folder it was imported from; `None` for a starred page or one
    /// at its source's root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The browser it was imported from; `None` for a starred page, or one
    /// imported before the source was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ImportSource>,
    /// The project (group root) a local page was starred in; `None` for a
    /// favorite every project shares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

impl Favorite {
    /// Listed in `project`: its own, or shared.
    pub fn is_seen_in(&self, project: &str) -> bool {
        self.project.as_deref().is_none_or(|own| own == project)
    }

    /// Starred by hand rather than imported.
    pub fn is_starred(&self) -> bool {
        self.source.is_none() && self.folder.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub count: u32,
    /// Unix seconds.
    pub last: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub title: String,
    pub url: String,
    pub favorite: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub favorites: Vec<Favorite>,
    pub history: Vec<Visit>,
}

impl Library {
    /// The favorites listed in `project`: its own and the shared ones.
    pub fn favorites_in<'a>(&'a self, project: &'a str) -> impl Iterator<Item = &'a Favorite> {
        self.favorites
            .iter()
            .filter(move |favorite| favorite.is_seen_in(project))
    }

    pub fn is_favorite(&self, url: &str, project: &str) -> bool {
        self.favorites_in(project)
            .any(|favorite| favorite.url == url)
    }

    /// Adds the page — a local one to `project` alone — or removes it when it
    /// already is a favorite there.
    pub fn toggle_favorite(&mut self, url: &str, title: &str, project: &str) {
        if self.is_favorite(url, project) {
            self.remove_favorite(url, project);
            return;
        }
        let title = if title.trim().is_empty() {
            url::display_parts(url).0
        } else {
            title.trim()
        };
        self.favorites.push(Favorite {
            title: title.to_owned(),
            url: url.to_owned(),
            project: url::is_local(url).then(|| project.to_owned()),
            ..Favorite::default()
        });
    }

    pub fn remove_favorite(&mut self, url: &str, project: &str) {
        self.favorites
            .retain(|favorite| !(favorite.url == url && favorite.is_seen_in(project)));
    }

    /// A blank title keeps the old one.
    pub fn rename_favorite(&mut self, url: &str, project: &str, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        for favorite in self.favorites.iter_mut() {
            if favorite.url == url && favorite.is_seen_in(project) {
                title.clone_into(&mut favorite.title);
            }
        }
    }

    /// Appends the web favorites no project shares yet; how many came in.
    pub fn add_favorites(&mut self, favorites: Vec<Favorite>) -> usize {
        let before = self.favorites.len();
        for favorite in favorites {
            let shared = self
                .favorites
                .iter()
                .any(|kept| kept.project.is_none() && kept.url == favorite.url);
            if url::is_web(&favorite.url) && !shared {
                self.favorites.push(favorite);
            }
        }
        self.favorites.len() - before
    }

    /// One more visit of an `http(s)` page.
    pub fn record_visit(&mut self, url: &str, title: &str, now: u64) {
        if !url::is_web(url) {
            return;
        }
        match self.history.iter_mut().find(|visit| visit.url == url) {
            Some(visit) => {
                visit.count += 1;
                visit.last = now;
                if !title.is_empty() {
                    title.clone_into(&mut visit.title);
                }
            }
            None => self.history.push(Visit {
                url: url.to_owned(),
                title: title.to_owned(),
                count: 1,
                last: now,
            }),
        }
        self.prune_history();
    }

    /// Merges imported web visits by URL (highest count, latest visit).
    pub fn merge_history(&mut self, visits: Vec<Visit>) {
        for imported in visits.into_iter().filter(|visit| url::is_web(&visit.url)) {
            match self
                .history
                .iter_mut()
                .find(|visit| visit.url == imported.url)
            {
                Some(visit) => {
                    visit.count = visit.count.max(imported.count);
                    visit.last = visit.last.max(imported.last);
                    if visit.title.is_empty() {
                        visit.title = imported.title;
                    }
                }
                None => self.history.push(imported),
            }
        }
        self.prune_history();
    }

    /// The favorites of `project` first, then history by visits and recency,
    /// whose URL or title contains `query`.
    pub fn suggestions(&self, query: &str, project: &str) -> Vec<Suggestion> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let favorites = self
            .favorites_in(project)
            .filter(|favorite| mentions(&query, &favorite.url, &favorite.title))
            .map(|favorite| Suggestion {
                title: favorite.title.clone(),
                url: favorite.url.clone(),
                favorite: true,
            });
        let favorite_urls: HashSet<&str> = self
            .favorites_in(project)
            .map(|favorite| favorite.url.as_str())
            .collect();
        let visits = self.most_visited().filter(|visit| {
            !favorite_urls.contains(visit.url.as_str())
                && mentions(&query, &visit.url, &visit.title)
        });
        let history = visits.map(|visit| Suggestion {
            title: visit.title.clone(),
            url: visit.url.clone(),
            favorite: false,
        });
        favorites.chain(history).take(SUGGESTION_LIMIT).collect()
    }

    /// The history, most visited then most recent first.
    pub fn most_visited(&self) -> impl Iterator<Item = &Visit> {
        let mut visits: Vec<&Visit> = self.history.iter().collect();
        visits.sort_by_key(|visit| std::cmp::Reverse(rank(visit)));
        visits.into_iter()
    }

    fn prune_history(&mut self) {
        if self.history.len() <= HISTORY_LIMIT {
            return;
        }
        self.history
            .sort_by_key(|visit| std::cmp::Reverse(rank(visit)));
        self.history.truncate(HISTORY_LIMIT);
    }
}

/// `query`, lowercased, within the URL or the title, case-insensitive.
pub(crate) fn mentions(query: &str, url: &str, title: &str) -> bool {
    url.to_lowercase().contains(query) || title.to_lowercase().contains(query)
}

fn rank(visit: &Visit) -> (u32, u64) {
    (visit.count, visit.last)
}

/// The library and the file it is saved to (none in memory).
#[derive(Default)]
pub struct LibraryStore {
    pub library: Library,
    path: Option<PathBuf>,
    dirty: bool,
}

impl LibraryStore {
    pub fn load() -> Self {
        match crate::persistence::support_file(LIBRARY_FILE) {
            Some(path) => Self::load_from(path),
            None => Self::default(),
        }
    }

    pub fn load_from(path: PathBuf) -> Self {
        Self {
            library: read_library(&path),
            path: Some(path),
            dirty: false,
        }
    }

    /// Edits the library; written on the next `save`.
    pub fn edit<R>(&mut self, edit: impl FnOnce(&mut Library) -> R) -> R {
        self.dirty = true;
        edit(&mut self.library)
    }

    pub fn save(&mut self) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let Some(path) = &self.path else {
            return;
        };
        let written = serde_json::to_string(&self.library)
            .map_err(std::io::Error::other)
            .and_then(|text| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(path, text)
            });
        if let Err(err) = written {
            eprintln!("helm: cannot save {}: {err}", path.display());
        }
    }
}

fn read_library(path: &Path) -> Library {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Library::default(),
        Err(err) => {
            eprintln!("helm: cannot read {}: {err}", path.display());
            return Library::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|err| {
        eprintln!("helm: cannot parse {}: {err}", path.display());
        Library::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "/dev/helm";

    fn favorite(title: &str, url: &str) -> Favorite {
        Favorite {
            title: title.to_owned(),
            url: url.to_owned(),
            ..Favorite::default()
        }
    }

    fn visit(url: &str, title: &str, count: u32, last: u64) -> Visit {
        Visit {
            url: url.to_owned(),
            title: title.to_owned(),
            count,
            last,
        }
    }

    #[test]
    fn starring_twice_removes_and_an_untitled_page_takes_its_host() {
        let mut library = Library::default();
        library.toggle_favorite("https://a.dev/admin", "", PROJECT);
        assert_eq!(
            library.favorites,
            vec![favorite("a.dev", "https://a.dev/admin")]
        );
        library.toggle_favorite("https://a.dev/admin", "", PROJECT);
        assert!(library.favorites.is_empty());
    }

    #[test]
    fn imported_favorites_skip_the_urls_already_there_and_non_web_ones() {
        let mut library = Library::default();
        library.toggle_favorite("https://a.dev/", "A", PROJECT);
        let added = library.add_favorites(vec![
            favorite("A again", "https://a.dev/"),
            favorite("Settings", "chrome://settings/"),
            favorite("B", "https://b.dev/"),
        ]);
        assert_eq!(added, 1);
        assert_eq!(library.favorites[1].url, "https://b.dev/");
    }

    #[test]
    fn a_blank_rename_keeps_the_title() {
        let mut library = Library::default();
        library.toggle_favorite("https://a.dev/", "A", PROJECT);
        library.rename_favorite("https://a.dev/", PROJECT, "  ");
        library.rename_favorite("https://a.dev/", PROJECT, " Admin ");
        assert_eq!(library.favorites, vec![favorite("Admin", "https://a.dev/")]);
    }

    #[test]
    fn visits_count_only_web_pages() {
        let mut library = Library::default();
        library.record_visit("https://a.dev/", "A", 10);
        library.record_visit("https://a.dev/", "A", 20);
        library.record_visit("data:text/html,x", "", 30);
        assert_eq!(library.history, vec![visit("https://a.dev/", "A", 2, 20)]);
    }

    #[test]
    fn imported_history_keeps_the_highest_count_and_latest_visit() {
        let mut library = Library::default();
        library.merge_history(vec![
            visit("https://a.dev/", "A", 5, 100),
            visit("chrome-extension://x/", "X", 9, 100),
        ]);
        library.merge_history(vec![visit("https://a.dev/", "A", 3, 200)]);
        assert_eq!(library.history, vec![visit("https://a.dev/", "A", 5, 200)]);
    }

    #[test]
    fn suggestions_list_favorites_then_the_most_visited_history() {
        let mut library = Library::default();
        library.add_favorites(vec![favorite("Docs", "https://docs.rs/")]);
        library.merge_history(vec![
            visit("https://docs.rs/", "Docs", 9, 1),
            visit("https://rust-lang.org/docs", "Rust", 2, 5),
            visit("https://example.com/", "Rare docs", 7, 1),
            visit("https://other.dev/", "Other", 50, 1),
        ]);
        let urls: Vec<String> = library
            .suggestions("DOCS", PROJECT)
            .into_iter()
            .map(|suggestion| suggestion.url)
            .collect();
        assert_eq!(
            urls,
            [
                "https://docs.rs/",
                "https://example.com/",
                "https://rust-lang.org/docs"
            ]
        );
    }

    #[test]
    fn the_store_reads_back_what_it_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LIBRARY_FILE);
        let mut store = LibraryStore::load_from(path.clone());
        store.edit(|library| library.toggle_favorite("https://a.dev/", "A", PROJECT));
        store.save();
        assert_eq!(
            LibraryStore::load_from(path).library.favorites,
            vec![favorite("A", "https://a.dev/")]
        );
    }
}
