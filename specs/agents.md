# helm — AI agent detection (activity badge)

When a CLI agent (Claude Code, Codex, opencode…) runs in a helm
terminal, the left sidebar shows **where each workspace stands** at a glance,
without hooks or configuration on the agent side. Module: `agent_watch` (+
`terminal::activity` for the stamps). Requirement #1: **minimize false
positives** — when in doubt, show nothing.

## 1. Badge states

Per sidebar **worktree row** (max over the panes of **all the row's tabs**,
order `None < Idle < Done < Working`). The **project header** carries the
**aggregate** — the max over its worktree rows — **visible when the group is
collapsed** ([`worktrees.md`](worktrees.md) §1):

| State | Rendering | Semantics |
|------|-------|------------|
| `None` | nothing | no agent in the foreground of an entry's pane |
| `Idle` | hollow gray ring | agent present, at rest (nothing to report) |
| `Done` | green dot + faint static halo | the agent finished a work episode the user has not seen |
| `Working` | accent arc spinner | the agent is producing sustained output (it is working) |

`Done` is a persistent until-acknowledged **state**, so its indicator is **static**
(no animation): an animated badge in the always-visible sidebar would pin the whole
app at the animation frame rate for as long as the completion lingered, never
letting the app return to idle. Only `Working` — genuine transient activity — animates.

The green one is an **unread completion signal**: it clears on **tab focus**
(active entry + active tab + focused window — seeing = acknowledging)
or on **typing in the pane** (replying = acknowledging). An acknowledged episode
never re-arms the green one; only a **new** work episode can. Use case: send a
prompt, switch workspace — spinner during the turn, green at the end, gray on
returning to the workspace.

## 2. Two-layer detection (1 s poll per live pane)

**Layer A — process gate.** `Pty::foreground_pgid()` (tcgetpgrp via
portable-pty) then macOS probe of the foreground group: `proc_listpgrppids` +
`proc_name` (comm), and argv via sysctl `KERN_PROCARGS2` when comm alone does
not match. Two argv escalations, by installation family:

- **Interpreter** (`node`, `bun`, `deno`, `python`…) — npm/pip agent, comm =
  the interpreter: scan the arguments by **exact path component**
  (`name`, `name-code`, `name-cli`) — a project named `claude-test/` does not
  match.
- **Versioned binary** (any other comm) — Claude Code native installer:
  `~/.local/bin/claude` is a symlink to `versions/<x.y.z>` (file named
  after the version) ⇒ the kernel derives p_comm from the **resolved**
  binary ("2.1.162"); the invoked name survives in **argv[0]** — only its
  exact basename counts, never the arguments (`vim claude` does not match).

argv read failure ⇒ comm-only degradation (fail-safe: less detection,
never more).

**No agent in the foreground ⇒ `None`, whatever comes out of the PTY.** This is
what kills the `cargo build`, vim, htop, animated-prompt false positives.

Watchlist (const, extensible): `claude`, `codex`, `opencode`, `gemini`,
`aider`, `amp`.

**Layer B — activity heuristic.** `PaneActivity` (lock-free atomics,
stamped by the reader thread and the UI thread) distinguishes **spontaneous**
output from typing echo (output ≤ 350 ms after an input = echo), tracks output
episodes (runs) and a rolling byte window.

## 3. State machine (pure, injected clock)

Evaluated at the 1 s tick, per pane (`PaneAgentState::tick`):

```
Working = silence < SILENCE && run ≥ MIN_RUN && recent bytes ≥ MIN_WORK_BYTES
Done    = pending_attention (armed: silence ≥ ATTENTION_SILENCE
          && Working episode observed ≥ ATTENTION_MIN_WORK && unacknowledged)
Idle    = otherwise (agent present)
```

Completion is judged on the **Working episode observed by the ticks**, at
boundaries stamped on the **reader's output timestamps** — not on the tick
clock: a real ~2-3 s turn is only seen by 1-2 ticks and, measured at the
tick, would miss the floor depending on the phase. Two episodes separated by
less than `ATTENTION_SILENCE` merge (tool call) — except an already
**acknowledged** episode: a trailing post-acknowledgment redraw (resize) does
not re-arm an already-seen green. Judged on the episode, **not on the raw
run**: at the end of the turn, the agent's TUI emits trailing redraws (the
prompt coming back, the "esc to interrupt" bar erased) that start a tiny run
again and would disarm a condition sitting on the last run.

| Constant | Value | Against which false positive |
|---|---|---|
| `ECHO_WINDOW_MS` | 350 ms | typing echo / navigation in the agent's TUI |
| `SILENCE_MS` | 2.5 s | sub-second TUI spinner ⇒ true silence = no work |
| `MIN_RUN_MS` | 1 s | one-shot prompt redraw ≠ work |
| `MIN_WORK_BYTES` | 200 B / 2.5 s window | low-rate animated prompt stuck in Working |
| `ATTENTION_MIN_WORK_MS` | 2 s of observed episode | banner / resize redraw ⇒ never green |
| `ATTENTION_SILENCE_MS` | 6 s | silent pause ≤ 6 s mid-turn ⇒ no Working→green flap |

The 2.5 s / 6 s asymmetry is intentional: the spinner drops back quickly, the
green only arrives after a silence markedly longer than a mid-turn pause.

## 4. Accepted limitations

- **tmux/screen**: an agent inside a multiplexer lives in another session /
  another tty — invisible to the probe (the PTY's foreground is the tmux
  client). Documented, not worked around.
- **Heuristic, not protocol**: no Claude Code hooks nor Codex notify
  (phase C dropped — too invasive). An agent that "thinks" without writing
  anything for ≥ 2.5 s drops back to `Idle`/`Done`; acceptable, agent TUIs
  animate their spinner continuously during tool calls.
