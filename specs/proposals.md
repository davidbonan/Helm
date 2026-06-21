# helm — Feature proposals (backlog)

> Idea backlog, not committed scope. `specs/*.md` freeze product intent and
> `specs/plan/STATE.md` tracks execution; this file is the staging area between
> the two. Each item is grounded in the current code. Promote an item to a
> milestone (`STATE.md`) before implementing.

> **Reorganized:** the 4 chronological batches were merged into themed groups,
> deduplicated, and re-prioritized. The original IDs (`P1`–`P38`) are kept for
> traceability. Priority markers: 🔥 **Now** (highest value/leverage, low risk) ·
> ➡ **Next** (clear win, modest effort) · ◇ **Later** (good, not urgent) ·
> ⚖ **Bet** (big effort or a tradeoff/decision to arbitrate first).

---

## Recommended order (shortlist)

1. **P20** — Honor commit signing 🔥 — *correctness bug, not a feature.*
2. **P1** — Agent completion notifications + cross-repo view 🔥 — *biggest product leverage; infra mostly paid.*
3. **P12** — Branch ahead/behind indicator 🔥 — *primitive already computed, never surfaced.*
4. **P21** — Per-repo dirty indicator in the sidebar ➡ — *small; pairs with P12.*
5. **P30** — Granular discard (hunk/line) ➡ — *mirrors the existing granular staging path.*
6. **P5 + P35** — Terminal settings: font + scrollback ➡ — *documented gaps, prefs slot reserved.*
7. **P2** — Terminal search (Cmd+F) ➡ — *parity with the graph, already specified there.*
8. **P10** — Branch + worktree from a graph commit ➡ — *wires modal (M28) + graph menu (M27), explicitly anticipated.*

---

## 1 · Agents & notifications

**P1 — Agent completion notifications + cross-repo agents view** 🔥 ⭐
- `agent_watch.rs` computes `Idle/Working/Done/Attention` per pane, but the only
  sink is the sidebar badge (`agents.md` §3) — loop not closed. Native macOS
  notification on `Working→Done` / `Attention`; click → focus the pane/tab/repo.
- Out-of-scope v1 by decision (`agents.md` §5) → a decided next step. `objc2` /
  `objc2-app-kit` already in deps; add a `UserNotifications` binding. Thresholds
  (2.5 s / 6 s) already tuned. Complement: a cross-repo "running agents" panel.

**P11 — Terminal bell / long-command notification** ◇
- No bell handling today (no `bell` in `src/terminal/`). Generalizes P1 beyond
  agents: a BEL or a foreground command finishing while unfocused → visual flash
  + optional notification. Reuses the P1 notification path (do after P1).

## 2 · Cross-repo git awareness (sidebar)

> One initiative, three separable deliverables. P12 first (the data already
> exists), then P21, then P24 to keep them fresh.

**P12 — Branch ahead/behind indicator** 🔥
- The branch indicator shows no divergence, yet the primitive exists:
  `graph_ahead_behind` (`git/worktree.rs:615`) is computed for fast-forward
  refresh but never surfaced. Low effort, high "push/pull now?" signal.

**P21 — Per-repo dirty indicator in the left sidebar** ➡
- The row badge column is agent-only (`repo_sidebar.rs:36`); the status poll is
  active-repo-only (`git.md` §213). Add a light dirty dot / count per repo to see
  at a glance which repos have uncommitted work.

**P24 — Background fetch-all across repos** ◇
- Periodically fetch every sidebar repo so P12 + P21 stay fresh without opening
  each. Network op on the existing runner; cadence/throttle to define.

## 3 · Git correctness, safety & history

**P20 — Honor commit signing config (GPG / SSH)** 🔥 — correctness gap
- `commit.rs:36` commits via `git2::Repository::commit`, which does **not** honor
  `commit.gpgsign` / `gpg.format=ssh`. A user with signing enabled gets
  **unsigned commits silently** (`git.md` §15's "signature from git config" is the
  author identity, not the cryptographic signature).
- Fix: route through the `git commit` subprocess (runner already used for
  push/pull/stash, `git.md` §10) when signing is configured, or `commit_signed`.

**P22 — Reflog view / recover lost commits (read-only)** ◇
- Fits the graph's read-only philosophy (`git.md` §9); `git2` exposes the reflog.
  A read-only list to recover a detached/lost commit — safer, smaller cousin of P9.

**P4 — Blame / file history (read-only)** ◇
- `git2` does blame natively. Per-line blame in the diff, or a file-history view.
  No network, no auth (`git.md` §9).

**P9 — Undo / redo of git operations** ⚖
- Already deferred with a reserved toolbar slot (`git.md` §10). Large; high value.

## 4 · Staging, commit & diff

**P30 — Granular discard (hunk / line)** ➡
- Discard is file/all only (`discard.rs`: `discard_file` + `discard_all`), while
  staging is per file/hunk/line (`git.md` §1). Mirror the granular staging path
  so a single hunk or line can be discarded from the diff.

**P13 — Side-by-side diff + intra-line word diff** ◇
- Diff is unified-only (`git.md` §134), already syntax-highlighted
  (`diff_view.rs`). Add a side-by-side toggle and word-level highlighting.

**P25 — Image / binary file diff preview** ◇
- Diff view is text-only. Before/after preview for images; a clean "binary file
  changed" affordance instead of an empty diff.

**P26 — Keyboard-driven staging** ◇
- Staging selection is mouse-driven. `j/k` to move between files/hunks, `s`/`u`
  to stage/unstage in the git panel and diff view. Pairs with P15.

