# helm — Conflict resolution

Spec for the **in-app merge/rebase conflict editor**. Backend: **`git2`** to read
the three merge stages and to write a resolution into the index; the **`git`
subprocess** runner for the sequencer finalisation (`--continue`). Visual tokens:
[`design-system.md`](design-system.md). Related: [`git.md`](git.md) §2 (conflict
status), §9–§10 (graph operations + conflict panel).

## 1. Scope & decision

This **reverses the earlier locked decision** ([`git.md`](git.md) §1/§2/§10:
"conflict resolution always in the terminal"). Conflicts left by any operation —
a Pull, a graph **Merge / Rebase / Cherry-pick / Revert** (git.md §9–§10), or an
operation started **in the terminal** — are now resolvable **inside helm**,
through a dedicated **3-zone conflict editor**; the terminal stays a **fallback**
for the kinds the editor does not cover (§7). This spec **supersedes** the
"resolution in the terminal" wording wherever git.md §9 repeats it per operation.

**In scope (v1):**
- Read the conflict state of the active repo from the **index merge stages**
  (`index.conflicts()`), **not** from the on-disk conflict markers.
- A **3-zone editor**: the two sides as **checkbox panes** on top (**A · ours** |
  **B · theirs**), the **always-editable Output** at the bottom, composed per
  conflict region by **ticking A and/or B** and free to hand-edit at any time.
- Resolve **per file**, **Save** the result into the index (clears the conflict),
  then **finalise** the in-progress operation from the status banner (**Continue**).
- Conflict kinds covered by the editor: **both-modified**, **added-by-both**,
  **deleted-by-us**, **deleted-by-them** (§6).

**Out of v1** (terminal fallback, no regression): submodule and rename/rename
conflicts, binary and oversize files (file-level choice only, §7), **keyboard
navigation** between conflicts, **"Save & next"** auto-advance, and editing the
**base** (the ancestor is **display only**, §4). Deferred, not refused.

**Source of truth.** helm keeps **no conflict state of its own**: the index merge
stages are authoritative. Quitting, switching repo or a crash mid-resolution loses
nothing **saved** — on return the editor re-reads `index.conflicts()`. The only
ephemeral state is the **unsaved composition** of the file currently open (§5).

## 2. The conflict panel (entry & finalisation)

While an operation is **in progress** (`Repository::state() != Clean`), the
**conflict panel takes over the right sidebar** in place of the normal status +
commit layout (it returns once the op ends). Layout:

```
┌─────────────────────────────────────────────┐
│ ⚠  Merge conflicts detected                  │  header (verb noun + alert)
│ Merging [theirs] into [main]                 │  source/target chips (when known)
├─────────────────────────────────────────────┤
│ Conflicted Files (2)        [Mark All Resolved]│
│   ⚠ src/app/render.rs                         │  rows open the editor
│   ⚠ src/ui/git_panel.rs                       │
│ Resolved Files (1)                            │
│   ● src/git/status.rs              +12  −3     │  read-only (the staged set)
├─────────────────────────────────────────────┤
│            [ Continue Merge ]                 │  enabled only when 0 conflicts
│            [ Abort Merge ]                    │  danger
└─────────────────────────────────────────────┘
```

- **Header** — `⚠ <Op> conflicts detected` (the op noun: Merge / Rebase /
  Cherry-pick / Revert) while any conflict remains, else `Ready to continue`.
  A sub-line `<verb> <source> into <target>` shows the branch names as chips
  ([`OpSummary`](../src/git/status.rs)); it is omitted when either name is
  unknown (the bare header stands alone).
- **Conflicted Files (N)** — the unstaged entries with `kind == Conflicted`.
  **Clicking a row** opens the editor on that file — the rows are the entry point.
  The editor is a **full-page center-zone view** (like the interactive-rebase
  page, git.md §9), not the diff overlay — it is richer than a diff. Closing
  returns to the terminal / graph; a repo switch also closes it.
