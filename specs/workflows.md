# helm — Workflows (chained Claude Code steps)

A **workflow** is a saved sequence of steps that helm plays into one Claude Code
session: fill a few inputs, launch, and helm sends step 1, waits for Claude to
finish it, sends step 2 into the **same** session, and so on. Use cases: a
pre-configured dev → review → fix cycle, a ticket kick-off, a recurring audit.
Module: `workflow` (domain) + `app::workflow` (wiring) + `ui::workflow_*` (render).

## 1. Scope (v1)

- **Agent**: Claude Code only. Another agent later adds an `agent` field
  defaulting to `claude` — existing definitions stay valid.
- **Linear**: steps run in order, no branches, no loops. Loops (repeat until a
  marker) come later, on a real need — the end marker (§5) is their foundation.
- **Step types**: **Prompt** and **Gate** (§4). No shell step, no compact/clear step.
- **Not persisted across launches**: a run dies with its tab (§8).

## 2. Definitions — global, edited in Preferences

Workflows are **global** (every project lists them), stored in helm's prefs and
edited in a **Workflows** section of Preferences ([`preferences.md`](preferences.md)).
No file to hand-edit, no per-repo definition.

```
Workflow { name, inputs: [Input], steps: [Step] }
Input    { key, label, default?, kind: Text | LongText | Select { options } }
Step     { title, kind: Prompt { text } | Gate { message } }
```

An input `key` is `[a-z0-9_]+`, unique, and not a built-in name. A placeholder is
`{key}` with that same shape — any other brace (code, JSON) is plain text.

**Editor** — a structured form, never raw text:
- List of workflows: add, rename, duplicate, delete (behind a confirmation).
- A workflow's detail: name; **Inputs** list (key, label, kind, options for a
  Select, default) — add / remove / reorder; **Steps** list (title, kind, then a
  multiline prompt field like *Commit message prompt*, or the gate's message) —
  add / remove / reorder.
- **Validation inline**, a workflow with an error cannot be launched:
  - a blank name, an empty workflow (no step), a blank prompt;
  - an input key malformed, duplicated or reserved (`branch`, `worktree`, `repo`);
  - a `{placeholder}` (prompt or gate message) naming neither an input nor a built-in;
  - a Select with no option, or a default that is not one of its options.
- **Warning** (not an error): a Prompt starting with `/compact` or `/clear` — those
  built-ins end no model turn, so the step would never finish (§5).
  Custom slash commands and skills (`/review`) are model turns and work.

## 3. Launch

- `Cmd+P` → **Launch workflow ›** (sub-screen, listed when the active worktree
  exists and at least one valid workflow exists) → one row per **valid** workflow
  (name + step count); an invalid one stays in Preferences until fixed.
- `Enter` opens the **inputs modal**: one field per input, in order, defaults
  pre-filled; every input is required. `Enter` in a single-line field / `Cmd+Enter`
  anywhere launches once every input is filled — until then `Enter` moves to the next
  field; `Esc` cancels. The whole form works from the keyboard (`Tab`, a select opens
  on `Enter`/`Space`), and the terminal behind never takes the focus back while the
  modal is open. A workflow with no input skips the modal.
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

On each `Stop` of the run's session, helm reads Claude's **last message** — the
hook input's `last_assistant_message` (absent ⇒ treated as no marker):

| Last message | Effect |
|---|---|
| contains `[[STEP_DONE]]` | step done → next step is sent (or the run is **Done**) |
| no marker | run → **Awaiting reply**: Claude asked something or stopped short |

**Awaiting reply** clears by itself: the user answers in the TUI, the next `Stop`
is tested again. If Claude finished but forgot the marker, **Next step** forces
the move.

**Permissions**: Claude runs with the user's own settings — helm adds no
`--permission-mode`. When Claude blocks on a permission, its **`Notification`
hook** (`notification_type` = `permission_prompt`; other types are ignored — an
`idle_prompt` only repeats Awaiting reply) puts the run in **Needs you** and posts a native
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
and replies `{"result":"accepted"}` at once — unknown run included. The UI reads the runs' state to draw the strip.

**Sending a step.** Steps 2+ are written as a **bracketed paste** (`PaneHandle::paste`)
then `\r` — multi-line prompts stay one message. Claude runs its `Stop` hook
synchronously, so the paste waits ~1 s for the hook to return and the prompt to be
back; it happens on its own thread, never on the socket's.

**Trust.** The socket gains one verb whose effect is bounded to a run helm created:
an unknown run id is ignored. Event text is never executed; the only thing helm
ever types into the pane is the next prompt of the definition.

## 7. The workflow strip

A pill at the right end of the workflow tab's **tab bar row** — it takes no height
from the terminal, so the PTY never resizes when a run starts or is dismissed. Left
to right: a **step track** (one dot per step, max 10: done filled, upcoming hollow,
the current one in the run's hue — the agent pinwheel while Running), **name**,
step **title**, **state** (muted while Running, else in the run's hue), the Gate's
message, then the actions as round icon buttons (Continue = filled accent). The
pill tints in `git.modified` while it waits on the user, `git.added` once Done.

| State | Meaning | Actions |
|---|---|---|
| Running | a Prompt step is in progress | Stop |
| Awaiting reply | `Stop` without marker | Next step · Stop |
| Needs you | `Notification` received | Next step · Stop |
| Gate | paused on a Gate step (its message shown) | Continue · Stop |
| Done | last step completed | Dismiss |
| Stopped | user stopped it, or Claude exited | Dismiss |

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
- A `Stop` delivered twice for the same turn — the run advances once: the hook's
  `prompt_id` identifies the turn, and the turn that advanced a step is never
  applied again.
- A Permission prompt or **Next step** while on a Gate is ignored; **Next step** while
  Running too (it only forces a run waiting on the user). **Next step** on the last
  step finishes the run (**Done**).
- Hidden helm: events still advance the run (§6); the strip catches up on the next
  frame.
- **Untrusted folder**: Claude's trust dialog comes first, its default choice being
  *No, exit*. helm never answers it — the user does, in the tab. *Yes* ⇒ step 1
  (the initial prompt) starts as usual; *No* ⇒ Claude exits ⇒ **Stopped**.
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
