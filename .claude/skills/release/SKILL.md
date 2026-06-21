---
name: release
description: >-
  Deploys a new helm version end to end: preflight (clean main, synced with
  origin, local gate green), version bump in Cargo.toml (single source of
  truth), "Release v<version>" commit + tag, push, CI watch
  (.github/workflows/release.yml), then verification of the published GitHub
  Release (helm-macos.zip asset downloaded, quarantine-free, codesign valid,
  Info.plist version matches). Argument: patch | minor | major | x.y.z;
  without an argument, proposes the patch bump and asks for confirmation.
  Never force-pushes, never deletes a published tag, never installs into
  /Applications.
argument-hint: "[patch|minor|major|x.y.z — optional]"
---

# release

Publishes a **helm** release end to end and proves it shipped. The pipeline
(update.md §2, README §Release procedure): pushing a `v<semver>` tag triggers
[`.github/workflows/release.yml`](../../../.github/workflows/release.yml)
(macos-15) which runs `cargo test`, builds the signed `.app`
([`scripts/bundle.sh`](../../../scripts/bundle.sh)), zips it with `ditto` and
publishes the GitHub Release with the **`helm-macos.zip`** asset. A tag that
does not match the `Cargo.toml` version **fails the run** — the version is
bumped first, the tag is derived from it, never the other way around.

Repo: `davidbonan/Helm` (public ⇒ the API is readable without auth).

## Procedure

### 0. Preflight — stop on any failure, fix nothing silently

```sh
git rev-parse --abbrev-ref HEAD     # must be: main
git status --short                  # must be empty
git fetch origin && git rev-list --left-right --count origin/main...main
```

- **Dirty tree** → stop and ask: a release commit carries only the version
  bump, never unrelated work.
- **Behind origin/main** → stop and ask (pull first); **ahead** → list the
  unpushed commits to the user: they all ship with the release.
- `git tag -l` — the target tag must not already exist (local or remote).
- `gh auth status` decides the CI-watch path (step 6): `gh` if authenticated,
  public API via `curl` otherwise.

### 1. Pick the version

Read the current version (single source):

```sh
grep -m1 '^version' Cargo.toml
```

- **Argument `x.y.z`** → use it (must be greater than current —
  `update::Version` ordering, no pre-release suffixes).
- **Argument `patch`/`minor`/`major`** → compute the bump.
- **No argument** → compute the **patch** bump, show the commits since the
  last tag (`git log v<last>..HEAD --oneline`) and **ask for confirmation**
  (AskUserQuestion) before touching anything — the version choice is the
  user's.

### 2. Bump the version

1. Edit `version = "<new>"` in `Cargo.toml` (Edit tool, not sed).
2. `cargo check` — refreshes the `helm` entry in `Cargo.lock`.

### 3. Author the release notes (`release-notes.md`)

The bundled notes (update.md §9) ship **in the release commit** — the boot
"What's new" modal and Preferences › Updates render this file, embedded at build
time (`include_str!`). Author the new version's section **before the gate**, so
`cargo test` validates it (`src/release_notes.rs`: the 10-version cap and that
every `## <version>` parses as semver):

1. Collect the commits since the last tag:
   ```sh
   git log v<last>..HEAD --no-merges --pretty=format:'%s'
   ```
2. Write a `## <new-version>` section — one terse, user-facing bullet per
   notable change. **Reformat** the subjects into product language and **drop
   noise** (refactors, test-only, CI, formatting): the reader is a user, not a
   git log.
3. **Prepend** it under the `# Release notes` title (newest first), then **trim
   to the 10 most recent** `## <version>` sections — the oldest drop off
   (update.md §9.1).
4. **Review the drafted section with the user** and apply edits — the wording is
   the user's call.

Nothing user-facing since the last tag → an honest one-liner, never padding.

### 4. Local gate

Full gate **before** tagging (a red CI run burns the tag — see §Failures):

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

If the gate fails → revert nothing automatically; report and stop.

### 5. Commit, tag, push

```sh
git add Cargo.toml Cargo.lock release-notes.md
git commit -m "Release v<version>"
git tag v<version>
git push origin main v<version>
```

- **Never `--force`**, never move or delete an existing tag.
- The remote may print a branch-protection **bypass warning** ("Changes must
  be made through a pull request"): the push goes through for admins —
  **surface the warning** in the report, do not hide it.

### 6. Watch CI

The run takes several minutes — poll in the background (`run_in_background`
Bash with a sleep loop, ~60–90 s interval), never busy-wait.

- **With `gh`**: `gh run list --workflow=Release --branch v<version>` then
  `gh run watch <id> --exit-status`.
- **Without `gh`** (public API; for tag pushes `head_branch` = the tag name):

```sh
curl -fsSL "https://api.github.com/repos/davidbonan/Helm/actions/runs?branch=v<version>&event=push" \
  | grep -m1 '"conclusion"'
```

`"conclusion": null` → still running; `"success"` → step 7; anything else →
§Failures.

### 7. Verify the published release

Non-destructive, in a tempdir — **never** into `/Applications` (installing is
the user's call; they can run
`! curl -fsSL https://raw.githubusercontent.com/davidbonan/Helm/main/install.sh | sh`
themselves):

```sh
tmp="$(mktemp -d)"
curl -fsSL -o "$tmp/helm-macos.zip" \
  "https://github.com/davidbonan/Helm/releases/download/v<version>/helm-macos.zip"
ditto -x -k "$tmp/helm-macos.zip" "$tmp"
xattr -p com.apple.quarantine "$tmp/helm.app"   # expected: "No such xattr"
codesign --verify --strict "$tmp/helm.app"
/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' \
  "$tmp/helm.app/Contents/Info.plist"            # expected: <version>
```

Optional smoke launch: `open "$tmp/helm.app"`, confirm it starts, quit it
(`pkill -x helm`). Clean the tempdir afterwards.

### 8. Report

- Version, release commit hash, tag, CI run URL, release URL, asset size.
- Verification evidence: quarantine absent, codesign output, plist version.
- Any branch-protection bypass warning from the push.
- Reminder: bundled installs of **older** versions now show the boot toast
  "Update available v<version>" → Install & Relaunch (update.md §6) — this is
  the in-app update demo.

## Failures

- **CI red after the tag was pushed**: read `gh run view <id> --log-failed`
  (or the run URL) and report. The tag exists but no release was published —
  **fix forward**: repair on `main`, then release the **next** patch version.
  Deleting/re-pushing a published tag only happens on an explicit user ask
  (caches and clones may already hold it).
- **Release exists but the asset is missing/corrupt**: report; re-running the
  workflow from the Actions UI is the user's call.
- **Push rejected** (protection, auth): report verbatim, do not retry with
  force or alternate remotes.

## Guardrails

- **One release at a time**; the tag is always derived from `Cargo.toml`
  after the bump (CI enforces the match).
- The release commit contains **only** `Cargo.toml` + `Cargo.lock` +
  `release-notes.md` (the authored notes section, §3).
- Never force-push, never retag, never touch `/Applications`.
- Skill scope is deployment only: no `STATE.md` / specs edits.
