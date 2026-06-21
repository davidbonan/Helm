---
name: implement-state
description: >-
  Implements the next task from specs/plan/STATE.md (or continues the in-progress
  task), end to end through the Definition of Done, then updates the progress
  state. Reads specs/plan/STATE.md + README.md to pick the task, implements the
  minimum (domain isolated from rendering), writes and runs the relevant tests
  (cargo fmt + clippy -D warnings + test), then checks the task off in STATE.md.
  ONE task per invocation. Optional argument: a task ID or selector from STATE.md.
argument-hint: "[task ID or selector — optional]"
---

# implement-state

Advances helm by **exactly one task** from [`specs/plan/STATE.md`](../../../specs/plan/STATE.md),
end to end through the *Definition of Done*, then updates the state.

- **Without an argument** → continue the **in-progress** task (`◐`), otherwise take the
  **next** task to do per `STATE.md`.
- **With an argument** → match a task ID or selector from `STATE.md`; confirm if
  several tasks match.

> **One task per invocation.** This is intentional: small verifiable diffs (AGENTS.md
> *Surgical Changes*), a full test+verify+tracking cycle every time. To chain them,
> re-run the skill.

## The source of truth is `specs/plan/`

The **per-task workflow** and the **Definition of Done** are defined in
[`specs/plan/README.md`](../../../specs/plan/README.md) — this skill **executes** them, it
does not reinvent them. In case of divergence, `README.md` is authoritative.

## Procedure

### 1. Read the state

Read in order:
1. `specs/plan/STATE.md` — active milestone, in-progress task (`◐`), task cards,
   "Next actions", blockers/pending decisions.
2. `specs/plan/README.md` — status legend, per-task workflow, *Definition of Done*,
   golden rules.

### 2. Choose the task

Priority order:
1. **Argument provided** → matching task ID or selector. If ambiguous, confirm the
   chosen task before coding.
2. Otherwise, **in-progress task** `◐` in `STATE.md` → continue it.
3. Otherwise, first task `☐` from **"Next actions"**.
4. Otherwise, first task `☐` in checklist order.

Then **validate feasibility** before coding:
- The task is not `⊘`/`⏭`. If `⊘`, read the reason in §Blockers.
- Any dependency declared in the `STATE.md` task card is already `☑`. Otherwise:
  do not force it — flag the missing dependency, propose handling it first,
  and **stop** (see *When to stop*).
- The task does not hinge on a **"to be decided" decision**. If it does → ask the
  user, **do not guess**.

### 3. Frame and mark `◐`

- Read the task card in `STATE.md` (target files, acceptance criteria, required
  tests) **and** the source spec it references (`specs/*.md`).
- Restate in one sentence: goal, files touched, acceptance criteria, target test
  levels.
- In `STATE.md`: set the task to `◐`, fill in the in-progress field. That way, if
  the session is interrupted, the next invocation resumes here.

### 4. Implement the minimum

Follow AGENTS.md and `specs/architecture.md`:
- **Domain isolated from rendering** (§1): business logic in `git`/`terminal`/`workspace`/
  `theme`/`persistence`, `pub` from the lib; rendering = `fn(&mut egui::Ui, …)` functions
  drivable by kittest (`specs/testing.md` §5).
- **Threads** (§3): PTY I/O and libgit2 calls off the UI thread; grid under a short lock;
  `crossbeam-channel` channels.
- *Simplicity First* / *Surgical Changes*: the minimum that satisfies the criteria, no
  speculative abstraction, do not refactor adjacent code.
- **No hardcoded hex** outside `theme`/`terminal::palette`. **No** comment that
  paraphrases the code.
- **Locked decisions** (`specs/overview.md` §4) preserved; any deviation →
  justified in the commit message.

### 5. Write the tests

At the relevant levels listed in the task card (`specs/testing.md`):
- `U` unit (pure logic, `#[cfg(test)]` in the module),
- `Eb` business e2e (`tests/<domain>_e2e.rs`, real throwaway resource),
- `Eu` UI e2e (`tests/`, egui_kittest on the render function).

The tests must exercise the task's **acceptance criteria**.

### 6. Verify — the *Definition of Done* gate

A green bar is **mandatory** before checking `☑` (AGENTS.md *Evidence Before
Conclusion*). Run:

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

For a **visible UI / keyboard / git** change, additionally run the
**`headless-verify`** skill (capture + a11y tree in a timestamped subfolder
`verify-artifacts/<timestamp>/`, one per run) to prove the behavior in the real
app.

If a command fails → **do not check `☑`**. Fix it; if the blocker is real,
leave the task `◐` and go to *When to stop*.

### 7. Update the tracking

Once green:
- **`STATE.md`**:
  - task → `☑` (checklist + any "Next actions");
  - recompute the milestone counter if present;
  - update the in-progress field and "Next actions".
  - If **all** the milestone's tasks are `☑`/`⏭` → mark the milestone `☑` and verify
    its demo scenario via `cargo test` or `headless-verify`.

> Modify product specs only when the scope genuinely changes; justify such a change
> in the commit message.

### 8. Report

- Task handled (ID/title) and final status (`☑` or `◐` + reason).
- Files touched (diff summary).
- **Evidence**: `cargo test` result (count), clean clippy, path of the HV
  session folder (`verify-artifacts/<timestamp>/`) when applicable.
- Recommended next action (see `STATE.md`).

## When to stop and ask (do not guess)

Leave the task `◐` (or `⊘` with the reason in `STATE.md` §Blockers), then
**ask the user** if:
- a dependency is not `☑`;
- a **"to be decided" decision** is blocking (e.g. crate to choose);
- the acceptance criteria are **ambiguous** or several interpretations exist
  (AGENTS.md *Understand Before Coding*);
- the task would require **deviating** from a locked decision or touching many
  files / shared code.

## Guardrails

- **A single task** per invocation — do not silently chain the whole milestone.
- **Never `☑` without evidence** (green tests). No evidence ⇒ `◐`.
- **Minimal and surgical** diff; do not clean up/refactor adjacent code.
- Respect the **domain ↔ rendering** boundary and the threading model (architecture §1, §3).
- Keep `STATE.md` **in sync** with the reality of the code on every run.
