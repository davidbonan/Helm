# Progress state — helm

> **Source of truth for active progress.**
> Conventions and *Definition of Done*: [`README.md`](README.md). Statuses:
> `☐` to do · `◐` in progress · `☑` done+verified · `⊘` blocked · `⏭` deferred.

---

## ◐ Milestone — M-Edit · Edit the file from the diff

Spec: [`specs/git.md`](../git.md) §4 (+ [`keybindings.md`](../keybindings.md) §3,
[`design-system.md`](../design-system.md) §4). A click in the diff content puts a
caret on the line; the buffer reaches the working tree on exit and on idle typing,
with no save control. Counter: **4/6**.

- ☑ **T1 — Spec.** `git.md` §4 (inline editing, section-of-origin staging rule,
  non-editable list, edit mechanism), §7 (diff poll suspended while editing), §8
  (buffer never clobbered by a reload); `keybindings.md` §3/§4/§6;
  `design-system.md` §4 (*Inline code editor*) + cursor rule.
  *Files*: `specs/{git,keybindings,design-system}.md`.
- ☑ **T2 — Domain `src/git/edit.rs`.** `EditError`, `editable`, `write_range`:
  workdir containment, symlink / non-regular / NUL / non-UTF-8 / oversize refusal,
  byte-exact `file[range] == original` precondition, line terminator + final
  newline preserved (`conflict::LineEnding`), atomic temp + `rename` keeping the
  original permissions.
  *Files*: `src/git/edit.rs`, `src/git/mod.rs`. *Tests*: unit (splice, EOL, final
  newline, divergence) + `tests/it/git_edit_e2e.rs` (real repo: CRLF, no final
  newline, divergence refused, symlink, binary, perms kept, oversize).
- ☑ **T3 — Worker command.** `GitCommand::EditFile { path, range, original,
  replacement, stage_after }` (mutation answered by its own `GitResult::Edit`,
  `ResultKind::Edit`, never gated by staleness); `edit::flush` = precondition read
  **before** the write, then `write_range`, then the file-level stage;
  `Landing::{Unstaged, Staged, NotStaged(reason)}`; `on_edit` toasts the landing /
  the failure and re-requests status + open diff. `FileDiff::editable` surfaced by
  `diff::file_diff` so the click needs no disk access.
  *Files*: `src/git/{edit,worker,diff}.rs`, `src/app/{mod,git_session}.rs`. *Tests*:
  `tests/it/git_edit_e2e.rs` (unstaged / staged+stage / precondition lost ⇒ written
  unstaged / anchor lost stages nothing / 2 worker round-trips),
  `tests/it/git_diff_e2e.rs` (`editable` carried), `src/app/tests.rs` (toast +
  refresh).
- ☑ **T4 — Inline editor rendering.** `DiffViewState::inline_edit` +
  `InlineEdit { hunk, range, original, buffer }`; the hunk's rows swapped for a
  `TextEdit` (`Frame::NONE`, `line_height` + `valign: Center` ⇒ same 17 px band,
  `IncrementalHighlighter` for the buffer's colours), accent bar, dimmed `~` sign
  column, gutter renumbered off the laid-out galley. Entry: `row_zone` splits each
  row into **number strip** (the line pick, moved there) and **content** (a plain
  click ⇒ caret at that column, `Cmd+E` on the hovered row); `edit_anchor` widens
  the window with the displayed extended context; `Esc` leaves the editor first.
  *Files*: `src/ui/diff_view.rs`, `tests/shots_gen.rs`. *Tests*:
  `tests/it/ui_diff_view.rs` (caret from a click proven by the typed character,
  non-editable no-op, content x + row band unchanged, `Cmd+E`, `Esc`, number-strip
  pick) + the 4 row-click tests retargeted to the strip (`numbers_x_offset`).
- ☐ **T5 — Autosave + guards.** Flush on exit / blur / file nav / close and after
  800 ms idle; diff frozen while open, diff poll suspended, `↑`/`↓` and
  `Cmd+Enter` disarmed, `Esc` cascade, divergence notice (*Reload* /
  *Overwrite*), non-editable toast carrying **Open in editor**.
  *Files*: `src/app/{render,git_session,keys}.rs`, `src/ui/diff_view.rs`.
  *Tests*: `tests/it/ui_app_keys.rs`, `tests/it/ui_diff_view.rs`.
- ☐ **T6 — Verification.** `headless-verify`: click → type → `Esc` → recomposed
  diff, plus a before/after capture proving no metric shift.
- ⏭ **T7 — Deferred.** Whole-file editing (same `write_range`, full range),
  auto-indent on `Enter`, several editors at once, *Save & next hunk*.

### Next actions (M-Edit)
- **T5** — autosave + guards: flush the open buffer on exit / blur / file nav /
  close and after 800 ms idle, freeze the diff and suspend its poll while the
  editor is open, disarm `↑`/`↓` and `Cmd+Enter`, surface the divergence notice.
- Noted while verifying T4: in the **headless app** harness
  (`Harness::new_eframe` + `HelmApp::with_workspace`), no click inside the git
  panel registers (file row, *Tree view*) although the right-rail toggles do — so
  T4 was verified on `diff_view` driven with a real `file_diff` instead. The same
  clicks pass in the isolated `git_panel` harness (`tests/it/ui_git_panel.rs`);
  cause unexplained, to look at when T6 needs the end-to-end path.

---

## ☑ Milestone — M-CLI · `helm <path>` from a terminal or another app

Spec: [`specs/cli.md`](../cli.md). One binary, argv-dispatched; the CLI resolves the
path and hands it to the running instance through the `helm://` scheme. Counter: **4/4**.

- ☑ **T1 — CLI mode.** `src/cli.rs`: `parse` (argv ⇒ `Args`), `execute`,
  `resolve_target` (canonicalize → `discover` → refuse bare/non-UTF-8),
  `open_url`/`target_from_url` (+ percent codec), `main.rs` dispatch.
  *Files*: `src/cli.rs`, `src/main.rs`, `src/lib.rs`. *Tests*: 12 unit (parse,
  URL round-trip), 5 business e2e (root / subdir / worktree / bare / non-git).
- ☑ **T2 — URL delivery.** `CFBundleURLTypes` in `scripts/bundle.sh`;
  `app::url_scheme` (kAEGetURL handler on `NSAppleEventManager`, pending buffer +
  repaint); `--open-url` GUI flag; drained at the top of `ui()`.
  *Files*: `scripts/bundle.sh`, `src/app/url_scheme.rs`, `src/app/mod.rs`, `Cargo.toml`.
- ☑ **T3 — Target application.** `app::activate_target`: group import when unknown,
  `reveal_row` (unhide + unfold), `set_active`; app side sets `Page::Main` +
  `CentralMode::Terminal`, persists, toasts on refusal.
  *Files*: `src/app/mod.rs`. *Tests*: 4 business e2e (import / reveal / late worktree /
  refusal leaves the workspace untouched) + 2 in-crate (leaves Preferences for the
  terminal; a refusal moves nothing).
- ☑ **T4 — Single instance + install.** `flock` on `instance.lock` in `app::run`
  (busy ⇒ raise + exit 0); `shell_command_state`/`install_shell_command` and the
  *Preferences › Terminal › Shell command* row.
  *Files*: `src/cli.rs`, `src/app/{mod,render}.rs`, `src/ui/preferences.rs`.

### Next actions (M-CLI)
- **M-CLI complete** (4/4). The `kAEGetURL` delivery path is verified on a
  bundled winit app (throwaway probe, own scheme): the handler installed before
  the event loop survives AppKit's launch and fires cold *and* warm. Remaining
  manual check: the same on **helm's own** bundle (`scripts/bundle.sh`, then
  `helm .` with the app running and stopped) — end to end, toast included.

---

## ☑ Milestone — M-GitHard · Git actions hardening

Spec: [`specs/git.md`](../git.md) + [`specs/conflicts.md`](../conflicts.md) +
[`specs/worktrees.md`](../worktrees.md). Audit of every Git write path (staging,
discard, commit, sync, branch/tag/stash, rebase/conflicts, graph, worktrees, worker
threading, panel/diff UI) followed by an adversarial **review pass** (T0) that
confirmed each finding against the specs, `git log` and the test suite. Goal: **no Git
action acts on a target the user did not point at, and none silently corrupts or drops
work**. Counter: **38/38**.

- ☑ **T0 — Review pass.** 35 findings triaged against specs + history + tests:
  **28 to fix** (8 re-scoped by the review), **4 closed** (T8, T10, T20, T23) plus
  3 nits closed inside T35. Verdicts inlined per task.

### Lot A — cross-repo safety & destructive gating

- ☑ **T1 — Git session keyed by repo identity.** `sync_git_session` gates on the
  workspace **index** (`app/mod.rs:795`) while `Workspace::remove` reassigns `active`
  to the same index now holding another repo (`workspace.rs:645-656`) ⇒ the panel
  reads/writes the removed repo. The reorder path already carries a manual
  `git.index = active` fixup (`render.rs:2456`); `remove` and the 5 s
  `sync_workspace_groups` got none. Keying on `RepoKey` removes the fixup and the
  spurious respawn that drops diff/branch-editor/rebase/conflict state.
  *Files*: `src/app/mod.rs`, `src/app/render.rs`.
- ☑ **T2 — Discard/stash arming cleared on repo switch.** `park_active_session`
  (`mod.rs:766-783`) parks only the commit draft; `Ctrl+Tab` is routed with no modal
  gate (`keys.rs:307`) so an armed confirm re-renders and fires into the **new**
  session (`git_panel.rs:1524`). Worst case `DiscardTarget::All` ⇒ unconditional
  `DiscardAll` on the wrong repo. Scope: clear `pending_discard`, `pending_stash`,
  `marked_files`, `selection_anchor`, `selected_file`; **collapsed-dirs sets are not
  worth parking** (review). *Files*: `src/app/mod.rs`, `src/ui/git_panel.rs`.
- ☑ **T3 — Destructive modals + graph menu dropped on repo switch.** Modals carry a
  name/oid and resolve `self.git` at confirm time (`render.rs:2674-2760`);
  `close_ai_rebase_modal` deliberately clears only the AI variants. `ForcePush` carries
  **no branch at all** ⇒ force-pushes the new repo's current branch; `DeleteBranch`/
  `DeleteTag`/`AbortOp` act by name. Oid-addressed `DropStash`/`ResetHard` degrade to a
  clean "not found" — not part of the fix. Chip menu is global egui state
  (`graph_view.rs:746`, `:931`). Cheapest: stamp the modal with its `RepoKey`.
  *Files*: `src/app/mod.rs`, `src/app/render.rs`, `src/ui/graph_view.rs`.
- ☑ **T4 — Inherited diff carries no staging affordance.** The render path destructures
  `DiffState` without `inherited` (`render.rs:1206`), which is read only in the two
  error handlers (`git_session.rs:575`, `:752`); the header shows file A while
  `overlay_or_command` joins the intent to `path` = B (`keys.rs:52-70`). The window is a
  full sequential-worker round-trip, so it is clickable. Keep rendering the frozen
  content, suppress the granular intents. *Files*: `src/app/render.rs`,
  `src/app/keys.rs`, `src/app/git_session.rs`.
