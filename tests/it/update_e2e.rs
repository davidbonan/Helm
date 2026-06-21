//! Business e2e for `update::check` / `update::install` (roadmap M16-4/M16-5):
//! real `curl`/`ditto`/`codesign` subprocesses, `file://` URLs, fake `.app`
//! bundles in tempdirs — no network, no fake runner.

use std::path::{Path, PathBuf};
use std::process::Command;

use std::time::{Duration, Instant};

use helm::update::{
    check_with, install, install_to, relaunch_with, swap, CheckError, InstallError, UpdateOutcome,
    UpdateRunner, UpdateState, Version, MACOS_ASSET_NAME,
};

fn release_json(tag: &str) -> String {
    format!(
        r#"{{"tag_name":"{tag}","assets":[{{"name":"{MACOS_ASSET_NAME}","browser_download_url":"https://example.invalid/{MACOS_ASSET_NAME}","size":1}}]}}"#
    )
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[test]
fn check_reports_a_newer_release_via_a_real_curl() {
    let tmp = tempfile::tempdir().unwrap();
    let json_path = tmp.path().join("latest.json");
    std::fs::write(&json_path, release_json("v0.2.0")).unwrap();

    let check = check_with(
        Path::new("curl"),
        &file_url(&json_path),
        Version::parse("0.1.0").unwrap(),
    )
    .unwrap();

    assert_eq!(check.latest, Version::parse("0.2.0").unwrap());
    assert_eq!(
        check.asset_url,
        format!("https://example.invalid/{MACOS_ASSET_NAME}")
    );
    assert!(check.newer);
}

#[test]
fn check_reports_up_to_date_when_local_is_ahead_or_equal() {
    let tmp = tempfile::tempdir().unwrap();
    let json_path = tmp.path().join("latest.json");
    std::fs::write(&json_path, release_json("v0.2.0")).unwrap();

    for local in ["0.2.0", "0.3.0"] {
        let check = check_with(
            Path::new("curl"),
            &file_url(&json_path),
            Version::parse(local).unwrap(),
        )
        .unwrap();
        assert!(!check.newer, "local {local} must stay up to date");
    }
}

#[test]
fn a_missing_curl_binary_is_a_dedicated_error() {
    let tmp = tempfile::tempdir().unwrap();
    let result = check_with(
        &tmp.path().join("no-curl-here"),
        "https://example.invalid",
        Version::parse("0.1.0").unwrap(),
    );
    assert_eq!(result, Err(CheckError::CurlNotFound));
}

#[test]
fn a_curl_failure_is_a_dedicated_error() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("absent.json");
    let result = check_with(
        Path::new("curl"),
        &file_url(&missing),
        Version::parse("0.1.0").unwrap(),
    );
    assert!(
        matches!(result, Err(CheckError::CurlFailed(_))),
        "expected CurlFailed, got {result:?}"
    );
}