- **Mark All Resolved** — stages every conflicted file **as-is** (one
  `index.add_path` per file clears its three stages, §5), moving the whole group
  into Resolved without opening the editor. Disabled when the group is empty.
- **Resolved Files (M)** — the **staged** entries, read-only: a resolved conflict
  becomes a normal staged file (§5), listed here for the duration of the op.
- **Finalisation stays in the footer.** When **no conflict stage remains** (every
  file resolved and saved), the footer's **Continue `<Op>`** button (above Abort,
  enabled only then) runs the right sequencer continuation —
  `merge --continue --no-edit` / `rebase --continue` / `cherry-pick --continue` /
  `revert --continue`, flavor from `Repository::state()` (the same selection used
  by Abort, git.md §10) — on the **`git` subprocess runner**, `GIT_EDITOR=true` so
  no editor ever opens.
  - **Merge / cherry-pick / revert**: the operation completes, the panel clears.
  - **Rebase**: `--continue` may immediately surface the **next** commit's
    conflicts; the panel re-populates and the new conflicted rows reopen the
    editor on the new set — the loop continues until the rebase finishes.
- **Abort `<Op>`** (danger) opens the confirmation modal (Cancel / red Abort —
  resolutions in progress are discarded), then the abort flavor follows
  `Repository::state()` (git.md §10).
- The two runners (git2 worker for the resolution write, subprocess for
  Continue/Abort) share the existing **mutation lock** (architecture.md §3), so
  the writes and the finalisation never race.

## 3. The 3-zone editor

```
┌─ ⚠ app.rs · 2 conflicts ─────────────────────────────────────── [ ✕ Close ]─┐
├──────────────────────────────────────┬────────────────────────────────────────┤
│ [A] CURRENT · ours              [all] │  [B] INCOMING · theirs           [all] │  checkbox panes,
│   1  fn run(cfg: &Cfg) {               │    1  fn run(cfg: Cfg) {                │  line-number gutters,
│ ▌☑  2     let t = cfg.timeout;         │  ▌☐  2     let t = cfg.t;              │  scroll-synced
│   3  }                                 │    3  }                                 │
├──────────────────────────────────────┴────────────────────────────────────────┤
│ OUTPUT          1/2 resolved          [ Save ]   ▲ 1/2 ▼  ⇅                    │  nav + swap-both
│   1  fn run(cfg: &Cfg) {                                                        │  always editable,
│ ▌ 2     let t = cfg.timeout;                                                  ▮ │  gutter + band +
│   3  }                                                                          │  scrollbar mark
└───────────────────────────────────────────────────────────────────────────────┘
```

- **Toolbar** (top): `⚠ <path> · <N> conflict(s)` on the left, **Close** on the
  right (an **unsaved** composition prompts a discard confirmation, §5). The set of
  conflicted files lives in the **sidebar conflict panel** (§8), not a second in-editor
  rail; clicking a file there opens it here.
- **A | B panes** (`CURRENT · ours` | `INCOMING · theirs`): the two sides side by
  side in **one scroll area** (scroll-synced), each with a **line-number gutter**.
  A conflict region is **highlighted** (a soft band + a left accent bar — A teal,
  B gold) and the shorter side is padded with blank rows so A and B stay aligned.
  The region's first line carries a **take checkbox** (§4); a per-pane **all**
  ticks every region of that side. Labels are **semantic**, from
  `Repository::state()`: **rebase** inverts the stages (stage 2 = rebase target,
  stage 3 = replayed commit), **merge** → *Current · ours* / *Incoming · theirs*;
  likewise for cherry-pick / revert.
- **Output** (bottom): the composed file in an **always-live editor** (§5) — a real
  `TextEdit` with its own scroll and a **line-number gutter**, never a painted/edit
  toggle. Each conflict region gets a soft band (**purple** resolved / **orange**
  unresolved) and a **scrollbar tick**; an unresolved region is one **orange
  placeholder** line and counts in "N resolved". The header carries **Save**, a
  **prev/next** nav (`▲ i/N ▼`) that scrolls the Output to the conflict, and the
  **⇅** both-order swap (the v1 keyboard-free navigation).

