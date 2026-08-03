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
posts a macOS banner — title *"Claude finished"*, body `repo · branch`
(`notify` module). Gated by a Preferences toggle (*Agents → Completion
notifications*, on by default); clicking the banner is a no-op — focus is the
dashboard's job.

Two backends behind `notify::post`. From the `.app`, **`UserNotifications`**:
the banner carries helm's own bundle identity, the only form the system
attributes to *helm* — listed under its name in System Settings › Notifications
and, decisively, allowlistable in a **Focus mode**, which otherwise suppresses
it outright. `notify::install` runs once at startup (main thread, after
`NSApplication` exists): it requests authorization — system prompt on first
launch, a refusal being the user's answer, not an error — and registers the
presentation delegate, without which macOS drops the banner whenever helm is
frontmost, i.e. the common case, since the agent that just finished runs in one
of helm's own background tabs.

Outside a bundle (`cargo run`, tests) the process has no bundle identifier and
`UNUserNotificationCenter` raises an uncatchable exception, so `install` probes
it once and the fallback stays `osascript -e 'display notification …'` on a
detached thread (the ~100 ms subprocess never blocks the UI). That path needs no
entitlement, but the banner is attributed to **Script Editor** — wrong app, and
no Focus mode can allowlist it as helm. macOS also refuses authorization to a
bundle sitting outside a standard location (*"Notifications are not allowed for
this application"*), so the backend can only be verified from an installed
`.app`, not from a scratch build directory.

**Cross-repo agents dashboard** (`CentralMode::Agents`, in-layout — *not* a
full-window page). Reached from an **Agents entry** under a **Helm** section
label at the top of the left sidebar (mirroring the Projects section above the
repository list) or via `Cmd+Ctrl+0` ([`keybindings.md`](keybindings.md) §1),
shown **once the workspace has a project** — hidden on the
first-launch empty state, where no agent can run; the entry carries the
workspace-wide max badge (accent spinner = working, green dot = a finished turn). Opening it sets
the central area to the dashboard while the **project sidebar stays** (the entry
highlighted); the per-repo git panel is **hidden** (the view is cross-repo).

The dashboard is a **wall of live terminals** — its only view, with no control of
its own in the titlebar (the title row stays empty, clear of the traffic lights): a
**header strip** listing every running agent as a **chip**, over the mirrored
terminals of the ones picked from it. A chip carries the agent's **state indicator**
(accent arc spinner / green dot + faint static halo / hollow grey ring) and **where it
runs** — the **project leading**, in the chip's own weight, its branch trailing in
quieter mono, because the dashboard is cross-repo and two agents on `main` must read
apart. The agent's **name is not painted**: a strip of identical `Claude` labels drowns
the one thing that identifies a terminal; it rides on the chip's **hover text** with the
tab, and on the accessibility label. Clicking a chip **shows** that agent's terminal on the wall, or
**hides** it when it is already there; a chip whose agent is on the wall is filled
in its project's hue. At most **`MAX_SHOWN` = 4** terminals are shown at once —
past that the remaining chips read **disabled** and say so on hover, since a fifth
pane would leave none of them watchable; hiding one is how room is made. The strip
wraps onto further lines and scrolls past three of them, so a workspace full of
agents never eats the wall.

The wall is laid out by the **terminal's own split tree**
([`terminal.md`](terminal.md) §5), not by a layout of its own, so it behaves
exactly like a workspace tab: **drag a seam** to resize, **drag a tile's grip**
(top-right, on hover) onto another to re-split on that edge or **swap** on its
centre, and the **focus / resize chords** ([`keybindings.md`](keybindings.md) §2)
drive it — split and close are not routed, a tile mirrors an agent the tree
neither creates nor kills. Showing an agent **splits the roomiest tile across its
longer axis**, so one fills the wall, two sit side by side, and a third and fourth
subdivide the youngest region (the tiles already placed keep their spot, and a wall
the user resized or rearranged keeps its shape). Hiding one gives its room to its
sibling. The composition is **session state**, deliberately not persisted: an agent
key only means something while its pane runs, and the wall drops the tile of an
agent that stopped running. Opening the dashboard **seeds** the wall with the agent
the page selects anyway (Working > Done > Idle, ties by workspace order), so the
view never opens on an empty grid; a wall the user then empties **stays** empty for
the rest of the visit, with a hint pointing back at the header.

Each tile is a **status band** over its pane, flush and full-bleed — no card frame,
no gap: the band carries the state indicator, `project · branch` (the project first
and firmest, as on a chip), the tab, the state caption (*Working…* / *Finished Nm ago* / *Idle*) and a discreet
**jump icon** (external-link, clear of the grip's corner), and it wears the
project's hue — firmest on the tile the keyboard drives, lifting under the pointer.
The pane below it is the **same terminal widget as the workspace**, mirrored live
and fully interactive (read / scroll the scrollback, type a reply,
`Esc`-as-interrupt). Only the **selected** tile is *active* — the single
`selected_agent` that owns the keyboard focus lock; every other pane recedes behind
the **split-unfocused dim** it applies to itself, so no extra mark is needed on the
wall. Clicking a band or a pane selects that tile; the jump icon **focuses** that
pane in its workspace instead — activates the repo, its tab, sets pane focus,
switches back to the terminal. The selection is a stable `(repo, tab, pane)` triple
that survives the per-frame agent-list rebuild; if its tab/pane closes it is dropped
and the **most urgent** agent re-picked (Working > Done > Idle, ties by workspace
order).

Leaving the dashboard: picking any project, or `Esc` — **except** when a mirrored
terminal holds keyboard focus, where `Esc` reaches the agent as an interrupt
(the dashboard stays). Because the page stays `Main`, the regular worker drain
keeps the watch live; viewing the dashboard does **not** acknowledge greens (focus
gating excludes `CentralMode::Agents`).

## 6. Out of scope for v1

Configurable watchlist in Preferences; agents under tmux; in-app
notification-center history.