/// Minimal signable bundle: real Mach-O executable (`/bin/ls`), Info.plist,
/// and a marker telling the versions apart.
fn fake_app(dir: &Path, marker: &str) -> PathBuf {
    let app = dir.join("helm.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    std::fs::copy("/bin/ls", app.join("Contents/MacOS/helm")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key><string>helm</string>
	<key>CFBundleIdentifier</key><string>io.github.davidbonan.helm.test</string>
	<key>CFBundlePackageType</key><string>APPL</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    std::fs::write(app.join("Contents/Resources/marker.txt"), marker).unwrap();
    app
}

fn marker_of(app: &Path) -> String {
    std::fs::read_to_string(app.join("Contents/Resources/marker.txt")).unwrap()
}

fn run_ok(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sign(app: &Path) {
    run_ok("codesign", &["--force", "-s", "-", &app.to_string_lossy()]);
}

fn zip_app(app: &Path, zip: &Path) {
    run_ok(
        "ditto",
        &[
            "-c",
            "-k",
            "--keepParent",
            &app.to_string_lossy(),
            &zip.to_string_lossy(),
        ],
    );
}

#[test]
fn install_to_swaps_in_the_downloaded_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    let current = fake_app(&tmp.path().join("installed"), "v1");
    let new_app = fake_app(&tmp.path().join("staged"), "v2");
    sign(&new_app);
    let zip = tmp.path().join(MACOS_ASSET_NAME);
    zip_app(&new_app, &zip);
    let work_root = tmp.path().join("work");
    std::fs::create_dir_all(&work_root).unwrap();

    install_to(&current, &file_url(&zip), &work_root).unwrap();

    assert_eq!(marker_of(&current), "v2", "new bundle is in place");
    let backup = walkdir_find(&work_root, "backup.app").expect("old bundle kept in the work dir");
    assert_eq!(marker_of(&backup), "v1", "old bundle survives in temp");
}

#[test]
fn install_to_rejects_a_tampered_bundle_and_cleans_temp() {
    let tmp = tempfile::tempdir().unwrap();
    let current = fake_app(&tmp.path().join("installed"), "v1");
    let new_app = fake_app(&tmp.path().join("staged"), "v2");
    sign(&new_app);
    std::fs::copy("/bin/cat", new_app.join("Contents/MacOS/helm")).unwrap();
    let zip = tmp.path().join(MACOS_ASSET_NAME);
    zip_app(&new_app, &zip);
    let work_root = tmp.path().join("work");
    std::fs::create_dir_all(&work_root).unwrap();

    let result = install_to(&current, &file_url(&zip), &work_root);

    assert!(
        matches!(result, Err(InstallError::InvalidSignature(_))),
        "expected InvalidSignature, got {result:?}"
    );
    assert_eq!(marker_of(&current), "v1", "current bundle untouched");
    assert_eq!(
        std::fs::read_dir(&work_root).unwrap().count(),
        0,
        "temp cleaned after the abort"
    );
}

#[test]
fn install_to_rejects_an_archive_without_an_app() {
    let tmp = tempfile::tempdir().unwrap();
    let current = fake_app(&tmp.path().join("installed"), "v1");
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(plain.join("readme.txt"), "no bundle here").unwrap();
    let zip = tmp.path().join(MACOS_ASSET_NAME);
    zip_app(&plain, &zip);
    let work_root = tmp.path().join("work");
    std::fs::create_dir_all(&work_root).unwrap();

    let result = install_to(&current, &file_url(&zip), &work_root);

    assert!(
        matches!(result, Err(InstallError::ExtractFailed(_))),
        "expected ExtractFailed, got {result:?}"
    );
    assert_eq!(std::fs::read_dir(&work_root).unwrap().count(), 0);
}

#[test]
fn install_outside_a_bundle_is_not_bundled() {
    // The test binary lives in target/…/deps, not in an `.app`.
    assert_eq!(
        install("file:///ignored").unwrap_err(),
        InstallError::NotBundled
    );
}

#[test]
fn swap_rolls_back_when_the_new_app_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let current = fake_app(&tmp.path().join("installed"), "v1");
    let missing = tmp.path().join("nowhere.app");
    let backup_dir = tmp.path().join("work");
    std::fs::create_dir_all(&backup_dir).unwrap();

    let result = swap(&current, &missing, &backup_dir);

    assert!(
        matches!(result, Err(InstallError::SwapFailed(_))),
        "expected SwapFailed, got {result:?}"
    );
    assert_eq!(marker_of(&current), "v1", "old bundle restored intact");
}

#[test]
fn swap_reports_a_read_only_location() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().join("installed");
    let current = fake_app(&parent, "v1");
    let new_app = fake_app(&tmp.path().join("staged"), "v2");
    let backup_dir = tmp.path().join("work");
    std::fs::create_dir_all(&backup_dir).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = swap(&current, &new_app, &backup_dir);

    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(result, Err(InstallError::NotWritable(_))),
        "expected NotWritable, got {result:?}"
    );
    assert_eq!(marker_of(&current), "v1", "old bundle left in place");
}

/// Polls the runner like the app does each frame, until `done` or timeout.
fn drive(
    runner: &mut UpdateRunner,
    done: impl Fn(&UpdateRunner, &Option<UpdateOutcome>) -> bool,
) -> Option<UpdateOutcome> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut outcome = None;
    while !done(runner, &outcome) {
        assert!(Instant::now() < deadline, "runner timed out");
        if let Some(o) = runner.poll() {
            outcome = Some(o);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    outcome
}

#[test]
fn runner_reports_available_then_installs_off_the_caller_thread() {
    let tmp = tempfile::tempdir().unwrap();
    let current = fake_app(&tmp.path().join("installed"), "v1");
    let new_app = fake_app(&tmp.path().join("staged"), "v2");
    sign(&new_app);
    let zip = tmp.path().join(MACOS_ASSET_NAME);
    zip_app(&new_app, &zip);
    let json_path = tmp.path().join("latest.json");
    std::fs::write(
        &json_path,
        format!(
            r#"{{"tag_name":"v0.2.0","assets":[{{"name":"{MACOS_ASSET_NAME}","browser_download_url":"{}"}}]}}"#,
            file_url(&zip)
        ),
    )
    .unwrap();
    let work_root = tmp.path().join("work");
    std::fs::create_dir_all(&work_root).unwrap();
    let mut runner = UpdateRunner::new(|| {});

    assert!(runner.request_check_from(
        PathBuf::from("curl"),
        file_url(&json_path),
        Version::parse("0.1.0").unwrap(),
        false,
    ));
    assert_eq!(runner.state(), &UpdateState::Checking);
    let outcome = drive(&mut runner, |r, _| !r.busy());
    assert_eq!(
        outcome,
        Some(UpdateOutcome::Available {
            version: Version::parse("0.2.0").unwrap(),
            at_boot: false,
        })
    );
    let UpdateState::Available { asset_url, .. } = runner.state().clone() else {
        panic!("expected Available, got {:?}", runner.state());
    };

    assert!(runner.request_install_to(current.clone(), asset_url, work_root));
    assert_eq!(runner.state(), &UpdateState::Downloading);
    let outcome = drive(&mut runner, |_, outcome| outcome.is_some());
    assert_eq!(
        outcome,
        Some(UpdateOutcome::Installed {
            bundle: current.clone(),
        })
    );
    assert_eq!(runner.state(), &UpdateState::Installing);
    assert!(runner.busy(), "busy until relaunch");
    assert_eq!(marker_of(&current), "v2", "bundle swapped by the worker");
}

#[test]
fn runner_boot_style_check_failure_is_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let mut runner = UpdateRunner::new(|| {});
    assert!(runner.request_check_from(
        PathBuf::from("curl"),
        file_url(&tmp.path().join("absent.json")),
        Version::parse("0.1.0").unwrap(),
        true,
    ));
    let outcome = drive(&mut runner, |r, _| !r.busy());
    assert_eq!(outcome, None, "no toast material on a silent failure");
    assert_eq!(runner.state(), &UpdateState::Idle);
}

