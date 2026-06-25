# helm — Architecture

Module breakdown, thread model, data flow and persistence.
Aligned with the rules in [`CLAUDE.md`](../CLAUDE.md): **DDD / Clean Code** —
the domain (git, terminal/PTY, split tree) is isolated from the UI rendering.

## 1. Principles

- **Pure domain with no UI dependency**: the `git`, `terminal`, `workspace`
  modules do not know about egui; they expose state and operations.
- **UI = rendering/adaptation layer**: `ui` reads the domain state and emits
  intents (commands); it contains no business logic.
- **No speculative abstraction** (CLAUDE.md): a single **lib + bin** crate,
  internal modules; we move to a multi-crate workspace only if a real need
  requires it.
- **Lib + bin split**: `src/lib.rs` carries the modules (testable surface),
  `src/main.rs` is a thin wrapper (`helm::app::run()`). Necessary because
  integration tests (`tests/`) only see the library's public API —
  cf. [`testing.md`](testing.md) §2.

### UI components — the intent pattern (normative)

Canonical component shape (review finding 5):

```rust
fn component(ui: &mut egui::Ui, state: &State, intents: &mut Vec<Intent>) -> …
```

- **Domain mutations only via intents**: a component never writes domain
  state (status, graph, workspace…) directly; it pushes a typed intent
  (`GitIntent`, …) that `app` applies after rendering.
- **Pure view-state may live in the component's state struct** (taken
  `&mut`): collapsed sections, selection, scroll, hover — anything whose
  loss would not change domain behavior.
- **Reference implementation**: `ui::diff_view` — reads a `FileDiff`,
  mutates only its `DiffViewState` (line selection…), emits `GitIntent`s;
  its `bool` return ("close requested") is view-state too.

New components follow this shape; existing deviations converge on it (notably
`git_panel` and `graph_view`).

## 2. Module breakdown

**lib + bin** crate (cf. §1). Modules exposed by `src/lib.rs`:

| Module | Responsibility | Depends on |
|--------|----------------|-----------|
| `app` | Global state, `eframe` loop, keyboard/command routing | all |
| `ui` | egui rendering: sidebars, terminal view, diff view, token application | `theme`, reads the domain |
| `theme` | Tokens (design-system §1), Auto/Light/Dark mode | — |
| `workspace` | Repository model, ordered list, active repository; **per repo: a set of tabs** (each a tree of splits) + active tab | `terminal`, `git` |
| `terminal` | Split tree, pane, `alacritty_terminal` + `portable-pty` integration | — |
| `git` | Repo, status model, hunk/line staging, commit, branch (wraps `git2`) | — |
| `pull_requests` | Workspace PR model + sources (GitHub via `gh`, Bitbucket Cloud via `curl`), `PrRunner`; pure parsers I/O-free ([`pull-requests.md`](pull-requests.md)) | `git` (forge parse) |
| `persistence` | Load/save preferences & repository list (serde) | `workspace`, `theme` |

Anticipated submodules: `terminal::{pty, emu, layout}`, `git::{status, diff, stage, commit}`.

## 3. Threads & data flow

- **UI thread (main)**: `eframe`/egui loop; owns the application state;
  draws; translates events into commands. For each pane, it **reads** the
  terminal grid and **writes** the keyboard input / `resize` to the PTY.
- **PTY reader threads**: one per pane; read the PTY stream and feed the
  `alacritty_terminal` parser, which mutates a grid (`Term`) **shared with
  the UI under lock** (e.g. `Arc<FairMutex<Term>>`); request a repaint
  (`egui::Context::request_repaint`) when the grid changes.
- **Git worker thread**: **owns** the `git2` `Repository` (not `Sync`) and
  runs the blocking calls (status, diff, apply, commit) off the UI thread;
  returns the results over a channel.
- **Git refresh**: the cadence and functional rules are defined in
  [`git.md`](git.md) §7. On the architecture side, the worker **wakes the UI**
  after each result (`request_repaint` callback) and the UI schedules its wakeup
  at rest via `request_repaint_after`.
- **Communication** — two distinct mechanisms:
  - **`crossbeam-channel` channels**: *commands* (UI → git worker, UI → PTY) and
    *git results* (worker → UI);
  - **shared state under lock**: the *terminal grid* (the reader writes, the UI reads).
  The UI never blocks on **I/O** (PTY reads and git calls run on their own
  threads); it only takes a **short lock** to read the grid on each frame.

Git path (channels):

```
[UI] --git command / status request--> [Git worker] --status/diff--> [UI]
                                              ^   (wakes the UI: request_repaint)
                                              +--- owns the git2 Repository ---
```

