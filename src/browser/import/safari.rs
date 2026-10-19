//! Safari's data: `Bookmarks.plist`, `History.db` (SQLite) and
//! `Cookies.binarycookies`, all behind macOS's Full Disk Access.

use std::path::Path;

use plist::Value;

use super::{folder_path, io_error, now_secs, read_database, ImportError, Imported, HISTORY_LIMIT};
use crate::browser::library::{Favorite, Visit};
use crate::browser::Cookie;

const SAFARI_DIR: &str = "Library/Safari";
const COOKIE_FILES: [&str; 2] = [
    "Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies",
    "Library/Cookies/Cookies.binarycookies",
];
const READING_LIST: &str = "com.apple.ReadingList";
/// Seconds from 1970-01-01 to 2001-01-01, Apple's reference date.
const APPLE_EPOCH_OFFSET: f64 = 978_307_200.0;
const SECURE_FLAG: u32 = 1;
const HTTP_ONLY_FLAG: u32 = 4;

pub(super) fn import(home: &Path) -> Result<Imported, ImportError> {
    let root = home.join(SAFARI_DIR);
    if !root.is_dir() {
        return Err(ImportError::NotFound);
    }
    let bookmarks =
        Value::from_file(root.join("Bookmarks.plist")).map_err(|err| match err.as_io() {
            Some(io) => io_error(io),
            None => ImportError::Failed(err.to_string()),
        })?;
    let history = read_database(&root.join("History.db"), read_history)?;
    let (cookies, cookies_skipped) = match read_cookies(home) {
        Ok(cookies) => (cookies, None),
        Err(reason) => (Vec::new(), Some(reason)),
    };
    Ok(Imported {
        favorites: favorites_from_plist(&bookmarks),
        history,
        cookies,
        cookies_skipped,
        icons: Vec::new(),
    })
}

/// Every bookmark leaf with its folder path, the Reading List left out.
fn favorites_from_plist(bookmarks: &Value) -> Vec<Favorite> {
    let mut favorites = Vec::new();
    collect_bookmarks(bookmarks, None, &mut favorites);
    favorites
}

fn collect_bookmarks(node: &Value, folder: Option<&str>, favorites: &mut Vec<Favorite>) {
    let Some(node) = node.as_dictionary() else {
        return;
    };
    let text = |key: &str| node.get(key).and_then(Value::as_string);
    if let Some(url) = text("URLString") {
        let title = node
            .get("URIDictionary")
            .and_then(Value::as_dictionary)
            .and_then(|uri| uri.get("title"))
            .and_then(Value::as_string)
            .unwrap_or_default();
        favorites.push(Favorite {
            title: title.to_owned(),
            url: url.to_owned(),
            folder: folder.map(str::to_owned),
            ..Favorite::default()
        });
        return;
    }
    let inner = match text("Title") {
        Some(READING_LIST) => return,
        None | Some("") => None,
        Some(title) => Some(folder_path(folder, folder_name(title))),
    };
    let inner = inner.as_deref().or(folder);
    let children = node.get("Children").and_then(Value::as_array);
    for child in children.into_iter().flatten() {
        collect_bookmarks(child, inner, favorites);
    }
}

/// Safari's own folders go by internal titles.
fn folder_name(title: &str) -> &str {
    match title {
        "BookmarksBar" => "Favorites Bar",
        "BookmarksMenu" => "Bookmarks Menu",
        title => title,
    }
}

fn read_history(connection: &rusqlite::Connection) -> rusqlite::Result<Vec<Visit>> {
    // SQLite takes the bare `title` from the row holding MAX(visit_time).
    let mut query = connection.prepare(
        "SELECT i.url, COALESCE(v.title, ''), i.visit_count, MAX(v.visit_time) \
         FROM history_items i JOIN history_visits v ON v.history_item = i.id \
         GROUP BY i.id ORDER BY i.visit_count DESC, MAX(v.visit_time) DESC LIMIT ?1",
    )?;
    let visits = query.query_map([HISTORY_LIMIT as i64], |row| {
        let last: f64 = row.get(3)?;
        Ok(Visit {
            url: row.get(0)?,
            title: row.get(1)?,
            count: row.get(2)?,
            last: (last + APPLE_EPOCH_OFFSET).max(0.0) as u64,
        })
    })?;
    visits.collect()
}

fn read_cookies(home: &Path) -> Result<Vec<Cookie>, String> {
    let path = COOKIE_FILES
        .iter()
        .map(|file| home.join(file))
        .find(|path| path.exists())
        .ok_or("no cookie store")?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let now = now_secs();
    let cookies = cookies_from_binary(&bytes).ok_or("unreadable cookie store")?;
    Ok(cookies
        .into_iter()
        .filter(|cookie| cookie.expires.is_none_or(|expires| expires > now))
        .collect())
}