#[test]
fn runner_ignores_requests_while_busy() {
    let tmp = tempfile::tempdir().unwrap();
    let json_path = tmp.path().join("latest.json");
    std::fs::write(&json_path, release_json("v0.2.0")).unwrap();
    let mut runner = UpdateRunner::new(|| {});

    assert!(runner.request_check_from(
        PathBuf::from("curl"),
        file_url(&json_path),
        Version::parse("0.1.0").unwrap(),
        false,
    ));
    assert!(
        !runner.request_check_from(
            PathBuf::from("curl"),
            file_url(&json_path),
            Version::parse("0.1.0").unwrap(),
            false,
        ),
        "second check ignored while busy"
    );
    assert!(
        !runner.request_install_to(
            tmp.path().join("x.app"),
            "file:///ignored".to_owned(),
            tmp.path().to_path_buf(),
        ),
        "install ignored while busy"
    );
    drive(&mut runner, |r, _| !r.busy());
}

#[test]
fn runner_install_failure_frees_the_runner_with_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let current = fake_app(&tmp.path().join("installed"), "v1");
    let mut runner = UpdateRunner::new(|| {});
    assert!(runner.request_install_to(
        current.clone(),
        file_url(&tmp.path().join("absent.zip")),
        tmp.path().join("work"),
    ));
    drive(&mut runner, |r, _| !r.busy());
    assert!(
        matches!(runner.state(), UpdateState::Error(_)),
        "expected Error, got {:?}",
        runner.state()
    );
    assert_eq!(marker_of(&current), "v1", "bundle untouched");
}

fn walkdir_find(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()? {
        let path = entry.ok()?.path();
        if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = walkdir_find(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

#[test]
fn a_non_release_payload_is_a_clean_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let json_path = tmp.path().join("latest.json");
    std::fs::write(&json_path, r#"{"message":"Not Found"}"#).unwrap();

    let result = check_with(
        Path::new("curl"),
        &file_url(&json_path),
        Version::parse("0.1.0").unwrap(),
    );
    assert!(
        matches!(result, Err(CheckError::Parse(_))),
        "expected Parse, got {result:?}"
    );
}

#[test]
fn relaunch_spawns_detached_and_does_not_kill_the_new_process() {
    use std::os::unix::fs::PermissionsExt;

    // Launcher standing in for `open`: records its args, then takes longer than
    // the caller — the marker only appears if the spawned process survives the
    // dropped child handle (update.md §8: relaunch never kills the new one).
    let tmp = tempfile::tempdir().unwrap();
    let args_path = tmp.path().join("args.txt");
    let marker_path = tmp.path().join("marker.txt");
    let launcher = tmp.path().join("fake-open");
    std::fs::write(
        &launcher,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\nsleep 0.4\necho alive > '{}'\n",
            args_path.display(),
            marker_path.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();

    let bundle = tmp.path().join("helm.app");
    let started = Instant::now();
    relaunch_with(&launcher, &bundle).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "relaunch must not wait for the launcher"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker_path.exists() {
        assert!(Instant::now() < deadline, "launcher killed or never ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let args = std::fs::read_to_string(&args_path).unwrap();
    assert_eq!(args.trim(), format!("-n {}", bundle.display()));
}