- ☑ **T5 — Discard-hunk confirmation invalidated by a diff reload.** The modal stores
  `{path, hunk}` (`render.rs:2152`) while the 1 s poll swaps `loaded`
  (`git_session.rs:227`). `DiffViewState::reconcile` already has the equivalent guard
  one level down (`diff_view.rs:184`, banner `:797`). Minimal fix: drop a pending
  `Modal::DiscardHunk` when `adopt` replaces `loaded` for that path — no content
  re-matching. *Files*: `src/app/git_session.rs`, `src/app/render.rs`.
- ☑ **T6 — Reset refused mid-operation + `ORIG_HEAD`.** ⚠ **re-scoped**: the rebase
  scenario is unreachable — a conflicted rebase detaches HEAD and the "Reset `<branch>`
  to here" section is only built for a named HEAD (`graph_view.rs:686`, `:1352`). What
  is reachable: **merge / cherry-pick / revert** conflicts keep HEAD on a branch, and a
  libgit2 Mixed/Hard reset then wipes `MERGE_HEAD`+`MERGE_MSG` (measured; Soft is
  refused by libgit2 itself) ⇒ `git merge --abort` dead. Plus libgit2 never writes
  `ORIG_HEAD` (recovery still possible via the HEAD reflog). Gate on `repo.state()` in
  `branch::reset`, matching `commit.rs:2` / `sync.rs:154`.
  *Files*: `src/git/branch.rs`, `src/ui/graph_view.rs`, `src/app/render.rs`.