**P28 — Conventional-commit helper / message presets** ◇
- Optional type/scope prefix helper in the commit card (`feat:`, `fix:`…) or
  saved templates. Opt-in so it never gets in the way.

**P14 — AI review/explain staged changes + AI PR description** ◇
- Extends the AI surface (`ai.rs` commit msg, `ai_rebase.rs` agentic rebase).
  "Review/explain my staged diff" via the agentic provider; pre-generate the PR
  title/body for create-PR (M32) and pass via URL params (feeds P10/forge).

## 5 · PR / forge & branch ops

**P10 — Create branch + worktree from a graph commit** ➡
- Explicitly anticipated (`worktrees.md` §10). The create-worktree modal (M28)
  and the graph row menu (M27) already exist — this wires them.

**P8 — PR / CI status (read-only)** ⚖ — tradeoff
- create-PR is a prefilled URL today (zero API/auth, `git/forge.rs`, M32). Showing
  PR/CI state needs a token → breaks the "no network auth" posture
  (`overview.md` §4, `update.md` §9). Only on an explicit decision.

## 6 · Terminal — search, navigation & panes

**P2 — Terminal search (Cmd+F)** ➡
- The graph has `Cmd+F` (filter + cycle, `keybindings.md` §69); the terminal has
  none, though scrollback exists (`terminal.md` §8). Parity with a spec'd behavior.

**P3 — Persist the tab/split layout across sessions** ➡
- Each repo restarts with a single empty tab (`terminal.md` §10). The PTY isn't
  restorable (fine), but the tab/split tree + cwd is — re-spawn on launch.
  Persistence infra present (`persistence.rs`).

**P15 — Keyboard copy-mode + scrollback navigation** ◇
- Selection is mouse-driven (`terminal.md` §7). A tmux-like keyboard copy mode
  (navigate, select, yank) pairs with P2 and P26.

**P31 — Pane zoom / maximize toggle** ◇
- No zoom in `terminal/layout.rs`. tmux-style toggle to maximize the focused split
  to the full center zone, restoring the tree on toggle-off.

**P32 — Broadcast / synchronized input across panes** ◇
- Type once, send to all panes of a tab (e.g. same command across sibling
  worktrees). Gate behind an explicit toggle so it never fires by surprise.

## 7 · Terminal — config & appearance

**P5 + P35 — Terminal settings: font + scrollback** ➡ (merged)
- **Font (family + size):** zoom is runtime/global, not persisted/configurable;
  slot reserved (`preferences.md` §6), Terminal section exists (M30-4).
- **Scrollback size:** `SCROLLBACK_LINES` hardcoded at 10_000 (`emu.rs:14`).
- Both land in the same Terminal prefs section → one small milestone.

**P19 — Importable terminal color themes** ◇
- Import iTerm / Ghostty / base16 schemes over the ANSI palette
  (`terminal/palette.rs`). Pairs with the Appearance section.

**P16 — Per-repo startup command / terminal profile** ◇
- Run a command on tab creation (activate venv, `nvm use`, …). The Project section
  in Preferences already exists (`preferences.md` §4).

## 8 · Navigation, search & UX surfaces

**P6 — Command palette / quick switcher (Cmd+K)** ◇
- Fuzzy over repos + branches + files + actions. Unifies navigation; consistent
  with the Codex Desktop aesthetic. Preferences search already anticipated
  (`preferences.md` §6).

**P17 — In-repo file/content search panel** ◇
- Integrated grep panel; click a result → open in the configured editor (reuses
  `links::execute`, M30-4). Overlaps the terminal — scope carefully.

**P33 — Status bar / footer** ◇
- Consolidate scattered signals into one footer: active branch, ahead/behind
  (P12), agent state (`agent_watch.rs`), repo path. Single glanceable line.

**P23 — Row quick actions: Reveal in Finder / Open in editor / Copy path** ◇
- Context-menu actions on repo & worktree rows; reuses the configured editor path
  (`links::execute`, M30-4). Small quality-of-life win.

**P18 — Drag-and-drop a folder onto the sidebar to add it** ◇
- Onboarding quick win. Recursive scan is forbidden (`overview.md` §3.1), but
  dropping a folder to add a single repo is not — complements Open Folder / ⌘O.

**P38 — In-app activity / operation log** ◇
- Toasts are transient (`ui::toast`). A persisted log panel to review past
  operations and errors (network ops, rebases, failures) after they auto-expire.

## 9 · Big bets, platform & tradeoffs

**P7 — In-app conflict resolution** ⚖
- Always punted to the terminal today (`git.md` §1, §10). Big chantier, high value;
  the "Merge/Rebase in progress" banner + conflicting-files list anchor it.

**P34 — `helm` CLI launcher (`helm .`)** ◇
- Open a folder in helm from the terminal, like `code .`. A small CLI shim talking
  to the running .app (distribution: `update.md`). Lowers adoption friction.

**P27 — Detach tab into a new window / multi-window** ⚖
- Single window today (`overview.md` §2). macOS users expect multiple windows /
  Spaces; detaching a terminal tab is the natural step. Larger architectural change.

**P29 — Submodule status** ◇
- Not addressed in the specs. Show submodule state and offer update. Niche; only
  if a target repo needs it.

**P36 — Opt-in accessibility mode (AccessKit)** ⚖ — tradeoff
- AccessKit deliberately disabled for perf (Cargo.toml comment: first AX query
  rebuilds the full a11y tree every frame for the process lifetime). An opt-in mode
  restores VoiceOver at a known perf cost — only behind an explicit setting.

**P37 — Localization / i18n** ⚖ — decided exclusion
- UI is English-only by decision (`git.md` §1). Revisit only for a localized
  audience; needs a string-extraction pass first.
