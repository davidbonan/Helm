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
| `run <verb> [path]` | **CLI** — ask the running app about the Run strip (§9) |
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
  exits 0, having read or written nothing. Outside a bundle there is nothing for
  LaunchServices to raise, so the process says so on stderr instead of vanishing.

`flock` is released by the kernel even on a crash — no stale lock to collect,
unlike a pid file.

The rule is per **support dir**, and an unbundled build has its own:
`helm-dev` rather than `helm` (`persistence::support_dir_name`, which also names
eframe's storage dir). A `cargo run` build therefore starts while the installed
`.app` is open — the two never share prefs, window state or lock. The dev
instance starts on an empty workspace, its own.

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

## 9. Control socket — `helm run …`

`helm://` only pushes: LaunchServices delivers a URL and nothing comes back. But
"is a server already up on this worktree?" is a **question**, and the answer lives
in the running app — the Run strip's process is a PTY owned by `HelmApp`
([`git.md`](git.md) §3), not something on disk. The CLI therefore also speaks to
the app over a **Unix socket**, `<support dir>/helm.sock`, beside the prefs and
the instance lock. Same support dir ⇒ the same split for free: a `cargo run`
build (`helm-dev`) never answers for the installed bundle.

```sh
helm run status [path]      # that worktree's Run server (path defaults to .)
helm run list               # every worktree of the workspace
helm run start [path]       # start it — a running server is left alone
helm run stop [path]        # stop it (drops the pane, killing the process tree)
helm run relaunch [path]    # stop then start
helm run logs [path] [-n N] # tail of what the server printed (N default 40)
--json                      # machine-readable output, on any of them
```

The point is an **agent working in a worktree**: before spawning `npm run dev`
into its own shell it asks helm, and either finds the server already up (with the
port helm assigned it, `$PORT` resolved per worktree) or has helm start it — in
the Run strip, where the user can watch it, rather than in a shell nobody sees.

- **Protocol**: one JSON line in, one JSON line out, connection closed.
  Requests are `{"op":"status"|"list"|"start"|"stop"|"relaunch", "path":…}` and
  `{"op":"logs","path":…,"lines":N}`; the answer is `{"result":"runs","runs":[…]}`,
  `{"result":"logs","entry":{…},"lines":[…]}` or
  `{"result":"error","message":…}`. Each entry carries `worktree`, `project`,
  `branch`, `state` (`running` / `stopped` / `exited` / `failed`), `port`,
  `command` (the template, `$PORT` unsubstituted), `launch_command` (what
  actually spawns), `error` on a spawn failure and `exit_code` on `exited` — a
  server that was asked to stop and one that died with 7 are not the same news,
  and the code is right there in the `try_wait` that reaps the child.
- **`logs` reads the strip's own viewer** — the pane's grid, scrollback included
  (`emu::tail_text`), newest last, trailing blank rows dropped. Rows the terminal
  **wrapped are joined back** (`WRAPLINE` on a row's last cell), so `-n` counts
  logical lines and the output does not depend on how wide the strip happens to be
  on screen — it would otherwise, since a run pane spawns at 80 columns and is only
  resized once the strip actually paints. ANSI styling is already applied (the text
  is plain) and the buffer is capped by the shared 10 000-line scrollback; the tail
  walks up from the bottom and stops at `-n`, so it never pays for the whole
  history. Grid indexing ignores the display offset, so a user scrolling the strip
  does not move what the CLI reads. A **stopped** strip has no pane and therefore no
  buffer: the answer is an empty `lines`, not an error — `stop` throws the output
  away, `relaunch` starts a fresh buffer.
  On the terminal side the captured output goes to **stdout alone** (the state
  line and the empty-buffer note go to stderr), so `helm run logs | grep …` works.
- **Resolution stays in the CLI** (§3): the path is canonicalized and validated
  as a working tree before anything is sent, so a typo is a terminal error, and
  the app only ever receives a canonical path.
- **The app answers on the UI thread**. The socket thread owns no state: it parks
  the request and blocks on the reply, exactly like the `helm://` handler parks a
  target (§4). The drain sits at the top of the frame, ahead of the Preferences
  gate — every worktree's Run pane lives on regardless of what is on screen, so
  the answer never depends on which repo is active or which page is open.
- **Mutations go through the strip's own path** (`apply_run_intent`): the same
  spawn, the same kill-on-drop, the same panes the buttons drive. Nothing is
  started outside helm's sight — and **no arbitrary command can be sent**: the
  socket carries a *verb and a path*, never a command line. What runs is what the
  project already resolves to (settings, else the manifest, §3 of `git.md`).
- **`start` is "make sure it is up"**, not "restart": on a live process it is a
  no-op that reports the running entry, so an agent cannot silently kill the
  server the user is reading. `relaunch` is the explicit restart.
- **The UI is left alone**: a socket-driven start does not reveal the sidebar,
  expand the strip or switch the active repo (unlike `Cmd+R`, `git.md` §3) — a
  background request must not steal the user's view.
- **An unknown worktree is synced before being refused.** A worktree created
  outside helm only joins its group on the next sync, and that tick is gated on the
  window being **focused** ([`worktrees.md`](worktrees.md) §4) — an agent would be
  turned away for as long as helm sits in the background. A miss therefore runs one
  group sync and looks again; only then is it a refusal. (The branch label comes
  from the off-thread group refresh, so the very first answer on a freshly adopted
  worktree carries `branch: null`.)
- **Exit codes**: `0` answered, `1` refused (unknown path, worktree not open in
  helm, no run command resolved), `2` misuse, **`3` no answer to be had**. `3`
  covers both "helm is not running" and "helm is running but silent": a **hidden or
  minimized** app gets no draw callbacks from macOS, so `update` never runs and the
  answer — written on a frame — never comes (measured: hidden ⇒ nothing, unhidden ⇒
  25 ms). The socket thread then closes the connection **unanswered** rather than
  dressing silence up as a refusal, and the client (whose own timeout is longer)
  reports it as `3`. One rule for a caller: on `3`, do the job yourself. A stale
  socket file left by a crash reads as `3` too (`ECONNREFUSED`), and the next launch
  rebinds it: the instance lock has already proved no other helm is live.
- **Trust**: the socket is user-only (`0600`) in the user's support dir, and its
  whole vocabulary is five verbs over a path. A local process that could reach it
  could already run the project's command itself; what it *cannot* do is make
  helm run something the project has not configured.
- **`helm run` shadows a folder named `run`**: `helm run` is the subcommand;
  `helm ./run` opens the directory.

Covered by tests: argv parsing of every subcommand, `-n` and the human line format
including the exit code (unit, `cli::tests`); the socket round-trip — entries,
captured output, refusal, silence, no listener — (unit, `ipc::tests`); the tail
across the scrollback, the rejoining of wrapped rows and the blank lines kept
inside the output (unit, `emu::tests`); and the app side against a real workspace:
list resolution, start/stop of a real PTY, the no-op start on a live process, the
output of a live process and the empty buffer of a stopped one, the code a dead
command returns, the adoption of a worktree created outside helm, the refusals
(unit, `app::tests`). Verified by hand on a dev instance: the hidden-window
timeout, a 121-character line coming back whole out of an 80-column grid, and
`exit_code` 7 surfacing in both output forms.
