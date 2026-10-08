# Files — worktree explorer in the right sidebar

Spec for the **Files** tab of the right sidebar (Terminal mode) and its
read-only file viewer. Backend: `std::fs` + `git2` (ignore rules, status).

## 1. Scope

| In v1 | Out of v1 |
|-------|-----------|
| **Git \| Files** tabs in the right sidebar header, Terminal mode only | Files tab in Graph mode (tree at a commit) |
| Lazy tree of the **active worktree**: every entry but `.git`, gitignored ones dimmed | Filter / fuzzy search in the tree |
| Git status tint on files and their parent folders | Any write: create, rename, move, delete, drag & drop |
| Click a file ⇒ **viewer** in the center zone (overlay, like the diff): click to edit, agent notes (§4.1, §4.2) | Context menu (Open in editor, Reveal in Finder, Copy path) |
| Keyboard navigation in the tree, same arming rule as the git file list | Forge (PR) notes in the viewer, insert a path into the terminal |
| Tab, unfolded folders and selection kept **per worktree**, persisted | FS watching (the poll stays the only refresh, [`git.md`](git.md) §7) |

## 2. Tabs

- The main card header ([`git.md`](git.md) §3, band 1) swaps the **Git** title for
  a two-tab strip **Git** · **Files** (folder icon), aligned left. Active tab:
  `text.primary` + 2px `accent` underline; inactive: `text.secondary`, hover
  `text.primary` ([`design-system.md`](design-system.md) §4).
- **Git** tab: the sidebar as today (Discard all / Refresh, summary, Unstaged /
  Staged, commit card). No count on its label, and no branch chip beside the tabs:
  the workspace sidebar already names the branch.
- **Files** tab: Discard all / Refresh give way to a single **Collapse all** icon. The tree takes the **whole sidebar
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

### 3.1 File-type icons

- A file's icon is the **Nerd Font** glyph of its type (the embedded JetBrains Mono
  Nerd Font, no extra dependency), painted in that type's **brand color** — the
  nvim-web-devicons table: e.g. Rust orange, TypeScript blue, JSON yellow,
  Markdown, TOML, YAML, Dockerfile, shell, images, lock files…
- Lookup order: **exact name** (`Cargo.toml`, `Cargo.lock`, `Dockerfile`,
  `Makefile`, `.gitignore`, `package.json`, `LICENSE`, `README.md`…), then the
  **extension**, case-insensitive (last one; a few doubles like `.d.ts`); unknown ⇒
  the current plain file icon in `text.secondary`.
- Colors are theme tokens, one per type, readable on `bg.canvas` in **both** modes
  (a light variant where the brand color is too pale on white).
- Folders keep the Lucide folder icon; symlinks keep the link glyph.
- Shown in the **tree** and in the **header** of the viewer and of the diff view
  (status lists, commit detail and PR files keep their git status icon).

## 4. Viewer

- Click a file ⇒ the **viewer** opens as an overlay over the center zone, same
  chrome and dismissal as the diff view ([`git.md`](git.md) §4: `Esc` / close
  button returns to the terminal; a repo switch closes it).
- **Header**: file-type icon (§3.1), relative path (dimmed folder, accented name),
  size, git status chip if changed, the notes recap chip (§4.2); on the right the
  **close icon** (`X`, tooltip *Close (Esc)*) — the same icon closes the diff view.
- **Text**: line numbers + syntect highlighting, same syntaxes and fallback as the
  diff ([`git.md`](git.md) §4). Selectable (drag, double/triple click), `Cmd+C`
  copies.
- **Image** (png, jpg, gif, webp, svg): the zoomable preview of the diff view.
- **Other binary**, non-UTF-8, or over **2 MB / 50,000 lines** ([`git.md`](git.md)
  §8): *Binary file* / *File too large to display* + size, nothing else.
- **Live**: while open, the file is re-read on the poll tick when its mtime or
  size changes; scroll position kept. Deleted meanwhile ⇒ *File no longer exists*.
- **Scroll position** kept per file (as the diff).
- Opening a file that is also in Unstaged/Staged still opens the **viewer**, not
  the diff — the diff stays reachable from the Git tab.

### 4.1 Editing

Same semantics as the WIP diff's inline editor ([`git.md`](git.md) §4), on the
**whole file** instead of a hunk:

- A **click** (press + release, no drag) in the text opens the editor with the
  caret where clicked; `Cmd+E` opens it on the hovered line. A drag still selects.
- The editor is the diff's inline editor widget (syntax colored, own gutter, own
  undo history) spanning the file; the view keeps its scroll.
- **Leaving writes**: click outside the text or `Cmd+S` writes the buffer if it
  changed, through the diff's write path (byte-exact precondition on the whole
  file; divergence ⇒ the **Reload** / **Overwrite** notice). `Esc` drops the
  buffer, nothing written; a second `Esc` closes the viewer. No idle write.
- Opening another file, a diff, switching repo or sending a review **writes** an
  open buffer first (as the diff does).
- While the editor is open the live re-read (§4) is suspended; it resumes on exit.
- **Not editable** (click does nothing, `Cmd+E` toasts the reason with *Open in
  editor*): any placeholder state (binary, too large, image, missing, unreadable,
  symlink), a file the process cannot write, or a file above the **editable line
  cap** (measured at implementation so typing stays fluid; initial target
  10,000 lines).

### 4.2 Agent notes

Same notes as the WIP diff ([`pull-requests.md`](pull-requests.md) §11, agent pool
only):

- Hovering a line shows the **note** button next to its number; click ⇒ the note
  editor under the line (`Enter` *Save note* queues it, `Cmd+Enter` saves and
  sends the batch, `Shift+Enter` newline, `Esc` cancels). A saved note shows as a
  card under its line; clicking it edits it.
- Anchor: the working-tree line number + its text, as a WIP diff note on an added /
  context line — so viewer and diff notes are **one batch per worktree**: the
  recap chip (viewer and diff headers) lists both, and *Send to {agent}* sends
  them together, then clears the batch.
- Notes are hidden while the editor is open; line numbers are not re-anchored
  after an edit (same limit as the diff).
- `Esc` cascade in the viewer: editor → note editor → close.

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
| UI e2e (kittest) | Tabs switch and persist per worktree; unfold + click opens viewer with content; `↑/↓/←/→` navigation; Graph mode hides tabs; binary/too-large placeholders |
