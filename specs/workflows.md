# helm — Workflows (chained Claude Code steps)

A **workflow** is a saved sequence of steps that helm plays into one Claude Code
session: fill a few inputs, launch, and helm sends step 1, waits for Claude to
finish it, sends step 2 into the **same** session, and so on. Use cases: a
pre-configured dev → review → fix cycle, a ticket kick-off, a recurring audit.
Module: `workflow` (domain) + `app::workflow` (wiring) + `ui::workflow_*` (render).

## 1. Scope (v1)

- **Agent**: Claude Code only. The definition carries an `agent` field (only
  `claude` accepted) so another agent costs an adapter, not a schema change.
- **Linear**: steps run in order, no branches, no loops. Loops (repeat until a
  marker) come later, on a real need — the end marker (§5) is their foundation.
- **Step types**: **Prompt** and **Gate** (§4). No shell step, no compact/clear step.
- **Not persisted across launches**: a run dies with its tab (§8).

## 2. Definitions — global, edited in Preferences

Workflows are **global** (every project lists them), stored in helm's prefs and
edited in a **Workflows** section of Preferences ([`preferences.md`](preferences.md)).
No file to hand-edit, no per-repo definition.

```
Workflow { id, name, agent: claude, inputs: [Input], steps: [Step] }
Input    { key, label, kind: Text | LongText | Select { options }, default? }
Step     { title, kind: Prompt { text } | Gate { message } }
```

**Editor** — a structured form, never raw text:
- List of workflows: add, rename, duplicate, delete (behind a confirmation).
- A workflow's detail: name; **Inputs** list (key, label, kind, options for a
  Select, default) — add / remove / reorder; **Steps** list (title, kind, then a
  multiline prompt field like *Commit message prompt*, or the gate's message) —
  add / remove / reorder.
- **Validation inline**, a workflow with an error cannot be launched:
  - an empty workflow (no step), a blank prompt, a duplicate input key;
  - a `{placeholder}` that names neither an input nor a built-in variable;
  - a Select with no option.
- **Warning** (not an error): a Prompt starting with `/compact` or `/clear` — those
  built-ins end no model turn, so the step would never finish (§5).
  Custom slash commands and skills (`/review`) are model turns and work.

## 3. Launch

- `Cmd+P` → **Launch workflow ›** (sub-screen, listed when the active worktree
  exists and at least one valid workflow exists) → one row per workflow.
- `Enter` opens the **inputs modal**: one field per input, in order, defaults
  pre-filled; every input is required. `Enter` in a single-line field / `Cmd+Enter`
  anywhere launches; `Esc` cancels. A workflow with no input skips the modal.
- **Templating**: `{key}` is replaced by the input's value; built-ins `{branch}`,
  `{worktree}` (absolute path), `{repo}` (project name) come from the active
  worktree. Single braces, like `{changes}` in the commit prompt. Rendering happens
  **once, at launch**: the whole run's prompts are fixed before step 1 is sent.
- Launching opens a **new tab** in the active worktree, titled with the workflow's
  name, and spawns Claude there (§6). The tab is the run: one run per tab, several
  runs in parallel across tabs and worktrees.

## 4. Step types

- **Prompt** — sends the rendered text to Claude, followed by helm's **end-marker
  instruction** (not editable, appended like the commit prompt's reply format):
  *"When this step is complete, end your final message with `[[STEP_DONE]]`."*
- **Gate** — sends nothing. The run pauses on the gate's message; **Continue**
  moves to the next step, **Stop** ends the run. Use: read the dev step's result
  before the review step starts.

## 5. Step completion

A Prompt step ends on Claude's **`Stop` hook** (§6) — never on the output-silence
heuristic of [`agents.md`](agents.md), which arms mid-turn on a silent tool call
> 6 s and would send the next step while Claude still works.

On each `Stop` of the run's session, helm reads Claude's **last message** (from the
hook input, else the last assistant entry of `transcript_path`):

| Last message | Effect |
|---|---|
| contains `[[STEP_DONE]]` | step done → next step is sent (or the run is **Done**) |
| no marker | run → **Awaiting reply**: Claude asked something or stopped short |

**Awaiting reply** clears by itself: the user answers in the TUI, the next `Stop`
is tested again. If Claude finished but forgot the marker, **Next step** forces
the move.

**Permissions**: Claude runs with the user's own settings — helm adds no
`--permission-mode`. When Claude blocks on a permission or waits for input, its
**`Notification` hook** puts the run in **Needs you** and posts a native
notification (`notify`, gated by *Agents → Completion notifications*). The next
`Stop` leaves it.

Typing in the workflow's pane is always allowed: it is a normal Claude turn, and
its `Stop` is tested like any other.

## 6. Mechanics

**Spawn.** helm writes a per-run settings file (support dir,
`workflows/<run-id>.json`) declaring two hooks — `Stop` and `Notification` — whose
command is **helm's own binary** (`std::env::current_exe()`, so no `PATH`
dependency and a dev build talks to itself) with `workflow-event`. The tab runs,
through the login shell (`pty::run_command`):

```sh
HELM_WORKFLOW_RUN=<run-id> claude --settings <file> "<step 1 rendered>"
```

Step 1 travels as Claude's initial prompt: no race with the TUI's readiness.
`--settings` scopes the hooks to this session — the user's `~/.claude` is never
written. The file is removed when the run ends.

**Events.** `helm workflow-event <stop|notification>` reads the hook's JSON on
stdin, takes the run id from `HELM_WORKFLOW_RUN`, and sends one line on the control
socket ([`cli.md`](cli.md) §9): `{"op":"workflow_event","run":…,"event":…,
"session_id":…,"last_message":…}`. It always exits `0` and fast: a hook must never
block or fail Claude's turn.

**Off the UI thread.** Unlike `helm run`, this op is **not** parked for a frame: a
hidden or minimized helm draws no frame (cli.md §9), and a workflow left running
while the user is away is the main case. The socket thread hands the event to the
**workflow runner**, which owns each run's state and its pane's `PaneHandle` — the
write path the phone already uses off the UI thread ([`remote.md`](remote.md) §4) —
and replies `ok` at once. The UI reads the runs' state to draw the strip.

**Sending a step.** Steps 2+ are written as a **bracketed paste** (`PaneHandle::paste`)
then `\r` — multi-line prompts stay one message.

**Trust.** The socket gains one verb whose effect is bounded to a run helm created:
an unknown run id is ignored. Event text is never executed; the only thing helm
ever types into the pane is the next prompt of the definition.

## 7. The workflow strip

A thin bar on top of the workflow tab's terminal (same family as the Run strip,
[`design-system.md`](design-system.md)): **name · step n/N — title · state** and
the actions that apply.

