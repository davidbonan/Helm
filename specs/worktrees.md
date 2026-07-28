# Worktrees — grouping in the left sidebar

A git repository can hold **linked worktrees** (`git worktree`). The left
sidebar groups each project under a **non-selectable header** (group identity +
actions); the **main** worktree and the linked worktrees are selectable **rows**
below it.

## 1. Principle

```
   helm                + ▾  ← project header (non-selectable): name (left) · + · chevron (right)
   🗀 main                   ← main worktree: folder icon, branch label, single line, pinned first
   ⑃ feature-x              ← linked worktree: worktree icon, folder name (title)
       feature-x                + branch (caption) on two lines, manual order
   ⑃ fix-bug
       fix/bug
  ──────────────────────── ← 1px border.subtle separator, full sidebar width, between projects
   other-project        + ▾ ← header always present, even with just a main row
   🗀 main
```

- **The project header is not selectable**: it carries the project identity (the
  root folder name, left-aligned at the shared sidebar indent) and, clustered at
  the **right edge**, a **`+`** button (Create worktree, §6) and the **collapse
  chevron**. *Reveal in Finder · Copy path · Hide project · Remove from sidebar*
  (§6) live on the header's **right-click** context menu. An
  **aggregate agent badge** (max over the project's worktrees,
  [`agents.md`](agents.md) §1) shows in the cluster when the group is collapsed.
  Clicking the header toggles the group's collapse. The header is **always
  present**, even for a project with no linked worktree.
- **Hiding a project** filters its whole block (header + every worktree row) out
  of the sidebar. It is triggered from the header's right-click *Hide project*
  or by unchecking the project in the **eye** dropdown at the **PROJECTS**
  section label ([`design-system.md`](design-system.md)) — that dropdown lists
  **every** project, hidden ones included, and is the only place to bring one
  back. Hiding is purely a view filter: the project keeps its place, config and
  worktrees, and the state is **persisted** (§5). If the **active** worktree
  belongs to the project being hidden, the central area falls back to the
  **agents dashboard** ([`agents.md`](agents.md)).
- **Each worktree row is a full-fledged repository**: selectable, with its own
  set of tabs/splits ([`terminal.md`](terminal.md) §1), its own git session
  (status / diff / graph). The **main** worktree is pinned **first** with a
  **folder** icon; linked worktrees follow in manual order with a **worktree**
  (folder-with-git) icon. Row icons share the **same indent** as the header name
  and the section labels.
- **Row content**: the **main** row is a single line — its **branch** as the
  label (folder name on hover). A **linked worktree** row is **two lines** — the
  **folder name** as the title, the **branch** as a dimmer caption beneath it
  (the branch indicator also stays in the git panel, [`git.md`](git.md) §6).
- A **1px `border.subtle` separator** spanning the **full sidebar width** sits
  between one project and the next.
- Projects mix in the import / manual order of the list ([`overview.md`](overview.md) §3.1, §9).

## 2. Resolution at import (Open Folder)

- Chosen folder ⇒ `Repository::open`; if `is_worktree()` ⇒ **root = parent of
  `commondir()`**. A **submodule** is not a worktree (the `is_worktree()` test
  excludes it) ⇒ standalone entry, never grouped.
- The **full group** is added: root + all enumerated worktrees
  (`worktrees()`), even if the user only chose one.
- **Deduplication by root path** (canonicalized): importing a worktree of a
  project already present does not add a duplicate — the existing group is completed.
- The line **activated** after import = the path the user chose (not
  necessarily the root).
- **Non-git** folder: **refused** — Open Folder is git-only
  ([`overview.md`](overview.md) §3.1): an error toast, nothing added.

## 3. Display

- **Order**: the **header** comes first, then the **main** worktree row (pinned
  first, never reordered), then the linked worktrees. The order is **manual**:
  projects reorder among themselves and the linked worktrees reorder **within
  their group** by drag-and-drop (§9), and the chosen order is **persisted** (§5).
  A fresh import seeds the linked worktrees **alphabetically** (the `git worktree
  list` order = creation order, less predictable); afterwards the manual order is
  authoritative.