Terminal path (input via PTY handle, grid via lock):

```
[UI] --input / resize--> [PTY] --bytes--> [PTY reader] --writes--> [Term grid]
  ^                                                              (Arc<FairMutex>)
  +----------------------- reads the grid (lock) ----------------------+
```

### Thread lifetimes & the unbounded-channel contract (normative)

Two thread-lifetime families, both deliberate (review finding 15):

| Threads | Lifetime | Why |
|---------|----------|-----|
| Git worker (`git::worker`) | **Joined** on `Drop` (closing the command channel ends the loop) | Owns the `git2::Repository`; a clean exit point exists |
| PTY readers (`terminal::emu`) | **Detached** at drop; exit on PTY EOF | A `setsid` survivor still holding the slave would block the join — and the UI thread with it |
| One-shot runners — `ai::AiRunner`, `git::worker::SyncRunner`, `git::worktree::DeleteRunner`, `update::UpdateRunner`, `pull_requests::PrRunner` | **Detached**, one thread per request | Abandoning the session/repo lets the subprocess finish on its own; the late reply is discarded |

All UI ⇄ thread channels are **unbounded** (`crossbeam_channel::unbounded`).
This is sound only under the standing invariant:

- **one reply per request** — the runners gate with `in_flight`/`busy` (a new
  request is refused while one runs; `UpdateRunner` emits at most a short
  fixed event burst per request);
- results are **drained every frame** by the UI (`try_recv` loops), so queue
  depth is bounded by construction;
- every send is followed by the `on_event` callback (→ `request_repaint`), so
  a reply never sits in the queue waiting for user input.

Any future **streaming** producer (progress ticks, log following — many
messages per request) breaks this invariant and must switch to a **bounded**
channel with explicit backpressure instead of growing the queue.

## 4. Persistence

- **Location**: `~/Library/Application Support/helm/` (via `directories`).
- **Format**: a single **TOML** preferences file (`serde` + `toml`) —
  **application** preferences only (not the window chrome, cf. below).
- **Persisted content**:
  - **ordered** list of repositories (absolute paths) + active repository;
  - theme mode (Auto / Light / Dark);
  - widths and open/closed state of the left/right sidebars.
- **Window geometry** (size, position): **delegated to eframe's native
  persistence** (`persistence` feature), which restores size/position taking
  DPI and multi-screen into account. We do not duplicate it in our TOML — a
  single source of truth per responsibility (chrome → eframe, domain → TOML).
- **Not persisted**: live terminal sessions (PTYs not restorable; cf.
  terminal.md §10), **number/order of tabs** (each repository starts again with a
  fresh tab), content/scrollback, git index state (lives in the repository).
- Repository paths gone at startup: flagged (grayed-out row) rather than
  silently removed.

## 5. Locked dependencies

Decided (overview.md §4). Runtime foundation in `Cargo.toml`: `eframe`/`egui`,
`git2`, `portable-pty`; the others are added at their milestone.

| Crate | Role |
|-------|------|
| `eframe` / `egui` | Window + UI (GPU) |
| `alacritty_terminal` | Terminal emulation |
| `portable-pty` | PTY + shell |
| `git2` | Status / index / commit / diff |
| `serde` (+ `toml`) | Preferences persistence |
| `directories` | macOS paths (Application Support) |
| `crossbeam-channel` | UI ⇄ worker channels (git, PTY) |
| `libc` | libproc binding (`proc_pidinfo`): current `cwd` of a pane inherited on split (terminal.md §2) |

`git2` is compiled **`default-features = false`**: no https/ssh transport —
push/pull/fetch are out of MVP (git.md §1), so neither `openssl` nor `libssh2`
to link.

## 6. Tests

Feedback loop at **3 levels**, all under `cargo test`. Detail, patterns and
examples: [`testing.md`](testing.md).

- **Unit** — pure logic (no I/O), within the module: git status mapping
  (flags → sections), filtered diff for hunk/line staging, operations on
  the split tree (split, reabsorption, focus navigation), preferences
  serialization.
- **Business e2e** (`tests/`) — domain against a real resource: status on a
  temporary repository (`tempfile` + `git2`), real PTY behavior (`portable-pty`).
- **UI e2e** (`tests/`) — egui rendering driven **headless** via `egui_kittest`
  (accessibility tree query, event simulation, assertions on the emitted
  intents). The rendering stays as `fn(&mut egui::Ui, …)` functions so it is
  callable outside `eframe`.

egui_kittest pixel snapshot: optional extension not enabled (testing.md §8).
Slices already in place: `git::status` (3 levels) + PTY e2e.
