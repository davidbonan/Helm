---
name: implement-milestone
description: >-
  Implements an entire milestone from specs/plan/STATE.md inside a dedicated git
  worktree. Creates a worktree for the milestone, then runs the implement-state
  skill task by task in the worktree until every task of the milestone is ☑/⏭,
  committing after each green task. Stops and reports on the first blocker
  instead of forcing through. Never merges back automatically. Optional
  argument: a milestone ID (e.g. M24); defaults to the active milestone.
argument-hint: "[milestone ID, e.g. M24 — optional]"
---

# implement-milestone

Advances helm by **one full milestone** from [`specs/plan/STATE.md`](../../../specs/plan/STATE.md),
in an isolated worktree, by chaining the **`implement-state`** skill — one
invocation per task, exactly as if it had been run by hand for each task.

- **Without an argument** → the milestone of the in-progress task (`◐`),
  otherwise the first milestone with `☐` tasks in `STATE.md` order.
- **With an argument** → the matching milestone ID (e.g. `M24`). Error out if it
  does not exist or has no remaining `☐`/`◐` task.

> `implement-state` stays "one task per invocation"; this skill is the explicit,
> user-requested way to chain it across a milestone. The chaining lives here,
> never inside `implement-state`.

## Procedure

### 1. Read the state and validate the milestone

Read `specs/plan/STATE.md` + `specs/plan/README.md` (statuses, per-task
workflow, *Definition of Done*) from the **main** checkout, then pick the
milestone as above and validate before creating anything:

- It has at least one `☐`/`◐` task.
- No task is `⊘`, hinges on a **"to be decided" decision**, or depends on a
  task outside the milestone that is not `☑`. If so → **stop and ask**, do not
  create the worktree.

### 2. Create the worktree

Create a git worktree dedicated to the milestone, on its own branch. If a
worktree for this milestone already exists (interrupted previous run), reuse it
instead of failing: resume from the worktree's `STATE.md`.

### 3. Worktree discipline

From this point on, **everything happens in the worktree path**:

- every file read/edit (`src/`, `tests/`, `specs/plan/STATE.md`),
- every command (`cargo fmt` / `clippy` / `test`, `headless-verify`),
- every commit.

Never modify the main checkout while the loop runs. The `STATE.md` that gets
updated is the **worktree's copy**; it reaches `main` with the merge.

### 4. Loop — one `implement-state` per task

For each remaining task of the milestone, in `STATE.md` checklist order:

1. Invoke the **`implement-state`** skill with the task ID as argument,
   stating explicitly that all work happens in the worktree path (reads,
   edits, tests, `STATE.md` update).
2. When the task comes back `☑` (green gate per the *Definition of Done*),
   **commit in the worktree**: one commit per task, message explaining the
   "why" of non-trivial decisions (plan/README.md §Concurrency).
3. Move to the next task.

**Stop the loop** (and go to *Report*) as soon as an invocation ends `◐`/`⊘`
or raises a question (ambiguous criteria, missing dependency, pending
decision). Leave the worktree and its commits in place — the run is resumable.

### 5. Close the milestone

When all the milestone's tasks are `☑`/`⏭`:

- Mark the milestone `☑` in the worktree's `STATE.md` and verify its **demo
  scenario** via `cargo test` or `headless-verify` (plan/README.md *Definition
  of Done* — milestone), then commit that final state update.
- **Do not merge into `main` and do not push** — that is the user's call.

### 6. Report

- Milestone (ID/title) and final status: completed, or stopped at task X + reason.
- Tasks handled with their statuses, and the commits created (one line each).
- Worktree path and branch name.
- Evidence of the milestone gate (test count / `headless-verify` artifacts).
- Suggested next step: review then merge the worktree's branch into `main`,
  or resume with `/implement-milestone M<NN>` after unblocking.

## Guardrails

- **One milestone per invocation**; tasks strictly sequential — never two
  `implement-state` in parallel in the same worktree.
- Every task goes through `implement-state` unchanged: its *Definition of Done*,
  evidence requirements, and stop conditions apply as-is. No `☑` without a
  green gate.
- First blocker ⇒ stop and ask; never skip a task to "finish" the milestone
  (use `⏭` only with the user's explicit go-ahead).
- The worktree is the only place touched; `main` stays clean until the user
  merges.
