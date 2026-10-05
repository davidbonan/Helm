//! Phones paired with helm (specs/remote.md §3.2, §3.4): a pairing outlives the
//! server — helm keeps, per device, only the SHA-256 of its session token, rotated
//! on every page load. Kept in its own TOML beside the prefs: the server threads
//! write it, the UI never rewrites it whole.

use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::remote::access::{constant_time_eq, Token};

pub const DEVICES_FILE: &str = "phone_devices.toml";

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// After a rotation the replaced token still opens this long: the page's own
/// requests in flight carry it.
pub const ROTATION_GRACE_MS: u64 = 30 * 1000;

/// A device unseen this long is dropped.
pub const UNSEEN_DROP_MS: u64 = 30 * DAY_MS;

/// The cookie lives as long as an unseen device's pairing.
pub const COOKIE_MAX_AGE_SECS: u64 = UNSEEN_DROP_MS / 1000;

const DEVICE_TOKEN_BYTES: usize = 32;
const DEVICE_ID_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub paired_at_ms: u64,
    pub last_seen_ms: u64,
    token_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_sha256: Option<String>,
    #[serde(default)]
    rotated_at_ms: u64,
}

impl PairedDevice {
    fn opens_with(&self, token_sha256: &str, now_ms: u64) -> bool {
        if now_ms.saturating_sub(self.last_seen_ms) >= UNSEEN_DROP_MS {
            return false;
        }
        constant_time_eq(&self.token_sha256, token_sha256)
            || self.previous_sha256.as_deref().is_some_and(|previous| {
                now_ms.saturating_sub(self.rotated_at_ms) < ROTATION_GRACE_MS
                    && constant_time_eq(previous, token_sha256)
            })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceBook {
    /// Last port the server bound: tried first, so the phone's bookmark holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Last address the server bound: another one leaves the bookmarks dead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Ipv4Addr>,
    /// Gateway MACs of the networks a device paired on (§3.4).
    #[serde(default)]
    pub networks: Vec<String>,
    #[serde(default)]
    pub devices: Vec<PairedDevice>,
}

impl DeviceBook {
    /// A new device and the token its cookie carries.
    pub fn pair(&mut self, name: &str, now_ms: u64) -> Token {
        let token = Token::random(DEVICE_TOKEN_BYTES);
        self.devices.push(PairedDevice {
            id: Token::random(DEVICE_ID_BYTES).as_str().to_owned(),
            name: name.to_owned(),
            paired_at_ms: now_ms,
            last_seen_ms: now_ms,
            token_sha256: sha256_hex(token.as_str()),
            previous_sha256: None,
            rotated_at_ms: 0,
        });
        token
    }

    /// The device `token` opens, if any.
    pub fn device(&self, token: &str, now_ms: u64) -> Option<&PairedDevice> {
        let hash = sha256_hex(token);
        self.devices.iter().find(|d| d.opens_with(&hash, now_ms))
    }

    /// Replaces the token `token` opens with a new one, returned; the replaced one
    /// keeps opening for [`ROTATION_GRACE_MS`]. Counts as a visit.
    pub fn rotate(&mut self, token: &str, now_ms: u64) -> Option<Token> {
        let hash = sha256_hex(token);
        let device = self
            .devices
            .iter_mut()
            .find(|d| d.opens_with(&hash, now_ms))?;
        let fresh = Token::random(DEVICE_TOKEN_BYTES);
        let replaced = std::mem::replace(&mut device.token_sha256, sha256_hex(fresh.as_str()));
        device.previous_sha256 = Some(replaced);
        device.rotated_at_ms = now_ms;
        device.last_seen_ms = now_ms;
        Some(fresh)
    }

    pub fn is_paired(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id == id)
    }

    pub fn revoke(&mut self, id: &str) {
        self.devices.retain(|d| d.id != id);
    }

    pub fn revoke_all(&mut self) {
        self.devices.clear();
    }

    pub fn drop_unseen(&mut self, now_ms: u64) {
        self.devices
            .retain(|d| now_ms.saturating_sub(d.last_seen_ms) < UNSEEN_DROP_MS);
    }

    pub fn record_network(&mut self, gateway_mac: &str) {
        if !self.networks.iter().any(|known| known == gateway_mac) {
            self.networks.push(gateway_mac.to_owned());
        }
    }

    pub fn is_recorded_network(&self, gateway_mac: &str) -> bool {
        self.networks.iter().any(|known| known == gateway_mac)
    }
}

fn sha256_hex(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What the device list shows for a phone, from its `User-Agent`.
pub fn device_name(user_agent: Option<&str>) -> &'static str {
    let agent = user_agent.unwrap_or_default();
    if agent.contains("iPad") {
        "iPad"
    } else if agent.contains("iPhone") {
        "iPhone"
    } else if agent.contains("Android") {
        "Android phone"
    } else {
        "Browser"
    }
}

/// Wall-clock milliseconds: the dates outlive the process.
pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The book shared by the app and the server threads, written to disk on every
/// edit (none when in memory).
#[derive(Clone, Default)]
pub struct PairedDevices {
    book: Arc<Mutex<DeviceBook>>,
    path: Option<PathBuf>,
}

impl PairedDevices {
    /// `phone_devices.toml` in the support dir; the devices unseen too long are
    /// dropped on the way in.
    pub fn load() -> Self {
        match crate::persistence::support_file(DEVICES_FILE) {
            Some(path) => Self::load_from(path),
            None => Self::default(),
        }
    }

