# Files — worktree explorer in the right sidebar

Spec for the **Files** tab of the right sidebar (Terminal mode) and its
read-only file viewer. Backend: `std::fs` + `git2` (ignore rules, status).

## 1. Scope

| In v1 | Out of v1 |
|-------|-----------|
| **Git \| Files** tabs in the right sidebar header, Terminal mode only | Files tab in Graph mode (tree at a commit) |
| Lazy tree of the **active worktree**: every entry but `.git`, gitignored ones dimmed | Filter / fuzzy search in the tree |
| Git status tint on files and their parent folders | Any write: create, rename, move, delete, drag & drop |
| Click a file ⇒ **read-only viewer** in the center zone (overlay, like the diff) | Context menu (Open in editor, Reveal in Finder, Copy path) |
| Keyboard navigation in the tree, same arming rule as the git file list | Editing in the viewer (`Cmd+E`), insert a path into the terminal |
| Tab, unfolded folders and selection kept **per worktree**, persisted | FS watching (the poll stays the only refresh, [`git.md`](git.md) §7) |

## 2. Tabs

- The main card header ([`git.md`](git.md) §3, band 1) swaps the **Git** title for
  a two-tab strip **Git** · **Files** (folder icon), aligned left. Active tab:
  `text.primary` + 2px `accent` underline; inactive: `text.secondary`, hover
  `text.primary` ([`design-system.md`](design-system.md) §4).
- **Git** tab: the sidebar as today (branch chip, Discard all / Refresh, summary,
  Unstaged / Staged, commit card). Its label carries the **N files changed**
  count as a muted badge, hidden at 0, so changes stay visible from Files.
- **Files** tab: the header keeps the **branch chip**; Discard all / Refresh give
  way to a single **Collapse all** icon. The tree takes the **whole sidebar
  height** — the commit card belongs to Git and is hidden here.
- **Graph mode**: no tabs; the sidebar shows the WIP status or the commit detail
  as today ([`git.md`](git.md) §9). Back in Terminal mode, the worktree's last
  tab returns.
- Switching tab never closes an open diff or viewer.
- Shortcut `Cmd+Shift+E` toggles Git ⇄ Files and reveals the sidebar if hidden
  ([`keybindings.md`](keybindings.md)).

## 3. Tree

- **Root**: the active worktree's directory, not shown as a row — its children are
  the first level. A repo/worktree switch shows that worktree's own tree state.
- **Entries**: everything `std::fs::read_dir` returns, dotfiles included, **except
  `.git`** (dir or file). Symlinks listed with a link glyph, **never followed**
  (not expandable, opening one shows its target path). Non-UTF-8 names skipped.
- **Order**: folders first, then files; each group by name, case-insensitive,
  natural order (`file2` before `file10`).
- **Lazy**: a folder is read when first unfolded; at most **2,000** entries per
  folder, then a muted *N more not shown* row.
- **Ignored**: an entry matched by the repo's ignore rules
  (`Repository::is_path_ignored`) renders in `text.muted`; inside an ignored
  folder everything is muted without per-entry checks.
- **Git status**: from the status the poll already holds (no extra git call). A
  changed file takes its `git.*` color (modified / added-untracked / conflicted);
  a folder holding any change shows a small dot of the strongest color below it
  (conflict > modified > added). Deleted files are not in the tree.
- **Row**: chevron (folders) · icon · name; indent 12pt per level; hover
  `bg.surface.hover`, selected `accent.subtle`. Long names truncate at the end,
  full relative path in the tooltip.
- **Empty worktree** (only `.git`): *No files*.

## 4. Viewer

- Click a file ⇒ the **viewer** opens as an overlay over the center zone, same
  chrome and dismissal as the diff view ([`git.md`](git.md) §4: `Esc` / close
  button returns to the terminal; a repo switch closes it).
- **Header**: relative path (dimmed folder, accented name), size, git status chip
  if changed. No action buttons.
- **Text**: line numbers + syntect highlighting, same syntaxes and fallback as the
  diff ([`git.md`](git.md) §4). Read-only, selectable, `Cmd+C` copies.
- **Image** (png, jpg, gif, webp, svg): the zoomable preview of the diff view.
- **Other binary**, non-UTF-8, or over **2 MB / 50,000 lines** ([`git.md`](git.md)
  §8): *Binary file* / *File too large to display* + size, nothing else.
- **Live**: while open, the file is re-read on the poll tick when its mtime or
  size changes; scroll position kept. Deleted meanwhile ⇒ *File no longer exists*.
- **Scroll position** kept per file (as the diff).
- Opening a file that is also in Unstaged/Staged still opens the **viewer**, not
  the diff — the diff stays reachable from the Git tab.

## 5. Keyboard

Armed by a click on a tree row, **disarmed** when a terminal regains focus — same
rule as the git file list ([`git.md`](git.md) §3).

| Key | Action |
|-----|--------|
| `↑` / `↓` | Previous / next visible row (no wrap); on a file, opens it in the viewer |
| `→` | Unfold a folder; on an unfolded one, go to its first child |
| `←` | Fold a folder; on a child, go to its parent |
| `Enter` | Toggle a folder / open a file |

## 6. Refresh

- No FS watcher. On each status poll tick ([`git.md`](git.md) §7), **only while the
  Files tab is visible**, the unfolded folders are re-listed in the worker (gated
  like status: a tick is skipped while the previous listing runs). Added/removed
  entries appear without touching fold state or selection.
- A selected entry that disappears moves the selection to its parent.

## 7. Persistence

Per worktree, in `Prefs` keyed by worktree path (as the `$PORT` overrides): active tab (`git` / `files`),
set of unfolded folders (relative paths), selected path. Missing paths on reload
are dropped silently. Default: Git tab, everything folded.

## 8. Architecture

- **Domain** `files` (lib, no egui): `resolve` / `list` currently in
  `remote::files` move here and gain a sort order (newest first for the phone,
  folders-first natural for the tree); ignore check and status-to-tint mapping
  are pure functions over `git2` / the status model.
- **UI** `ui::file_tree` (tree rendering + keyboard) and `ui::file_viewer`
  (reuses the diff view's highlighter, image preview and overlay chrome).
- Listing and file reads run on the git worker, never on the UI thread.

## 9. Tests

| Level | Covers |
|-------|--------|
| Unit | Sort (folders first, natural, case-insensitive); status → folder dot priority; `.git` and non-UTF-8 skipped; 2,000 cap |
| Business e2e | Real repo: ignored entry flagged, untracked/modified tints, symlink not followed, re-list picks up a new file |
| UI e2e (kittest) | Tabs switch and persist per worktree; Git badge count; unfold + click opens viewer with content; `↑/↓/←/→` navigation; Graph mode hides tabs; binary/too-large placeholders |
