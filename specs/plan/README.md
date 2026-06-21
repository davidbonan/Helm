# specs/plan — Implementation tracking

This folder is the working memory for building helm. It turns the product specs
(`specs/*.md`, product intent) into a compact **progress state** maintained from
session to session.

> The `specs/*.md` files describe **what we want**. This folder describes
> **where we are** and **what to do next**.

## Files

| File | Role | Volatility |
|---------|------|-----------|
| `README.md` (this file) | Conventions: statuses, per-task workflow, *Definition of Done*. | Stable |
| [`STATE.md`](STATE.md) | **Living dashboard**: active milestone, task cards/checklist, next actions, blockers. **Source of truth for progress.** | Updated every session |

`STATE.md` carries the active plan. History (the "what" and the "why") is
carried by `git log`, not by a changelog.

## Status legend

| Symbol | Meaning |
|---------|------|
| `☐` | To do |
| `◐` | In progress |
| `☑` | Done **and verified** (green tests, see DoD) |
| `⊘` | Blocked (reason + reference in STATE §Blockers) |
| `⏭` | Deferred / out of current scope (with justification) |

## Task identifiers

Use stable, human-readable IDs chosen in `STATE.md`. Never reuse or renumber an
ID once work has started.

## Concurrency (multiple workers in parallel)

- **`STATE.md`** → the only progress file rewritten; keep it **terse** (checkbox +
  counters, no narrative) so edits land on disjoint lines and auto-merge.
  Implementation detail lives in the **commit message**, not in `STATE.md`.

## Per-task workflow

For each task taken from `STATE.md`:

1. **Read** the task card in `STATE.md` (target files, acceptance criteria,
   required tests) and the referenced source spec.
2. **Implement the minimum** that satisfies the acceptance criteria (CLAUDE.md:
   *Simplicity First*, *Surgical Changes*). Business logic goes in the domain
   (`git` / `terminal` / `workspace` / `theme` / `persistence`), `pub` from the lib;
   rendering stays as `fn(&mut egui::Ui, …)` functions (testing.md §5).
3. **Test** at the relevant levels (testing.md): unit on pure logic,
   business e2e on a real resource, UI e2e kittest on a component.
4. **Verify** (a green bar is mandatory before checking `☑`):
   ```sh
   cargo fmt
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
   For a visible UI/keyboard/git change: also run the
   `headless-verify` skill (screenshot + accessibility tree in a timestamped subfolder
   `verify-artifacts/<timestamp>/`, one per run).
5. **Update the tracking**: `STATE.md` — move the task to `☑`, recompute the
   milestone counter if present, update "Next actions". **Status only** — no
   narrative (cf. *Concurrency*). The "why" of a non-trivial implementation
   decision goes in the **commit message**.

## Definition of Done

**Task `☑`**: acceptance criteria met · tests at the relevant levels written
and **green** · `cargo clippy -D warnings` clean · no dead code or leftover TODO ·
domain isolated from rendering (CLAUDE.md/architecture §1) · `STATE.md` up to date.

**Milestone `☑`**: all its tasks `☑` (or `⏭` justified) · the milestone scenario
is demonstrable via `cargo test` or `headless-verify`.

## Golden rules (CLAUDE.md reminder)

- No speculative abstraction; a single **lib + bin** crate as long as no real
  need forces multi-crate (architecture §1).
- Every non-trivial conclusion rests on **evidence** (test, output, screenshot) —
  never on plausibility. No evidence ⇒ status `◐`/`⊘`, not `☑`.
- Preserve the **locked decisions** of the specs (overview §4); any deviation
  is justified in the commit message.