- **Row content**: the **main** row is a single line (the **branch** as label,
  folder name on hover); a **linked worktree** row is **two lines** — the folder
  name as the title, the branch as a dimmer caption beneath. (The branch also
  stays visible in the git panel indicator, [`git.md`](git.md) §6.)
- **Icon**: the **main** worktree bears a **folder** icon; a linked worktree
  bears a **worktree** (folder-with-git) icon, at the shared sidebar indent.
- **Collapse**: every header bears a disclosure **chevron** at its **right edge**
  (in the action cluster, after `+`); clicking the header
  collapses the group, hiding its rows (main + linked). The state is
  **persisted** (§5).
- **Separator**: a 1px `border.subtle` line between one project and the next.

## 4. Synchronization (discovery & purge)

A group's worktree list is **synchronized with the disk**, both
ways, and **persisted** on every change:

- **Triggers**: startup · the window **regains focus** · a **5 s periodic tick
  while the window is focused** (so a worktree created from a terminal appears
  without a defocus/refocus round-trip) · after a **Delete worktree** (§6). The
  tick is gated on focus: off-focus, the focus-regain trigger covers the user
  coming back and the app sleeps. No FS watcher for discovery (v1).
- **Discovery**: a worktree created outside the app (terminal, another tool) appears in
  the group (**appended** after the existing manual order, since there is no
  alphabetical slot to honour anymore) and is added to the prefs.
- **Purge**: a worktree deleted outside the app disappears from the sidebar and prefs;
  its PTYs are killed; if it was **active**, the selection falls back to the root.
- **Root gone** from disk ⇒ **whole group purged** (extension of the existing
  startup purge).

## 5. Persistence

`prefs.toml` moves from the flat list `repos = [paths]` to a list of projects;
`active` becomes a **path** (indices move with the sync):

```toml
active = "/Users/dev/helm-studio.worktrees/feature-x"

[[projects]]
root = "/Users/dev/helm-studio"
worktrees = ["/Users/dev/helm-studio.worktrees/feature-x"]
```

- A plain git repo without a linked worktree = `[[projects]]` with `root` alone.
- A **collapsed** group records `collapsed = true` on its `[[projects]]` entry;
  the key is omitted when the group is expanded (the default).
- A **hidden** project records `hidden = true` on its `[[projects]]` entry; the
  key is omitted when the project is visible (the default).
- **Migration**: the old flat format (`repos` + `active` index) is converted on
  first load — each path resolved to its group (§2), the active index
  remapped to a path — then the TOML is rewritten in the new format.

## 6. Actions

| Element | Menu / action |
|-------|------|
| **Project header** | **+** button ⇒ **Create worktree** modal; **right-click** context menu: Reveal in Finder · Copy path · Hide project · **Remove from sidebar** ⇒ removes the **whole group** from the app (prefs included), does not touch the disk (= current Remove, [`overview.md`](overview.md) §3.1). |
| **Main row** | Context menu: Reveal in Finder · Copy path. No Delete — the main worktree is the repository itself. |
| **Linked worktree row** | Context menu: Reveal in Finder · Copy path · **Rename worktree…** (§6) · **Delete worktree from disk** — **actually** deletes the worktree from the disk. |

**Create worktree**:

- The project header has a `+` button. It opens a modal with an **autocomplete filter
  input** over a **single list** of eligible source branches (no local/remote
  grouping — the `origin/` prefix tells remotes apart): local branches not
  already checked out in the root or another linked worktree, and
  remote-tracking branches whose local branch does not already exist **or whose
  local homonym is a stale leftover safe to refresh** (not checked out, no
  unpushed commits, strictly behind the remote — creating then deletes that
  local branch and recreates it on the remote tip). `origin/HEAD`, invalid path
  segments and branches whose destination already exists are omitted. The input is focused on opening; typing filters the list
  (the first visible branch takes the selection if the filter hides it), ↑/↓
  move the selection, Enter creates.
