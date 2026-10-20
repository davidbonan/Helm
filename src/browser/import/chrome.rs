//! Chrome's last-used profile: `Bookmarks` (JSON), `History`, `Favicons` and
//! `Cookies` (SQLite, values encrypted with the *Chrome Safe Storage* Keychain
//! item).

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use super::{folder_path, io_error, now_secs, read_database, ImportError, Imported, HISTORY_LIMIT};
use crate::browser::library::{Favorite, Visit};
use crate::browser::{favicons, Cookie};

const CHROME_DIR: &str = "Library/Application Support/Google/Chrome";
const DEFAULT_PROFILE: &str = "Default";
const KEYCHAIN_SERVICE: &str = "Chrome Safe Storage";
/// Each root of `Bookmarks` and the folder its bookmarks land in.
const BOOKMARK_ROOTS: [(&str, &str); 3] = [
    ("bookmark_bar", "Bookmarks Bar"),
    ("other", "Other Bookmarks"),
    ("synced", "Mobile Bookmarks"),
];
/// Microseconds from 1601-01-01 (Chrome's epoch) to 1970-01-01.
const EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;
/// From this `meta` version on, a decrypted value starts with the SHA-256 of
/// its host.
const HOST_HASH_VERSION: i64 = 24;
const HOST_HASH_LEN: usize = 32;

