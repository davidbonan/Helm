# Application update — Specification

> Covers **distribution** (macOS `.app` bundle, GitHub Releases) and the
> **in-app update**: check, download, validation, bundle replacement,
> relaunch.

## 1. Goal

Take helm beyond `cargo run`: the app is distributed as an **`.app`
bundle** via **GitHub Releases**, and updates itself **from the app** — silent
check at startup + on demand in Preferences, download of the new version,
signature validation, bundle replacement, relaunch.

## 2. Distribution channel

- **Public GitHub repo** (`<owner>` to be set at push time — it also
  determines the bundle identifier, §3). Public ⇒ the in-app check queries the
  API **without authentication** (60 req/h/IP, largely sufficient).
- **Release = `v<semver>` tag** (e.g. `v0.2.0`) ⇒ CI builds, bundles, signs,
  zips and publishes the Release with the **`helm-macos.zip`** asset.
- **User install = `curl` script** (OSS pattern): `install.sh` at the repo
  root, one-liner
  `curl -fsSL https://raw.githubusercontent.com/<owner>/helm/main/install.sh | sh`
  — downloads the zip of the latest release, `ditto -x -k` to
  `/Applications`. `curl` does not set the quarantine ⇒ **no Gatekeeper
  warning**, even with ad-hoc signing. Fallback: zip downloaded via the
  browser ⇒ quarantine ⇒ manual approval (System Settings ›
  Privacy & Security), documented in the README.
- The **`Cargo.toml` version is the single source**: the Info.plist copies it
  to the bundle and the release tag must match it. Procedure: bump
  `Cargo.toml` → commit → tag `v<version>` → push.

## 3. Packaging — `.app` bundle

- `scripts/bundle.sh` assembles `helm.app`: release binary in
  `Contents/MacOS/`, `Info.plist` (CFBundleIdentifier
  **`io.github.<owner>.helm`**, CFBundleShortVersionString = `Cargo.toml`
  version, LSMinimumSystemVersion), `.icns` icon in
  `Contents/Resources/`.
- **Signing**: **ad-hoc** (`codesign -s -`) — sufficient for local use,
  the updater's integrity validation (§5) and install via the
  `curl` script (§2, no quarantine). Developer ID + notarization: **out of
  scope** (§10); the identity stays configurable (`CODESIGN_IDENTITY`) if
  distribution changes one day.
- **Zip asset via `ditto -c -k --keepParent`** (preserves symlinks/xattrs —
  prerequisite for signatures).

## 4. Version check

- **When**: at **startup** (in the background, never blocking; a network
  failure at boot produces **no** message) and **on demand** (Check for updates
  button, §6).
- **How**: `curl` as a **subprocess** to
  `https://api.github.com/repos/<owner>/helm/releases/latest`;
  JSON response parsed (`tag_name`, zip asset url); strict **semver**
  comparison with the compiled version (`CARGO_PKG_VERSION`). No
  HTTP/TLS crate: networking goes through system binaries, matching the git
  subprocess pattern ([`git.md`](git.md) §10).
- Remote newer ⇒ **Update available**; equal or older
  (dev build ahead) ⇒ **Up to date**.

## 5. Installation (download → swap → relaunch)

1. **Download**: `curl -L` of the zip asset to a temp folder.
2. **Extraction**: `ditto -x -k`.
3. **Validation**: `codesign --verify --strict` on the extracted `.app` —
   failure ⇒ abort, temp cleaned up, error displayed. Integrity relies on
   TLS + the bundle signature; no homemade checksum.
4. **Swap**: the current `.app` is moved to temp, the new one renamed into its
   place (same volume ⇒ atomic rename); any failure ⇒ **rollback** (old one
   restored).
5. **Relaunch**: detached subprocess that relaunches the new bundle
   (`open -n`) after the current process exits, then exit.

`curl` does not set the `com.apple.quarantine` attribute ⇒ no Gatekeeper
translocation on the replaced bundle.

## 6. UX

- **Toast at boot** (reuses `ui::toast`, [`git.md`](git.md) §10) if a
  newer version exists: "Update available v0.2.0" + **Install**
  action.
- **Preferences › Updates** (new section of the page,
  [`preferences.md`](preferences.md)):
  - **Version** row: current version;
  - **Check for updates** row: button + inline result — spinner during the
    check, "Up to date", "New version v0.2.0" + **Install & Relaunch**
    button, or error;
  - during installation: progress (downloading / installing),
    controls disabled.
- **Dev mode**: binary launched outside an `.app` (`cargo run`) ⇒ updater
  disabled — no check at boot, the section shows "Running outside an
  app bundle — updates disabled".

## 7. States