## 4. Conflict regions & the take checkboxes

Each conflict region is resolved by **ticking A (ours) and/or B (theirs)** on the
top panes; the Output region follows:

| A | B | Output region |
|---|---|---------------|
| ☐ | ☐ | **Unresolved** — orange placeholder; counts in "conflicts left" |
| ☑ | ☐ | the **ours** (A) lines |
| ☐ | ☑ | the **theirs** (B) lines |
| ☑ | ☑ | **both**, concatenated **in the order the boxes were ticked** (first ticked leads) |

- Ticking both sides concatenates them; the order follows the **tick sequence**
  (the side already taken stays first), so no separate swap control is needed.
- **Base** (the common ancestor, stage 1) is **display only**: the Output's
  **base** toggle reveals the ancestor under each region — never a third permanent
  pane, never editable. Absent for added-by-both (no base).

## 5. Output — composed and always editable

- The Output is **always a live `egui::TextEdit::multiline`** (no painted/edit
  toggle): seeded from the picks' composition and **recomposed** whenever an A/B box
  changes; a **hand edit** keeps the typed text verbatim. Save writes the buffer
  verbatim, with the file's own **line terminator** (detected from the ours/theirs
  blob) re-applied and no trailing newline added — a CRLF file stays CRLF.
- **Like a real editor**: a **line-number gutter** (numbered once per logical line,
  derived from the laid-out galley), a **soft band** behind each conflict region
  (purple resolved / orange unresolved) and a **scrollbar tick** per region. The
  bands + ticks track the buffer while it still mirrors the picks; a free hand edit
  keeps the gutter only. syntect is **not** incremental, so re-highlighting the whole
  buffer per keystroke froze big files; instead an **incremental highlighter** re-parses
  only the lines an edit touched (first changed line → parse-state reconvergence) and
  reuses cached spans for the rest, so a keystroke costs ~one line (~0.2 ms) and runs
  every frame with **no flicker**. The laid-out galley is memoised too (idle frames skip
  relayout), and above a **size cap** (64 KB) live highlighting is dropped entirely (the
  A|B panes keep their colours, highlighted once at load).
- **Unresolved region**: a single **orange placeholder** line (`‹ unresolved — pick
  A or B above ›`) with an orange band — not the conflict body; Save stays disabled
  until every region has a side (or a whole-file override).
- **Save** (explicit) writes the **current file**: the composed buffer goes to the
  working tree, then `index.add_path` **clears the three stages** (atomic — git
  has no "resolved but unstaged" state; a resolved file is a normal staged file).
  The file leaves the conflict set; its sidebar entry moves to **Resolved** (§8). The
  composed result is reviewable afterwards via the staged diff (git.md §4).
- **Close** leaves the editor (back to terminal / graph). Saved files persist; an
  **unsaved** composition prompts a discard confirmation.
- **Already edited on disk**: on open, the disk file is compared to a clean
  reconstruction from the stages. If it **diverges** (hand-edited before opening),
  an in-editor notice offers *Load my version* (the disk content fills the
  Output buffer) or *Start from the merge* (the default reconstruction). The
  comparison is **normalised** on both sides: git writes the working tree in
  `merge` style with branch labels, the reconstruction is diff3 with fixed ones,
  and diff3 keeps inside the hunk the lines the two sides share at its edges. So
  neither the line terminator, the marker labels, nor the width of a conflict
  hunk counts as a divergence — only the sides' content does. Otherwise every
  untouched file would open on the notice.
- **Modes and symlinks**: Save writes the resolution **through** the entry's
  mode. The composed buffer replaces the file after an **unlink** (never a write
  in place — the path may still be a symlink, whose target would be followed and
  the wrong file overwritten); when the conflicted entry is a **symlink**, the
  buffer becomes the link's new target rather than a regular file's content.
  Taking a side on an **executable** conflict re-applies *that side's* exec bit
  before staging, so the merge's working-tree mode never leaks into the result.

## 6. Conflict kinds (read from the index stages)

