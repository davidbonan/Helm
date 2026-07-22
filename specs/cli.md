# CLI — opening a project or worktree from outside the app

`helm <path>` opens a repository — or one of its worktrees — in the running
Helm. The same door serves any other application through a `helm://` URL.

## 1. Intent

Helm stays open all day. Coming from a terminal (or from Raycast, a script, a
link) the user wants **one gesture**: point at a checkout, get Helm on that
worktree, focused, ready to type. Never a second window, never a duplicated
workspace, never a file dialog.

```sh
helm                 # launch the app (no target)
helm .               # open the repository containing the current directory
helm ~/dev/api       # open that project
helm ~/dev/api.worktrees/feature-x   # open that worktree
helm --help | --version
```

A **path is the whole address**: a linked worktree is a directory, and
`git::worktree::resolve_root` maps any path back to its group root. There is no
project/worktree argument pair to disambiguate.

## 2. One binary, two modes

`src/main.rs` dispatches on argv (`cli::parse`):

| argv | Mode |
|---|---|
| *(empty)* | **GUI** — unchanged launch, what Finder / Dock / `open` produce |
| `-psn_…` | **GUI** — Carbon-era process serial number some LaunchServices launches still append; it is a launch, not a CLI call |
| `--open-url <url>` | **GUI**, with a startup target injected (§5) |
| `<path>` | **CLI** — resolve, hand over, exit |
| `-h`/`--help`, `-V`/`--version` | print, exit 0 |
| anything else | usage on stderr, exit 2 |

Only one binary is built, bundled and signed; CLI mode never initializes
`eframe`.

## 3. Resolution and validation — in the CLI

The terminal is where a terminal error belongs, so the CLI resolves **before**
waking the app. `cli::resolve_target`:

1. `canonicalize` — a missing path stops here (`no such directory`).
2. `Repository::discover` — walks **up**, so `helm .` works from any
   subdirectory of a checkout. A path under a submodule therefore opens the
   submodule; accepted, it is what the path points at.
3. `workdir()` — a **bare** repository has none: refused with *"point at one of
   its worktrees"*, since a bare root owns no selectable row
   ([`worktrees.md`](worktrees.md) §8).
4. The result must be **UTF-8**: the workspace is persisted as TOML, which
   cannot round-trip anything else.

Failure ⇒ message on stderr prefixed `helm:` and **exit 1**, without launching
anything. Success ⇒ the canonical **working tree** path travels in the URL: the
exact row to activate, the app derives its group root from there.

## 4. Handover — the `helm://` scheme

The bundle registers `CFBundleURLTypes` for the `helm` scheme
(`scripts/bundle.sh`), and the CLI runs:

```
open "helm://open?path=/Users/me/dev/api.worktrees/feature-x"
```

LaunchServices then provides, for free, the three things a hand-rolled IPC would
have to build: **launch if not running**, **deliver**, **raise to front**. The
CLI returns immediately; it does not wait for the app.

- **Shape**: `helm://open?path=<percent-encoded>`. `/` stays literal for
  readability; `+` is **not** a space (a path may contain one). One verb today;
  an unknown host or an unknown extra parameter is **ignored**, so an older
  binary survives a URL minted by a newer one.
- **The path must be absolute** — a relative one would resolve against the app's
  working directory, not the caller's.
- **Trust**: any web page can fire a `helm://` link, and since the CLI goes
  through `open` the app cannot tell a shell invocation from a clicked link. The
  answer is not a prompt (it would kill the gesture) but a **narrow contract**:
  `open` is the only verb, `path` the only parameter, and it is re-validated as
  an existing git working tree on arrival. Nothing in the scheme can carry a
  command to execute.