`Idle → Checking → UpToDate | UpdateAvailable(version)` then
`UpdateAvailable → Downloading → Installing → (relaunch)`; any step can
end in `Error(message)`. **One operation at a time** (busy ⇒ actions
ignored); everything off the UI thread (dedicated-worker / `AiRunner` pattern).

## 8. Edge cases

| Case | Behavior |
|-----|--------------|
| Offline / `curl` missing or failing | boot: silent; manual: inline error |
| GitHub API rate-limit | = network failure (same) |
| Malformed tag / missing asset in the release | clean error, never a panic |
| Local version > remote (dev build) | Up to date |
| `.app` on a read-only volume / non-writable path | explicit error inviting to move the app to Applications |
| Invalid download signature | abort + temp cleaned up + error |
| Binary outside a bundle (`cargo run`) | updater disabled (§6) |
| Action during an operation in progress | ignored (busy) |

## 9. Release notes after an update

> After an update, on the **next launch**, show the user the release notes of
> the new version plus the previous ones — once, in a modal — and keep them
> **browsable any time** in Preferences › Updates. The notes are a single
> **bundled file** authored by `/release` from the commits: no network, no
> cache, no GitHub API.

### 9.1 Source — a bundled `release-notes.md`

- A markdown file **`release-notes.md`** at the repo root, **embedded in the
  binary** via `include_str!` (`src/release_notes.rs`). It ships inside the
  build, so the notes are always present — **offline by construction**, no fetch,
  no cache file, no API quota.
- One section per version, **newest-first**, each starting with a
  `## <version>` header (e.g. `## 0.9.0`). At most **10 versions** are kept; the
  oldest sections drop off when a new one is added (§9.2).
- The app renders the file as-is (already capped at 10 versions); the
  `## <version>` headers let it optionally scroll the current version into view.

### 9.2 Authoring — `/release`

The notes are written **at release time** by the `/release` skill, never by
hand:

1. read the commits since the last tag (`git log v<last>..HEAD`);
2. **reformat** them into a clean, grouped section (drop internal/noise commits,
   rewrite terse subjects) under a new `## <new_version>` header;
3. **prepend** that section to `release-notes.md` and **trim** the file to the
   latest 10 versions;
4. show the draft for review, then include `release-notes.md` in the **release
   commit** — so the new notes are baked into the build CI produces for that tag.

Because the file is part of the release commit, the build a user updates to
**always contains its own** section (and the previous ones).

### 9.3 Trigger

- Compare the compiled `current_version()` (`CARGO_PKG_VERSION`, §4) with the
  persisted **`last_seen_version`** (in `prefs.toml`, scalar field).
- `current > last_seen` **and** running inside an `.app` bundle ⇒ show the
  **What's new** modal once, then persist `last_seen = current`.
- `last_seen` empty (**first install ever**) ⇒ silently set
  `last_seen = current`, **no** modal (a fresh install is not "what's new").
- Outside a bundle (`cargo run`) ⇒ disabled (consistent with the updater, §6).
- The watermark is the version, not the install path: an update applied **out of
  the app** (`install.sh` / `curl`, §2) is detected the same way at the next
  launch — and the notes are already bundled, so nothing else is needed.

### 9.4 Surfaces

- **Modal — boot, after an update only**: centered, scrollable; renders the
  bundled notes (newest / current at top); close button. Reuses the
  `egui::Modal` + `modal_frame` pattern. Shown once per version bump.
- **Preferences › Updates** ([`preferences.md`](preferences.md)): a persistent
  **Release notes** block rendering the same bundled file, browsable at any time
  (independent of any update).
- **Markdown rendering**: `egui_commonmark` (version paired with egui 0.34), with
  a shared `CommonMarkCache`. Links in the notes are clickable (open the browser).

### 9.5 Edge cases

| Case | Behavior |
|-----|--------------|
| `current ≤ last_seen` (no bump / dev build ahead) | no modal |
| First install (`last_seen` empty) | silent baseline, no modal |
| Outside a bundle (`cargo run`) | no modal (notes still readable in Preferences) |
| Out-of-app update (`install.sh` / `curl`) | detected next launch; notes already bundled |
| `release-notes.md` empty / no matching section | render what's there; never a panic |

### 9.6 Out of scope (this feature)

Per-version "mark as read" beyond the single `last_seen` watermark; keeping more
than the latest 10 versions; rendering images/embedded media from the notes;
localizing the notes (they are authored in English).

## 10. Out of scope (v1)

Beta/stable channel, delta updates, silent automatic install, periodic
in-session check, downgrade from the UI, Sparkle, **Developer ID +
notarization** signing (install via `curl` does not trigger
Gatekeeper; the browser fallback is approved manually). Each via a
dedicated decision/milestone.