- **Fully silent tool call > 6 s**: indistinguishable from a turn ending
  in silence — the green arms mid-turn, the spinner replaces it on output
  resumption, and it re-arms at the true end (nothing is lost).
  Rare in practice (cf. spinners above). A **sustained** banner/redraw
  **≥ 2 s** (`ATTENTION_MIN_WORK_MS` floor) stays indistinguishable from a
  short turn: a trade-off accepted in favor of genuine short turns.
- **macOS only** (libproc probe); other targets ⇒ structural `None`.

## 5. Completion notification & cross-repo view

The badge closes the loop on the active workspace; these two sinks close it
across the rest. Both read the same per-pane `AgentBadge`, so no new detection.

**Native notification on `Working → Done`.** On the rising edge
(`newly_completed`: `now == Done && prev != Done`, one-shot per episode), helm
posts a macOS banner — title *"Claude finished"*, body `repo · branch`. Sent
via `osascript -e 'display notification …'` (`notify` module) on a detached
thread (the ~100 ms subprocess never blocks the UI). No bundle, entitlement, or
`UserNotifications` linkage. Gated by a Preferences toggle
(*Agents → Completion notifications*, on by default); clicking the banner is a
no-op (osascript limitation) — focus is the dashboard's job.

**Cross-repo agents dashboard** (`CentralMode::Agents`, in-layout — *not* a
full-window page). Reached from an **always-visible Agents entry** under a
**Helm** section label at the top of the left sidebar (mirroring the Projects
section above the repository list); the entry carries the workspace-wide max
badge (accent spinner = working, green dot = a finished turn). Opening it sets
the central area to the dashboard while the **project sidebar stays** (the entry
highlighted); the per-repo git panel is **hidden** (the view is cross-repo).

The dashboard is a **two-pane cockpit**: a left **list** and a right **terminal
panel**. The list (flush to the left edge) groups rows by **project**: a root and
its worktrees share one card (titled with the root's name + agent count), so their
agents sit together. One row per agenting pane: agent name, a per-row **branch
chip** (which worktree), tab, a state indicator (accent arc spinner / green dot +
faint static halo / hollow gray ring), a detail (*Working…* / *Finished Nm ago* /
*Idle*) and a discreet **jump icon** (external-link) at the right edge.

Clicking a row **body selects** it; the right panel then mirrors that agent's pane
**live** — the same terminal widget as the workspace, fully interactive
(read / scroll the scrollback, type a reply). The selection is a stable
`(repo, tab, pane)` triple that survives the per-frame agent-list rebuild; if its
tab/pane closes it's dropped and the **most urgent** agent re-picked (Working >
Done > Idle, ties by workspace order) so the panel never opens empty. Clicking the
row's **jump icon** instead **focuses** that pane in its workspace — activates the
repo, its tab, sets pane focus, switches back to the terminal. Below a minimum
width the panel folds away and the list spans the full area (the jump icon remains
the way to reach a workspace).

**Two view modes.** A segmented control — sharing the Terminal/Git switch's
design and **titlebar placement** (centered, shown in both modes, no separate
header bar) — switches between **List** and **Columns**; the choice is
**persisted** (`Prefs.agents_view`, default **List**) and restored next launch.
*List* is the cockpit above. *Columns* is a **wall of live terminals**: one column
per project that has a running agent, laid out left→right with **horizontal
scroll** when they overflow. The column width is **shared** across columns and
**resizable** — dragging the handle in any column gap widens/narrows them all
(clamped, persisted in `Prefs.agents_column_width`, restored on launch).
Inter-column margins are kept tight so terminals get the most room. A column
**hugs its content height** (a single short terminal leaves no empty lane below
it), scrolling vertically only once its cards outgrow the page.
Each column carries **its own hue** (cycled from the theme's graph-lane palette,
washed against the theme base so it stays balanced in light and dark) so projects
read apart at a glance.
Each column is split into **worktree sub-cards** (branch chip + an **uncommitted
ratio bar** when the worktree is dirty, mirroring the sidebar + agent count),
each holding a **sub-sub-card per agent** — agent name + state indicator + the
list view's **jump icon** (external-link) header over a live terminal whose
**height is shared and resizable**: dragging the handle along any card's bottom
edge resizes every terminal at once (clamped, persisted in
`Prefs.agents_terminal_height`, restored on launch). The nesting is **always
full** (project → worktree → agent, even with one of each). Every terminal is the
same interactive widget (type / scroll), so all agents are watchable and
reply-able at once; the **hovered** terminal owns the mouse wheel (scrollback /
TUI) so the column no longer scrolls in tandem. Both modes mirror panes from the
same `(repo, tab, pane)` keys; clicking any column terminal **focuses** it
(becomes the single `selected_agent` — drives the focus lock and
`Esc`-as-interrupt), reusing the cockpit's click→focus handshake, while the card's
**jump icon focuses** that pane in its workspace (same handshake as the list
row's). The **focused** agent's card stays opaque while the **others dim** — same
spotlight as an unfocused split pane (terminal.md), no outline.

Leaving the dashboard: picking any project, or `Esc` — **except** when the panel
terminal holds keyboard focus, where `Esc` reaches the agent as an interrupt
(the dashboard stays). Because the page stays `Main`, the regular worker drain
keeps the watch live; viewing the dashboard does **not** acknowledge greens (focus
gating excludes `CentralMode::Agents`).

## 6. Out of scope for v1

Configurable watchlist in Preferences; agents under tmux; in-app
notification-center history.