- **Delivery**: the handler is claimed on the `NSAppleEventManager`
  (`kInternetEventClass` / `kAEGetURL`), installed **before** `run_native`, i.e.
  before an `NSApplication` even exists — a cold launch must not miss the very
  URL that caused it, and `eframe` only hands us a hook once the event loop is
  already running. The handler parks the target and wakes the loop
  (`app::url_scheme`); the app drains it at the top of its next frame, ahead of
  the Preferences gate. Verified on a bundled winit app: the handler survives
  AppKit's launch — it fires on the cold-launch URL *before* the loop resumes,
  and again on every later URL. The documented alternative, a custom
  `NSApplicationDelegate` implementing `application:openURLs:` (winit guarantees
  it registers no delegate of its own, `winit::platform::macos`), is therefore
  not needed.

**Applying a target** (`app::activate_target`):

1. Project unknown ⇒ **full group import**, root + every worktree, the same
   `add_picked_folders` path Open Folder takes ([`worktrees.md`](worktrees.md) §2).
2. The row is **revealed**: the project is unhidden and its group unfolded — a
   hidden project would drop the central area onto the agents dashboard (§1),
   and a folded group hides even the root's own main row (§3).
3. `set_active` — a plain sidebar-click activation, so `sync_git_session` parks
   the leaving session and drops the modals armed on it exactly as on any repo
   switch. A target arriving while a dialog is open is applied, not queued.
4. `Page::Main` + `CentralMode::Terminal`, on the worktree's existing active tab.
   No tab is created: a reflex `helm .` must not pile shells up.

A target refused on arrival (vanished repo, hand-written URL on a bare root)
raises an **error toast** — the CLI has already exited by then.

## 5. Development and tests

LaunchServices only delivers `helm://` to a **registered `.app`**; `cargo run` is
not a bundle and will never receive an Apple Event. The GUI therefore accepts
`--open-url <url>`, which pushes into the same startup buffer:

```sh
cargo run -- --open-url 'helm://open?path=/Users/me/dev/api'
```

Covered by tests: argv parsing and the URL round-trip (unit, `cli::tests`), path
resolution against real repositories / worktrees / bare repos and the workspace
mutation — import, reveal, activation (business e2e, `tests/it/cli_e2e.rs`).
Not covered, verified by hand on a bundled build: the Apple Event bridge itself.

## 6. Single instance

Prefs are rewritten whole (`persistence::save_to`), so two live instances mean
the last writer silently erases the other's workspace. GUI mode therefore takes
an **advisory `flock`** on `<support dir>/instance.lock` before touching
anything:

- lock free ⇒ held for the life of the process, normal start;
- lock taken ⇒ the running instance is raised (`open <bundle>`) and this process
  exits 0, having read or written nothing.

`flock` is released by the kernel even on a crash — no stale lock to collect,
unlike a pid file.

## 7. Installing the command

*Preferences › Terminal › Shell command* symlinks `helm` into `/usr/local/bin`
(on the default macOS `PATH`, per `/etc/paths`). The link points **inside the
bundle** (`…/helm.app/Contents/MacOS/helm`), so an in-place update
([`update.md`](update.md) §5) keeps it working.

The row reads the link on the page's frames: **Install** when absent,
**Replace** when a foreign `helm` holds the path, the install directory when it
is ours. A non-writable `/usr/local/bin` — the common case without Homebrew —
is not a bare errno but the exact `sudo ln -sf …` to run. A real file at that
path is never replaced: it belongs to something else.

## 8. Edge cases

- **Bare root** — refused, by the CLI (§3) and on arrival alike: the inbound
  target goes back through the very same resolution, so a hand-written URL
  raises the same error toast without importing anything. Which also means
  `helm://open?path=<subdir>` walks up to its working tree, exactly like
  `helm <subdir>`.
- **Worktree created outside the app** — an unknown path whose root is already
  in the workspace joins its group (`sync_group`), then activates.
- **Removed project** — `helm <path>` re-imports it; the CLI is also the way
  back in.
- **Not installed as a bundle** — `open` reports no application for the scheme;
  the CLI passes its exit code through.
