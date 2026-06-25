# helm — Specification

## 0. Spec index

| Spec | Contents |
|------|---------|
| `overview.md` (this file) | Objective, scope, 3-zone layout, locked decisions, delivery slices |
| [`architecture.md`](architecture.md) | Modules (DDD), threads, data flow, persistence, dependencies |
| [`testing.md`](testing.md) | Feedback loop: 3 test levels (unit, business e2e, UI e2e egui_kittest) |
| [`terminal.md`](terminal.md) | PTY, emulation, splits, focus, scrollback, palette, per-repo tabs |
| [`git.md`](git.md) | Status, hunk/line staging, diff, commit, branch, refresh |
| [`conflicts.md`](conflicts.md) | In-app 3-zone merge/rebase conflict editor: read stages, take checkboxes, resolve + Continue |
| [`worktrees.md`](worktrees.md) | Worktrees grouped in the sidebar: root resolution, discovery/purge, Delete worktree |
| [`agents.md`](agents.md) | AI agent detection in terminals: sidebar activity badge (states, heuristic, limits) |
| [`pull-requests.md`](pull-requests.md) | Workspace PR cockpit: sidebar entry below Agents, GitHub (`gh`) + Bitbucket Cloud sources, list/detail/checkout |
| [`preferences.md`](preferences.md) | Full-window Preferences page: left nav + settings cards, Appearance/Git sections |
| [`update.md`](update.md) | Distribution (.app bundle, GitHub releases) + built-in update: check, download, replace, relaunch |
| [`keybindings.md`](keybindings.md) | Complete shortcut reference |
| [`design-system.md`](design-system.md) | Visual tokens (colors / typography / spacing) + components |

## 1. Objective

A native, high-performance development environment, written in Rust, **macOS
only**, bringing together in a single window:

1. A central **terminal**, with keyboard-driven Ghostty-style pane splits.
2. A **left sidebar**: navigation between git repositories (folders).
3. A **right sidebar**: git state of the active repository, in three sections
   (unstaged / staged / message + commit button).

Aesthetic target: get close to the **Codex Desktop** UI (modern, clean
interface, custom-drawn panels). **Light + dark** themes (default Auto, follows
macOS). Tokens and components: [`design-system.md`](design-system.md).

## 2. Scope

- Single window, 3-zone layout (left / center / right).
- Functional terminal (PTY + emulation) with keyboard splits; **per-repo
  tabs**, each tab being a tree of splits (see [`terminal.md`](terminal.md)).
- List of git repositories **manually added** by the user (**Open Folder**),
  with selection of the active repository.
- Git panel: status, stage/unstage **by file, hunk and line**, diff view,
  message + commit, branch indicator (read-only). Detail and limits:
  [`git.md`](git.md).
- **Post-MVP**: **Git graph & commit detail** (read-only) — a
  "Terminal ⇄ Git" switch in the center zone header displays the commit
  graph, its detail (metadata + files) and a commit's diff in full screen
  ([`git.md`](git.md) §9).

## 3. Interface layout

The right zone is part of the target layout, but it is **hidden initially**;
it is revealed via the git sidebar action (shortcut in
[`keybindings.md`](keybindings.md)) or the icon in the top right.

```
┌────────────┬───────────────────────────────┬──────────────────────────┐
│  LEFT      │           TERMINAL            │      GIT (3 sections)    │
│  SIDEBAR   │   (center zone, splittable)   │                          │
│            │                               │  ┌────────────────────┐  │
│  Git       │   ┌───────────┬───────────┐   │  │ 1. Unstaged        │  │
│  repos:    │   │  pane A   │  pane B    │   │  │   - file X         │  │
│   • repo-1 │   │           │            │   │  │   - file Y         │  │
│   • repo-2 │   ├───────────┴───────────┤   │  ├────────────────────┤  │
│   • repo-3 │   │        pane C          │   │  │ 2. Staged          │  │
│            │   │                        │   │  │   - file X         │  │
│            │   └────────────────────────┘   │  ├────────────────────┤  │
│            │                               │  │ 3. Commit message  │  │
│            │                               │  │   [____________]   │  │
│            │                               │  │       [ Commit ]   │  │
└────────────┴───────────────────────────────┴──────────────────────────┘
```

The **diff** view (click on a git file) opens as an overlay over the center
zone; its closing and the return to the terminal are defined in
[`keybindings.md`](keybindings.md) (see [`git.md`](git.md) §4).

**No repository**: on first launch (or after removing the last repository), the
center zone shows an empty state inviting **Open Folder…** (title + one-line
tagline; both yield to the button + ⌘O hint when the central zone is narrower
than the title); no terminal exists until a repository is added. The right
sidebar, if shown, reads **No repository open** and the workspace
launcher (top right) is hidden.

### 3.1 Left sidebar — navigation between repositories

