//! One-shot import of another browser's favorites, history and cookies
//! (browser.md §9).

mod chrome;
mod safari;

use std::path::{Path, PathBuf};

use super::library::{Favorite, Visit};
use super::Cookie;

/// The most visited pages a source hands over.
const HISTORY_LIMIT: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportSource {
    Chrome,
    Safari,
}

impl ImportSource {
    pub const ALL: [Self; 2] = [Self::Chrome, Self::Safari];

    pub fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Safari => "Safari",
        }
    }
}

#[derive(Debug, Default)]
pub struct Imported {
    pub favorites: Vec<Favorite>,
    pub history: Vec<Visit>,
    pub cookies: Vec<Cookie>,
    /// Why the cookies stayed behind, when they did.
    pub cookies_skipped: Option<String>,
    /// The favorites' site icons, as image files by site (browser.md §10).
    pub icons: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    NotFound,
    /// Safari's files need Full Disk Access for helm.
    NeedsFullDiskAccess,
    Failed(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::NeedsFullDiskAccess => f.write_str("needs Full Disk Access"),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

/// What the start page tells of the last import (browser.md §9).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ImportStatus {
    #[default]
    Idle,
    Running(ImportSource),
    Done(ImportReport),
    Failed(ImportSource, ImportError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub source: ImportSource,
    pub favorites: usize,
    pub history: usize,
    pub cookies: usize,
    pub cookies_skipped: Option<String>,
}

/// Reads `source`'s data. Blocking: Chrome's Keychain prompt waits on the user.
pub fn import(source: ImportSource) -> Result<Imported, ImportError> {
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or(ImportError::NotFound)?;
    let mut imported = match source {
        ImportSource::Chrome => chrome::import(&home),
        ImportSource::Safari => safari::import(&home),
    }?;
    for favorite in &mut imported.favorites {
        favorite.source = Some(source);
    }
    Ok(imported)
}

/// Between the folders of a bookmark's path (`Bookmarks Bar › Work`).
pub const FOLDER_SEPARATOR: &str = " › ";

/// A bookmark folder below `parent`, as the start page heads it.
fn folder_path(parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(parent) => format!("{parent}{FOLDER_SEPARATOR}{name}"),
        None => name.to_owned(),
    }
}

fn io_error(err: &std::io::Error) -> ImportError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ImportError::NotFound,
        std::io::ErrorKind::PermissionDenied => ImportError::NeedsFullDiskAccess,
        _ => ImportError::Failed(err.to_string()),
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// Reads a database the browser may hold open, from a copy (its write-ahead log
/// included) so its lock never stands in the way.
fn read_database<T>(
    path: &Path,
    read: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>,
) -> Result<T, ImportError> {
    let copy = DatabaseCopy::of(path)?;
    let connection = rusqlite::Connection::open(&copy.database)
        .map_err(|err| ImportError::Failed(err.to_string()))?;
    read(&connection).map_err(|err| ImportError::Failed(err.to_string()))
}

struct DatabaseCopy {
    dir: PathBuf,
    database: PathBuf,
}

impl DatabaseCopy {
    fn of(path: &Path) -> Result<Self, ImportError> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let dir = std::env::temp_dir().join(format!("helm-import-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|err| io_error(&err))?;
        let copy = Self {
            database: dir.join("db"),
            dir,
        };
        std::fs::copy(path, &copy.database).map_err(|err| io_error(&err))?;
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        if wal.exists() {
            std::fs::copy(&wal, copy.dir.join("db-wal")).map_err(|err| io_error(&err))?;
        }
        Ok(copy)
    }
}

impl Drop for DatabaseCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
