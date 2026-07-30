# helm — Keyboard shortcuts

Single source for shortcuts (macOS). The other specs reference this file
instead of redefining keys. Convention: `Cmd` = ⌘, `Alt` = ⌥, `Ctrl` = ⌃,
`Shift` = ⇧. The tables below are the **defaults**: a curated set of actions
is customizable from the Preferences page (§6).

> **Provenance.** `Cmd+D`, `Cmd+Shift+D`, `Cmd+W` are **Ghostty parallels**
> (same keys as Ghostty for split/close). The **navigation**
> and **resize** keys between panes are a **helm choice** (not
> checked against the Ghostty defaults), to be adjusted on the prototype.

## 1. Global (always active)

| Shortcut | Action |
|-----------|--------|
| `Cmd+O` | **Open Folder** → adds the folder as a repo in the left sidebar (see [`overview.md`](overview.md) §3.1) |
| `Cmd+Ctrl+0` | Open the **Agents** dashboard ([`agents.md`](agents.md) §5) — slot 0 of the positional repo family, above repo 1 in the sidebar. No-op on the empty workspace (no dashboard yet) |
| `Cmd+Ctrl+1` … `Cmd+Ctrl+9` | Select repo 1 to 9 (sidebar order). Beyond 9: no shortcut (click/scroll) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | **Cycle repo** next / previous in sidebar order, wrapping (worktrees included, folded/bare rows skipped). Layout-independent (no number row); rebindable (§6) |
| `Cmd+T` | **New Tab** terminal in the active repo (a fresh pane in the repo's folder). Without an active repo: no-op |
| `Cmd+1` … `Cmd+9` | Select **tab** 1 to 9 of the active repo. Beyond 9: no shortcut (click) |
| `Cmd+,` | Toggle the full-window **Preferences page** ([`preferences.md`](preferences.md)); reopening = closing |
| `Cmd+B` | Show / hide the **workspace sidebar** (left), with animation. Visible by default |
| `Cmd+G` | Show / hide the **git sidebar** (right), with animation. Hidden by default; also drivable via the top-right icon |
| `Cmd+Shift+G` | Toggle the center zone **Terminal ⇄ Git** (equivalent to the header switch, [`git.md`](git.md) §9; entering Graph reveals the git sidebar). Without an active repo: no-op |
| `Cmd+R` | **Run / Relaunch** the active project's server in the Run strip ([`git.md`](git.md) §3): starts it, or relaunches it if already running. Reveals the git sidebar and expands the strip; with no command resolved, opens the inline editor. Without an active repo: no-op |

`Cmd+0` is **not** a tab selector: it is reserved for resetting the terminal
zoom (§2). Not to be confused: `Cmd+1..9` changes **tab** in the active repo
([`terminal.md`](terminal.md) §1), `Cmd+Ctrl+1..9` changes **repo** (left
sidebar). `Cmd+Shift+1..9` is intentionally **unbound** (`Cmd+Shift+3..9` are
reserved by macOS for screenshots).

## 2. Terminal (focused pane)

| Shortcut | Action |
|-----------|--------|
| `Cmd+D` | Vertical split: new pane **to the right** |
| `Cmd+Shift+D` | Horizontal split: new pane **at the bottom** |
| `Cmd+W` | Close the focused pane (the sibling takes the space); last pane of a tab ⇒ closes the **tab** ([`terminal.md`](terminal.md) §11) |
| `Cmd+Alt+←/→/↑/↓` | Move the **focus** to the neighboring pane |
| `Cmd+Ctrl+←/→/↑/↓` | **Resize** the focused split (moves the divider shared with the neighboring pane) |
| `Cmd+C` | Copy the selection (if no selection: no-op, **not** forwarded to the PTY) |
| `Cmd+Backspace` | Deletes the line being edited (sends `Ctrl+U` to the PTY) |
| `Cmd+←` / `Cmd+→` | Cursor to start / end of line (sends `Ctrl+A` / `Ctrl+E` to the PTY) |
| `Alt+←` / `Alt+→` | Cursor to previous / next word (shell line editing) |
| `Alt+Backspace` | Deletes the previous word |
| `Cmd+K` | **Clear Terminal** (clears the screen and the scrollback) |
| `Shift+PageUp` / `Shift+PageDown` | Scroll through the scrollback |
| `Tab` / `Shift+Tab` | Forwarded to the PTY (`\t` / backtab `CSI Z`) — never consumed by the egui focus navigation, like `Esc` and the arrows |
| `Shift+Enter` | `CSI 13;2u` without negotiation (kitty/Ghostty convention) — newline in agent harnesses (Claude Code, Codex) ([`terminal.md`](terminal.md) §3) |
| `Option+Enter` | `Esc`+`CR` (meta+enter) — Claude Code newline ([`terminal.md`](terminal.md) §3) |
| `Cmd+V` | Paste into the PTY |
| `Cmd+click` | Opens the **URL / file path** under the pointer; holding `Cmd` underlines the hovered link ([`terminal.md`](terminal.md) §12) |
| `Cmd+=` / `Cmd+-` | Font zoom +/− (**global**: all panes and repos) |
| `Cmd+0` | Reset font zoom (global) |
| `Ctrl+C`, `Ctrl+D`, `Ctrl+Z`, … | Forwarded as is to the PTY (shell signals) |

Behavior details: [`terminal.md`](terminal.md).

## 3. Git & diff view

| Shortcut | Action |
|-----------|--------|
| `Click` on a file (right sidebar) | Opens the file's **diff** in the center zone (overlay) |
| `Drag` in the diff view | Selects text in the diff content (without the `+`/`-` signs) |
| `Double-click` / `Triple-click` in the diff view | Selects the word / the whole line in the diff content — on a **read-only** surface, and on the rows of an editable diff that cannot take a caret. Where a caret can open, the first click of the pair already swapped the rows for the buffer, and the selection is then the editor's own |
| `Click` in the diff **content** (editable diff) | Places the **caret** on that line and opens the **inline editor** ([`git.md`](git.md) §4); read-only surface ⇒ no-op |
| `Click` on a line's **number strip** (numbers + sign) | Toggles that line's pick for partial stage/unstage |
| `Cmd+E` (diff view) | Opens the inline editor on the **hovered** line; where no caret can open (non-editable file, or a hunk with nothing on the new side / above the line cap), toasts the reason with an **Open in editor** action |
| `Cmd+S` (inline editor open) | Writes the buffer and **leaves** the editor — the keyboard's version of clicking elsewhere (there is no save control) |
| `Cmd+Z` (inline editor open) | Undo inside the buffer (`Esc` never reverts a change); each editor has its own history, never the previous hunk's |
| `Esc` (inline editor open) | Leaves the editor **keeping** the change; a second `Esc` closes the diff |
| `Cmd+C` (diff view) | Copies the diff's text selection; without a selection: no-op |
| `↑` / `↓` (file selected in the sidebar) | Opens the diff of the previous / next file, traversing only the files (**Unstaged** then **Staged**) with start/end wrap. Disarmed as soon as a **terminal** regains keyboard focus (the arrows go back to the PTY); rearmed on the next click on a file |
| `↑` / `↓` (**Graph** mode) | Moves the **commit selection** row by row (**WIP** row included), scrolling the targeted row in the viewport — **no wrap** (paginated history). Inactive if a widget has keyboard focus or if the arrows already navigate elsewhere (rows below) |
| `Cmd+F` (**Graph** mode) | Opens the **search box** (top-right of the graph): filters the loaded commits and cycles the matches. `Enter` / chevrons → next match (`Shift+Enter` → previous), each scrolled into view; `Esc` / ✕ closes ([`git.md`](git.md) §9) |
| `↑` / `↓` (**commit** diff open) | Opens the diff of the previous / next file **of the commit** (sidebar list), with start/end wrap — same traversal as the status files |
| `Esc` | Closes the diff view, returns to the repo's terminal |
| `Esc` (diff opened from the **graph**) | Closes the commit diff, returns to the **graph** (post-MVP, [`git.md`](git.md) §9) |
| `Cmd+Enter` | **Commit** (if the message is non-empty and at least one file is staged) |

Per-hunk/line staging is done from the diff view via mouse
controls (no dedicated shortcut in the MVP); the `Esc` cascade in the diff runs
**inline editor → note editor → close the diff**. The **Git graph** (post-MVP) is toggled via
the **header switch** "Terminal ⇄ Git" or `Cmd+Shift+G` (§1,
[`git.md`](git.md) §9). Details: [`git.md`](git.md).

## 4. Focus & reservations

helm has a single **active zone** that receives the keyboard, chosen on click:
terminal pane, left sidebar, right sidebar or commit field. The selection
of shortcuts depends on this zone.

- **Global** (`Cmd+O`, `Cmd+1..9`, `Cmd+Ctrl+0`, `Cmd+Ctrl+1..9`, `Ctrl+Tab`/`Ctrl+Shift+Tab`,
  `Cmd+T`, `Cmd+,`, `Cmd+B`, `Cmd+G`, `Cmd+Shift+G`, `Cmd+R`):
  active whatever the active zone; `Cmd+1..9` (tabs) and `Cmd+Ctrl+1..9`
  (repos) take precedence over any terminal use of the same combinations.
- **Terminal** (§2): active **only** when a terminal pane has focus.
- **Commit field**: captures text input and `Cmd+Enter` (commit); the
  terminal shortcuts are inactive there.
- **Diff view** open: only the §3 shortcuts (`Cmd+C` included) and `Esc` apply.
- **Inline editor** open ([`git.md`](git.md) §4): it takes the text input, so the
  sidebar's `↑`/`↓` file navigation is **disarmed** and `Cmd+Enter` (commit) is
  inactive until it closes; the global shortcuts of §1 keep applying (an action
  that would tear the diff down flushes the buffer first).
- **Preferences page** open ([`preferences.md`](preferences.md)): **exclusive**
  active zone — the global app shortcuts are inactive; only the
  **preferences toggle** (`Cmd+,` by default, §6) and `Esc` (close) apply.

## 5. Shortcut hints (holding Cmd)

Discoverability aid: as long as the user **holds `Cmd`**, a badge
showing the shortcut appears **next to each clickable zone** that has
a global shortcut (§1). The badge appears on `Cmd` `keydown` and disappears on
`keyup` (or on the window losing focus); it overlays on top, without
changing the layout.

| Clickable zone | Badge shown |
|----------------|---------------|
| Repos 1 to 9 in the left sidebar (display order) | `⌃⌘1` … `⌃⌘9` (for `Cmd+Ctrl+1` … `Cmd+Ctrl+9`) |
| Tabs 1 to 9 in the active repo (tab bar) | `⌘1` … `⌘9` (for `Cmd+1` … `Cmd+9`) |
| **Open Folder** button / zone | `O` (for `Cmd+O`) |
| Workspace sidebar icon (in the left sidebar) | `B` (for `Cmd+B`) |
| Git sidebar icon (top right) | `G` (for `Cmd+G`) |
| "Terminal ⇄ Git" switch (center zone header) | `⇧⌘G` (for `Cmd+Shift+G`) |
| **Run / Relaunch** button (Run strip, git sidebar) | `⌘R` (for `Cmd+R`) — left of the button the shortcut triggers (Run when stopped, Relaunch when running) |
| Access to **Preferences** | `,` (for `Cmd+,`) |

Beyond the 9th repo, no badge (no associated shortcut, §1). Only the
**global shortcuts associated with a clickable zone** are shown: triggerable
by click as by keyboard, the overlay visually links the two. The display of the
badges is purely indicative and does not capture the mouse (the zones stay
clickable).

The table above shows the **default** bindings: badges (and any inline shortcut
reminder, e.g. the Open Folder empty state) render the **current** binding from
the keymap (§6) — a rebound action shows its new combo, an unbound action shows
**no badge**.

## 6. Customization (Preferences → Keyboard)

A curated set of actions is **rebindable** from the Preferences page
([`preferences.md`](preferences.md) §4, Keyboard section); everything else is
fixed. The §1–§3 tables are the defaults.

### Rebindable actions

| Group | Action id | Default |
|-------|-----------|---------|
| Global | `open-folder` | `Cmd+O` |
| Global | `new-tab` | `Cmd+T` |
| Global | `toggle-preferences` | `Cmd+,` |
| Global | `toggle-workspace-sidebar` | `Cmd+B` |
| Global | `toggle-git-sidebar` | `Cmd+G` |
| Global | `toggle-graph` | `Cmd+Shift+G` |
| Global | `next-repo` / `prev-repo` | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Terminal | `split-right` / `split-down` | `Cmd+D` / `Cmd+Shift+D` |
| Terminal | `close-pane` | `Cmd+W` |
| Terminal | `focus-left` / `focus-right` / `focus-up` / `focus-down` | `Cmd+Alt+←/→/↑/↓` |
| Terminal | `resize-left` / `resize-right` / `resize-up` / `resize-down` | `Cmd+Ctrl+←/→/↑/↓` |
| Terminal | `zoom-in` / `zoom-out` / `zoom-reset` | `Cmd+=` / `Cmd+-` / `Cmd+0` |
| Terminal | `clear-terminal` | `Cmd+K` |
| Git | `commit` | `Cmd+Enter` |

### Not rebindable

- The **positional ranges** `Cmd+1..9` (tabs) and `Cmd+Ctrl+1..9` (repos):
  fixed, and **reserved** (refused at capture time).
- The **PTY passthrough / shell editing** keys (§2): `Cmd+C`, `Cmd+V`,
  `Cmd+Backspace`, `Cmd+←/→`, `Alt+…`, `Tab`, `Shift+Enter`, `Option+Enter`,
  `Ctrl+*` signals — their semantics belong to the terminal, not to helm.
- The **diff / graph navigation** keys (§3): `↑/↓`, `Cmd+F`, `Esc`.
- The **inline editor** keys (§3): `Cmd+E`, `Cmd+S`, `Cmd+Z` — fixed, they only
  exist while a diff (resp. the editor) is open.

### Binding rules

- A binding is exactly **one non-modifier key** plus at least one of
  `Cmd` / `Ctrl` / `Alt` (`Shift` alone is refused — it would swallow typing).
- **Conflicts**: a combo already bound to another rebindable action (whatever
  its group — zones overlap at runtime) or reserved (ranges, `Esc`) is
  **refused** at capture, with an inline error naming the holder. No silent
  stealing.
- **Unbind** is allowed: the action then has no shortcut — never matched, no
  badge (§5), still reachable by mouse.
- **Reset** per action (back to default) and **Restore defaults** for the whole
  set (preferences.md §4).
- Bindings apply **immediately** (the keymap is rebuilt and the routing reads
  it) and are persisted in the `keybindings` table of `prefs.toml`
  (preferences.md §5): only deviations from the defaults are written, `""` =
  unbound; an unknown action id or unparsable combo is **ignored at resolution**
  (default applies) without rewriting the TOML.