pub(super) fn import(home: &Path) -> Result<Imported, ImportError> {
    let root = home.join(CHROME_DIR);
    if !root.is_dir() {
        return Err(ImportError::NotFound);
    }
    let profile = root.join(last_used_profile(&root));
    let favorites = match std::fs::read_to_string(profile.join("Bookmarks")) {
        Ok(text) => favorites_from_json(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(io_error(&err)),
    };
    let history_db = profile.join("History");
    let history = if history_db.exists() {
        read_database(&history_db, read_history)?
    } else {
        Vec::new()
    };
    let (cookies, cookies_skipped) = match read_cookies(&profile) {
        Ok(cookies) => (cookies, None),
        Err(reason) => (Vec::new(), Some(reason)),
    };
    let icons_db = profile.join("Favicons");
    let sites: HashSet<&str> = favorites
        .iter()
        .filter_map(|favorite| favicons::site_of(&favorite.url))
        .collect();
    let icons = if icons_db.exists() {
        read_database(&icons_db, |connection| read_icons(connection, &sites)).unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(Imported {
        favorites,
        history,
        cookies,
        cookies_skipped,
        icons,
    })
}

/// `Local State` names the profile last used; `Default` otherwise.
fn last_used_profile(root: &Path) -> String {
    std::fs::read_to_string(root.join("Local State"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|state| state["profile"]["last_used"].as_str().map(str::to_owned))
        .filter(|name| root.join(name).is_dir())
        .unwrap_or_else(|| DEFAULT_PROFILE.to_owned())
}

/// Every bookmark of the bar, *Other* and *Mobile*, each with its folder path.
fn favorites_from_json(text: &str) -> Vec<Favorite> {
    let Ok(bookmarks) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let mut favorites = Vec::new();
    for (root, folder) in BOOKMARK_ROOTS {
        collect_bookmarks(&bookmarks["roots"][root], folder, &mut favorites);
    }
    favorites
}

fn collect_bookmarks(folder_node: &Value, folder: &str, favorites: &mut Vec<Favorite>) {
    for child in folder_node["children"].as_array().into_iter().flatten() {
        let name = child["name"].as_str().unwrap_or_default();
        match child["url"].as_str() {
            Some(url) => favorites.push(Favorite {
                title: name.to_owned(),
                url: url.to_owned(),
                folder: Some(folder.to_owned()),
                ..Favorite::default()
            }),
            None => collect_bookmarks(child, &folder_path(Some(folder), name), favorites),
        }
    }
}

fn read_history(connection: &rusqlite::Connection) -> rusqlite::Result<Vec<Visit>> {
    let mut query = connection.prepare(
        "SELECT url, title, visit_count, last_visit_time FROM urls \
         WHERE hidden = 0 AND visit_count > 0 \
         ORDER BY visit_count DESC, last_visit_time DESC LIMIT ?1",
    )?;
    let visits = query.query_map([HISTORY_LIMIT as i64], |row| {
        Ok(Visit {
            url: row.get(0)?,
            title: row.get(1)?,
            count: row.get(2)?,
            last: unix_secs(row.get(3)?).max(0.0) as u64,
        })
    })?;
    visits.collect()
}

/// The largest bitmap Chrome keeps for each of `sites`, from any of its pages.
fn read_icons(
    connection: &rusqlite::Connection,
    sites: &HashSet<&str>,
) -> rusqlite::Result<Vec<(String, Vec<u8>)>> {
    let mut query = connection.prepare(
        "SELECT icon_mapping.page_url, favicon_bitmaps.image_data FROM icon_mapping \
         JOIN favicon_bitmaps ON favicon_bitmaps.icon_id = icon_mapping.icon_id \
         WHERE favicon_bitmaps.image_data IS NOT NULL \
         ORDER BY favicon_bitmaps.width DESC",
    )?;
    let mut rows = query.query([])?;
    let mut found = HashSet::new();
    let mut icons = Vec::new();
    while let Some(row) = rows.next()? {
        let page_url: String = row.get(0)?;
        let Some(site) = favicons::site_of(&page_url).filter(|site| sites.contains(site)) else {
            continue;
        };
        if found.insert(site.to_owned()) {
            icons.push((site.to_owned(), row.get(1)?));
        }
    }
    Ok(icons)
}

fn unix_secs(chrome_micros: i64) -> f64 {
    (chrome_micros - EPOCH_OFFSET_MICROS) as f64 / 1_000_000.0
}

/// The profile's live cookies, or why they cannot be read.
fn read_cookies(profile: &Path) -> Result<Vec<Cookie>, String> {
    let database = ["Network/Cookies", "Cookies"]
        .into_iter()
        .map(|name| profile.join(name))
        .find(|path| path.exists())
        .ok_or("no cookie store")?;
    let key = derive_key(&keychain_password()?).ok_or("key derivation failed")?;
    let now = now_secs();
    read_database(&database, |connection| {
        let version: i64 = connection
            .query_row("SELECT value FROM meta WHERE key = 'version'", [], |row| {
                row.get::<_, String>(0)
            })
            .map_or(0, |version| version.parse().unwrap_or(0));
        let host_hashed = version >= HOST_HASH_VERSION;
        let mut query = connection.prepare(
            "SELECT host_key, name, value, encrypted_value, path, expires_utc, \
             is_secure, is_httponly FROM cookies",
        )?;
        let rows = query.query_map([], |row| {
            let plain: String = row.get(2)?;
            let encrypted: Vec<u8> = row.get(3)?;
            let expires_utc: i64 = row.get(5)?;
            let value = if plain.is_empty() {
                decrypt_value(&encrypted, &key, host_hashed)
            } else {
                Some(plain)
            };
            Ok(value.map(|value| Cookie {
                domain: row.get(0).unwrap_or_default(),
                name: row.get(1).unwrap_or_default(),
                value,
                path: row.get(4).unwrap_or_default(),
                expires: (expires_utc != 0).then(|| unix_secs(expires_utc)),
                secure: row.get(6).unwrap_or_default(),
                http_only: row.get(7).unwrap_or_default(),
            }))
        })?;
        Ok(rows
            .filter_map(Result::ok)
            .flatten()
            .filter(|cookie| cookie.expires.is_none_or(|expires| expires > now))
            .collect())
    })
    .map_err(|err| err.to_string())
}

fn keychain_password() -> Result<String, String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-w", "-s", KEYCHAIN_SERVICE])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err("Keychain access refused".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// AES-128-CBC behind a `v10` prefix, key and IV fixed by Chrome on macOS.
fn decrypt_value(encrypted: &[u8], key: &[u8; 16], host_hashed: bool) -> Option<String> {
    let cipher = encrypted.strip_prefix(b"v10")?;
    let plain = crypto::aes128_cbc_decrypt(key, &[b' '; 16], cipher)?;
    let value = if host_hashed {
        plain.get(HOST_HASH_LEN..)?
    } else {
        &plain
    };
    String::from_utf8(value.to_vec()).ok()
}

fn derive_key(password: &str) -> Option<[u8; 16]> {
    crypto::pbkdf2_sha1(password.as_bytes(), b"saltysalt", 1003)
}

/// CommonCrypto (libSystem): no crypto crate for two calls.
mod crypto {
    use std::ffi::c_void;

    const PBKDF2: u32 = 2;
    const PRF_HMAC_SHA1: u32 = 1;
    const DECRYPT: u32 = 1;
    const AES: u32 = 0;
    const PKCS7_PADDING: u32 = 1;
    const SUCCESS: i32 = 0;

    extern "C" {
        fn CCKeyDerivationPBKDF(
            algorithm: u32,
            password: *const u8,
            password_len: usize,
            salt: *const u8,
            salt_len: usize,
            prf: u32,
            rounds: u32,
            derived_key: *mut u8,
            derived_key_len: usize,
        ) -> i32;
        fn CCCrypt(
            op: u32,
            alg: u32,
            options: u32,
            key: *const c_void,
            key_length: usize,
            iv: *const c_void,
            data_in: *const c_void,
            data_in_length: usize,
            data_out: *mut c_void,
            data_out_available: usize,
            data_out_moved: *mut usize,
        ) -> i32;
    }

    pub(super) fn pbkdf2_sha1(password: &[u8], salt: &[u8], rounds: u32) -> Option<[u8; 16]> {
        let mut key = [0u8; 16];
        let status = unsafe {
            CCKeyDerivationPBKDF(
                PBKDF2,
                password.as_ptr(),
                password.len(),
                salt.as_ptr(),
                salt.len(),
                PRF_HMAC_SHA1,
                rounds,
                key.as_mut_ptr(),
                key.len(),
            )
        };
        (status == SUCCESS).then_some(key)
    }

    pub(super) fn aes128_cbc_decrypt(
        key: &[u8; 16],
        iv: &[u8; 16],
        data: &[u8],
    ) -> Option<Vec<u8>> {
        let mut out = vec![0u8; data.len() + 16];
        let mut moved = 0usize;
        let status = unsafe {
            CCCrypt(
                DECRYPT,
                AES,
                PKCS7_PADDING,
                key.as_ptr().cast(),
                key.len(),
                iv.as_ptr().cast(),
                data.as_ptr().cast(),
                data.len(),
                out.as_mut_ptr().cast(),
                out.len(),
                &mut moved,
            )
        };
        if status != SUCCESS {
            return None;
        }
        out.truncate(moved);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_keep_their_folder_path() {
        let json = r#"{"roots": {
            "bookmark_bar": {"type": "folder", "children": [
                {"type": "url", "name": "Docs", "url": "https://docs.rs/"},
                {"type": "folder", "name": "Work", "children": [
                    {"type": "url", "name": "Jira", "url": "https://jira.dev/"}
                ]}
            ]},
            "other": {"type": "folder", "children": [
                {"type": "url", "name": "Blog", "url": "https://blog.dev/"}
            ]},
            "synced": {"type": "folder", "children": []}
        }}"#;
        let placed: Vec<(String, Option<String>)> = favorites_from_json(json)
            .into_iter()
            .map(|favorite| (favorite.url, favorite.folder))
            .collect();
        let at = |url: &str, folder: &str| (url.to_owned(), Some(folder.to_owned()));
        assert_eq!(
            placed,
            [
                at("https://docs.rs/", "Bookmarks Bar"),
                at("https://jira.dev/", "Bookmarks Bar › Work"),
                at("https://blog.dev/", "Other Bookmarks"),
            ]
        );
    }

    #[test]
    fn each_favorite_site_takes_its_largest_icon() {
        let connection = rusqlite::Connection::open_in_memory().expect("db");
        connection
            .execute_batch(
                "CREATE TABLE icon_mapping(page_url TEXT, icon_id INTEGER);
                 CREATE TABLE favicon_bitmaps(icon_id INTEGER, image_data BLOB, width INTEGER);
                 INSERT INTO icon_mapping VALUES
                   ('https://docs.rs/std', 1), ('https://docs.rs/', 2), ('https://ads.dev/', 3);
                 INSERT INTO favicon_bitmaps VALUES
                   (1, x'10', 16), (2, x'20', 32), (3, x'30', 64);",
            )
            .expect("schema");
        let icons = read_icons(&connection, &HashSet::from(["docs.rs"])).expect("read");
        assert_eq!(icons, vec![("docs.rs".to_owned(), vec![0x20])]);
    }

    /// Vector made with `openssl enc -aes-128-cbc`, key = PBKDF2-SHA1 of
    /// `test-password`, plaintext = SHA-256(`.example.com`) + `session=abc123`.
    #[test]
    fn a_v10_value_decrypts_past_its_host_hash() {
        let cipher = hex("c9f9d215dfe3fcea9f5f48b78c27252a61339f8511ba847e3060c0caa08c030cdbb924ed163a58d120e590a9ac36f608");
        let encrypted = [b"v10".as_slice(), &cipher].concat();
        let key = derive_key("test-password").unwrap();
        assert_eq!(hex_of(&key), "c0ffe4c25f07f62bfc6ab011d9efa54e");
        assert_eq!(
            decrypt_value(&encrypted, &key, true).as_deref(),
            Some("session=abc123")
        );
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