The present stages classify the conflict and drive the available resolutions:

| Stages present | Kind | Editor |
|----------------|------|--------|
| 1 + 2 + 3 | both-modified | full 3-zone editor |
| 2 + 3, no 1 | added-by-both | full editor, no base |
| 1 + 2, no 3 | deleted-by-them | focused card *Keep the modified version / Delete the file* |
| 1 + 3, no 2 | deleted-by-us | focused card *Keep the incoming version / Delete the file* |
| a binary side | binary | file-level *Use incoming / Use current* (§7) |

Region content is reconstructed deterministically with
`git2::merge_file(ancestor, ours, theirs)` (a marked buffer helm **generates**,
hence safe to parse), split into `Stable` (auto-merged / context) and `Conflict`
(ours / theirs / base) regions.

## 7. Fallbacks (the terminal stays available)

| Case | Behaviour |
|------|-----------|
| **Binary** conflict | no 3-zone editor → file-level *Use incoming / Use current* card |
| **delete/modify** | focused *Keep / Delete* card (§6) |
| **oversize** (> 2 MB / > 50k lines, git.md §8) | file-level only + "resolve in the terminal" note |
| **submodule / rename-rename** | delegated to the terminal (the banner does not pretend otherwise); a conflicted **gitlink** is left out of the editor's file rail — its stages are commits of the submodule's own ODB, unreadable as blobs, and one of them must not take the whole rail down with it |
| **disk reload mid-edit** (1 s poll, git.md §7/§8) | flagged like the diff; the composition is not clobbered |

## 8. Architecture

- **Domain** (`git/conflict.rs`, new, `pub` from the lib): `ConflictFile { path,
  kind, ours_label, theirs_label, regions, has_base, eol, disk_divergence }`
  (`eol` the detected line terminator, `disk_divergence` the working-tree content
  when it no longer matches the reconstruction — both §5), `ConflictKind`,
  `Region::{Stable, Conflict { ours, theirs, base }}`. A new
  `GitCommand::ReadConflict` returns it over the worker channel (like `FileDiff`).
- **Resolution write** (git2 worker): `GitCommand::ResolveFile { path, content }`
  writes the working tree + `index.add_path` (or removes the path for a delete
  resolution), mirroring `stage.rs`' filtered-apply / index-write off the UI
  thread. The **merge stages are the write's precondition**, re-read from disk:
  a path resolved meanwhile (another pane, the terminal) or an operation aborted
  under the editor is **refused**, never overwritten from a buffer composed
  against the old state.
- **Finalisation** (subprocess runner): `SyncCommand::ContinueOp` generalises
  `AbortOp` — same flavor selection from `Repository::state()`.
- **Rendering** (`ui/conflict_view.rs`, new): `fn(&mut egui::Ui, …)` driven by
  kittest; resolution choices live in a `ConflictEditorState` (session) keyed by
  region — `RegionChoice::{Unresolved, Ours, Theirs, Both { ours_first }, Manual}`
  — the same separation as `DiffViewState` (rendering state, never domain).

## 9. Testing ([`testing.md`](testing.md))

- **Unit**: kind classification from the present stages; semantic label derivation
  per `Repository::state()`; region parsing of a `merge_file` buffer; Result
  composition per `RegionChoice`.
- **Business e2e** (real repo): create both-modified / added-by-both /
  delete-modify conflicts under **merge** and **rebase**; assert kinds, regions,
  inverted labels; resolve ours/theirs/both → index clean → **Continue** finalises
  (merge ⇒ commit; rebase ⇒ advance or next-conflict loop); delete/modify keep +
  delete.
- **UI e2e** (kittest, `ui/conflict_view`): the A | B panes and the Output render
  with gutters; ticking **A** / **B** composes that side; ticking **both** →
  concatenation in tick order; the always-editable Output saves **hand edits**
  verbatim; long context shows **in full** (no fold); the `i/n` nav jumps between
  conflicts; Save emits `ResolveFile`; Close warns on an unsaved composition.
