# helm — Git

Spec for the right sidebar and the diff view. Backend: **`git2`** (libgit2).
Visual tokens: [`design-system.md`](design-system.md) §7. Shortcuts:
[`keybindings.md`](keybindings.md) §3.

## 1. Scope

**In the MVP:**
- `status` of the active repo (modified / untracked / staged / partial).
- Stage / unstage **per file**, **per hunk** and **per line**.
- **Discard** a file or all unstaged changes, **behind a confirmation**
  (opt-in addition decided after the fact).
- **Diff** view (read + selection for staging).
- **Commit** (summary + optional description, signature from the git config).
- Current **branch indicator** (read only).

> The whole UI is in **English**; the labels cited here (Stage, Unstage,
> Stage All, Discard, Commit…) are the strings actually rendered.

**Post-MVP** (see §9): commit **graph / history** (read only), detail of a
commit (meta + files) and full-screen commit diff. **Post-MVP** (see §10):
**graph action toolbar** — fetch / pull / push (via the `git` binary as a
subprocess), branch creation, Stash / Pop.

**Out of MVP** (explicitly not implemented; do not add without a decision):
amend (Reword in the interactive rebase covers message edits, §9), remote
management, annotated tags (the
graph creates lightweight tags only, §9), a dedicated stash list (the graph
rows and the toolbar cover the stash flows, §9–§10), Undo / Redo of operations
(deferred to a later milestone). The graph started **read only**; every write
it carries today is a **decided exception**, specified in §9 — **checkout**
(a branch by double-click or menu, a tag menu-only and detached, always with a
safety **automatic stash**), branch **create / rename / delete** (deletions
behind a **confirmation modal**), the three **rebase** flavors (plain,
**interactive**, **AI-driven** — never a history rewrite of another branch),
**merge**, **cherry-pick**, **revert**, **reset** (hard behind a modal), tag
**create / push / delete**, per-stash **apply / pop / delete** — and in §10
for the toolbar (pull / push / branch / stash / pop; **force push** only as
`--force-with-lease` behind an explicit one-shot entry and a modal). Anything
outside that list stays out without a new decision; a conflict left behind by
any of these operations is resolved in the in-app conflict editor
([`conflicts.md`](conflicts.md)) or aborted from the banner, like a Pull
conflict (§10).

## 2. State model

Built from `git2::Statuses` (`StatusOptions` with `include_untracked` and
`renames`). Mapping of flags to our sections:

| git2 flag | Section |
|-----------|---------|
| `WT_NEW` | Unstaged (untracked) |
| `WT_MODIFIED`, `WT_DELETED`, `WT_TYPECHANGE`, `WT_RENAMED` | Unstaged |
| `INDEX_NEW`, `INDEX_MODIFIED`, `INDEX_DELETED`, `INDEX_TYPECHANGE`, `INDEX_RENAMED` | Staged |

- A single file can carry **both** a `WT_*` flag and an `INDEX_*` flag
  → it appears **in both sections** ("partially staged" case:
  some hunks indexed, others not).
- **Renamed** entries act on **both paths**: Stage / Unstage (per file or All)
  move the old path's deletion together with the new path — never a
  half-staged rename residue. Discard restores the old path while deleting
  the new one.
- **Ignored** files (`.gitignore`): not listed.
- Conflicts (`CONFLICTED`): listed with a **Conflict** badge; resolution runs
  through the in-app conflict editor ([`conflicts.md`](conflicts.md)), with the
  terminal as a fallback for the kinds it does not cover.

## 3. Right sidebar — two cards

Light/dark mockup redesign: the sidebar is made of **two cards** on `bg.canvas`,
**with no border or background of their own**
(the sidebar edges form the frame — design-system §4).

**Main card** — bands separated by full-width rules:

1. **Header**: git-branch icon + **Git** title + **branch chip** (mono, §6);
   on the right, **Discard all** (trash — destructive, behind the modal) and
   **Refresh** icons.
2. **Summary**: "**N files changed**" (entries of both sections) + totals
   **+A** / **−D** (sum of the deltas of both sections; a half-staged file
   cumulates both deltas) + a proportional green/red **ratio bar** (a non-zero
   side keeps a minimum readable width; no change ⇒ empty track).
3. **Unstaged** — working-tree files not indexed. Per-file action on
   hover: **Stage** + **Discard** (pills). Global action: **Stage All**
   (conflicts skipped — read only, §2; a single index write whatever the
   file count).
4. **Staged** — indexed files. Per-file action on hover: **Unstage** (pill).
   Global action: **Unstage All**.

Sections 3–4 each occupy a **fixed-height block** (same height at
0 entries; internal scroll if the list overflows; 1px separators between rows);
**collapsible** headers "Unstaged (N)" / "Staged (N)" (chevron,
`text.primary`) with their global action aligned to the right; both sections
empty ⇒ **Nothing to commit**.

**Commit card** (detached at the bottom): **Commit message** label (red asterisk —
required) + framed input with an inline **n / 72** counter; **Description
(optional)** label + framed textarea with an **n / 1000** counter (indicative
limits: beyond them the counter turns the conflict color, never blocking); a
full-width primary button **Commit N files** (N = staged files; "Commit" at 0), disabled
(`state.disabled`) if the summary is empty **or** nothing is staged. **No
Commit & Push** nor a "…" overflow menu: push lives in the graph toolbar
(§10), never in this card.

Each row: **colored** status icon (`git.*`, design-system §1) — plus
(addition / untracked), pencil (modified), minus (deleted), arrow (renamed),
exclamation (conflict); a conflicted row **opens the conflict editor**
([`conflicts.md`](conflicts.md)) on click, with no stats pill. Relative path (dimmed folder, accented
file); **`+N` / `−N` stats** of the section's delta aligned to the right
(hidden for a binary or an empty delta), replaced by the action pills on
hover. Click on the path → opens the **diff view** (§4). After this click, `↑` / `↓`
opens the previous / next visible file, traversing only the files
(Unstaged then Staged, with no stop/focus on the headers), wraps to the start/end
when the list boundary is reached, and keeps the selected row visible within
its internal scroll. This navigation **disarms** as soon as a terminal regains
keyboard focus (click in the pane, or `Esc` that closes the diff view): the arrows
then belong to the PTY and never reopen a diff; a click on a file
rearms it.