- **Create a branch on the fly**: the filter input doubles as the new-branch
  name field. When its text is a valid branch name (same validation as the
  worktree name) that matches no existing branch — compared
  **case-insensitively** (loose refs collide on APFS) against all local
  branches (including checked-out ones, absent from the list) and the local
  name of every remote source — a pinned **“Create branch ‘<text>’ from
  <base>”** row appears below the list, outside the scroll area. The base is
  the **root worktree's HEAD** (branch name, or short commit id when detached)
  — `git worktree add -b`'s default — displayed in the row, never chosen. The
  row sits **last**: while matches are visible the default selection stays on
  the first match (Enter never creates a branch by accident); ↓ past the last
  match reaches it, and with zero matches it takes the selection. The branch is
  created at the base commit **without upstream** (set on first push); the
  Worktree name field follows the typed name like any selection. The create
  action revalidates that the name is still valid and still unused, and the
  branch is deleted if the worktree checkout then fails (no trace on failure).
  With zero eligible sources the input stays visible — the empty list reads as
  an invitation to type a name to create a branch.
- Destination: the **base** defaults to `<root>.worktrees` (for root
  `/Users/dev/helm-studio` ⇒ `/Users/dev/helm-studio.worktrees`) and is
  overridable per project (see *Per-project settings* below); the folder under
  the base comes from the
  **Worktree name** field, pre-filled with the branch name (it follows the
  selection until the user types a custom name; clearing the field resumes the
  follow). Slashes nest folders (branch `feat/toto` ⇒
  `…worktrees/feat/toto` by default); the name is validated like a branch
  path (relative, no `..`/`.`/empty segments — invalid ⇒ inline error, Create
  disabled). A remote source `origin/feat/toto` creates/checks out local
  branch `feat/toto`, so the remote name is never part of the default path.
- The create action revalidates the source immediately before writing. If a
  remote source creates a local branch and worktree checkout then fails, the
  local branch created by helm is rolled back. When the remote source refreshed
  a stale local homonym, that branch's prior tip is restored on failure.
- Git graph context menus also offer **Create worktree** for eligible branch
  chips / row branch entries; stash rows and tags never offer it.

**Per-project settings** (configured in Preferences → **Project**,
[`preferences.md`](preferences.md) §4):

- **Worktrees base** — base folder new worktrees of this project are created
  under. Empty ⇒ default `<root>.worktrees`; an absolute path is used verbatim;
  a relative path is resolved against the root. A **Choose…** native folder
  picker fills it. The create modal previews the resolved destination live, and
  a missing base directory is created on first worktree.
- **Post-create script** — bash run in the new worktree's **first terminal**
  immediately after creation (live, interactive, fire-and-forget — no
  pass/fail). The user types either a script path (`./setup.sh`) or inline
  commands. It runs with `HELM_WORKTREE_PATH`, `HELM_WORKTREE_BRANCH`,
  `HELM_PROJECT_ROOT` and `HELM_SOURCE_BRANCH` exported (set on the pane, not
  echoed). For a branch created on the fly, `HELM_SOURCE_BRANCH` is the base
  (the root HEAD), not the new branch. An empty/whitespace-only script is a
  no-op.
- Both are **personal** prefs (stored in `prefs.toml`, scoped by project root),
  **not** checked into the repo: a repo-sourced post-create script would be an
  arbitrary-code-execution vector on pull. Team-sharing is out of v1 scope.

**Rename worktree** (linked worktrees only — the main worktree is the repository
itself):

- The row's *Rename worktree…* opens a modal with a single **Worktree name**
  field, pre-filled with the current folder name and validated like the create
  modal's (relative, no `..`/`.`/empty segment); the resolved destination is
  previewed live. **Rename** stays disabled while the name is invalid or
  unchanged, and `Enter` confirms.