- **Adding a repository**: the **Open Folder** action opens a folder (macOS
  dialog, multi-selection possible) and adds it to the list. **Open Folder is
  git-only**: a folder that is **not** a git repository is **refused** — an error
  **toast**, nothing added. **No recursive scan** of a root folder: the user adds
  repositories one by one. The list is **persisted** between sessions
  ([`architecture.md`](architecture.md) §4).
- The active repository drives: the `cwd` of new terminal panes **and** the
  content of the right sidebar. Each repository has **its own set of terminal
  tabs** (each tab = a tree of splits), restored when returning to it
  ([`terminal.md`](terminal.md) §1).
- Repository selection via shortcut is defined in
  [`keybindings.md`](keybindings.md); beyond the 9th repository, the user uses
  click/scroll.
- **Removing a repository**: right-click on the row → **Remove from
  sidebar**. The repository
  leaves the persisted list and **all its terminal tabs are closed**
  ([`terminal.md`](terminal.md) §11); the folder on disk is not touched.
- **Worktrees**: an imported folder that is a worktree is resolved to its root
  repository and the sidebar groups the project under a **non-selectable header**
  (the main worktree and linked worktrees as rows below it); see
  [`worktrees.md`](worktrees.md).

### 3.2 Center zone — terminal

Real emulation (PTY + parser), split actions, pane tree, keyboard focus,
closing, resizing, scrollback.
**Tabs**: a **tab bar** sits atop the center zone (above the splits); each
repository has its own set of tabs, the **New Tab** action adds some and the tab
shortcuts are defined in [`keybindings.md`](keybindings.md)
([`terminal.md`](terminal.md) §1, §4). Full spec: [`terminal.md`](terminal.md).

### 3.3 Right sidebar — git (3 sections)

The **branch indicator** (read-only) sits atop the sidebar, above the three
sections ([`git.md`](git.md) §6).

1. **Unstaged**: **Stage** action (by file, by hunk/line via the diff, and
   **Stage All**).
2. **Staged**: **Unstage** action (symmetric) and **Unstage All**.
3. **Commit**: **Summary** / **Description (optional)** fields + **Commit**
   button (disabled if the message is empty or nothing is staged).

A file can appear in **both** sections (partially staged).
The panel reflects the **active repository** and refreshes after an action and
on disk change. State model, diff view, granular staging, branch and edge
cases: [`git.md`](git.md).

## 4. Technical stack — locked decisions

| Domain | Choice | Notes |
|---------|-------|-------|
| **UI** | `eframe` / `egui` | 100% Rust, GPU, stable. `gpui` (Zed) considered for high fidelity but **dropped from the MVP** (poorly documented/unstable as a dependency). Re-platforming possible later, out of scope. |
| **Terminal — emulation** | `alacritty_terminal` | Grid + VTE parser + scrollback. |
| **Terminal — PTY** | `portable-pty` | PTY + shell opening. |
| **Git** | `git2` (libgit2) | Covers status, index (stage/unstage hunk/line), diff, commit. `gix` dropped (index/commit less ergonomic today). |
| **Git refresh** | git worker | Refresh cadence and rules defined in [`git.md`](git.md) §7. |
| **Persistence** | `serde` + `toml` + `directories` | Preferences (TOML) in Application Support (architecture.md §4). |
| **Concurrency** | `crossbeam-channel` | UI ⇄ worker channels (never block the UI; architecture.md §3). |

Foundation runtime crates: `eframe`, `egui`, `git2`, `portable-pty`. Test
tooling (dev-dependencies): `egui_kittest` (headless UI e2e), `tempfile`
(disposable git repos) — see [`testing.md`](testing.md). The other locked crates
are added at their respective milestone.

## 5. Delivery slices

- **Scaffold**: cargo project that builds/runs, specs, README, commands.
- **Window + layout**: 3 empty zones, light/dark themes (design-system),
  dependencies added to `Cargo.toml`.
- **Terminal**: a functional pane (PTY + emulation + rendering).
- **Splits**: pane tree, focus, closing.
- **Left sidebar**: add repository, persisted list, selection by shortcut,
  **terminal space per repository** (restoration on switch).
- **Git (status + commit)**: unstaged/staged sections, stage/unstage by file,
  commit, branch indicator, refresh.
- **Diff & granular staging**: central diff view, stage/unstage by hunk/line
  ([`git.md`](git.md) §4).
- **Polish**: complete shortcuts, Codex visual alignment, empty states,
  **Preferences** window (theme toggle), window/theme persistence.
- **Terminal tabs (MVP core)**: several tabs per repository (each tab = a tree
  of splits), tab bar in the center zone header, creation, selection and closing
  of tabs ([`terminal.md`](terminal.md) §1).
- **Git graph & commit detail**: header switch **Terminal ⇄ Git**;
  **commit graph** (all local refs) in place of the terminal; **commit detail +
  files** in the right sidebar; click on a file ⇒ **full-screen diff**
  (read-only). **Post-MVP** — read-only history ([`git.md`](git.md) §9).