- ☑ **T7 — Worktree delete warns about ignored files.** ⚠ **re-scoped**: the current
  behaviour matches two locked decisions — `worktrees.md:207` ("clean ⇒ immediate
  deletion, no confirmation") and `git.md:64` ("ignored files: not listed") — and
  mirrors `git worktree remove`. But `prune(working_tree(true))` → `rmdir_r` takes the
  `.env` with it, and the post-create-script flow (`worktrees.md:195`) makes that the
  normal case. Fix = surface it (count ignored, warn/confirm), **not** treat clean as
  dirty. *Files*: `src/git/worktree.rs`, `src/ui/repo_sidebar.rs`.
- ⏭ **T8 — AI rebase backup ref + outcome verification.** Closed: `classify()`
  (`ai_rebase.rs:405`) implements `git.md:513-515` **verbatim** ("Completed (state
  clean, HEAD moved) / Branch unchanged / Rebase left in progress"), and the spec's
  designed restore is `--abort` only (`git.md:505`), with the branch reflog covering a
  provider `reset --hard`. Stronger verification (checking the replayed commits are
  reachable) would be a **spec change**, not a bug fix.
- ☑ **T9 — Bulk ops never abort half-way, and a failed mutation never leaks into the
  next commit.** 🔴 most severe of the lot. A plain nested clone is reported as one
  untracked entry `vendor/` — `nested_in_workdir` only collects `repo.worktrees()`
  (`worktree.rs:140`) so neither filter catches it. Measured: `index.add_path("vendor/")`
  ⇒ `stage_all` `?`-exits (`stage.rs:91`) before `index.write()` (`:109`);
  `remove_file("vendor/")` ⇒ `discard_all` `?`-exits (`discard.rs:43`) before the
  restore checkout (`:60`), after having already deleted the untracked files it walked.
  Then the corruption vector: the worker holds one long-lived `Repository`
  (`worker.rs:513`) and `fresh_index`'s `index.read(false)` is a no-op because the
  on-disk index never changed (`stage.rs:11`) ⇒ `commit`'s `write_tree` ships the
  in-memory phantom entries (measured). Fix = collect errors instead of `?` in both
  loops **and** make `fresh_index` re-read unconditionally.
  *Files*: `src/git/stage.rs`, `src/git/discard.rs`.
- ⏭ **T10 — Conflicted rows read-only.** Closed as invalid: `git_panel.rs:2020` returns
  **before** the Stage/Discard pill block on `entry.kind == Conflicted` (not on
  `repo.state()`), conflicted entries are excluded from `openable_files` (`:1834`), and
  `stage_all`/`discard_all` skip `CONFLICTED`. The genuine residue is tracked as T36.

### Lot B — Git correctness

- ☑ **T11 — Hunk/line staging keeps raw bytes.** Reproduced: latin-1 line → index blob
  `… 63 EF BF BD 64 …`, `apply` returns `Ok`, silent corruption; libgit2 does not flag
  the delta binary so the Stage-hunk/Stage-lines pills reach it.
  `diff.rs:432` `from_utf8_lossy` → `stage.rs:410` `body.push_str`. ⚠ **fix re-shaped**:
  `content` is display-only elsewhere (`diff_view.rs`, `syntax_highlight.rs`,
  `pull_requests_view.rs`), and adding `raw: Vec<u8>` to `DiffLine` would double a cache
  holding up to `MAX_DIFF_BYTES` per entry — instead re-derive the raw line bytes from
  `git2::Patch` inside the staging path and return `Vec<u8>` from `render_hunk_patch`
  (`from_buffer` already takes `&[u8]`). Must land with T12 so line indices stay
  aligned. *Files*: `src/git/diff.rs`, `src/git/stage.rs`.
- ☑ **T12 — `*_EOFNL` are markers, not lines.** Worse than filed: 2 of the 3 shapes make
  granular staging **fail outright** (`invalid patch hunk at line 9`), so every hunk
  touching the tail of a no-final-newline file is unstageable. `line_origin`
  (`diff.rs:469`) types the marker as real content, `push_line` then emits it twice, and
  `+N −M` is off by one (`diff_view.rs:548`). The test
  `line_origin_maps_eofnl_variants_to_base_kinds` (`diff.rs:480`) **asserts the bug** and
  must flip. One-line fix verified: `None` for the three variants ⇒ all 3 shapes apply
  with byte-identical blobs. *Files*: `src/git/diff.rs`, `src/git/stage.rs`.
- ⏭ **T13 — Commit/amend honour signing (+ hooks).** Deferred (D-2026-07-21): the fix
  hinges on routing the commit write through the `git` CLI, which blocks the sequential
  worker on slow hooks and collides with the 120 s SIGKILL (T32); revisit as its own
  decision. ⚠ **re-scoped**: the "rebase and
  cherry-pick already run hooks" premise is **false** — measured with `core.hooksPath`,
  only `git commit` runs `pre-commit`/`commit-msg`; the comment at `sync.rs:219`
  claiming otherwise is wrong and should be fixed with this task. **Signing is a real
  inconsistency**: cherry-pick/rebase do invoke gpg under `commit.gpgsign=true`, while
  `commit.rs:31` never does (= `proposals.md` P20, itself flagged "correctness bug, not
  a feature" — this task *is* its promotion). Risks to weigh before routing the write
  through the CLI: a slow hook blocks the sequential worker (`worker.rs:505`) and gets
  SIGKILLed at 120 s (`cli.rs:9`, see T32); `Stdio::null()` + `GIT_TERMINAL_PROMPT=0`
  block TTY pinentry; `architecture.md:53,152` list commit under `git2`. Verified safe:
  `amend_message` maps cleanly to `git commit --amend --only -m` (tree/author preserved,
  committer refreshed, unrelated staged files untouched). Keep the libgit2 pre-flight
  guards so `git_commit_e2e` stays green; `message_prettify` is cosmetic (only the
  missing trailing `\n`). *Files*: `src/git/commit.rs`, `src/git/cli.rs`, `src/git/sync.rs`.
- ☑ **T14 — Pull (ff) does not rebase.** Verified on git 2.55: with `pull.rebase=true`,
  `git pull --ff` rebases and rewrites the local oid. `sync.rs:621` emits `--ff` for the
  entry `git.md:707` names "Pull (fast-forward if possible)" and makes the default
  (`:723`) ⇒ the default button silently rewrites history. `--ff-only` is unaffected.
  One line: `--no-rebase --ff`. *Files*: `src/git/sync.rs`.
- ☑ **T15 — `--force-with-lease` pinned to the displayed oid.** The bare lease
  (`sync.rs:531`) compares against a remote-tracking ref that helm itself refreshes
  every 10 s (`worker.rs:886`) ⇒ it can never refuse; `SyncError::StaleInfo` and its
  toast (`graph_toolbar.rs:930`) were designed for a refusal that never fires, and
  `git.md:748` states the contract it breaks. The snapshot carries `upstream_oid`,
  the modal is armed with it (`Modal::ForcePush { branch, remote, lease }`) and
  `force_push` emits `--force-with-lease=refs/heads/<branch>:<lease>`; it also refuses
  when HEAD left the armed branch. `git.md` §10 rewritten to state the pinned contract.
  *Files*: `src/git/sync.rs`, `src/git/worker.rs`, `src/app/git_session.rs`,
  `src/app/mod.rs`, `src/app/render.rs`, `src/ui/graph_toolbar.rs`, `specs/git.md`.
- ☑ **T16 — Interactive reword targets its own commit.** Reproduced: `pick` + independent
  `exec git commit --amend -F` (`sync.rs:414`); the pick conflicts, the user follows
  git's own `--skip` hint (and `sync.rs:391` explicitly designs for continuing from the
  terminal) ⇒ the message lands on the replayed onto-branch commit, "Successfully
  rebased". Fixed: the `exec` is guarded on the original message
  (`test "$(git log -1 --format=%B)" = "$(git log -1 --format=%B <oid>)"`) — mismatch ⇒
  stderr note + non-zero exit, the rebase stops instead of rewording the commit below.
  `git.md` §9 states the guard. *Files*: `src/git/sync.rs`, `specs/git.md`.
- ☑ **T17 — Push tag fully qualified.** ⚠ **downgraded to consistency**: git *refuses*
  the ambiguous refspec (`src refspec v9 matches more than one`), so the remote is never
  wrongly written — only a confusing toast, plus a very narrow race if the tag is
  deleted between click and execution. `push_tag` now emits `refs/tags/{name}`, symmetric
  with `delete_remote_tag`; `git.md` §9 states the qualified form. *Files*:
  `src/git/sync.rs`, `specs/git.md`.

### Lot C — fidelity

- ☑ **T18 — Renames diff and count as renames.** `source_diff` keeps its single-path
  pathspec on the fast path and, only when the delta comes back unpaired
  (`Untracked`/`Added`), re-diffs with `status::rename_old_path`'s old path in the
  pathspec then `find_similar` — the old side has to be *in* the diff before it can be
  paired. `hunk_line_bytes` shares `source_diff`, so both stay on the same hunk
  sequence. `status::find_renames` gains `for_untracked` (what libgit2's own status pass
  sets), fixing `+<whole file> −0` on an unstaged rename. Fallout fixed in the same
  breath: the filtered patch of a rename target absent from the index now carries
  `similarity index` + `rename from`/`rename to` instead of `new file mode`, which
  libgit2 applies as a `RENAMED` delta (a `/dev/null` header rejected the hunk's context
  lines). Tests: 2 diff e2e + 2 status e2e + 2 stage e2e. A **pure** rename now shows
  *No changes* (0 hunks, 0/0) like `git diff` does. *Files*: `src/git/diff.rs`,
  `src/git/status.rs`, `src/git/stage.rs`.
- ☑ **T19 — Symlinks staged by `symlink_metadata`.** `.exists()` follows the link
  (`stage.rs:20`) ⇒ repointing a symlink at a not-yet-created target stages its
  **deletion**; `stage_all` is immune (it switches on the delta status), so per-file
  Stage and Stage All disagree on the same row. Nothing in specs/tests covers symlinks.
  *Files*: `src/git/stage.rs`.
- ⏭ **T20 — Submodules.** Closed: acknowledged backlog item `proposals.md:203` ("**P29 —
  Submodule status** — niche; only if a target repo needs it"). `exclude_submodules(true)`
  is consistent across `is_dirty` and `work_statuses` (`status.rs:209`, `:222`) so no
  write path can act on a submodule — invisibility only, no corruption. Action: one line
  in `git.md` §2 next to "Ignored files: not listed", pointing at P29.
- ☑ **T21 — Conflict resolve preserves line endings.** `parse_regions` iterates
  `text.lines()` (`conflict.rs:304`, drops `\r`), `compose_string` rejoins on `"\n"` and
  force-appends `'\n'` (`conflict_view.rs:354`) while `conflicts.md:156` says "Save
  writes the buffer verbatim". Compounded: `resolve_file` uses `std::fs::write`
  (`conflict.rs:119`), bypassing git's smudge filters, so an `eol=crlf` repo is
  normalized too. No test locks LF normalization. Detect the terminator from the
  ours/theirs blob at reconstruction and re-apply at compose time.
  *Files*: `src/git/conflict.rs`, `src/ui/conflict_view.rs`.
- ☑ **T22 — `resolve_file_side` reads a fresh index.** Only index access in the module
  that skips the refresh (`conflict.rs:144` vs `:58`, `:87`, `:116`, `:152`), violating
  the invariant documented at `stage.rs:6-10`. Window bounded by the 1 s poll; can write
  a stale side's blob over a file already resolved in a terminal pane. Take
  `stage::fresh_index` once at `:144` and drop the second open at `:152`.
  *Files*: `src/git/conflict.rs`.
- ⏭ **T23 — Conflict markers block Continue.** Closed: `conflicts.md:72-74` locks the
  behaviour verbatim ("**Mark All Resolved** — stages every conflicted file **as-is**
  … without opening the editor"), the code matches exactly (`git_panel.rs:445`), the
  *editor* path is guarded as specified (`conflict_view.rs:349`), and core git behaves
  the same. If anything changes it is the spec (a marker warning in §2), not the code.
- ☑ **T24 — Disk-divergence notice wired.** `flag_disk_divergence`
  (`conflict_view.rs:255`) has **zero callers** ⇒ `whole_override` is dead code
  (`:1449`, `:1469`) and `conflicts.md:178-181` (present tense, reinforced by the §7
  fallback row `:208`) is unimplemented; `on_conflicts` never reads the working-tree
  file (`git_session.rs:685`) and `resolve_file` overwrites it unconditionally ⇒ edits
  made in a terminal pane are lost on Save. Compare disk vs reconstruction where the
  editor adopts. *Files*: `src/ui/conflict_view.rs`, `src/app/git_session.rs`.
- ☑ **T25 — Line selection anchored on content.** `reconcile` (`diff_view.rs:184-205`)
  validates only bounds + origin, so a reload that keeps the hunk shape but changes the
  content silently retargets the selection **and leaves `stale` false** — against
  `git.md:306-308` ("report if a selection no longer applies"). Reached from the 1 s
  poll; `stage_lines` then re-derives the diff from disk (`stage.rs:251`). The four
  `reconcile_*` tests (`diff_view.rs:2942`) only cover out-of-range/became-context.
  Storing the lines' content hash is enough — no selection-model change.
  *Files*: `src/ui/diff_view.rs`.
- ☑ **T26 — Scoped stash of a rename passes both sides.** The row menu sends the single
  displayed path (`git_panel.rs:1670`) and `entry_path` reports a rename's **new** path
  only ⇒ `stash_paths` never sees the old side (`stash.rs:100-133`). Unstaged rename is
  the worse case: after Stash the old file is missing from disk with nothing recording
  it; staged rename leaves a staged `D <old>`. `git.md:60-62` requires renames to act on
  **both paths** (it enumerates Stage/Unstage/Discard, not Stash). Expand inside
  `stash_paths` via `status::rename_old_path` so every caller is covered.
  *Files*: `src/git/stash.rs`.
- ☑ **T27 — Partial staging keeps the exec bit.** `new file mode 100644` hardcoded
  (`stage.rs:349`); needs a *partial* selection on an untracked file (`apply_filtered`
  short-circuits to `stage()` when the selection covers the whole add, `:256`). Stat the
  worktree file and pass the mode in. *Files*: `src/git/stage.rs`.

### Lot D — flow & robustness

- ☑ **T28 — Sidebar pills greyed during a sync/AI op.** ⚠ **re-scoped**: refusing (not
  queueing) is spec-locked — `git.md:502-504` ("staging, commits and sync ops are
  **refused** meanwhile") — and the refusal *is* surfaced as a toast
  (`git_session.rs:525`). The defect is only that the sidebar keeps offering pills that
  are guaranteed to fail during a 30-min AI rebase. Project `sync.busy()` /
  `ai_rebase.busy()` into `GitPanelState` like `mutation_busy`; **do not queue**.
  *Files*: `src/ui/git_panel.rs`, `src/app/render.rs`, `src/app/git_session.rs`.
- ☑ **T29 — `MutationLock` per repo, not per session.** `git_session.rs:328` mints a new
  lock per spawn and `SyncRunner` has no `Drop` — its thread is deliberately left
  running (`worker.rs:797`). The author already documented this exact race for the AI
  case: `ai_rebase.rs:552` ("an unjoined run would race the fresh `MutationLock` of the
  session reopened on the same repo") — the join in `Drop` is a workaround for the hole,
  which sync ops still have. Keying the lock by `RepoKey` also lets that join go.
  *Files*: `src/app/mod.rs`, `src/app/git_session.rs`.
- ☑ **T30 — Checkout auto-stash.** ⚠ **re-scoped, 4 sub-claims:** (a) the *remote chip*
  no-op is **invalid** — `merge_local_remote` (`graph/mod.rs:217`) drops a remote entry
  sitting on the same commit, so a surviving chip is always a real move; the reachable
  no-op is the **tag menu** ("a tag is always checkout-eligible", `graph_view.rs:518`)
  with HEAD already detached on that tag ⇒ stash taken, never popped. (b) no success
  toast (`git_session.rs:492`) — minor, mitigated by the toolbar Pop button and the
  graph stash rows. (c) `is_dirty` lacks the `nested_in_workdir` filter
  (`status.rs:207` vs `:257`) — only reachable with a *relative* worktree base
  (`worktree.rs:181`, `worktrees.md:190`), then every checkout stashes a tree the panel
  calls clean. (d) `set_target` moves the local ref **before** `checkout_reference` can
  fail (`branch.rs:76-81`), and also moves a branch checked out in another worktree
  (git2 does not enforce the CLI guard). *Files*: `src/git/branch.rs`, `src/git/tag.rs`,
  `src/git/status.rs`, `src/app/git_session.rs`.
- ☑ **T31 — Superseded command side effects.** ⚠ **re-scoped**: the panic watchdog is
  dropped — 0 `unwrap`/`expect`/raw indexing outside `#[cfg(test)]` across `src/git/*.rs`,
  so it would be defensive code for a case with no trigger. Kept: a `Commit` reply
  superseded by a later mutation skips `panel.subject.clear()` (`git_session.rs:503`) ⇒
  the committed message stays in the composer (same shape for the branch/tag editor,
  `:515`); run command-scoped side effects **before** the `carries_state` gate. Minor
  companion: an abandoned session's mutation refused by the lock is dropped with no
  possible toast (`worker.rs:528`). *Files*: `src/app/git_session.rs`, `src/git/worker.rs`.
- ☑ **T32 — Network ops are not SIGKILLed at 120 s.** `DEFAULT_TIMEOUT` (`cli.rs:9`)
  reaches every sync op (`sync.rs:658` → `run_with_env` → `run_program_with_timeout`);
  the cancellable path has exactly one caller (`ai_rebase.rs:294`). No spec or commit
  documents the value (`git log -S DEFAULT_TIMEOUT` → only the initial squash) — the
  only specified timeout is the AI rebase's 30 min. Also `git.md:686` states "never a
  hung prompt" but only `GIT_TERMINAL_PROMPT=0` + null stdin are set (`cli.rs:101`): a
  configured `GIT_ASKPASS`/`SSH_ASKPASS`/`core.askPass` still spawns a GUI helper and
  burns the full timeout. (The `GIT_DIR` half is dropped — no path launches helm from a
  git subprocess.) *Files*: `src/git/cli.rs`, `src/git/sync.rs`.
- ☑ **T33 — Diff view render cost.** ⚠ **re-scoped by measurement**: `can_extend` is
  quadratic but **not** the binding constraint — 50 hunks (the busiest single-file diff
  in the last 50 commits) costs 62 µs/frame, 200 hunks 0.77 ms; only generated files
  near `MAX_DIFF_LINES` reach the ms range, where the non-virtualised row loop already
  co-dominates. Keep the trivial hoist, drop the urgency. Real: `display_rows` is
  rebuilt every frame purely for a `max_chars` width measure (`diff_view.rs:844`, O(all
  lines) + `chars().count()`), and `ScrollArea::both()` has no `id_salt` (`:855`) so the
  offset is reused when switching files. *Files*: `src/ui/diff_view.rs`.
- ☑ **T34 — `workspace_dirty_stats` off the UI thread.** ⚠ **re-framed**: not the render
  path — the three sites are discrete events (sidebar Remove `render.rs:2420`, ⌘O
  `mod.rs:3606`, Finder drop `:3630`); the recurring path was already moved to
  `GroupRefreshRunner` ("the full diff per dirty repo froze the frame here",
  `mod.rs:1157`). Still a full `is_dirty` + `load_repo` walk of **every** workspace repo
  on the UI thread per event. Reuse `workspace_probes` + `GroupRefreshRunner::request`
  and delete `workspace_dirty_stats`/`workspace_branches` (the only other caller is the
  headless test seam). *Files*: `src/app/mod.rs`, `src/app/render.rs`.
- ☑ **T35 — Nits batch (6 kept, 3 closed).** Kept: `forge.rs:47` host matched
  case-sensitively ⇒ `git@GitHub.com:o/r` silently loses Create-PR **and** the PR
  cockpit; `graph_view.rs:1061` selects the row under an expanded chip overlay
  (`response.clicked()` without the `hover_lock` guard its right-click twin has at
  `:1145`); `render.rs:2216` nulls `conflict_editor` even when `request_sync` refused,
  **and** bypasses the editor's unsaved-work confirmation (`conflict_view.rs:174`);
  stale "resolve from the terminal" copy contradicting `git.md:726` and `conflicts.md`
  (`graph_toolbar.rs:289`, `commit.rs:4`, `:48` — 3 assertions at
  `graph_toolbar.rs:859` move with it); tag editor validates with `valid_branch_name`
  instead of `Tag::name_is_valid` (`graph_view.rs:1861`, cosmetic); shift-click anchor
  left stale by a right-click (`git_panel.rs:1988` vs `range_select` `:1785`).
  **Closed**: `fetch --all` prune (⏭ `git.md:701` names the command verbatim; the only
  decided prune is pull's `RemoteBranchGone`, D-2026-06-16) · branch delete `-D` wording
  (⏭ the modal already says "its commits stay in the reflog", `graph_view.rs:1671`, and
  `git.md:548-562` never specifies a merged check) · `.git/helm-rebase` litter (⏭ the
  lifecycle is stated in code, `sync.rs:366`, and every run wipes it first).
- ⏭ **T36 — Conflicted entry outside an operation is a dead end.** Deferred
  (D-2026-07-21): the answer is a spec change, to be taken with the other spec edits
  (T20, T23). Surfaced by T0 while
  closing T10: after a conflicting `stash pop` (or a merge run from a terminal pane)
  `repo.state()` is `Clean` while the file is `CONFLICTED`, so `op_in_progress` is false
  and `render.rs:714` nulls `self.conflict_editor` every frame ⇒ clicking the row does
  nothing and no banner explains why. `conflicts.md:42` scopes the editor to
  `state() != Clean`, so this is a **spec gap**, not a regression: decide between opening
  the editor for a stateless conflict or showing an explicit banner, then fold the answer
  back into `conflicts.md`. *Files*: `specs/conflicts.md`, `src/app/render.rs`,
  `src/ui/git_panel.rs`.

- ☑ **T37 — Branch review, 13 fixes.** Full read of the milestone's diff (adversarially
  verified before applying; each fix carries a test proven to fail without it).
  *Conflicts* (`git/conflict.rs`): the merge stages now gate `resolve_file` (a path
  resolved elsewhere is refused, not overwritten) · the resolution is written after an
  **unlink** and a `120000` entry comes back as a **symlink** (a write in place followed
  the old link out of the repo) · deletion via `symlink_metadata`, `Path::exists` left a
  dangling link behind · taking a side re-applies **that side's** exec bit · a conflicted
  **gitlink** leaves the rail instead of failing every other file · the divergence notice
  compares **normalised** regions (git's merge-style worktree vs the diff3
  reconstruction, whose hunks keep the sides' common edges) — it used to fire on every
  untouched file. *Staging* (`git/stage.rs`, `git/diff.rs`): symlinks fall back to
  whole-file stage/unstage · the hunk patch header carries the **index entry's mode**
  (staging a hunk dropped the exec bit) · a reverse patch emits the old side first
  (`apply` rejected mixed runs) · untracked hunk bytes go through the **ODB** so
  `text=auto` / clean filters apply. *Sync* (`git/sync.rs`): the network budget is read
  past the `-c` pairs (the background fetch was on the 120 s budget) · branch pushes and
  the remote delete use `refs/heads/<branch>` (a same-named tag made the refspec
  ambiguous) · the reword guard reads both messages with `--no-show-signature`
  (`log.showSignature=true` refused every reword). *UI*: the frozen working-tree surface
  keeps its scroll salt (`ui/diff_view.rs`) · a line pick is fingerprinted on its
  **origin** too (same text flipped side staged the opposite) · the file row's context
  menu greys its mutating entries while a command runs (`ui/git_panel.rs`) · the
  force-push lease is armed through `armed_force_push` (`app/render.rs`, testable).
  Spec edits folded in: `git.md` §2/§4/§8/§9/§10, `conflicts.md` §5/§7/§8.

### Next actions (M-GitHard)
- **Milestone complete**: Lot A (T1–T10), Lot B (T11–T17), Lot C (T18–T27), Lot D
  (T28–T35), T37 (branch review), all ☑/⏭.
- T13 ⏭ (decision pending: route the commit write through the `git` CLI, with the
  worker-blocking and timeout risks above) — it promotes `proposals.md` P20.
- T20 / T23 / T36 are spec edits, not code: fold them in when touching their spec.

---

## ☑ Milestone — M-Amend · Amend HEAD's commit message

Spec: [`specs/git.md`](../git.md) §9 (commit detail) + §1 (decided write). In-place
**reword of `HEAD`** from the commit detail panel: double-click the message block ⇒
inline editor (subject + description, prefilled) with **Amend** / **Cancel**.
Message-only (tree + author preserved, committer refreshed); HEAD-only; blocked mid
merge/rebase. Counter: **1/1**.

- ☑ **T1 — Amend.** Domain `commit::amend_message` (`Commit::amend`; guards clean
  state + non-empty message + existing `HEAD`); `GitCommand::AmendMessage(String)`
  routed through `send_then_reload_graph`; `select_head_after_amend` one-shot
  re-selects the new `HEAD` in `on_graph` (oid changes on reword). UI:
  `commit_detail::message_block` double-click → editor (egui temp keyed by oid) →
  `GitIntent::AmendMessage`. Tests: 3 domain (reword / preserve / blank) + 4 UI e2e
  (open-prefilled / emit / cancel / read-only-off-HEAD).
  *Files*: `src/git/commit.rs`, `src/git/worker.rs`, `src/app/git_session.rs`,
  `src/app/render.rs`, `src/ui/commit_detail.rs`, `src/ui/git_panel.rs`,
  `src/app/keys.rs`, `src/ui/mod.rs`.

---

## ☑ Milestone — M-PR5 · PR detail → mockup parity

Spec: [`specs/pull-requests.md`](../pull-requests.md) §4/§11 +
[`design-system.md`](../design-system.md) §4. Brought the **PR detail**
(center, no file open) up to the sent mockup: a **carded** layout (author block /
meta-row / description card / conversation cards), **reviewers + labels + role
pills**, a **Created … ago** age and an **Oldest/Newest** conversation toggle.
Locked decisions: **labels = GitHub-only** (Bitbucket Cloud has no PR labels →
empty); **linked issue / conversation filter / reactions = cut** (no backing — the
Jira key already leads the title); reviewers wired from the existing
`PullRequest.reviewers` (GitHub-populated, **BB empty in v1**). Counter: **7/7**.

- ☑ **T1 — Reviewers cluster + role pills.** Detail meta-row reuses
  `reviewer_stack` (allocated rect → painter); `comment_role` derives **Author** /
  **Reviewer** neutral tags on conversation cards (never `accent.ai`).
  *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **T2 — Labels (GitHub fetch).** `PullRequest.labels: Vec<String>`; GitHub
  `LIST_FIELDS += labels`, `parse_pr` maps `o["labels"][].name`; Bitbucket empty.
  Rendered as neutral pills in the meta-row (`neutral_pill`, §4 pill grammar), row
  hidden when reviewers + labels both empty.
- ☑ **T3 — Created date.** `PrDetail.created_at`; GitHub `DETAIL_FIELDS +=
  createdAt`, Bitbucket `created_on`. `model::relative_age` (pure, `now` injected)
  renders "Created … ago", right-aligned in the author row. (The list *Updated*
  column was later widened + humanized the same way — M-PR4 T2.)
- ☑ **T4 — Carded detail layout.** Author block + right-aligned Created age, a
  reviewers + labels meta-row, the **body in a `bg.surface` card**, conversation
  **comment cards** (1px `border.subtle` + radius `CARD_RADIUS`). **Token nitpicks
  needed no code change** — heading casing (band titles are 13pt strong primary, not
  the sidebar uppercase-muted header), body contrast, `#number` chip cursor and the
  verdict segmented fill already match §4/the mockup; documented the detail card in
  `design-system.md` §4.
- ☑ **T5 — Conversation sort.** `conversation_header` carries an **Oldest|Newest**
  toggle (`ui.data_temp` per PR url), shown with >1 comment; reverses the
  oldest-first list. Filter / reactions / linked-issue **cut**.
- ☑ **T6 — Body render fix.** `\r` stripped from body + comment text in both parsers
  (`github::parse_detail`, `bitbucket::parse_body`/`parse_comments`). The mockup's
  stray leading `·` was its own sample data (the header's `author · src → dest` is
  intentional), not a render leak.
- ☑ **T7 — Spec + STATE + verify.** Folded into `pull-requests.md` §4
  (`labels`/`created_at`) + §11 (layout), `design-system.md` §4 (tag variant + detail
  card); counter recomputed; `gen_pr_detail` screenshot reviewed (cards, meta-row,
  role tags, Created age, sort toggle all present).

### Next actions (M-PR5)
- Done. Bitbucket reviewers now wired (M-PR4 T2); labels stay GitHub-only.

**Post-milestone polish** (live-render gaps the mockup screenshots surfaced):
- ☑ **Markdown bodies + readability.** Description, conversation cards and inline-comment
  threads were leaking raw markdown (`## …`, `**…**`, `` `…` ``) and, once rendered via
  `egui_commonmark`, read too dense. Replaced by an in-house renderer
  (`pull_requests_view::markdown`, on `pulldown-cmark` — already in the tree via
  `egui_commonmark`) that builds `LayoutJob`s carrying controlled size (`MD_TEXT_SIZE`),
  intra-line leading (`MD_LINE_HEIGHT`) and **letter-spacing** (`MD_LETTER_SPACING`) —
  none of which `egui_commonmark` 0.23's plain `RichText` output can set. Handles
  headings, bold/italic/strike/inline-code, bullet + ordered lists, block quotes and
  fenced code blocks; `pulldown-cmark` keeps `cookie_name` / `upsertInDatabase` from
  italicizing on intraword underscores. The `CommonMarkCache` plumbing is gone.
- ☑ **Reply on every conversation card.** Was Bitbucket-only (the pill needed a forge
  `id`); GitHub conversation comments (`github::parse_detail` → `id: None`) had none.
  `ConversationEdit::Reply` now keys by **conversation index** (stable across the
  Oldest/Newest reversal) and `conversation_reply_block` takes the optional `parent`:
  threaded forges nest via `parent: Some(id)`, flat (GitHub) posts a new top-level
  comment (`parent: None`). Spec §11 updated.
- ☑ **Reply + composer restyle (mockup parity).** The Reply affordance is now a quiet
  neutral pill (`reply_pill` → `pill_button` with a message-square glyph), and the
  standalone "Add a comment" pill became an **always-visible composer bar** at the foot
  of the band — avatar, input field bound to `DiffViewState::conversation_add_buffer`,
  and a filled-accent **Comment** button, raising the same parent-less
  `PostConversationComment`. The button is right-anchored (`right_to_left` so the field
  can't push it off the band) and reads solid accent throughout, only submitting once
  the draft is non-blank. No paperclip/emoji icons (no backend → dead decoration).
  `ConversationEdit::Add` / `open_conversation_add` dropped.
- ☑ **Composer avatar = real current user.** The composer avatar shows the signed-in
  user's initials, resolved per forge from the PR runner's identity pass: GitHub via
  `gh api user --jq '.name // .login'` (`github::current_name_args`), Bitbucket via
  `display_name`/`nickname` off the `/2.0/user` reply
  (`bitbucket::parse_current_user_display_name`). `PrReply` carries `github_name` /
  `bitbucket_name` (only `Some` on the first reply that resolves identity); the app
  keeps them in `HelmApp::pr_user_github` / `pr_user_bitbucket` and feeds the one
  matching `pr.forge_kind` into `PrReviewView::current_user`. `None` ⇒ the plain dot.
- ☑ **Loaders for list + detail fetches.** Cold list/refresh showed nothing (the cache
  reads `Absent`/`Absent` before the first reply ⇒ the misleading "No repository" empty
  state); detail opened to empty body/checks/conversation shells reading as "nothing
  here". Browse list now takes a `PrSourceHints::loading` (`!cache.loaded ||
  runner.busy()`): the refresh icon becomes a spinner and the body shows a centered
  "Loading pull requests…" instead of the empty state. Review center takes
  `PrReviewView::detail_loading` (`detail.is_none() && detail_error.is_none()`): a
  spinner + "Loading pull request…" stands in for the detail sections. Mirrors the
  existing `files_loading`/`diff_loading` rail placeholders.
- ☑ **Comment-card overhaul + code snippets + resolved threads.** Every comment card
  (overlay, center inline, conversation) now carries the **avatar + author** in
  `text.primary` (no more per-author hue), an **Author/Reviewer** role tag, a relative
  **age** (`model::relative_age`), **threaded replies indented**, a **selectable** markdown
  body (the card no longer swallows clicks — the snippet is the open target), and a
  **multi-row composer**. Code-anchored cards embed a **few-line code snippet**
  (`detail::code_snippet`): a numbered gutter + `+`/`-`/space signs tinted green/red/neutral.
  Source: GitHub `diff_hunk` → `model::hunk_snippet` (kept to the `INLINE_SNIPPET_LINES`
  tail ending at the anchor); Bitbucket (no hunk) windows the loaded diff, else nothing.
  **Resolve/Reopen** toggle on inline threads → `ReviewIntent::ResolveThread`: GitHub via a
  GraphQL `reviewThreads` join (`databaseId → (PRRT node id, isResolved)`,
  `apply_thread_resolution`) + `resolve/unresolveReviewThread` mutation on the node id;
  Bitbucket via `POST …/comments/{id}/resolve` (resolve) / `DELETE` (reopen, 204).
  `PrComment`/`ThreadComment` gained `created_at` / `resolved` / `thread_id`. *Files*:
  `src/pull_requests/{model,github,bitbucket,runner}.rs`, `src/app/mod.rs`, `src/review.rs`,
  `src/ui/{detail,pull_requests_view,diff_view}.rs`. *Tests*: domain (`hunk_snippet`,
  `relative_age`), I/O (`parse_review_threads`/`apply_thread_resolution`/`resolve_thread_args`,
  BB resolution parse + `resolve_comment_url`), UI e2e (snippet open target, resolve pill →
  `ResolveThread`). **Live forge round-trips unverified** (no creds/network here).
- ☑ **Conversation replies nest by `parent_id`.** The center Conversation band listed
  every `path.is_none()` comment as a flat top-level card, ignoring `parent_id` — so a
  Bitbucket conversation reply stood as its own card. `conversation_section` now groups
  comments into threads by walking the `parent_id` chain (GitHub stays flat: id/parent
  both None ⇒ one comment per thread), rendering the root then its replies **indented**
  with **one Reply affordance per thread** (nesting under the root id). *Files*:
  `src/ui/pull_requests_view.rs`. *Test*: `conversation_reply_nests_under_its_parent`.
- ☑ **Inline-comment code preview on the conversation page.** Bitbucket inline comments
  carry no forge `diff_hunk`, and the conversation page has no file open, so the center
  inline cards showed only the bare "Open …" link. `poll_pr_review` now prefetches the
  local diff of every commented file lacking a hunk (`ensure_comment_diffs`, deduped via
  `PrReview.comment_diff_requests`), and the view passes the current-range diffs
  (`PrReviewView.comment_diffs`) so `inline_snippet` windows `source_lines` for any
  commented file — not just the open one. *Files*: `src/app/mod.rs`, `src/app/render.rs`,
  `src/ui/pull_requests_view.rs`. *Test*: `inline_comment_card_windows_comment_diff_when_no_hunk`.
- ☑ **Accordion resolved threads + richer snippets.** Resolved threads (center inline +
  conversation) collapse to a one-line **"Resolved · N comments"** summary row (tick +
  chevron), re-opening on click — per-thread expand state in `DiffViewState.expanded_resolved`
  (keyed by the root comment id; `is_resolved_expanded`/`toggle_resolved`); no forge/parser
  change. Snippets gained **syntax highlighting** (`detail::code_snippet` highlights the code
  via `syntax_highlight::highlight_buffer` from the file path; the `+`/`-` sign keeps its kind
  colour) and more context: `INLINE_SNIPPET_LINES` 4→8, and the Bitbucket fallback now windows
  the loaded diff's **hunk** (`diff_window_snippet`, colored add/delete) instead of flat
  `source_lines` when a hunk covers the anchor. *Files*: `src/ui/{detail,pull_requests_view,diff_view}.rs`.
  *Test*: `resolved_inline_thread_collapses_and_reopens_on_click`.
- ☑ **Unified comment-card grammar (conversation + inline + overlay).** The three comment
  surfaces shared no visual language; they now wear one **Detail card** grammar (`bg.surface`
  + `border.subtle` + 10pt radius + 12pt padding) via `comment_frame`. A thread renders as a
  single card — root at full weight, replies a notch in under a left **thread-rail** with a
  lighter avatar (`detail::author_avatar_small`, 20pt) — through the shared `thread_members` /
  `comment_meta_row` helpers; the **age** moved to the card's right edge, and the Reply/Resolve
  controls moved **inside** the card foot. Spacing unified on a `GAP_XS/SM/MD` (4/8/12) scale,
  the Oldest|Newest toggle became a real **segmented control** (`order_segment`, `accent.subtle`
  active), and the diff-overlay `thread_card` dropped its tinted-pill+left-edge look for the same
  card. *Files*: `src/ui/{detail,pull_requests_view,diff_view}.rs`. No new test (covered by the
  existing 30 PR-view render/label tests).
- ☑ **Comment-card readability polish.** Screenshot review showed the grammar was correct but
  still read flat. Each comment now lays out as an **avatar gutter + text column** (`comment_block`
  / `comment_meta_line`): the body aligns under the author instead of sliding back under the
  avatar, and replies nest by a wider gutter under a `border.input` thread-rail on the root
  avatar's centre. The conversation **composer** (`conversation_add_block`) is a quiet single-line
  field (surface fixed to `bg.surface` via `extreme_bg_color`) with a compact Comment button below
  it. The whole detail (overview, description, checks, comment threads) spans the full panel
  width. Section headers reach **parity** — both *Conversation* and *Inline comments* carry a
  `count_chip`; comment **age** brightened to `text.secondary`; author avatars desaturated a touch
  (`detail::muted_lane`). *Files*: `src/ui/{detail,pull_requests_view}.rs`. Verified headlessly
  (31 PR-view render/label tests + a one-off wgpu screenshot, since removed).

### Blockers / Open questions (M-PR5)
- **Bitbucket reviewers**: resolved in M-PR4 T2 (`fields=+values.participants` on the
  list query → parsed from `participants`). **Labels** stay GitHub-only — Bitbucket
  Cloud has no PR labels (genuinely absent, nothing to fetch).
- **List *Updated* column**: resolved in M-PR4 T2 — humanized via `model::relative_age`
  after the column was widened.

---

## ☑ Milestone — M-PR4 · PR list redesign (full-width table)

Spec: [`specs/pull-requests.md`](../pull-requests.md) §5 (reconciled to a mockup).
Turns the browse list from compact two-line rows into a **full-width column
table** per role group, in English. Locked design decisions: **EN** labels, **no
tabs** (the two role groups are the only split), **full-width list** (master →
detail, unchanged §11 surface), header = **title + Refresh** only (no global
search / notifications / theme / `+`), **no filters / sort**; each group is a
**rounded card** (section title + count above it, hairline row separators);
**Status = a plain colored label** (state + review decision) — no pill, CI checks
live in the §11 detail (matched to the sent mockup). Counter: **2/2**.

- ☑ **T1 — Browse list → carded column table.** `render_list` gains a page header
  (`Pull Requests` title + Refresh button → `PullRequestsPageAction.refresh`);
  each group renders a **card** (reserved backdrop shape + `border.subtle` stroke)
  holding a column-header row + table rows with hairline separators. Per-section
  columns — **To review**: Title (+ branch) · Project · Author · Reviewer ·
  Status · Updated · ›; **Mine**: Title (+ branch) · Project · Reviewers · Status
  · Updated · ›. Cell helpers: `columns` (per-role x-ranges), `paint_avatar` /
  `reviewer_stack` (overlapping initials avatars + `+N`, ring tinted by reviewer
  state), `pr_status` (state/review → colored label). *Files*:
  `src/ui/pull_requests_view.rs`, `src/app/render.rs`. *Tests*: PR-cockpit UI e2e
  (groups, rows, select + Refresh intents); `headless-verify` (`shots_gen::gen_pr_list`).
- ☑ **T2 — Calibration + relative age + BB reviewers (feedback).** `columns` now
  caps Title (`TITLE_MIN_W`/`TITLE_MAX_W`) and spreads the fixed data columns with
  an even computed gap to the right edge (was right-anchored → Title hogged all
  slack on wide windows); widened **Updated** (46→96) and humanized it via
  `model::relative_age(updated_at, now)` (falls back to raw on unparseable input).
  Bitbucket reviewers now populate: `role_filtered_url` adds `fields=+values.participants`,
  `parse_pr` reads the roster + per-reviewer decision from `participants`
  (`aggregate_review` collapses it for the Status column). *Files*:
  `src/ui/pull_requests_view.rs`, `src/pull_requests/bitbucket.rs`. *Tests*:
  `bitbucket::parse_list_reads_reviewers_and_review_from_participants`; wide
  `gen_pr_list` shot reviewed.

### Next actions (M-PR4)
- Done. Proceed to M-PR5 (PR detail parity) when ready.

### Blockers / Open questions (M-PR4)
- none.

---

## ☑ Milestone — M-PR3 · PR review: cache & richer reviewing

Spec: [`specs/pull-requests.md`](../pull-requests.md) §5/§6/§11. Makes the cockpit
navigation instant (per-PR cache + diff cache), adds a **per-commit** view,
surfaces **inline comments in the center with code context**, and lifts the §10
limits on **inline replies** and **conversation comments** (add + reply).
Counter: **9/9** — complete, pending review + merge of the `m-pr` worktree branch.

- ☑ **T1 — Per-PR review cache.** Replace `pr_review: Option<PrReview>` with a
  bounded (~8) `HashMap<PrReviewKey, PrReview>` + the active key; `open_pr_review`
  adopts a cached entry **without re-running the runners** (instant), else builds
  + fetches. Preserve per PR: `draft`, `agent_notes`, `summary`, `verdict`,
  `selected_file`, `diff_view`, `detail`, `files`. Re-open re-fetches only when the
  entry is older than ~60 s, swapping on arrival without touching drafts. Pure
  `LruOrder<PrReviewKey>` helper (touch/evict) kept out of the rendering layer.
  *Files*: `src/app/mod.rs`, a small pure LRU module. *Tests*: unit on the LRU;
  UI e2e — PR A → open file → PR B → back to A shows no spinner, drafts intact.
- ☑ **T2 — Diff cache within a PR.** `PrReview.diffs: HashMap<(Oid,Oid,String),
  FileDiff>` instead of the single `diff`/`diff_path` slot (key includes base/head
  — required by T5); `ensure_selected_diff` fetches on miss only, the poll handler
  warms the cache even when the user has switched away. *Files*: `src/app/mod.rs`,
  `src/app/render.rs`. *Tests*: in-crate app — A→B→A across cached files fires no
  fetch; a cache miss fires exactly one.
- ☑ **T3 — Throttle the focus-regained list refetch.** Pure predicate
  `should_refresh_pr(cold, repos_changed, focus_regained, age, min_age)` gates the
  `focus_regained` branch by a 30 s min age on `last_pr_poll`; cold / repos_changed
  fire unconditionally and the periodic 60 s tick is kept at the call site. *Files*:
  `src/app/mod.rs`. *Tests*: unit on the predicate.
- ☑ **T4 — Commits in the detail fetch.** `PrCommit { sha, short, subject, author }`
  + `PrDetail.commits` (oldest-first). GitHub: `commits` added to `DETAIL_FIELDS` +
  `parse_commit` (oid→7-char short, `messageHeadline`, first author name/login).
  Bitbucket: paginated `commits_url` + `parse_commits` (hash, first message line,
  `display_name`/raw), reversed to oldest-first in the runner. *Files*:
  `src/pull_requests/model.rs`, `github.rs`, `bitbucket.rs`, `runner.rs`. *Tests*:
  unit on both fixtures.
- ☑ **T5 — Per-commit diff.** Commit band in the rail (above Files changed);
  selecting a commit recomputes files+diff over `commit^..commit` (explicit
  base/head), "All commits" = the current three-dot diff. Reuses
  `pr_changed_files`/`pr_file_diff` with other oids (local after `fetch
  pull/N/head`); `selected_commit` state in `PrReview`; a files request taking
  explicit base/head. *Files*: `runner.rs`, `mod.rs`, `pull_requests_view.rs`.
  *Tests*: business e2e (single-commit delta on a throwaway repo); UI e2e (band +
  selection).
- ☑ **T6 — Inline comments in the center, with code context (read).** `PrComment`
  gains `context: Option<String>` (GitHub `diff_hunk` from the already-fetched
  comments payload — no extra request; Bitbucket: window derived from the loaded
  `FileDiff`, else `None`). A new **Inline comments** section in the center detail,
  grouped per file: each card = a small monochrome code snippet over the thread;
  clicking opens the file at that line (`select_pr_file` + scroll). The diff overlay
  stays. *Files*: `model.rs` (`context` + `forge_threads`/parse), `github.rs`
  (capture `diff_hunk`), `bitbucket.rs` (fallback), `pull_requests_view.rs`.
  *Tests*: unit on `diff_hunk` parse; UI e2e — inline card with snippet in the
  center, click emits `select_file`.
- ☑ **T7 — Reply to an inline comment (write).** Thread `id` plumbed to the cards
  (`review::ThreadComment` gains `id`); a reply editor on **both** renders (diff
  overlay **and** T6 center card). Post: GitHub `POST pulls/{n}/comments/{id}/
  replies`; Bitbucket `{content.raw, parent:{id}}`. Detail refetches on success.
  *Files*: `review.rs`, `model.rs`, `github.rs`, `bitbucket.rs`, `runner.rs`,
  `mod.rs`, `diff_view.rs`/`pull_requests_view.rs`. *Tests*: unit on reply builders;
  UI e2e emits the reply intent from both places.
- ☑ **T8 — Conversation comments: add + reply (write).** Standalone composer in the
  Conversation section + reply on top-level cards. GitHub `POST issues/{n}/comments`;
  Bitbucket `POST .../comments` (no inline, `parent` for the reply). `ReviewIntent::
  PostConversationComment { parent: Option<u64>, body }` unifies add (`None`) and
  card reply (`Some(id)`); the runner gained `PrPostKind::Conversation` +
  `request_conversation`/`post_conversation`, reusing the §11 success-refetch path.
  *Files*: `review.rs`, `github.rs`, `bitbucket.rs`, `runner.rs`, `mod.rs`,
  `diff_view.rs`, `pull_requests_view.rs`. *Tests*: unit on `issue_comment_args`; UI
  e2e add (`conversation_composer_emits_post_conversation_comment`) + reply
  (`conversation_card_reply_emits_nested_post_conversation_comment`).
- ☑ **T9 — Spec + STATE + verify.** Folded M-PR3 into `pull-requests.md`: §4 model
  (`PrComment.context`, `PrCommit`, `PrDetail.commits`), §5 (inline-context cards +
  per-commit view in the center), §6 (bounded per-PR review cache + per-file diff
  cache → instant revisit; focus-regained 30 s throttle), §10 (replies &
  conversation comments lifted out of scope), §11 (M-PR3 block: per-commit view,
  inline comments in center, thread reply, conversation add/reply) + §9 test
  catalogue. `headless-verify` PASS — the review detail (inline-context card,
  Conversation composer + replies, commit band) renders and the conversation-card
  reply emits a nested `PostConversationComment`.

### Next actions (M-PR3)
- **Review then merge** the `m-pr` worktree branch into `main` (the milestone loop
  does not merge/push — user's call).

### Blockers / Open questions (M-PR3)
- none. Note: T7/T8 extend the frozen §10/§11 scope — fold into §11 in T9. Cache
  diff key (T2) must include `(base,head)` or the per-commit view (T5) serves a
  stale diff; T6/T7/T8 can share one `PrCommentRunner` (reply inline, reply/add
  conversation) + the success refetch.

---

## ☑ Milestone — M-PR2 · In-app PR review (diff · line comments · submit · Ask Claude)

Spec: [`specs/pull-requests.md`](../pull-requests.md) §11. Turns the read-only
detail panel into a **diff-centric review surface**: PR diff without cloning,
in-diff line comments, **Submit review** (Comment / Approve / Request changes) on
GitHub **and** Bitbucket Cloud, and **Ask Claude** on an existing thread. Reuses
the M-RC review engine (`review.rs`, `ui::diff_view`). Counter: **7/7** — complete,
pending review + merge of the `m-pr` worktree branch.

- ☑ **T1 — PR diff producer (domain).** `git::diff::pr_changed_files` +
  `pr_file_diff` over the three-dot `merge-base(base,head)..head` range (I/O-free).
  *Files*: `src/git/diff.rs`. *Tests*: business e2e on a throwaway repo (delta +
  three-dot isolation).
- ☑ **T2 — Wire PR detail fetch.** Base/head resolution in the matched workspace
  repo; changed-files + per-file diff as gated requests on the detail runner path.
  *Files*: `src/app/*`, `src/pull_requests/runner.rs`.
- ☑ **T3 — Detail panel → diff-centric.** Rail of changed files + lazy diff via
  `ui::diff_view`; header keeps Open / Checkout. *Files*:
  `src/ui/pull_requests_view.rs`, `src/app/{mod,render}.rs`.
- ☑ **T4 — Inline existing comments.** Posted threads overlay the diff anchored at
  their line, read-only, via `review::ForgeThreads`. *Files*: `src/review.rs`,
  `src/ui/diff_view.rs`.
- ☑ **T5 — Draft + post review (write).** Footer composer (verdict + summary +
  Submit (N)); `model::draft_comments` flattens the draft store; gated
  `PrPostRunner` posts GitHub `…/reviews` (`gh api --input -`) and Bitbucket inline
  comments + `approve`/`request-changes` (`curl`, Keychain auth); success resets
  the draft and refetches. *Files*: `src/pull_requests/{model,github,bitbucket,
  runner}.rs`, `src/ui/{pull_requests_view,diff_view}.rs`, `src/app/{mod,render}.rs`.
  *Tests*: builder units + 2 business e2e (GitHub payload, Bitbucket bodies/URLs) +
  UI e2e (Submit emits the intent).
- ☑ **T6 — Ask Claude on a thread.** `ReviewIntent::AskAgentOnThread { file, line }`
  from a per-thread **Ask {agent}** pill; the app builds the prompt from that
  thread and launches the agent in the PR worktree (shared `launch_pr_agent`).
  *Files*: `src/review.rs`, `src/ui/diff_view.rs`, `src/app/mod.rs`. *Tests*: UI
  e2e (pill emits the anchored intent).
- ☑ **T7 — Spec + STATE + verify.** `specs/pull-requests.md` §11 (+ §4/§5/§9/§10
  reconciled); this block; full `cargo fmt` + `clippy --all-targets -D warnings` +
  `cargo test` green.

### ☑ Review-surface fix pass (post-T7)
- ☑ **Rail in commit-detail language.** `review_rail` mirrors `ui::commit_detail`:
  author avatar + name + `source → dest`, PR body, a **Files changed** band (count
  chip + ±totals + ratio bar), file rows with separators, bold section titles.
  *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **Conversation = top-level only.** The rail's Conversation lists comments with
  no `path`/`line`; inline ones stay anchored in the diff (`ForgeThreads`).
  *Files*: `src/ui/pull_requests_view.rs`. *Tests*: UI e2e (inline author absent
  from the rail).
- ☑ **Header clears the global buttons.** PR cockpit reserves `TITLEBAR_HEIGHT`
  like the other central modes. *Files*: `src/app/render.rs`.
- ☑ **Ask Claude on a worktree-less branch.** `PendingPrAsk` defers the prompt
  across checkout + create, then `resume_pending_pr_ask` launches the agent in a
  new tab once the worktree is live and any post-create script is consumed.
  *Files*: `src/app/{mod,render}.rs`.

### ☑ Review-surface fix pass v2 (user feedback)
- ☑ **Rail moved to the RIGHT, collapsible.** `render_review` now fills the main
  area with the diff and puts the changed-files rail on the right (commit-detail's
  place); a header toggle (`PanelRightClose`/`Open`) collapses it, persisted as
  `Prefs::pr_rail_collapsed`. Resize handle drag inverted (drag left widens).
  *Files*: `src/ui/pull_requests_view.rs`, `src/app/{mod,render}.rs`,
  `src/persistence.rs`. *Tests*: UI e2e (toggle intent; collapsed hides files).
- ☑ **Per-line icon back to hover-only.** Reverted `DiffReview.always_comment`;
  the gutter button paints on hover only, as before. *Files*: `src/ui/diff_view.rs`,
  `src/ui/pull_requests_view.rs`, `src/app/render.rs`.
- ☑ **Comment affordance reads as a comment.** Gutter icon `Sparkles` → 
  `MessageSquarePlus`; the Sparkles now marks only the separate Send-to-agent pill,
  so the per-line action no longer reads as "AI". *Files*: `src/ui/diff_view.rs`.

### ☑ Review-surface fix pass v3 (user feedback)
- ☑ **Header less prominent, on canvas.** `review_header` paints on
  `palette.bg_canvas` (was `bg_sidebar`); PR title `14.5` → `13.5`.
  *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **Rail = commit-detail background, hideable like the git sidebar.** `review_rail`
  + `review_composer` on `bg_canvas` (was `bg_sidebar`), matching `commit_detail`.
  In the PR cockpit the standard git toggle is suppressed, so **⌘G** now flips
  `pr_rail_collapsed` (alongside the header toggle). *Files*:
  `src/ui/pull_requests_view.rs`, `src/app/render.rs`.
- ☑ **Both per-line affordances.** The hover gutter now shows **two** icons:
  `MessageSquarePlus` (slot 0 → forge draft comment) and `Sparkles` (slot 1 →
  `ReviewIntent::AskAgentOnLine` → `ask_claude_on_line` → `launch_pr_agent`, quoting
  the line). PR review only (`DiffReview.line_agent`); working-tree/commit diffs keep
  the single comment icon. *Files*: `src/review.rs`, `src/ui/diff_view.rs`,
  `src/app/mod.rs`, `src/ui/pull_requests_view.rs`. *Tests*: UI e2e
  (`clicking_the_line_sparkles_emits_ask_agent_on_line`).

### ☑ Review-surface fix pass v4 (user feedback)
- ☑ **Gutter note icon is surface-aware.** Working-tree / commit diffs show the
  `Sparkles` (AI) glyph (the note only feeds the agent); the PR diff shows
  `MessageSquarePlus` (the note is a **forge review comment**, posted to GitHub /
  Bitbucket via *Submit review (N)*, and also batchable to the agent via *Send to
  {agent}*). Picked from `DiffSurface::forge_review()`. *Files*: `src/ui/diff_view.rs`.
- ☑ **Per-line direct agent launch dropped — PR notes batch like the others.** The
  PR diff no longer has a second `Sparkles` button that launches the agent on one
  line; its gutter is the single note button, recording a draft comment handed off
  as a batch via *Send to {agent}*. Removed `ReviewIntent::AskAgentOnLine`,
  `DiffSurface::line_agent`/`claude_button`/`AskLineAgent`, `ask_claude_on_line`,
  `line_agent_prompt`. The existing-thread *Ask {agent}* pill (`AskAgentOnThread`)
  is unchanged. *Files*: `src/review.rs`, `src/ui/diff_view.rs`, `src/app/mod.rs`.
  *Tests*: UI e2e (`pr_surface_note_button_records_a_draft_comment`).

### ☑ Review-surface fix pass v5 (user feedback)
- ☑ **PR diff has two separate note pools.** A forge pool (`PrReview.draft` →
  `MessageSquarePlus`, slot 0 → *Submit review (N)* → GitHub / Bitbucket) and an
  agent pool (`PrReview.agent_notes` → `Sparkles`, slot 1 → *Send to {agent}* recap
  + whole-PR *Ask Claude*), kept apart so forge review comments are never forced
  through the agent. New `review::ReviewPool { Agent, Forge }` tags
  `SaveComment`/`DeleteComment`; `DiffReview` gained `forge: Option<&FileComments>`,
  `DiffViewState.active_comment` a pool key, `comment_block`/`save_note`/
  `open_inline_editor` a `pool` arg; `apply_pr_review_intents` routes by pool and
  `ask_claude_on_pr` now reads `agent_notes` (not the forge draft). Working-tree /
  commit diffs keep the single agent `Sparkles` pool (`forge: None`). *Files*:
  `src/review.rs`, `src/ui/diff_view.rs`, `src/app/mod.rs`, `src/app/render.rs`,
  `src/ui/pull_requests_view.rs`, `specs/pull-requests.md`. *Tests*: UI e2e
  (`pr_forge_button_records_a_forge_pool_comment`,
  `pr_agent_button_records_an_agent_pool_note`).

### ☑ Review-surface fix pass v6 (user feedback)
- ☑ **Rail toggle moved to the title bar.** The changed-files rail collapse button
  now lives in `top_right_actions` (the slot the suppressed git toggle vacates in a
  Helm central mode), same `sidebar_toggle` glyph + ⌘G shortcut as the other
  sidebars. `root_layout`/`top_right_actions` thread `pr_rail_collapsed` +
  `&mut pr_toggle_rail`; the in-view header toggle and `PullRequestsPageAction::toggle_rail`
  are gone. *Files*: `src/ui/mod.rs`, `src/app/render.rs`, `src/ui/pull_requests_view.rs`.
- ☑ **Full-width header band removed.** `review_header` + `rail_toggle_button`
  deleted; the diff now fills the whole surface. Later passes moved Back, the PR
  title and PR-level actions into the center detail, leaving rail collapse scoped
  to the changed-files list + composer. *Files*: `src/ui/pull_requests_view.rs`.
  *Tests*: UI e2e (collapsed rail hides files; toggle-intent test dropped).
- ☑ **Homogeneous button radius.** New `theme::RADIUS_BUTTON = 4` (matches the git
  sidebar commit button); the PR back / action / verdict / Submit buttons drop their
  hardcoded `6.0`. *Files*: `src/theme.rs`, `src/ui/pull_requests_view.rs`.

### ☑ Review-surface fix pass v7 (user feedback)
- ☑ **No file is force-opened; Close ≠ Back.** The review surface opens with **no
  file selected** (the "select a file" placeholder) instead of auto-selecting the
  first changed file. The diff's **Close** (and `Esc` over the diff) now clears the
  selection back to that placeholder **without leaving the surface**, distinct from
  **Back** (which still returns to the list). New
  `PullRequestsPageAction::close_file` + `App::close_pr_file`; dropped the
  auto-select in `poll_pr_review`. *Files*: `src/ui/pull_requests_view.rs`,
  `src/app/mod.rs`, `src/app/render.rs`, `specs/pull-requests.md`. *Tests*: UI e2e
  (`closing_the_open_file_emits_close_not_back`).

### ☑ Review-surface fix pass v8 (user feedback)
- ☑ **PR detail moved to the center; rail kept lean.** With no file open the center
  area now hosts the PR **detail** (author + `source → dest`, body, Checks,
  conversation) instead of an empty placeholder; the right rail keeps only the PR
  heading + actions (Back / Open / Checkout), the **Files changed** band + file
  list and the composer. Selecting a file still swaps the center to its diff. Pure
  view re-layout (new `review_detail`), no app/state change. *Files*:
  `src/ui/pull_requests_view.rs`, `specs/pull-requests.md`. *Tests*: UI e2e updated
  (`detail_conversation_lists_only_top_level_comments`,
  `collapsed_rail_hides_the_changed_files_but_keeps_the_center_area`).

### ☑ Review-surface fix pass v9 (user feedback)
- ☑ **Back + title moved to the center; rail header dropped.** The center detail
  now leads with a **Back** control and the PR **title**; the rail's old header row
  (Back + PR state icon + `#number` + title) is gone, so the sidebar gains vertical
  space for the Files changed band + file list + composer. Back lives in the detail
  (a diff's **Close** returns there). Pure view re-layout — `review_detail` gained
  the header + an `action` param. *Files*:
  `src/ui/pull_requests_view.rs`, `specs/pull-requests.md`. *Tests*: covered by the
  existing PR-cockpit UI e2e (Back/Open/Checkout still resolve).

### ☑ Review-surface fix pass v7 (user feedback)
- ☑ **Rail reaches the window top like a real side panel.** `render_review` now
  receives the full-height central rect; the rail frame (its divider + background)
  spans to the title strip while the diff/detail and the rail's scroll content inset
  past `TITLEBAR_HEIGHT` (where the floating toggle/feedback/prefs icons sit). The
  browse list keeps its inset body. *Files*: `src/ui/pull_requests_view.rs`.

### ☑ Comment-card restyle (user feedback)
- ☑ **Three distinct comment identities, compact, below the line.** The diff's
  per-line comment surfaces share one tinted-card-with-left-edge grammar in three
  inks: forge review draft = `accent` (MessageSquarePlus), agent note = new
  `accent.ai` violet (Sparkles), fetched PR thread = neutral `text.muted` (avatar +
  author). The inline composer dropped its card+header wrapper back to a bare field
  + compact Delete/validate footer; the saved note card dropped its title +
  right-anchored `Lnn`, and comment blocks are no longer indented to the code column
  — the body now flows left-aligned directly under its line. New
  `theme::Palette.accent_ai` (+ every preset); `pool_style`/`comment_block` lost
  their `agent` arg. *Files*: `src/theme.rs`, `src/ui/diff_view.rs`,
  `specs/design-system.md`. *Tests*: existing diff_view + PR-view UI e2e (a11y labels
  unchanged); on-demand shot `gen_pr_review_comments` (`tests/shots_gen.rs`).

### ☑ PR review polish pass (user feedback)
- ☑ **Rail file list plugged into Flat ⇄ Tree.** PR changed files reuse
  `Prefs.git_file_view`, `file_list::view_toggle` and `git::file_tree` with
  session-only collapsed directories. *Files*: `src/ui/pull_requests_view.rs`,
  `src/app/render.rs`.
- ☑ **Composer is a true segmented control.** Comment / Approve / Request changes
  render as one segmented control; the primary button label names the submitted
  action and disables empty Comment reviews. *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **Review signals are calmer per file.** Viewed state moved to an icon-only
  unread filter chip in the Files changed header; rows keep only muted forge-draft
  and agent-note icons with no counts. Existing-thread Ask action moved inside the
  thread card. *Files*: `src/ui/{pull_requests_view,diff_view}.rs`. *Tests*: PR
  rail UI e2e + diff thread intent e2e.
- ☑ **PR-level actions live in the center detail.** Open in browser / Checkout
  moved out of the rail and only appear when no file diff is open.
- ☑ **Center detail header compacted.** The PR detail now starts with a full-width
  header: Back on the left, title + `author · source → dest`, compact
  Open-in-browser / Checkout actions on the right, and a `#number` chip.

### Next actions (M-PR2)
- **Review then merge** the `m-pr` worktree branch into `main` (the milestone loop
  does not merge/push — user's call).

### Blockers / Open questions (M-PR2)
- none

---

## ☑ Milestone — M-PR · Pull Requests cockpit

Spec: [`specs/pull-requests.md`](../pull-requests.md). Entry below Agents listing
my PRs + PRs to review, from GitHub (`gh`) and Bitbucket Cloud, scoped to the
workspace repos. Counter: **9/9** — complete, pending review + merge of the
`m-pr` worktree branch.

- ☑ **PR1 — Domain model.** `pull_requests::model`: `PullRequest`, `PrRole`,
  `PrState`, `Checks`, `Review`, `Reviewer`, `PrDetail`. Reuse
  `git::forge::{Forge, parse_remote}`. *Files*: `src/pull_requests/model.rs`,
  `src/lib.rs`. *Tests*: unit on role/dedupe-by-`(forge,number)` helpers.
- ☑ **PR2 — GitHub source (pure).** `pull_requests::github`: `gh` arg builders
  (`pr list/view/checkout`, `api user`) + I/O-free `parse_list`/`parse_detail`
  over `--json` fixtures; `gh auth status` availability probe shape. *Files*:
  `src/pull_requests/github.rs`, `tests/fixtures/`. *Tests*: unit on captured
  JSON (open/draft, checks, review, reviewers).
- ☑ **PR3 — Bitbucket source (pure).** `pull_requests::bitbucket`: Cloud `2.0`
  URL builders + Basic-auth header + I/O-free `parse_list`/`parse_detail`.
  Keychain creds via `security` CLI (service `helm.bitbucket`); `bitbucket_email`
  in `Prefs`. *Files*: `src/pull_requests/bitbucket.rs`,
  `src/pull_requests/creds.rs`, `src/persistence.rs`. *Tests*: unit on JSON
  fixtures + URL/header builders.
- ☑ **PR4 — PrRunner + cache.** Detached one-shot runner (architecture §3
  contract: one reply/request, `in_flight`, drain+repaint each frame); fans
  per-`Forge` queries, classifies roles vs cached identity → `Vec<PullRequest>`
  + per-source status; app cache + refresh cadence (entry / manual / ~60 s
  focused tick). *Files*: `src/pull_requests/runner.rs`, `src/app/*`. *Tests*:
  business e2e on command/URL construction (no live network).
- ☑ **PR5 — Sidebar entry + central mode.** `Pull Requests` row below Agents
  (`Icon::GitPullRequest`), To-review count badge; `CentralMode::PullRequests`
  (sidebar stays, git panel hides); `SidebarAction.open_pull_requests`. **No
  keyboard shortcut.** *Files*: `src/ui/repo_sidebar.rs`, `src/ui/mod.rs`,
  `src/app/{mod,render}.rs`. *Tests*: UI e2e (entry renders, click sets mode).
- ☑ **PR6 — Cockpit page.** `ui::pull_requests_view::pull_requests_page`: two-pane
  (list grouped To review / Mine + detail panel: description, checks, reviewers,
  read-only comments; actions Open in browser / Checkout), resizable split
  `Prefs.pr_detail_width`. *Files*: `src/ui/pull_requests_view.rs`, `src/ui/mod.rs`,
  `src/persistence.rs`, `src/app/{mod,render}.rs`.
  *Tests*: UI e2e on a fixture list (groups, rows, select, action intents).
- ☑ **PR7 — Checkout = worktree.** If a worktree already sits on the PR source
  branch ⇒ activate that row (no git write); else fetch the branch (GitHub
  `pull/<n>/head`, Bitbucket `origin/<source>`) and create a worktree on it via
  `git::worktree::CreateRunner`, then activate it. *Files*:
  `src/pull_requests/runner.rs` (`CheckoutRunner` + `fetch_refspec`/`match_pr_root`/
  `matching_worktree`), `src/app/{mod,render}.rs`; reuse `src/git/worktree.rs`.
  *Tests*: unit on fetch-ref/argv build + existing-worktree match.
- ☑ **PR8 — Preferences section.** Preferences → **Pull Requests**: Bitbucket
  email field + Save token (Keychain), GitHub/Bitbucket `gh` status lines from the
  PR cache (section-open tick warms it); persist `bitbucket_email`,
  `pr_detail_width`. *Files*: `src/ui/preferences.rs`, `src/app/{mod,render}.rs`,
  `specs/preferences.md`. *Tests*: UI e2e on the section (nav opens it, statuses
  surface, email change + Save intents).
- ☑ **PR9 — End-to-end verify.** `headless-verify` on a fixture cockpit: To
  review / Mine groups + rows, the selection's detail (description, checks,
  reviewers, comments) and the Open-in-browser / Checkout actions all render and
  route their intents. Demonstrable milestone scenario (DoD).

- ☑ **Hardening — branch review pass.** Resilience + correctness fixes across the
  PR feature: per-source `Option<Vec<PullRequest>>` reply so a transient GitHub or
  Bitbucket failure keeps last-good rows + flags `PrCache.stale` (never blanks the
  cockpit); Bitbucket token off `curl` argv (stdin `--config -`) + connect/max
  timeouts + page-follow `next` pagination; `Esc` on a loaded PR diff no longer
  discards the draft; PR runner drained on the Preferences page; checkout from the
  review surface targets `review.pr`; selection reconciled after refresh; cockpit
  surfaces §5 empty / source-unavailable banners; rail-width NaN clamp; re-query on
  workspace repo-set change. *Files*: `src/pull_requests/{runner,bitbucket}.rs`,
  `src/ui/pull_requests_view.rs`, `src/app/{mod,render}.rs`. *Tests*: `PrCache`
  apply resilience units + the existing PR/UI suites. **Dropped:** GitHub
  team-requested reviews → To-review (per-repo `gh` role queries already cover
  direct requests; team expansion deferred).

### Next actions
- **Review then merge** the `m-pr` worktree branch into `main` (the milestone
  loop does not merge/push — user's call). Next milestone afterwards.

### Blockers
- none

### Open questions
- none

---

## Completed milestone — ☑ M-RC · In-diff code review → Send to Claude

In-diff annotation flow (no review "mode"): each diff line carries a borderless
**note (✦) icon** beside its stage `+`; clicking it opens an **inline editor**
(Enter / click-outside validates, Shift+Enter newline; ✕ deletes the comment, ✓
validates), a saved note shows as a **clickable card** (click re-opens its
editor) → annotate across multiple files (comments accumulate per repo). The
header **recap chip (✦ N)** opens a **popover** listing every note grouped by file
(each shows its truncated code anchor, edits in place, deletes) with a
right-anchored **✦ Send to {agent}** footer that opens a **new terminal tab**
running
`claude "<prompt>"` in the active worktree (prompt = aggregated
file/line/code/note) and **clears** the repo's comments (the tab is the only
signal). Works on **both** Git WIP and Commit Détail (every line annotable).
Locked: multi-file accumulation (app-level store), **dedicated** pref
`review_agent_command` (default `claude`), launch in a **new tab**.
In-memory only except `review_agent_command` (persisted). Counter: **6/6**.

- ☑ **RC1 — Domain + prompt.** `review::{LineComment, build_review_prompt}` —
  pure markdown grouped by file (BTreeMap), line-ref `new_lineno` else
  `old_lineno`, code + note. *Files*: `src/review.rs`, `src/lib.rs`. *Tests*:
  unit (grouping order, line-ref fallback, multi-file aggregation).
- ☑ **RC2 — Pref `review_agent_command`.** Scalar in `Prefs` before
  `keybindings`, `#[serde(default = "…")]` → `"claude"`; Preferences AI card row
  (reuse `run_command_row` singleline, persist via `action.ai_changed`). *Files*:
  `src/persistence.rs`, `src/ui/preferences.rs`, `src/app/render.rs`. *Tests*:
  round-trip (default/empty/custom) + UI e2e on the row.
- ☑ **RC3 — Review domain store.** `review::{FileComments, add_comment,
  delete_comment, count, ReviewIntent}` — pure store helpers (upsert by line,
  delete+purge empty file, total count). *Files*: `src/review.rs`. *Tests*: unit
  on add/delete/purge/count. (HelmApp/DiffViewState/DiffReview wiring folded into
  RC4 — those private fields only become live with RC4's rendering.)
- ☑ **RC4 — diff_view rendering + wiring (no review "mode").** Introduces
  `HelmApp.review: HashMap<RepoKey, FileComments>` + `apply_review_intent`
  (save/delete; `SendToAgent` no-op stub → RC5), `DiffViewState.{active_comment,
  popover_edit, popover_buffer, note_focus}` (+ `clear()`), `struct
  DiffReview<'a> { comments, agent, intents }` 8th param of `diff_view()` as
  `Option<&mut DiffReview>` (both call-sites; `None` at the test sites). Each
  diff line carries a per-line note (✦) icon beside its stage `+` (gutter widened
  via `LINE_ACTION_W`/`LINE_ACTION_GAP`); click → `DiffLineAction::OpenComment`
  (incl. read-only lines). `diff_line` returns `Option<DiffLineAction>`; rows
  allocate height before the clip check. Per-line loop renders an inline
  `note_editor` (Enter validates, Shift+Enter newline via `input_mut` event
  filtering; ✕/✓ icons under the field) or a saved-note `note_card` (clickable →
  re-opens its editor); `note_focus` one-shot autofocus. Header: recap chip
  (✦ N) → `CloseOnClickOutside` popover (per-file notes, each edit-in-place +
  delete, `Send to {agent}` footer). First `Esc` cancels editor/popover-edit.
  *Files*: `src/app/mod.rs`, `src/ui/diff_view.rs`, `src/app/render.rs`. *Tests*:
  UI e2e (click ✦ → type → Validate → `SaveComment`; chip → popover lists notes,
  delete, `Send to {agent}` → `SendToAgent`).
- ☑ **RC5 — Apply + spawn.** `apply_review_intent::SendToAgent` →
  `send_review_to_agent(ctx)`: build prompt (`build_review_prompt`) → `add_tab`
  → pre-insert the agent pane under `(run_key, new_tab_id)` at the fresh tab's
  focus `PaneId` (render's `or_insert_with` then a no-op) → rename tab to the
  agent command → **clear the repo's comments** (`self.review.remove(&key)`) →
  `central_mode = Terminal`, `diff = None`. `open_agent_terminal` spawns an
  **interactive login shell** with the prompt exported (`HELM_REVIEW_PROMPT`)
  then feeds `pty::agent_invocation` (`{program} "$HELM_REVIEW_PROMPT"`): the
  agent runs as a **shell job**, so Ctrl+C / exit drops back to a usable prompt
  instead of a dead pane. *Files*: `src/app/{mod,render}.rs`,
  `src/terminal/pty.rs`. *Tests*: unit on the fed invocation; in-crate e2e
  (seeded comments → `send_review_to_agent` adds an active tab carrying a `Live`
  agent pane + flips central mode, clears diff).
- ☑ **RC6 — End-to-end verify.** `headless-verify`: review WIP + Commit Détail,
  multi-file, Send opens a new tab with a live pane (stub agent in test).
  Demonstrable milestone scenario (DoD).

### Next actions (M-RC)
- **M-RC complete** (6/6). Review the worktree branch `m-rc`, then merge into
  `main`.

### Open questions (M-RC)
- none — Send now clears the repo's comments automatically (opening the tab is
  the only signal); the standalone Clear control was dropped.