| State | Meaning | Actions |
|---|---|---|
| Running | a Prompt step is in progress | Stop |
| Awaiting reply | `Stop` without marker | Next step · Stop |
| Needs you | `Notification` received | Next step · Stop |
| Gate | paused on a Gate step (its message shown) | Continue · Stop |
| Done | last step completed | — (strip stays, dismissible) |
| Stopped | user stopped it, or Claude exited | — (dismissible) |

The same actions appear in `Cmd+P` while the active tab holds a run (**Continue
workflow**, **Next workflow step**, **Stop workflow**). The run's end posts a
native notification *"Workflow finished"* — `repo · branch · name`. The
agent badge of the sidebar ([`agents.md`](agents.md)) is unchanged.

**Stop** ends the run, not the session: Claude stays open in the tab, the user can
keep talking to it.

## 8. Lifecycle

- **Closing the tab** = Stop (no confirmation, like any tab).
- **Quitting helm** abandons every run (PTYs are not restorable,
  [`architecture.md`](architecture.md)). The Claude session itself stays resumable
  by hand (`claude --resume`); the run does not.
- **Claude exits** (`/exit`, crash) → run **Stopped**.
- **Editing a definition** during a run does not affect it (prompts rendered at
  launch, §3).

## 9. Edge cases

- `[[STEP_DONE]]` quoted mid-message (Claude explaining the marker) still counts —
  accepted; the instruction asks for it at the end.
- A `Stop` arriving while the run is on a Gate or Done is ignored.
- Two `Stop` before helm sent the next step (double hook delivery) — the runner
  advances once per step: an event is applied to the step it was emitted for.
- Hidden helm: events still advance the run (§6); the strip catches up on the next
  frame.
- Claude not installed / not on the login shell's `PATH`: the tab shows the shell's
  error, the run goes **Stopped** when the process exits.

## 10. Tests

- **Unit** — definition validation, template rendering (inputs, built-ins, unknown
  placeholder), the run state machine (every event × state), marker detection, the
  settings file content, `workflow-event` argv + stdin parsing.
- **Business e2e** — a fake `claude` script that records its argv, then fires the
  hook commands from its settings file: launch → step 1 in argv, `Stop` + marker →
  step 2 pasted into the PTY, no marker → Awaiting reply, notification → Needs you.
- **UI e2e (kittest)** — palette sub-screen + inputs modal, the strip in each state,
  the Preferences editor (add / reorder / validation messages).
- **By hand** — a real Claude Code run of 3 steps with a gate, helm hidden during
  step 2.