    pub fn load_from(path: PathBuf) -> Self {
        let mut book = read_book(&path);
        book.drop_unseen(wall_ms());
        Self {
            book: Arc::new(Mutex::new(book)),
            path: Some(path),
        }
    }

    pub fn read<R>(&self, read: impl FnOnce(&DeviceBook) -> R) -> R {
        read(&self.lock())
    }

    pub fn edit<R>(&self, edit: impl FnOnce(&mut DeviceBook) -> R) -> R {
        let mut book = self.lock();
        let result = edit(&mut book);
        if let Some(path) = &self.path {
            if let Err(err) = write_book(&book, path) {
                eprintln!("helm: cannot save {}: {err}", path.display());
            }
        }
        result
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DeviceBook> {
        self.book
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn read_book(path: &Path) -> DeviceBook {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return DeviceBook::default(),
        Err(err) => {
            eprintln!("helm: cannot read {}: {err}", path.display());
            return DeviceBook::default();
        }
    };
    toml::from_str(&text).unwrap_or_else(|err| {
        eprintln!("helm: cannot parse {}: {err}", path.display());
        DeviceBook::default()
    })
}

/// Owner-only: the hashes open nothing, but the file is nobody else's business.
fn write_book(book: &DeviceBook, path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(toml::to_string_pretty(book)?.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    fn paired() -> (DeviceBook, Token) {
        let mut book = DeviceBook::default();
        let token = book.pair("iPhone", NOW);
        (book, token)
    }

    #[test]
    fn a_paired_token_opens_its_device_and_nothing_else_does() {
        let (book, token) = paired();

        assert_eq!(token.as_str().len(), 64, "256 bits of hex");
        assert_eq!(
            book.device(token.as_str(), NOW).map(|d| d.name.as_str()),
            Some("iPhone")
        );
        assert!(book.device(&token.as_str()[1..], NOW).is_none());
        assert!(
            !toml::to_string(&book).unwrap().contains(token.as_str()),
            "only the hash is kept"
        );
    }

    #[test]
    fn a_rotated_token_keeps_opening_for_the_grace_then_only_the_new_one_does() {
        let (mut book, old) = paired();

        let new = book.rotate(old.as_str(), NOW).unwrap();

        assert!(book.device(new.as_str(), NOW).is_some());
        assert!(book
            .device(old.as_str(), NOW + ROTATION_GRACE_MS - 1)
            .is_some());
        assert!(book.device(old.as_str(), NOW + ROTATION_GRACE_MS).is_none());
        assert!(book.rotate(old.as_str(), NOW + ROTATION_GRACE_MS).is_none());
    }

    #[test]
    fn a_device_unseen_for_thirty_days_opens_nothing_and_is_dropped() {
        let (mut book, token) = paired();
        let later = NOW + UNSEEN_DROP_MS;

        assert!(book.device(token.as_str(), later).is_none());
        book.drop_unseen(later);
        assert!(book.devices.is_empty());
    }

    #[test]
    fn a_rotation_counts_as_a_visit() {
        let (mut book, token) = paired();
        let later = NOW + UNSEEN_DROP_MS - 1;

        let fresh = book.rotate(token.as_str(), later).unwrap();
        book.drop_unseen(NOW + UNSEEN_DROP_MS);

        assert!(book.device(fresh.as_str(), later + 1).is_some());
    }

    #[test]
    fn a_revoked_device_opens_nothing() {
        let (mut book, token) = paired();
        let other = book.pair("iPad", NOW);
        let id = book.device(token.as_str(), NOW).unwrap().id.clone();

        book.revoke(&id);

        assert!(!book.is_paired(&id));
        assert!(book.device(token.as_str(), NOW).is_none());
        assert!(book.device(other.as_str(), NOW).is_some());
        book.revoke_all();
        assert!(book.device(other.as_str(), NOW).is_none());
    }

    #[test]
    fn a_network_is_recorded_once() {
        let mut book = DeviceBook::default();
        book.record_network("a4:2b:b0:11:22:33");
        book.record_network("a4:2b:b0:11:22:33");

        assert_eq!(book.networks.len(), 1);
        assert!(book.is_recorded_network("a4:2b:b0:11:22:33"));
        assert!(!book.is_recorded_network("00:00:00:00:00:00"));
    }

    #[test]
    fn the_name_comes_from_the_user_agent() {
        let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15";
        assert_eq!(device_name(Some(iphone)), "iPhone");
        assert_eq!(device_name(Some("Mozilla/5.0 (iPad; CPU OS 18_0)")), "iPad");
        assert_eq!(
            device_name(Some("Mozilla/5.0 (Linux; Android 15)")),
            "Android phone"
        );
        assert_eq!(device_name(None), "Browser");
    }

    #[test]
    fn the_book_survives_a_restart_on_disk_and_stays_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("helm-devices-{}", Token::mint().as_str()));
        let path = dir.join(DEVICES_FILE);

        let token = PairedDevices::load_from(path.clone()).edit(|book| {
            book.port = Some(5123);
            book.record_network("a4:2b:b0:11:22:33");
            book.pair("iPhone", wall_ms())
        });
        let reloaded = PairedDevices::load_from(path.clone());

        assert!(reloaded.read(|book| book.device(token.as_str(), wall_ms()).is_some()));
        assert_eq!(reloaded.read(|book| book.port), Some(5123));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