/// `Cookies.binarycookies`: `cook`, big-endian page sizes, then pages of
/// little-endian cookie records.
fn cookies_from_binary(bytes: &[u8]) -> Option<Vec<Cookie>> {
    if bytes.get(..4)? != b"cook" {
        return None;
    }
    let pages = be_u32(bytes, 4)? as usize;
    let mut page_start = 8 + 4 * pages;
    let mut cookies = Vec::new();
    for index in 0..pages {
        let size = be_u32(bytes, 8 + 4 * index)? as usize;
        let page = bytes.get(page_start..page_start + size)?;
        let count = le_u32(page, 4)? as usize;
        for slot in 0..count {
            let offset = le_u32(page, 8 + 4 * slot)? as usize;
            cookies.push(cookie_record(page.get(offset..)?)?);
        }
        page_start += size;
    }
    Some(cookies)
}

fn cookie_record(record: &[u8]) -> Option<Cookie> {
    let flags = le_u32(record, 8)?;
    let text = |field: usize| -> Option<String> {
        let start = le_u32(record, field)? as usize;
        let tail = record.get(start..)?;
        let end = tail.iter().position(|&byte| byte == 0)?;
        Some(String::from_utf8_lossy(&tail[..end]).into_owned())
    };
    let expires = f64::from_le_bytes(record.get(40..48)?.try_into().ok()?);
    Some(Cookie {
        domain: text(16)?,
        name: text(20)?,
        path: text(24)?,
        value: text(28)?,
        expires: (expires != 0.0).then_some(expires + APPLE_EPOCH_OFFSET),
        secure: flags & SECURE_FLAG != 0,
        http_only: flags & HTTP_ONLY_FLAG != 0,
    })
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use plist::Dictionary;

    use super::*;

    fn leaf(title: &str, url: &str) -> Value {
        let mut uri = Dictionary::new();
        uri.insert("title".into(), title.into());
        let mut node = Dictionary::new();
        node.insert("WebBookmarkType".into(), "WebBookmarkTypeLeaf".into());
        node.insert("URLString".into(), url.into());
        node.insert("URIDictionary".into(), uri.into());
        node.into()
    }

    fn folder(title: &str, children: Vec<Value>) -> Value {
        let mut node = Dictionary::new();
        node.insert("WebBookmarkType".into(), "WebBookmarkTypeList".into());
        node.insert("Title".into(), title.into());
        node.insert("Children".into(), children.into());
        node.into()
    }

    #[test]
    fn bookmarks_keep_their_folder_without_the_reading_list() {
        let root = folder(
            "",
            vec![
                folder("BookmarksBar", vec![leaf("Docs", "https://docs.rs/")]),
                folder(READING_LIST, vec![leaf("Later", "https://later.dev/")]),
                folder(
                    "Work",
                    vec![folder("Deep", vec![leaf("Jira", "https://jira.dev/")])],
                ),
            ],
        );
        assert_eq!(
            favorites_from_plist(&root),
            vec![
                Favorite {
                    title: "Docs".into(),
                    url: "https://docs.rs/".into(),
                    folder: Some("Favorites Bar".into()),
                    ..Favorite::default()
                },
                Favorite {
                    title: "Jira".into(),
                    url: "https://jira.dev/".into(),
                    folder: Some("Work › Deep".into()),
                    ..Favorite::default()
                },
            ]
        );
    }

    /// One page, one secure + HttpOnly cookie, laid out as Safari writes it.
    #[test]
    fn a_binary_cookie_reads_with_its_flags_and_expiry() {
        let mut record = vec![0u8; 56];
        let strings: [&[u8]; 4] = [b".example.com\0", b"sid\0", b"/\0", b"abc\0"];
        let mut fields = Vec::new();
        for text in strings {
            fields.push(record.len() as u32);
            record.extend_from_slice(text);
        }
        let size = record.len() as u32;
        record[0..4].copy_from_slice(&size.to_le_bytes());
        record[8..12].copy_from_slice(&(SECURE_FLAG | HTTP_ONLY_FLAG).to_le_bytes());
        for (slot, offset) in fields.iter().enumerate() {
            record[16 + 4 * slot..20 + 4 * slot].copy_from_slice(&offset.to_le_bytes());
        }
        record[40..48].copy_from_slice(&800_000_000.0_f64.to_le_bytes());

        let mut page = vec![0x00, 0x00, 0x01, 0x00];
        page.extend_from_slice(&1u32.to_le_bytes());
        page.extend_from_slice(&16u32.to_le_bytes());
        page.extend_from_slice(&0u32.to_le_bytes());
        page.extend_from_slice(&record);
        let mut file = b"cook".to_vec();
        file.extend_from_slice(&1u32.to_be_bytes());
        file.extend_from_slice(&(page.len() as u32).to_be_bytes());
        file.extend_from_slice(&page);

        assert_eq!(
            cookies_from_binary(&file),
            Some(vec![Cookie {
                domain: ".example.com".into(),
                name: "sid".into(),
                value: "abc".into(),
                path: "/".into(),
                expires: Some(800_000_000.0 + APPLE_EPOCH_OFFSET),
                secure: true,
                http_only: true,
            }])
        );
    }
}