**Selection & row context menu**: **Cmd+click** toggles a file in/out of a
multi-selection, **Shift+click** extends the selection to the clicked row from
the anchor (last plain/Cmd click); a plain click resets the selection to that
one file and opens its diff. **Right-click** anchors the menu on the clicked
row (making it the lone target when it sits outside the current selection). One
file ⇒ the contextual **Stage**/**Unstage** + **Discard** (unstaged rows) +
**Stash** over the shared **Copy path** / **Copy relative path** / **Reveal in
Finder** / **Open in editor** entries; a multi-selection ⇒ the batch
**Stage** / **Unstage** / **Discard** / **Stash** only. **Stash** shelves the
**whole** file — staged **and** unstaged changes together, untracked included,
**never a partial stash** — and a multi-file stash lands in a **single** entry;
behind a confirmation modal that spells this out.

**Discard** (**destructive** action, behind a confirmation modal):
- *untracked file* → deletion from disk;
- *tracked file* → restore from the **index** (an already-staged part is
  **preserved** — discard is the inverse of the working-tree delta, not a reset);
- *conflict* → **ignored** (read only, consistent with §2). "Discard all" iterates over the
  unstaged files, skipping conflicts. Paths are treated **literally**
  (never as globs). A deletion that fails on disk surfaces as a toast —
  never a silent no-op.

**Run strip** (bottom of the sidebar): a thin panel pinned below the cards to
launch and watch the project's server. **One process per worktree**, but the
**run command is shared by the whole project group** (root + worktrees) — stored
in the project's settings, editable both inline (pencil in the strip) and in
Preferences ([`preferences.md`](preferences.md)). When unset, it is
**auto-detected** from the worktree's manifest (`Cargo.toml` → `cargo run`,
`package.json` → `<pm> run <script>` with the package manager read from the
lockfile and the script being the first of `dev` / `start` / `serve`, `go.mod` →
`go run .`); empty when nothing matches, the strip then prompts for a command.

**Per-worktree port**: a shared command that references `$PORT` (or `${PORT}`)
gets the placeholder substituted at launch so each worktree binds a distinct
port. The value is the project's **base port** (Preferences, default `3000`) plus
the worktree's **offset** within the group: the root is `+0`, each worktree `+1`
and up by its **rank among the group's worktree paths** — keyed on the worktree's
own path, not its sidebar row, so a drag-reorder never reshuffles assigned ports.
A **manual override** pins a worktree's port regardless. A `$PORT` glued to
another identifier (`$PORTAL`) is left untouched; a command without the
placeholder launches verbatim.

- **Header**: a collapse chevron, a status dot (running = green, failed = red,
  idle/exited = muted), the **Run** label, then — right-aligned — the controls,
  the resolved **`:PORT` chip** (only when the command uses `$PORT`; click to set
  this worktree's override, blank ⇒ back to the auto offset) and the command.
  **Stopped/exited** ⇒ a single **Run**; **running** ⇒ **Stop** (drops the pane,
  killing its process tree) and **Relaunch** (Stop then Run). `Cmd+R`
  ([`keybindings.md`](keybindings.md) §1) triggers Run, or Relaunch when already
  running — revealing the git sidebar and expanding the strip; with no command
  resolved it opens the inline editor instead.
- **Viewer**: a **read-only** terminal mirroring the process output — it scrolls
  on wheel but takes no keyboard focus and forwards nothing to the PTY (the
  buttons are the only controls).
- **Layout**: **resizable** by dragging its top edge and **collapsible** to the
  header; the height and the collapsed flag are **persisted** and restored on
  launch.

## 4. Diff view & granular staging

- **Location**: the diff view opens in the **center zone**, as an overlay above
  the repo's terminal workspace. The diff's close action returns to the
  terminal (shortcut in [`keybindings.md`](keybindings.md)). A repo switch
  also closes it and shows the target repo's terminal.
- **Content**: unified diff of the file (`git2::Diff`). Depending on the originating section:
  - from Unstaged → **working tree vs index** diff (staging possible);
  - from Staged → **index vs HEAD** diff (unstaging possible).
- **Syntax highlighting**: for text files recognized by extension, the
  line content is colored via **syntect** with the embedded
  **two-face** syntaxes (TypeScript/TSX, TOML, Dockerfile, Vue, Svelte…). The
  addition/deletion backgrounds and the staging controls stay on top; an unknown
  language, a binary or a large diff ⇒ raw monospace rendering.
- **Image preview**: a binary file with a recognized image extension (png, jpg,
  jpeg, gif, webp, bmp, ico, tiff) renders its **new-side blob** as a zoomable,
  pannable image instead of the *Binary file* placeholder — a toolbar with
  **Fit** / **100%** / **−** / **+**, plus trackpad pinch (or ⌘+scroll) to zoom
  and two-finger scroll to pan. The bytes come from the working tree, the index
  or the commit/stash blob; a deleted image or one above ~32 MB stays on the
  placeholder. Staging granularity is unchanged (file level, as any binary).
- **Granularity**: **Stage hunk** / **Unstage hunk** buttons per hunk, and
  **line** selection for partial stage/unstage. On the **Unstaged** side each
  hunk also offers a **Discard hunk** button (destructive — reverts that hunk's
  working-tree change to the index, **behind a confirmation**); never on the
  Staged side nor on the read-only commit diff (§9).
- **Text selection**: dragging over the diff content selects the file's
  text; double-click selects the word, triple-click the line; `Cmd+C` copies the
  selection (without the `+`/`-` signs or the gutter).
- **Gutter & line numbers**: two number columns before each line
  (**old** no. | **new** no.) — context = both, deletion = old
  only, addition = new only; colored `+`/`−` sign between the gutter and the
  content. View header: file icon + path + `+N −M` stats + **Close**.
- **Context extension**: an **Extend context** button in the header
  band of each hunk — reveals 5 extra context lines
  above **and** below (cumulative; hidden when there is nothing left to
  show), clamped to the file boundaries and to neighboring hunks (never a
  duplicate on screen). The lines come from the **new** side of the diff
  (worktree / index / commit blob — `FileDiff::source_lines`); **display
  only**: hunk/line staging stays based on the original diff. Also available
  on the read-only commit diff (§9).

**Mechanism (libgit2)**: we compute the file's diff, build a **filtered
diff** containing only the selected hunks/lines, then apply it to
the index via `Repository::apply` with `ApplyLocation::Index` (stage) or its inverse
(unstage). Per-line staging amounts to splitting a hunk into sub-hunks at the
line. **Discard hunk** is the working-tree twin of unstage: the hunk's reversed
Unstaged patch is applied with `ApplyLocation::WorkDir`, reverting the worktree to
the index for that hunk alone (a whole-file addition/deletion falls back to the
file-level Discard). After application, we recompute the `status` (§7).

## 5. Commit

- **Signature**: `Repository::signature()` (`user.name` / `user.email` config).
  If not configured → commit disabled with a hint "configure git user.name /
  user.email".
- **Message**: **Commit message** (one line, inline indicative counter at
  72 characters) + **Description (optional)** (multi-line, 1000 counter). The
  final git message joins the two with a **blank line** (git convention); without
  a description, only the summary is committed.
- **Activation conditions**: non-empty summary **and** ≥ 1 staged entry.
- **Action**: creates a commit on `HEAD` with the current index tree and the
  composed message. On success: summary + description cleared, status refreshed.
  While the commit is being written, the button shows a **spinner** and ignores
  clicks (shortcut included) — a double submission never enqueues a second
  commit.
- The commit shortcut is defined in [`keybindings.md`](keybindings.md) and
  triggers the same action as the button.
- **AI-assisted message**: a sparkles icon button ("Generate commit
  message") to the right of the **Commit message** label — has the summary +
  description drafted by the configured AI CLI ([`preferences.md`](preferences.md)
  §4: provider `claude --model haiku -p` / `codex -p` / `opencode -p` + prompt
  instructions — summarizing a staged diff is cheap, so Claude is pinned to the
  small/fast Haiku model), as a subprocess off the UI thread. Context sent: **only the
  staged** — file list + index diff; the working tree and
  untracked files never enter the prompt. The result **fills the
  inputs** — never an automatic commit. Active under the same conditions as the
  commit: ≥ 1 staged entry; spinner + click ignored during generation;
  failure (missing binary, errored process, empty response) ⇒ toast (§10).

## 6. Branch indicator

- **Location**: **chip** in the main card header (§3), next to
  the **Git** title.
- Shows the current branch name (symbolic `HEAD`) read only,
  mono typography (design-system §2).
- **Detached HEAD**: shows the short hash in place of the name.
- **Repo with no commit** (unborn `HEAD`): shows the target branch name
  (e.g. `main`); the initial commit stays possible.

## 7. Refresh

- **Poll**: we re-query the status of the **active repo** at a fixed interval
  (`GIT_POLL_INTERVAL`, **1 s**), independently of any interaction and any
  source of change (editing, `git` in the terminal…). No FS watching / pub-sub.
  A tick is **skipped** while the previous same-kind request is still in the
  worker (status, diff and graph each gated separately): a repo slower than
  the cadence never grows an unbounded queue.
- In reactive mode, the app wakes up via `request_repaint_after(GIT_POLL_INTERVAL)`
  so that the next tick fires even when idle.
- **Background fetch**: the active repo runs a silent `git fetch --all` on its own
  cadence (**10 s**) so `refs/remotes/*` stay fresh and the graph shows the **real
  remote position** (e.g. `origin/x` ahead of the local `x`) without a manual
  fetch/pull. The poll reload above then renders the moved refs. It runs lock-free
  with **auto-maintenance disabled** (`gc.auto=0`, `maintenance.auto=false`) so the
  cadence never repacks: a fetch only writes loose `refs/remotes` + objects, disjoint
  from the index/local refs the mutation lock guards. It **defers** to any in-flight
  manual network op / AI rebase; failures (offline/auth) are swallowed — invisible
  until a ref actually moves. **Local branches are never advanced** (that stays a pull/checkout); only
  the remote-tracking refs the graph draws are refreshed.
- **Immediate refresh after each action** (stage/unstage/commit/discard):
  the worker computes the new status then **wakes the UI** (callback
  `request_repaint`), without waiting for the next poll. Manual refresh via **Refresh**.
- Git operations run on a **worker thread** (blocking libgit2) so as not to
  freeze the UI; see [`architecture.md`](architecture.md) §3.

## 8. Edge cases

- **Large diff** (> ~2 MB or > ~50,000 lines) **or binary file**: the diff
  shows **Binary file** or a summary; staging stays at the **file** level
  in this case. An **image** binary instead shows a zoomable preview (§4).
- **Deletions / renames**: handled via the `*_DELETED` /
  `*_RENAMED` flags; the diff shows the deletion/rename.
- **Disk change while editing the diff**: a refresh may
  invalidate the selection; we reload the diff and report if a selection no
  longer applies.

## 9. Commit graph / history (post-MVP)

**Read first, explicit writes.** A **switch in the center zone's header**
("Terminal ⇄ Git Graph", [`design-system.md`](design-system.md) §4, shortcut
`Cmd+Shift+G` — [`keybindings.md`](keybindings.md) §1) shows the **commit
graph** in place of the terminal. Every write the graph offers belongs to the
decided list of §1; everything else stays **read only**.

- **Walk scope**: **all the repo's refs** (`refs/heads`,
  `refs/remotes`, `refs/tags`) read **locally** via `git2::Revwalk` — no network
  access (consistent with transport-less `git2`, §1; the remote-tracking refs are
  those already present in `.git`, with no fetch). **Topological + time** sort.
  Decorations shown per commit: branches / tags / `HEAD`.
- **Pagination**: we load the first `N` commits; beyond that, **Load more**
  (never silent truncation). The page **always contains the `HEAD`
  commit**: a checked-out branch beyond the first `N` (workspace on an
  old branch) ⇒ the walk extends automatically up to it, so that
  the locating auto-scroll (below) succeeds without clicking Load more in a
  loop; **Load more** then resumes from the size actually loaded.
- **Commit detail** (click): the **right sidebar** shows, **in place** of the
  status sections, the commit detail (hash, author, date, message) + the **list
  of changed files** (diff of the commit vs its **1st parent**; root commit ⇒ vs
  empty tree; merge ⇒ vs 1st parent). Entering Graph mode **reveals** the git sidebar
  if it is hidden.
- **Commit diff** (click on a file): opens the file's diff **full
  screen** in place of the graph, **read only** (no stage/unstage — this is
  history). The diff's close action returns **to the graph**; flipping the
  switch back to Terminal or changing repo also exits.
- **Double-click checkout** (the only write in the graph):
  **double-click on a local branch chip** not checked out ⇒ checkout of
  that branch. **Remote chip** `<remote>/x` ⇒ git DWIM: the same-named local
  `x` as is if it points to the remote's commit,
  **fast-forwarded** onto it if it is simply behind (we always land
  on the targeted commit), **detached** checkout on the remote commit if
  it has **diverged** (its own commits do not move); local absent ⇒
  **creation** of `x` on the remote's commit with **upstream configured**,
  then checkout. Dirty tree ⇒ the uncommitted changes (untracked included)
  go first into an **automatic stash** (`helm: auto-stash
  before checkout <branch>`) — never a destructive checkout. Double-click on
  a tag or the current branch: **ignored**. The rest of the graph stays
  read only.
- **Chips' context menu**: right
  click on a chip (local, remote **or tag**) ⇒ context
  menu **anchored under the chip** (never overlapping the label; opened
  from the expanded chips, the row keeps its labels **up to the targeted
  chip** — it does not disappear, the menu takes the place of the following ones — and
  the other rows stay collapsed while it is open): **Checkout**
  (same target and same DWIM as the double-click; absent on the current
  branch) + **Create worktree** (only when that branch can create one; see
  [`worktrees.md`](worktrees.md) §6) + **Create branch** (below; on every ref) +
  **Rebase onto `<branch>`** (below; absent
  on the current branch) + **Interactive rebase onto `<branch>`** (below; same
  eligibility) + **AI rebase onto `<branch>`** (below; same eligibility) +
  **Merge `<branch>` into `<current>`** (below; same eligibility) +
  **Rename** (below; local branches only) +
  **Copy branch name** (the ref's full name
  to the clipboard; branches only) + **Delete** entries (below). A **tag** carries
  **Checkout** (menu-only, detached), **Create branch**, **Copy tag name**,
  **Push tag** and **Delete tag** (tag actions below; never worktree, rebase,
  merge or rename). Closed on
  action, click elsewhere or `Esc`.
- **Row context menu**: right click anywhere else on a row ⇒ a menu anchored
  at the pointer, in two sections. First the **commit actions**, present on
  **every** commit row even without a ref (never on the WIP row — its content
  is not a commit — nor on a stash row, which keeps its own menu, below):
  **Copy commit SHA** (the full hash to the clipboard) + **Create tag**
  (below) + **Cherry-pick** (below) + **Revert** (below) + **Reset
  `<current>` to here** (below). Then the **ref actions** — the same menu as
  the chips' for **all** the row's refs (tags included, with the tag entries
  above). A single ref keeps the flat entries; several nest them
  into **Checkout** / **Create worktree** / **Create branch** / **Rebase onto** /
  **Interactive rebase onto** / **AI rebase onto** / **Merge** / **Rename** /
  **Copy branch
  name** / **Delete**
  submenus — one entry per ref, the deletions and the merges still explicitly
  named. A
  hovered expanded-chips
  overlay keeps priority over the right-click: it targets the overlay's chip,
  never a covered row or its inline chips.
- **Menu grouping**: both menus render their entries in **fixed buckets, a
  divider between each**, in order — refs (Checkout / Create worktree / Create
  branch), history rewrites (Cherry-pick / Revert / Reset / the three Rebase
  flavors / Merge), tags (Create tag / Push tag), Rename, copies (Copy commit
  SHA / branch name / tag name), then the destructive **Delete** entries last.
  Empty buckets collapse (no stray divider); the stash menu likewise splits
  Apply / Pop from the destructive Delete.
- **Rebase from the chips' menu**: **Rebase onto `<branch>`** rebases the
  **current branch** onto the clicked branch (local, or a remote ref as-is —
  a valid committish), `git rebase <branch>` via the `git` subprocess with the
  **same execution rules as Pull/Push** (§10: non-interactive, dedicated
  thread, a single op at a time — runner busy ⇒ explicit refusal toast, never
  a queued surprise; spinner at the end of the toolbar row and all buttons
  greyed out while it runs). Never offered on the current branch (no rebase
  onto itself); detached HEAD ⇒ clean failure before anything runs (like
  Push). On return: success / already-up-to-date toast and status + graph
  refresh; a **conflict leaves the rebase in progress** (same rule as a Pull
  conflict §10 — toast + conflict panel, resolution in the in-app editor or the
  terminal); a dirty working tree surfaces git's refusal as-is (no automatic
  stash: the rebase rewrites the checked-out branch, unlike the checkout
  exception above). **Committless branch**: when the current branch has no
  commits of its own (its tip already lives in another **local** branch) and has
  **diverged** from the target, a plain `git rebase <branch>` would replay the
  shared mainline commits onto the target — so instead the branch is **moved
  onto** it (`git rebase --onto <branch> HEAD`, no replay). Only local branches
  count as another line: a remote mirror (`origin/<self>`) holding the tip is the
  branch's own, so its already-pushed commits are never dropped.
- **Merge from the chips' menu**: **Merge `<branch>` into `<current>`** merges
  the clicked branch (local, or a remote ref as-is — a valid committish) into
  the **current branch**, `git merge <branch>` via the `git` subprocess with
  the **same execution rules as the plain rebase** (§10: non-interactive,
  dedicated thread, a single op at a time — runner busy ⇒ explicit refusal
  toast; generic spinner at the end of the toolbar row, all buttons greyed out
  while it runs). Same eligibility as Rebase onto (never the current branch —
  no merge into itself —, `origin/HEAD` and tags excluded), and the entry
  always **names both sides** (like Delete — a bare "Merge" would not say
  which way the merge goes), including under the nested **Merge** submenu of a
  multi-ref row. Detached HEAD ⇒ no entry (no current branch to name as the
  target; the domain refuses it anyway, like the rebase flavors). On return:
  success ("Merged `<branch>`") / already-up-to-date toast and status + graph
  refresh; a **conflict leaves the merge in progress** (same rule as a Pull
  conflict §10 — toast + conflict panel, resolution in the in-app editor or
  `merge --abort` in the terminal, or Abort from the panel); a working tree
  too dirty to merge surfaces git's refusal as-is.
- **Create pull request from the chips' menu**: **Create pull request into
  `<branch>`** opens the **prefilled create-PR web page** in the browser — the
  clicked ref is the **destination**, the current branch the **source**. No
  API, no auth, no network from helm (`git2` keeps no transport, §overview 4):
  the URL carries source/destination, the forge page handles the rest (title,
  reviewers) and an unpushed source is its problem to surface, not helm's. The
  forge is **autodetected per repo** from the `origin` remote URL (all URL
  forms — scp-like, ssh, https) — **cloud only**: `github.com`
  (`/compare/<dest>...<src>?expand=1`) and `bitbucket.org`
  (`/pull-requests/new?source=<src>&dest=<dest>`); a self-hosted or unknown
  host (GitLab, …) ⇒ **no entry**. Same eligibility as Merge/Rebase (never the
  current branch, `origin/HEAD` and tags excluded; detached HEAD ⇒ no entry —
  no source branch); a remote chip's `<remote>/` prefix is stripped to the
  branch name the forge expects. The entry **names the clicked ref** (like
  Merge), nested under a **Create pull request** submenu on a multi-ref row.
- **Interactive rebase from the chips' menu**: **Interactive rebase onto
  `<branch>`** (same eligibility as the plain rebase) opens a **full page** in
  place of the graph — loader while the worker lists `onto..HEAD` (merge
  commits flattened, like `git rebase -i`; more than 500 commits ⇒ explicit
  refusal, never a silent truncation), then one row per commit, **newest on
  top** like the graph: action combo (**Pick / Reword / Squash / Fixup /
  Drop**), short SHA, summary (struck through when dropped), author; Reword
  opens a multiline editor prefilled with the original message. The plan is
  validated **live**: the oldest kept commit cannot Squash/Fixup (nothing
  below to meld into) and a blank Reword is refused — inline error + Start
  disabled; dropping **everything** is allowed but states the consequence
  (branch reset onto the target). `Esc`/Cancel closes without running; the
  page also closes on repo switch (the plan targets the other repo's refs).
  **Start rebase** executes via the §10 runner (single op at a time; busy ⇒
  Start greyed out, "Operation in progress" tooltip): `git rebase -i` with the
  **todo injected** through `GIT_SEQUENCE_EDITOR` — no editor ever opens; a
  Reword runs as `pick` + `exec git commit --amend -F <file>` (the message
  never crosses a shell), `GIT_EDITOR=true` keeps git's combined message for
  squashes. The plan is **re-derived and compared** before running: the
  branch moved since the page opened ⇒ clean refusal ("reopen Interactive
  rebase") — a stale todo would silently drop the new commits. An operation
  already in progress at the menu click ⇒ refusal toast before the page
  opens (same with a detached HEAD); a conflict mid-run leaves the rebase
  **in progress** (same rule as the plain rebase: toast + banner §10,
  resolution in the terminal or **Abort** from the banner).
- **AI rebase from the chips' menu**: **AI rebase onto `<branch>`** (same
  eligibility as the other rebase flavors) opens a **recap modal** — loader
  while the worker lists `onto..HEAD` (same plan source as the interactive
  page, same 500-commit cap), then the rebase **not yet started**: current
  branch → target, the commits to replay (newest on top) and an **AI
  instructions** box handed verbatim to the provider (e.g. "Squash everything
  into a single commit"). Nothing runs before **Start AI rebase**;
  Cancel/`Esc` closes. An op already in progress or a detached HEAD ⇒ refusal
  toast before the modal opens (like the interactive flavor). **Start** hands
  the request to the **AI rebase runner**: the configured **agentic** provider
  ([`preferences.md`](preferences.md) §4 — `claude -p` with Bash and the file
  tools pre-approved and `git push` **denied**, `codex exec --full-auto`,
  `opencode run`) runs **in the repo** and performs the rebase itself —
  replays, resolves conflicts, honors the instructions; the prompt contract
  forbids pushing or touching any remote and asks to `git rebase --abort` if
  unsafe. Guards re-checked at execution: clean repo state, HEAD still on the
  recap's branch, **clean working tree** (stricter than the plain rebase on
  purpose — the provider must never be tempted to stash or commit the user's
  WIP; untracked files don't block), plan re-derived and compared (stale
  recap ⇒ clean refusal "reopen AI rebase"). An accepted Start ⇒ auto-expiring
  toast ("AI rebase started — …") and the modal **closes during the run** —
  the terminal stays usable. The run holds the repo's **mutation lock** for
  its whole duration (minutes are normal — 30 min timeout): staging, commits
  and sync ops are refused meanwhile, and the toolbar shows a named chip
  — spinner + **AI rebase · m:ss** (elapsed time) + **Cancel** — instead of
  the anonymous loader, all buttons greyed out. **Cancel** kills the provider,
  **aborts** any rebase it left in progress (branch restored) and reports the
  verified result ("Cancelled — …"); the button turns inert ("Cancelling…")
  until the reply lands. Switching repos (or quitting) mid-run cancels the
  same way — a provider never outlives its session, it would keep rewriting
  history with no lock. On return: status + graph refresh, then a **report
  modal**: the provider's account (what it did, each conflict and how it was
  resolved — for codex, read from `--output-last-message` rather than its
  chatty stdout) under an outcome headline **verified on the repo**, never
  believed from the report — **Completed** (state clean, HEAD moved) /
  **Branch unchanged** / **Rebase left in progress** (banner + terminal or
  Abort take over, like any conflict §10); **Copy report** puts the account on
  the clipboard. The 30-min **timeout** kills the provider like a cancel and
  restores the same way (abort + verified state in the failure toast); a
  provider that stops **on its own** (missing binary, quota, crash, deliberate
  abort) leaves the repo as it left it — failure toast, the lasting state
  still told by the banner. helm never pushes on the provider's behalf.
- **Create branch from the chips' menu**: **Create branch** opens the same
  **inline field** as the toolbar Branch button (§10), but anchored on the
  **clicked ref's row** in place of its chips. The field opens **empty** (a
  pre-filled ref name would read like a rename). It creates a **local branch at
  the ref's commit without checking it out** — HEAD and the working tree are
  untouched,
  no upstream is configured (the toolbar flow, by contrast, creates on HEAD and
  checks out). Offered on **every** ref (local, remote, tag) except `origin/HEAD`
  (remote symref); the source ref is fully qualified on the app side so a branch
  and a tag sharing a name never collide. `Enter` with a valid name confirms;
  `Esc` or a click elsewhere cancels. Creation runs on the worker (graph
  reloaded behind it, FIFO, so the new chip appears at once) while the field
  stays open (`pending`); a **duplicate/invalid** name keeps it open with an
  **inline error**, and the field closes on success. The targeted row beyond the
  loaded page ⇒ no anchor, the editor closes (same assumed limit as §10).
- **Rename branch from the chips' menu**: **Rename** opens the same inline
  field, anchored on the branch's row, but **pre-filled** with the current
  name (here the pre-fill *is* the point — the Create flow stays empty
  precisely so it never reads like this one). **Local** branches only (a
  remote branch is renamed by push + delete — not offered), the **current
  branch included**: `git branch -m` semantics, the symbolic `HEAD` follows
  the new name and the upstream configuration moves with the branch. `Enter`
  renames on the worker (graph reloaded behind it, FIFO); a
  **duplicate/invalid** name keeps the field open with an **inline error**
  (never a forced overwrite); `Esc` or a click elsewhere cancels. A refusal
  (e.g. a branch checked out in another worktree) surfaces as a toast.
- **Branch deletion from the chips' menu**: **explicitly named**
  entries
  — `Delete <local>` and `Delete <full remote>` (e.g. `Delete hotfix/9.6.6`,
  `Delete origin/hotfix/9.6.6`). As soon as a branch exists **on both sides**
  (merged on the same commit or diverged into two chips), the menu offers
  both **plus the combined entry** `Delete <local> and <remote>`, whatever
  chip is clicked (`counterpart`, annotated on the domain side: each ref
  carries the **name** of its counterpart on the other side). The local deletion is
  never offered on the current branch (git refuses), even via its remote
  chip; `origin/HEAD` offers nothing (remote symref). Every entry goes
  through a **confirmation modal** naming the branch(es) (Cancel /
  red Delete) — nothing goes out before it. Local: `git2` via the worker (graph
  reloaded afterwards; branch checked out in a worktree ⇒ clean failure as a
  toast). Remote: `git push <remote> --delete <branch>` — a **network** op with the
  same rules as Pull/Push (§10: non-interactive subprocess, a single op at
  a time, success/failure as a toast); the local remote-tracking ref disappears
  with it, the graph follows on refresh. Combined: the network runner is requested
  **first** — busy ⇒ nothing goes out (toast), never a silent half.
- **Create tag from the row menu**: **Create tag** opens the same inline field
  as Create branch (empty), anchored on the commit's row, and creates a
  **lightweight** tag on that commit via the worker (annotated tags — a
  message, a signature — stay out of scope, §1). Nothing is checked out and
  nothing is pushed (publication is the explicit **Push tag**, below). `Enter`
  with a valid name confirms; a **duplicate tag** keeps the field open with an
  **inline error** (a branch sharing the name does not collide — refs are
  fully qualified, like Create branch); `Esc` or a click elsewhere cancels.
  The graph reloads behind the creation (FIFO) so the new chip appears at
  once.
- **Tag actions from the chips' menu**: a tag chip (or the row submenus)
  offers **Checkout** — **detached** checkout on the tag's commit, with the
  same safety **automatic stash** as the branch checkout; **menu only**, the
  double-click on a tag stays ignored (a detached HEAD must never be one
  accidental double-click away) —, **Copy tag name** (the tag's name to the
  clipboard), **Push tag** — `git push origin <tag>` on the sync runner
  (network rules §10; multi-remote selection stays out of scope: `origin` or
  nothing) — and **Delete tag** — a **confirmation modal** naming the tag
  (Cancel / red Delete) with an **"Also delete on origin"** option, offered
  whenever `origin` exists (the graph cannot know whether the tag lives
  remotely — `refs/tags` is a local namespace — so a remote-side miss simply
  surfaces git's error as a toast). Checked, the network runner is requested
  **first** (`git push origin --delete refs/tags/<tag>`, fully qualified so a
  same-named branch is never touched) — busy ⇒ nothing happens (toast), never
  a silent half — then the local deletion on the worker (graph reloaded);
  unchecked, local deletion only.
- **Cherry-pick / Revert from the row menu**: **Cherry-pick** replays the
  row's commit on the current branch (`git cherry-pick <sha>`); **Revert**
  commits its inverse (`git revert --no-edit <sha>` — no editor ever opens).
  Both run on the sync runner with the **same execution rules as the plain
  rebase** (§10: non-interactive, one op at a time — busy ⇒ refusal toast —,
  generic spinner, all buttons greyed). **Merge commits are refused cleanly**
  before anything runs (both would need a mainline choice there — out of scope
  until a decision); detached HEAD ⇒ entries absent (like merge: these write
  the checked-out branch). On return: success toast ("Cherry-picked
  `<short sha>`" / "Reverted `<short sha>`") and status + graph refresh; a
  **conflict leaves the operation in progress** — same rule as a Pull conflict
  (§10): toast + banner, resolution in the terminal or **Abort** from the
  banner (whose flavor already follows `Repository::state()`, cherry-pick and
  revert included). A dirty working tree or an empty result (the change
  already on the branch) surfaces git's refusal as-is — no automatic stash,
  like the rebase flavors.
- **Reset from the row menu**: **Reset `<current>` to here** nests the three
  git flavors — **Soft** (the branch moves; index and working tree untouched,
  the difference shows up staged), **Mixed** (the branch and the index move;
  working tree untouched, the difference shows up unstaged) and **Hard**
  (everything moves — **destructive**, so it sits behind a **confirmation
  modal** naming the branch and the target commit, Cancel / red Reset;
  untracked files survive, git semantics). Soft and Mixed run directly, they
  lose nothing. Local operation on the worker (`git2` reset, no network),
  status + graph refreshed behind it; detached HEAD ⇒ entry absent (no branch
  to move); an operation in progress ⇒ refused like the other mutations.
- **Stashes in the graph**: each stash
  (reflog `refs/stash`) is a row inserted **just above its base
  commit** (1st parent): a node with a **dotted border** with
  an archive icon (same visual language as the WIP row — off-branch content),
  a **dotted link** to the base, the stash message in the message column, **no
  chip**. Only the 1st parent is represented (the stash's index/untracked commits are
  not rows). A base beyond the loaded page ⇒ stash visible
  after **Load more**. Click ⇒ stash commit detail like any commit
  (diff vs base = stashed changes, **untracked included**: those files live
  in the stash's 3rd parent commit, absent from the stash tree — the detail
  and the file diff read them from there, like `git stash show -u`; without
  this a stash holding only untracked files shows as empty). **Stash/Pop**
  (§10) refresh the
  graph immediately. **Right-click** on a stash row ⇒ context menu **Apply
  stash** / **Pop
  stash** / **Delete stash**, targeting **that** stash (identified by its
  stash commit — indices shift, the worker re-resolves at execution; gone in
  the meantime ⇒ clean failure as a toast). Apply restores the changes and
  **keeps** the stash (the no-drop twin of Pop — same subprocess; on conflict
  nothing changes for the stash, it stays either way). Pop applies then drops,
  same
  conflict rule as the toolbar Pop (§10: conflict ⇒ stash kept + error);
  Delete goes through a **confirmation modal** naming the stash (Cancel / red
  Delete — a dropped stash is unrecoverable), nothing is sent before it. All
  three refresh the status immediately; Pop and Delete also
  reload the graph immediately behind the mutating command.
- **Locating the current branch**: on each **entry** into Graph mode (and on the
  repo switch in Graph mode), the view **scrolls automatically** to the
  `HEAD` commit's row (centered). The checked-out branch's chip (✓) keeps
  its text and glyphs in **crisp white**, with a **medium-weight** name and a
  `text.primary` **ring** (visible on the fill and the canvas in both modes) so
  it is spotted at a glance; the other chips are slightly dimmed.
- **Search** (`Cmd+F` — [`keybindings.md`](keybindings.md) §3): a **floating box**
  at the **top-right** of the graph filters the **loaded** commits (summary, short
  hash, author, message body, ref names — case-insensitive) and **cycles** the
  matches: `Enter` / the chevrons move the cursor (`Shift+Enter` backward),
  wrapping, with a `current/total` counter; each match is **scrolled into view**
  (centered) and **highlighted** in amber (distinct from the blue selection). Only
  the loaded page is searched (pagination); `Esc` / ✕ closes, and a center-zone
  mode switch resets it.
- **Bounded and resizable graph column**: the lanes area follows the
  history's width but is **bounded by default** (~6 lanes) so that the
  message column stays readable on wide histories — the surplus
  lanes are **clipped**, never painted under the text. The
  graph ⇄ message boundary resizes on **drag** (↔ cursor), between a minimum and
  the lanes' natural width; the setting is kept for the session.
- **Edge cases**: repo with no commit (unborn `HEAD`) ⇒ **No commits**; large
  diff / binary ⇒ same handling as in §8 (**Binary file** / summary).

## 10. Graph action toolbar (post-MVP)

An **action bar** tops the Graph view (above the column
headers row, Graph mode only — component:
[`design-system.md`](design-system.md) §4): **Pull** (split-button) · **Push**
(split-button) ·
**Branch** · **Stash** · **Pop**. Together with **Delete on remote**, **Push
tag** and the remote half of **Delete tag** from the chips' menus
(§9), it is the app's only network-write surface; the rest of the graph
stays as defined in §9. **Mouse only in
v1** (no new shortcut). **Undo / Redo** are
**deferred** to a later milestone — the toolbar is designed to host them on the
left.

- **Network execution via the `git` binary**: fetch / pull / push run as a
  **subprocess** of the system `git` in the repo's workdir — auth (ssh-agent,
  osxkeychain, credential helpers), proxies and the user config are
  inherited. `git2` stays **without network transport** (overview §4) and covers the
  local operations (branch, repo state); stash save/pop also run via the `git`
  subprocess — local, no network — because libgit2's stash re-hashes the whole
  worktree single-threaded (~13 s vs ~0.6 s on a 20k-file repo). **Non-interactive** subprocess
  (`GIT_TERMINAL_PROMPT=0`, stdin closed): a missing auth
  fails cleanly, never a hung prompt.
- **Asynchronous**: a network operation runs on a **dedicated thread** — the
  sequential git worker and the status poll (§7) continue. **A single network
  op at a time** per repo. While a **git command** is executing (network op
  or mutating worker command — stash, pop, branch, checkout,
  commit, staging), the toolbar shows a loader — spinner on the button at
  the origin of the action, at the end of the row for a mutation outside the toolbar — and
  **all** the buttons are grayed out (tooltip "Operation in progress"; the
  poll's reads do not count). On return: refresh status + graph (same
  rules as in §7).
- **Pull (split-button)**: the main area runs the **default
  operation**; the chevron opens a radio menu "Select a default pull/fetch
  operation to execute when clicking this button": **Fetch All**
  (`git fetch --all`) · **Pull (fast-forward if possible)** (`git pull --ff`) ·
  **Pull (fast-forward only)** (`git pull --ff-only`) · **Pull (rebase)**
  (`git pull --rebase`). Choosing an option **sets the default without
  running it**; the button label reflects the default ("Pull" / "Fetch").
  Pull is **limited to the current branch**: `git pull <flag> <remote>
  <branch>`, remote/branch derived from the upstream (`branch.<name>.remote` /
  `branch.<name>.merge`) — no fetch of the whole remote along the way;
  **Fetch All** stays the way to
  refresh all the remote refs. Without an upstream (or detached HEAD) ⇒
  fallback to a bare `git pull <flag>`, whose standard git error surfaces as a toast.
  When the upstream branch no longer exists on the remote (merged & deleted
  elsewhere, e.g. the Bitbucket UI), git's `couldn't find remote ref` is treated
  as a **no-op**: helm prunes the stale local remote-tracking ref (the branch
  stops showing as on the remote in the graph) and stays **silent** — no toast.
  Default **persisted** in `prefs.toml` (global, not per repo; initial default
  fast-forward if possible). The same setting is editable from the **Preferences
  page** (Git section, [`preferences.md`](preferences.md) §4) — the
  two surfaces read/write `pull_default`; a change on one side is
  reflected on the other.
- **Pull conflicts**: a merge / rebase that stops in conflict **stays in
  that state** (no automatic abort) — the **conflict panel** takes over the
  status sidebar (`Repository::state()` detection, including for an operation
  initiated in the terminal): a `⚠ <Op> conflicts detected` header, the
  **Conflicted Files** / **Resolved Files** groups and a **Mark All Resolved**
  master action ([`conflicts.md`](conflicts.md) §2). Resolution happens in the
  in-app **conflict editor** (a conflicted file row opens it) or the terminal,
  finalised by **Continue `<Op>`** once no conflict stage remains; or the
  footer's **Abort `<Op>`** button: confirmation modal (Cancel / red Abort —
  resolutions in progress are discarded), then the abort flavor follows
  `Repository::state()` (`rebase --abort` / `merge --abort` / cherry-pick,
  revert, am `--abort` / `bisect reset`) via the same runner as Pull/Push — it
  also covers an operation started in the terminal; resolved in the meantime ⇒
  clean failure toast, nothing runs. `--ff-only` not fast-forwardable ⇒
  clean failure, tree intact.
- **Push (split-button)**: the main area pushes the **current branch** to its
  upstream; without an upstream ⇒
  `git push -u origin <branch>`. The plain push **never forces**: a non
  fast-forward rejection is shown as is. The **chevron** opens a menu with a
  single **one-shot** entry, **Push (force with lease)** — unlike Pull's
  chevron it **executes**, it never sets a default: forcing must stay a
  deliberate act each time. It is the outlet for a rebased branch (§9) whose
  plain push is rejected. Greyed without an upstream (nothing to overwrite —
  the plain `-u` push covers the first publication). A **confirmation modal**
  (Cancel / red Force push) names the branch and the remote, then `git push
  --force-with-lease <remote> <branch>` runs on the runner — the lease makes
  git refuse if the remote moved past the last fetch (refusal as a
  toast suggesting a fetch first). Bare `--force` is never used. Detached
  HEAD ⇒ Pull and Push disabled.
- **Branch**: the button opens an **inline field in the graph**, BRANCH / TAG
  column, placed on the **HEAD row** in place of its chips —
  exactly where the new branch's chip will appear (the view scrolls
  to the row on opening, same mechanism as the auto-scroll §9). Name
  field (`git2` validation of the ref name, duplicate ⇒ inline error under the
  field), `Enter` creates the branch **on HEAD** and **checks it out**; `Esc`,
  click elsewhere or a re-click on the button cancels. HEAD beyond the loaded
  page ⇒ no anchor, the editor closes (same assumed limit as
  the auto-scroll §9).
- **Stash / Pop**: **Stash** stashes the whole working tree (**untracked
  included**, like the checkout auto-stash §9) under a default message
  (`helm: stash`); disabled if the tree is clean. **Pop** applies
  then drops `stash@{0}`; disabled with no stash; a conflict on pop
  **keeps** the stash and the error is shown. Via the `git` subprocess (local,
  no network — see "Network execution" above for the why).
  A dedicated stash list stays out of scope (§1); a specific stash's
  Apply / Pop / Delete live in its graph row's context menu (§9).
- **Operation feedback**: **toasts** stacked at the bottom-left (`ui::toast`),
  on top of everything and in **all modes** — an error that occurred in Terminal
  mode stays visible (revision of the earlier Graph-only banner).
  A failure (auth, network, ff-only refused, push
  rejected, conflict on pop, checkout/branch/stash…) ⇒ an **auto-expiring error** toast
  (~4 s, cross to close early) with a contextual message: the action in plain language then the cause
  ("Stash pop failed — conflicts while applying — the stash was kept").
  Success of a **network** op ⇒ an **auto-expiring** success toast (~4 s): "Pulled —
  already up to date", "Fetched — remote refs updated", "Pushed" (the
  local actions stay silent, the refreshed graph is authoritative). A
  message already displayed identically is **refreshed**, never stacked; the
  **polled** failures (status, graph) toast only **once per episode** (rearmed
  on the first success) — no spam at the poll cadence. The failed
  **reads** (graph, commit detail, file diff) also toast instead of
  failing silently; a pull conflict toasts **and** leaves the Merge/Rebase in
  progress state banner (above) to tell the story of the lasting state.
- **Disabled states** (explanatory tooltip): no remote ⇒ Pull / Push
  grayed out; detached HEAD ⇒ Pull / Push grayed out; clean tree ⇒ Stash grayed out;
  no stash ⇒ Pop grayed out; repo with no commit (unborn `HEAD`) ⇒ Branch / Stash /
  Pop grayed out (only **Fetch All** stays runnable if a remote exists);
  `git` binary not found ⇒ network actions grayed out.