- The new name is resolved against the worktree's **own parent folder**: renaming
  moves the folder in place, it never relocates the worktree under another base.
  Slashes nest, as in the create modal.
- Implementation: `git worktree move` (libgit2 exposes no equivalent — the move
  also repoints the worktree's `gitdir`/`commondir`). The **branch is untouched**.
  A refusal (locked worktree, destination taken, git error) keeps the modal open
  with the reason inline.
- The sidebar entry **follows the move in place**: it keeps its slot, its
  tabs/splits, its running terminals (an agent at work included) and the
  selection — the rename is not a delete plus a discovery (§4).

**Delete worktree**:

- **clean and holding no ignored file** ⇒ immediate deletion, no confirmation
  (the branch survives in the repo);
- **dirty** ⇒ "*N file(s) with uncommitted changes*" modal with
  `[Cancel] [Delete anyway]`;
- **clean but holding ignored files** ⇒ "*N ignored file(s) will be deleted with
  the folder*" modal with `[Cancel] [Delete anyway]`. The folder deletion takes
  everything the post-create script wrote (`.env`, build output) — never listed
  in the git panel ([`git.md`](git.md) §2), so it has to be surfaced here. The
  count comes from a status pass that does **not** recurse into ignored
  directories (`target/` counts as one), keeping it cheap on a large worktree;
- **locked** ⇒ refused, lock reason displayed.

Implementation: status check → folder deletion →
`Worktree::prune` of the metadata (libgit2 has no full `git worktree remove`).
The deletion runs off the frame — a folder carrying `target/` or `node_modules/`
takes a while — with its row greyed out and inert under a spinner meanwhile;
**several worktrees delete at once**, so starting one never swallows the Delete
of the next. If the deleted worktree was active: PTYs killed, selection folded
back to the root. **No Remove (hiding) on a child**: discovery (§4) would
make it reappear.

## 7. Shortcuts

`⌘1..⌘9` follow the **flattened visual order of the selectable rows** (main +
linked worktrees, project after project), **skipping the non-selectable headers**
and any row hidden under a collapsed group or in a hidden project; past the 9th,
click/scroll ([`keybindings.md`](keybindings.md) otherwise unchanged).

## 8. Edge cases

- **Bare root** (worktrees of a bare repo): the header is shown (non-selectable,
  as for any project) and there is **no main row** — a bare repo has no main
  working tree — only the linked worktree rows. Assumed v1 limitation.
- **Prunable worktree** (broken gitdir): treated as deleted from disk ⇒ purged.
- **Submodule**: never grouped (§2).
- Equivalent paths (symlinks): dedup by **canonicalized** path.
- **Missing folder** (git repo whose folder vanished): current behavior
  preserved (purged on load, vanished-path state in the panel).

## 9. Reordering (drag-and-drop)

The sidebar order is user-controlled and **persisted** (§5):

- **Drag a row** to reorder it. An accent insertion line marks where it will
  land; releasing applies the move.
- **Projects** (a plain repo or a whole group) reorder among the top level. A
  group moves as a **block**: its root carries its children with it.
- **Worktrees** reorder **within their own group** only — a child cannot leave
  its group nor cross into another, and cannot displace its root (the root keeps
  the first slot, §3). A drop that would break these rules is a no-op.
- A drop onto a row's own slot changes nothing.
- The active selection is preserved across a reorder (it follows its repo by
  identity, not by index).

## 10. Out of scope (v1)

- Worktree lock from the app (rename = `git worktree move` within the worktree's
  own parent folder, §6; relocating one under another base stays out).
- FS watcher for discovery (the §4 triggers — including the focused 5 s tick —
  are sufficient).
- Choosing the base of a branch created on the fly (always the root HEAD; a
  git-graph context menu entry could later create branch + worktree from any
  commit).
