use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::git::cli::{self, CliError};

/// Check API URL and asset name fixed at M16-3 (repo `davidbonan/Helm`,
/// update.md §2/§4).
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/davidbonan/Helm/releases/latest";
pub const MACOS_ASSET_NAME: &str = "helm-macos.zip";

/// Homegrown semver, sufficient for `x.y.z` release tags (update.md §4) — no
/// pre-release/build suffixes, by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Accepts `x.y.z` with an optional `v` prefix; anything else ⇒ `None`.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Version compiled into the binary — `Cargo.toml` is the single source
/// (update.md §2).
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION is x.y.z")
}

/// What the boot release-notes trigger should do (update.md §9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhatsNew {
    /// Out of bundle, or this version (or newer) has already been seen: nothing.
    Skip,
    /// First install (empty/unparseable watermark): record the baseline silently,
    /// no modal — a fresh install is not "what's new".
    Stamp,
    /// A version bump not yet surfaced: show the notes once, then record it.
    Show,
}

/// Decides the boot action for the What's new modal (update.md §9.3). `bundled` =
/// running inside an `.app` (updater disabled outside one, §6); `last_seen` = the
/// persisted `Prefs.last_seen_version` watermark. `Show`/`Stamp` both advance the
/// watermark to `current`; only `Show` opens the modal.
pub fn whats_new_on_boot(bundled: bool, current: Version, last_seen: &str) -> WhatsNew {
    if !bundled {
        return WhatsNew::Skip;
    }
    match Version::parse(last_seen) {
        Some(seen) if current > seen => WhatsNew::Show,
        Some(_) => WhatsNew::Skip,
        None => WhatsNew::Stamp,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    pub latest: Version,
    pub asset_url: String,
    /// Strict: `true` only if the remote is newer than the local build — a dev
    /// build ahead of the latest release stays "Up to date" (update.md §8).
    pub newer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// `curl` binary absent from PATH.
    CurlNotFound,
    /// `curl` ran but failed (network, HTTP error, rate-limit).
    CurlFailed(String),
    /// Response is not the expected GitHub Release JSON.
    Parse(String),
    /// `tag_name` is not a `vx.y.z` version.
    MalformedTag(String),
    /// No `helm-macos.zip` asset in the latest release.
    MissingAsset,
}

impl CheckError {
    pub fn message(&self) -> String {
        match self {
            CheckError::CurlNotFound => "curl not found — cannot check for updates".to_owned(),
            CheckError::CurlFailed(detail) => format!("Update check failed — {detail}"),
            CheckError::Parse(detail) => {
                format!("Update check failed — unexpected API response ({detail})")
            }
            CheckError::MalformedTag(tag) => {
                format!("Update check failed — unexpected release tag '{tag}'")
            }
            CheckError::MissingAsset => {
                "Update check failed — no macOS asset in the latest release".to_owned()
            }
        }
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn check() -> Result<UpdateCheck, CheckError> {
    check_with(Path::new("curl"), LATEST_RELEASE_URL, current_version())
}

/// Seam: `check` pins curl from PATH and the real API URL; the parameters let
/// tests use a fake binary or a `file://` URL (git::cli / ai pattern).
pub fn check_with(curl: &Path, url: &str, current: Version) -> Result<UpdateCheck, CheckError> {
    let output = cli::run_program(
        curl,
        &std::env::temp_dir(),
        &["-fsSL", "--max-time", "15", url],
    )
    .map_err(|err| match err {
        CliError::NotFound => CheckError::CurlNotFound,
        CliError::TimedOut(duration) => {
            CheckError::CurlFailed(format!("curl timed out after {}s", duration.as_secs()))
        }
        CliError::Io(err) => CheckError::CurlFailed(err.to_string()),
    })?;
    if !output.success() {
        return Err(CheckError::CurlFailed(curl_failure_detail(&output)));
    }
    let (latest, asset_url) = parse_release(&output.stdout)?;
    Ok(UpdateCheck {
        latest,
        asset_url,
        newer: latest > current,
    })
}

fn curl_failure_detail(output: &cli::CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    match output.code {
        Some(code) => format!("curl exit code {code}"),
        None => "curl killed by a signal".to_owned(),
    }
}

/// Pure parse of the `releases/latest` JSON: version from `tag_name` + download
/// URL of the macOS zip asset.
pub fn parse_release(json: &str) -> Result<(Version, String), CheckError> {
    let release: Release =
        serde_json::from_str(json).map_err(|err| CheckError::Parse(err.to_string()))?;
    let version = Version::parse(&release.tag_name)
        .ok_or_else(|| CheckError::MalformedTag(release.tag_name.clone()))?;
    let asset_url = release
        .assets
        .into_iter()
        .find(|asset| asset.name == MACOS_ASSET_NAME)
        .map(|asset| asset.browser_download_url)
        .ok_or(CheckError::MissingAsset)?;
    Ok((version, asset_url))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// Executable not inside an `.app` — dev mode, updater disabled (update.md §6).
    NotBundled,
    DownloadFailed(String),
    ExtractFailed(String),
    /// `codesign --verify --strict` refused the downloaded bundle.
    InvalidSignature(String),
    /// The bundle's location cannot be written (read-only volume, update.md §8).
    NotWritable(String),
    SwapFailed(String),
    Io(String),
}

impl InstallError {
    pub fn message(&self) -> String {
        match self {
            InstallError::NotBundled => {
                "Running outside an app bundle — updates disabled".to_owned()
            }
            InstallError::DownloadFailed(detail) => format!("Update download failed — {detail}"),
            InstallError::ExtractFailed(detail) => format!("Update unpack failed — {detail}"),
            InstallError::InvalidSignature(detail) => {
                format!("Update rejected — signature validation failed ({detail})")
            }
            InstallError::NotWritable(_) => {
                "Cannot replace the app — move it to /Applications and try again".to_owned()
            }
            InstallError::SwapFailed(detail) => format!("Update install failed — {detail}"),
            InstallError::Io(detail) => format!("Update failed — {detail}"),
        }
    }
}

/// `.app` containing the current executable; `None` under `cargo run`/tests.
pub fn bundle_path() -> Option<PathBuf> {
    bundle_path_from(&std::env::current_exe().ok()?)
}

/// Pure resolution: `<App>.app/Contents/MacOS/<bin>` ⇒ `<App>.app`.
pub fn bundle_path_from(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    (app.extension()? == "app").then(|| app.to_path_buf())
}

/// Full pipeline on the running bundle (update.md §5); returns the bundle path
/// to relaunch. The old `.app` survives in the work dir (rollback evidence).
pub fn install(asset_url: &str) -> Result<PathBuf, InstallError> {
    let bundle = bundle_path().ok_or(InstallError::NotBundled)?;
    install_to(&bundle, asset_url, &std::env::temp_dir())?;
    Ok(bundle)
}

/// Seam: explicit bundle + work root for e2e tests on a fake `.app`.
pub fn install_to(bundle: &Path, asset_url: &str, work_root: &Path) -> Result<(), InstallError> {
    let staged = stage(asset_url, work_root)?;
    finish(bundle, &staged)
}

/// Download + extract + validate, without touching the current bundle. The
/// split marks the worker's Downloading → Installing transition (update.md §7).
pub struct StagedUpdate {
    work: PathBuf,
    new_app: PathBuf,
}

pub fn stage(asset_url: &str, work_root: &Path) -> Result<StagedUpdate, InstallError> {
    let work = work_root.join(format!(
        "helm-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&work).map_err(|err| InstallError::Io(err.to_string()))?;

    let staged = (|| {
        let zip = work.join(MACOS_ASSET_NAME);
        download(asset_url, &zip)?;
        let new_app = extract(&zip, &work.join("unzipped"))?;
        validate(&new_app)?;
        Ok(new_app)
    })();
    match staged {
        Ok(new_app) => Ok(StagedUpdate { work, new_app }),
        // Abort before touching the current bundle: nothing precious in temp.
        Err(err) => {
            let _ = fs::remove_dir_all(&work);
            Err(err)
        }
    }
}

/// Swap step. From here the work dir may hold the only copy of the old
/// bundle — never removed, even on failure.
pub fn finish(bundle: &Path, staged: &StagedUpdate) -> Result<(), InstallError> {
    swap(bundle, &staged.new_app, &staged.work)
}

fn download(url: &str, dest: &Path) -> Result<(), InstallError> {
    let dest_arg = dest.to_string_lossy();
    let output = cli::run_program(
        Path::new("curl"),
        dest.parent().unwrap_or(Path::new("/")),
        // `-s` keeps the progress meter off stderr so a failure surfaces curl's
        // real message, not the meter. `--retry 3` rides out GitHub's release CDN
        // returning transient 5xx (observed: repeated 504 then recovery).
        &[
            "-fsSL",
            "--retry",
            "3",
            "--connect-timeout",
            "15",
            "-o",
            &dest_arg,
            url,
        ],
    )
    .map_err(|err| match err {
        CliError::NotFound => InstallError::DownloadFailed("curl not found".to_owned()),
        CliError::TimedOut(duration) => {
            InstallError::DownloadFailed(format!("curl timed out after {}s", duration.as_secs()))
        }
        CliError::Io(err) => InstallError::DownloadFailed(err.to_string()),
    })?;
    if !output.success() {
        return Err(InstallError::DownloadFailed(curl_failure_detail(&output)));
    }
    Ok(())
}

fn extract(zip: &Path, dest: &Path) -> Result<PathBuf, InstallError> {
    let output = cli::run_program(
        Path::new("ditto"),
        zip.parent().unwrap_or(Path::new("/")),
        &["-x", "-k", &zip.to_string_lossy(), &dest.to_string_lossy()],
    )
    .map_err(|err| match err {
        CliError::NotFound => InstallError::ExtractFailed("ditto not found".to_owned()),
        CliError::TimedOut(duration) => {
            InstallError::ExtractFailed(format!("ditto timed out after {}s", duration.as_secs()))
        }
        CliError::Io(err) => InstallError::ExtractFailed(err.to_string()),
    })?;
    if !output.success() {
        return Err(InstallError::ExtractFailed(output.stderr.trim().to_owned()));
    }
    fs::read_dir(dest)
        .map_err(|err| InstallError::ExtractFailed(err.to_string()))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or_else(|| InstallError::ExtractFailed("no .app in the archive".to_owned()))
}

fn validate(app: &Path) -> Result<(), InstallError> {
    let output = cli::run_program(
        Path::new("codesign"),
        app.parent().unwrap_or(Path::new("/")),
        &["--verify", "--strict", &app.to_string_lossy()],
    )
    .map_err(|err| match err {
        CliError::NotFound => InstallError::InvalidSignature("codesign not found".to_owned()),
        CliError::TimedOut(duration) => InstallError::InvalidSignature(format!(
            "codesign timed out after {}s",
            duration.as_secs()
        )),
        CliError::Io(err) => InstallError::InvalidSignature(err.to_string()),
    })?;
    if !output.success() {
        return Err(InstallError::InvalidSignature(
            output.stderr.trim().to_owned(),
        ));
    }
    Ok(())
}

/// Same-volume renames (update.md §5): current → backup in the work dir, new →
/// in place; second step failing ⇒ the old bundle is restored.
pub fn swap(current: &Path, new: &Path, backup_dir: &Path) -> Result<(), InstallError> {
    let backup = backup_dir.join("backup.app");
    fs::rename(current, &backup).map_err(swap_error)?;
    if let Err(err) = fs::rename(new, current) {
        let _ = fs::rename(&backup, current);
        return Err(swap_error(err));
    }
    Ok(())
}

fn swap_error(err: std::io::Error) -> InstallError {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => InstallError::NotWritable(err.to_string()),
        _ => InstallError::SwapFailed(err.to_string()),
    }
}

/// `open -n` starts a fresh instance of the swapped bundle as a detached
/// process; the caller exits the current process right after (update.md §5).
pub fn relaunch(bundle: &Path) -> std::io::Result<()> {
    relaunch_with(Path::new("open"), bundle)
}

/// Seam: explicit launcher for e2e tests (a script standing in for `open`).
/// Spawns without waiting and drops the child handle: the new process is never
/// killed by the exiting one (update.md §8).
pub fn relaunch_with(launcher: &Path, bundle: &Path) -> std::io::Result<()> {
    Command::new(launcher)
        .arg("-n")
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Updater states (update.md §7), read by the UI every frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available {
        version: Version,
        asset_url: String,
    },
    Downloading,
    Installing,
    Error(String),
}

enum UpdateEvent {
    Checked {
        result: Result<UpdateCheck, CheckError>,
        silent: bool,
    },
    Installing,
    Installed(Result<PathBuf, InstallError>),
}

/// App-level reactions drained by `poll`: boot toast and relaunch (M16-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    Available { version: Version, at_boot: bool },
    Installed { bundle: PathBuf },
}

/// Runs check/install on a **dedicated thread per operation** (`AiRunner`
/// pattern, M12-3): the UI thread is never blocked. **One operation at a
/// time**: requests during busy are ignored. Threads are not joined.
pub struct UpdateRunner {
    state: UpdateState,
    events_tx: crossbeam_channel::Sender<UpdateEvent>,
    events_rx: crossbeam_channel::Receiver<UpdateEvent>,
    on_event: std::sync::Arc<dyn Fn() + Send + Sync>,
    in_flight: bool,
}

impl UpdateRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        Self {
            state: UpdateState::Idle,
            events_tx,
            events_rx,
            on_event: std::sync::Arc::new(on_event),
            in_flight: false,
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn busy(&self) -> bool {
        self.in_flight
    }

    /// Silent startup check (update.md §4): skipped outside a bundle, any
    /// failure falls back to Idle without a message.
    pub fn check_at_boot(&mut self) -> bool {
        if bundle_path().is_none() {
            return false;
        }
        self.request_check_from(
            PathBuf::from("curl"),
            LATEST_RELEASE_URL.to_owned(),
            current_version(),
            true,
        )
    }

    /// Manual check (Preferences button): errors surface inline.
    pub fn request_check(&mut self) -> bool {
        self.request_check_from(
            PathBuf::from("curl"),
            LATEST_RELEASE_URL.to_owned(),
            current_version(),
            false,
        )
    }

    /// Seam: explicit curl/URL/version for e2e tests (`file://`).
    pub fn request_check_from(
        &mut self,
        curl: PathBuf,
        url: String,
        current: Version,
        silent: bool,
    ) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        self.state = UpdateState::Checking;
        let tx = self.events_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = check_with(&curl, &url, current);
            let _ = tx.send(UpdateEvent::Checked { result, silent });
            on_event();
        });
        true
    }

    /// Install of the version reported Available; ignored otherwise.
    pub fn request_install(&mut self) -> bool {
        let UpdateState::Available { asset_url, .. } = &self.state else {
            return false;
        };
        let Some(bundle) = bundle_path() else {
            return false;
        };
        self.request_install_to(bundle, asset_url.clone(), std::env::temp_dir())
    }

    /// Seam: explicit bundle + work root for e2e tests on a fake `.app`.
    pub fn request_install_to(
        &mut self,
        bundle: PathBuf,
        asset_url: String,
        work_root: PathBuf,
    ) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        self.state = UpdateState::Downloading;
        let tx = self.events_tx.clone();
        let on_event = std::sync::Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            match stage(&asset_url, &work_root) {
                Ok(staged) => {
                    let _ = tx.send(UpdateEvent::Installing);
                    on_event();
                    let result = finish(&bundle, &staged).map(|()| bundle);
                    let _ = tx.send(UpdateEvent::Installed(result));
                }
                Err(err) => {
                    let _ = tx.send(UpdateEvent::Installed(Err(err)));
                }
            }
            on_event();
        });
        true
    }

    /// Drains the worker events; called every frame by the app.
    pub fn poll(&mut self) -> Option<UpdateOutcome> {
        let mut outcome = None;
        while let Ok(event) = self.events_rx.try_recv() {
            if let Some(o) = self.apply(event) {
                outcome = Some(o);
            }
        }
        outcome
    }

    fn apply(&mut self, event: UpdateEvent) -> Option<UpdateOutcome> {
        match event {
            UpdateEvent::Checked { result, silent } => {
                self.in_flight = false;
                match result {
                    Ok(check) if check.newer => {
                        let version = check.latest;
                        self.state = UpdateState::Available {
                            version,
                            asset_url: check.asset_url,
                        };
                        Some(UpdateOutcome::Available {
                            version,
                            at_boot: silent,
                        })
                    }
                    Ok(_) => {
                        self.state = UpdateState::UpToDate;
                        None
                    }
                    Err(err) => {
                        // Boot check stays silent: back to Idle, no message
                        // (update.md §4).
                        self.state = if silent {
                            UpdateState::Idle
                        } else {
                            UpdateState::Error(err.message())
                        };
                        None
                    }
                }
            }
            UpdateEvent::Installing => {
                self.state = UpdateState::Installing;
                None
            }
            UpdateEvent::Installed(Ok(bundle)) => {
                // Stays busy and Installing: the app relaunches and exits.
                Some(UpdateOutcome::Installed { bundle })
            }
            UpdateEvent::Installed(Err(err)) => {
                self.in_flight = false;
                self.state = UpdateState::Error(err.message());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str, assets: &[(&str, &str)]) -> String {
        let assets = assets
            .iter()
            .map(|(name, url)| {
                format!(r#"{{"name":"{name}","browser_download_url":"{url}","size":123}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"url":"https://api.github.com/repos/davidbonan/Helm/releases/1","tag_name":"{tag}","name":"{tag}","draft":false,"prerelease":false,"assets":[{assets}],"body":"notes"}}"#
        )
    }

    #[test]
    fn whats_new_on_boot_shows_once_per_bump_and_stays_silent_otherwise() {
        let v = |s| Version::parse(s).unwrap();
        // A real bump inside a bundle: show the notes once.
        assert_eq!(whats_new_on_boot(true, v("0.9.0"), "0.8.4"), WhatsNew::Show);
        // Same version already seen: no modal.
        assert_eq!(whats_new_on_boot(true, v("0.8.4"), "0.8.4"), WhatsNew::Skip);
        // Local build behind the watermark (dev ahead): no modal.
        assert_eq!(whats_new_on_boot(true, v("0.8.4"), "0.9.0"), WhatsNew::Skip);
        // First install (empty watermark): silent baseline, no modal.
        assert_eq!(whats_new_on_boot(true, v("0.8.4"), ""), WhatsNew::Stamp);
        // Unparseable watermark is treated like a first install (never panics).
        assert_eq!(
            whats_new_on_boot(true, v("0.8.4"), "garbage"),
            WhatsNew::Stamp
        );
        // Outside a bundle the trigger is disabled regardless of versions.
        assert_eq!(
            whats_new_on_boot(false, v("0.9.0"), "0.8.4"),
            WhatsNew::Skip
        );
        assert_eq!(whats_new_on_boot(false, v("0.9.0"), ""), WhatsNew::Skip);
    }

    #[test]
    fn version_parses_with_and_without_the_v_prefix() {
        let expected = Some(Version {
            major: 1,
            minor: 2,
            patch: 3,
        });
        assert_eq!(Version::parse("1.2.3"), expected);
        assert_eq!(Version::parse("v1.2.3"), expected);
        assert_eq!(Version::parse(" v1.2.3 "), expected);
    }

    #[test]
    fn version_rejects_malformed_text() {
        for text in ["", "1", "1.2", "1.2.3.4", "1.2.x", "1.2.3-beta", "abc"] {
            assert_eq!(Version::parse(text), None, "accepted {text:?}");
        }
    }

    #[test]
    fn version_orders_by_major_then_minor_then_patch() {
        let v = |t| Version::parse(t).unwrap();
        assert!(v("2.0.0") > v("1.9.9"));
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("1.0.10") > v("1.0.9"));
        assert_eq!(v("1.2.3"), v("v1.2.3"));
        assert_eq!(v("0.1.0").to_string(), "0.1.0");
    }

    #[test]
    fn current_version_matches_cargo_toml() {
        assert_eq!(
            current_version().to_string(),
            env!("CARGO_PKG_VERSION"),
            "Cargo.toml is the single version source"
        );
    }

    #[test]
    fn parse_release_extracts_tag_and_macos_asset() {
        let json = release_json(
            "v0.2.0",
            &[
                ("source.tar.gz", "https://example.invalid/src"),
                (MACOS_ASSET_NAME, "https://example.invalid/helm-macos.zip"),
            ],
        );
        let (version, asset_url) = parse_release(&json).unwrap();
        assert_eq!(version, Version::parse("0.2.0").unwrap());
        assert_eq!(asset_url, "https://example.invalid/helm-macos.zip");
    }

    #[test]
    fn parse_release_rejects_a_malformed_tag() {
        let json = release_json(
            "nightly",
            &[(MACOS_ASSET_NAME, "https://example.invalid/zip")],
        );
        assert_eq!(
            parse_release(&json),
            Err(CheckError::MalformedTag("nightly".to_owned()))
        );
    }

    #[test]
    fn parse_release_rejects_a_missing_macos_asset() {
        let json = release_json("v0.2.0", &[("other.zip", "https://example.invalid/other")]);
        assert_eq!(parse_release(&json), Err(CheckError::MissingAsset));
    }

    #[test]
    fn parse_release_rejects_non_release_json() {
        assert!(matches!(
            parse_release("{\"message\":\"Not Found\"}"),
            Err(CheckError::Parse(_))
        ));
        assert!(matches!(
            parse_release("not json"),
            Err(CheckError::Parse(_))
        ));
    }

    #[test]
    fn newer_is_strict_about_local_ahead_or_equal() {
        let json = release_json(
            "v0.1.0",
            &[(MACOS_ASSET_NAME, "https://example.invalid/zip")],
        );
        let (latest, _) = parse_release(&json).unwrap();
        assert!(
            latest <= Version::parse("0.1.0").unwrap(),
            "equal ⇒ up to date"
        );
        assert!(
            latest <= Version::parse("0.2.0").unwrap(),
            "local ahead ⇒ up to date"
        );
        assert!(latest > Version::parse("0.0.9").unwrap());
    }

    #[test]
    fn error_messages_are_actionable() {
        assert!(CheckError::CurlNotFound.message().contains("curl"));
        assert!(CheckError::MalformedTag("x".into())
            .message()
            .contains("'x'"));
        assert!(CheckError::MissingAsset.message().contains("asset"));
    }

    #[test]
    fn bundle_path_resolves_the_app_containing_the_executable() {
        assert_eq!(
            bundle_path_from(Path::new("/Applications/helm.app/Contents/MacOS/helm")),
            Some(PathBuf::from("/Applications/helm.app"))
        );
    }

    #[test]
    fn bundle_path_is_none_outside_an_app_bundle() {
        for exe in [
            "/usr/bin/ls",
            "/Users/me/dev/helm-studio/target/debug/helm",
            "/Applications/helm.app/Contents/Helpers/tool",
            "/Applications/not-a-bundle/Contents/MacOS/bin",
            "helm",
        ] {
            assert_eq!(bundle_path_from(Path::new(exe)), None, "accepted {exe}");
        }
    }

    fn runner() -> UpdateRunner {
        UpdateRunner::new(|| {})
    }

    fn checked(result: Result<UpdateCheck, CheckError>, silent: bool) -> UpdateEvent {
        UpdateEvent::Checked { result, silent }
    }

    fn newer_check() -> UpdateCheck {
        UpdateCheck {
            latest: Version::parse("0.2.0").unwrap(),
            asset_url: "https://example.invalid/zip".to_owned(),
            newer: true,
        }
    }

    #[test]
    fn a_newer_check_lands_on_available_with_an_outcome() {
        let mut runner = runner();
        runner.in_flight = true;
        let outcome = runner.apply(checked(Ok(newer_check()), true));
        assert_eq!(
            outcome,
            Some(UpdateOutcome::Available {
                version: Version::parse("0.2.0").unwrap(),
                at_boot: true,
            })
        );
        assert!(matches!(runner.state(), UpdateState::Available { .. }));
        assert!(!runner.busy());
    }

    #[test]
    fn an_equal_or_older_check_lands_on_up_to_date() {
        let mut runner = runner();
        runner.in_flight = true;
        let check = UpdateCheck {
            newer: false,
            ..newer_check()
        };
        assert_eq!(runner.apply(checked(Ok(check), false)), None);
        assert_eq!(runner.state(), &UpdateState::UpToDate);
    }

    #[test]
    fn a_boot_check_failure_falls_back_to_idle_without_a_message() {
        let mut runner = runner();
        runner.in_flight = true;
        assert_eq!(
            runner.apply(checked(Err(CheckError::CurlNotFound), true)),
            None
        );
        assert_eq!(runner.state(), &UpdateState::Idle);
        assert!(!runner.busy());
    }

    #[test]
    fn a_manual_check_failure_surfaces_an_error() {
        let mut runner = runner();
        runner.in_flight = true;
        runner.apply(checked(Err(CheckError::CurlNotFound), false));
        let UpdateState::Error(message) = runner.state() else {
            panic!("expected Error, got {:?}", runner.state());
        };
        assert!(message.contains("curl"));
    }

    #[test]
    fn install_events_drive_installing_then_outcome() {
        let mut runner = runner();
        runner.in_flight = true;
        runner.state = UpdateState::Downloading;
        assert_eq!(runner.apply(UpdateEvent::Installing), None);
        assert_eq!(runner.state(), &UpdateState::Installing);
        let outcome = runner.apply(UpdateEvent::Installed(Ok(PathBuf::from("/tmp/helm.app"))));
        assert_eq!(
            outcome,
            Some(UpdateOutcome::Installed {
                bundle: PathBuf::from("/tmp/helm.app"),
            })
        );
        assert!(runner.busy(), "stays busy until the relaunch");
        assert_eq!(runner.state(), &UpdateState::Installing);
    }

    #[test]
    fn an_install_failure_surfaces_an_error_and_frees_the_runner() {
        let mut runner = runner();
        runner.in_flight = true;
        runner.state = UpdateState::Installing;
        runner.apply(UpdateEvent::Installed(Err(InstallError::NotWritable(
            "denied".to_owned(),
        ))));
        assert!(matches!(runner.state(), UpdateState::Error(_)));
        assert!(!runner.busy());
    }

    #[test]
    fn check_at_boot_outside_a_bundle_does_nothing() {
        let mut runner = runner();
        assert!(!runner.check_at_boot(), "tests run outside an .app");
        assert_eq!(runner.state(), &UpdateState::Idle);
        assert!(!runner.busy());
    }

    #[test]
    fn install_error_messages_are_actionable() {
        assert!(InstallError::NotBundled
            .message()
            .contains("outside an app bundle"));
        assert!(InstallError::NotWritable("denied".into())
            .message()
            .contains("/Applications"));
        assert!(InstallError::InvalidSignature("seal broken".into())
            .message()
            .contains("seal broken"));
    }
}
