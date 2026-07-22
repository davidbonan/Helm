# helm

Native **macOS** development workspace in **Rust**: terminal (Ghostty-style
keyboard splits), left sidebar (navigation between git repositories), right
sidebar (git status: unstaged / staged / commit). Target aesthetic: Codex Desktop.

UI locked on `eframe`/`egui` (overview §4). Dependencies in place (`eframe`,
`egui`, `git2`, `portable-pty`). **lib + bin** architecture: `src/lib.rs`
(testable modules) + `src/main.rs` (`eframe` wrapper). Test loop operational at
3 levels (unit / business e2e / UI e2e) — see `specs/testing.md`.

## Documentation
- `specs/overview.md` — goal, 3-zone layout, locked decisions, specs index.
- `specs/architecture.md` — modules (DDD), threads, data flow, persistence, dependencies.
- `specs/testing.md` — feedback loop: 3 levels of tests (unit, business e2e, UI e2e egui_kittest).
- `specs/terminal.md` — PTY, emulation, splits, focus, scrollback, ANSI palette.
- `specs/git.md` — status, hunk/line staging, diff, commit, branch indicator, refresh.
- `specs/conflicts.md` — in-app conflict editor: 3 zones, take checkboxes, resolve + Continue, fallbacks.
- `specs/worktrees.md` — worktrees grouped in the sidebar: root resolution, discovery/purge, Delete worktree.
- `specs/agents.md` — AI agent detection in terminals: sidebar activity badge (states, heuristic, limits).
- `specs/pull-requests.md` — workspace PR cockpit: sidebar entry below Agents, GitHub (`gh`) + Bitbucket Cloud, list/detail/checkout.
- `specs/preferences.md` — full-window Preferences page: left nav + settings cards.
- `specs/update.md` — distribution (.app bundle, GitHub releases) + integrated app update.
- `specs/cli.md` — `helm <path>` + the `helm://` scheme: opening a project/worktree from outside, single instance.
- `specs/keybindings.md` — complete keybinding reference.
- `specs/design-system.md` — tokens (colors / typography / spacing) + components.

## Implementation tracking — `specs/plan/`
Progress state is maintained from session to session. **At the start of a dev
session, read `specs/plan/STATE.md`** (where things stand, next actions).
Conventions, *Definition of Done*, and **concurrency rules** (parallel workers):
`specs/plan/README.md`. **After each task**, update `STATE.md` (**pure** status);
the "why" behind implementation decisions lives in the commit messages. The
`specs/*.md` freeze the product intent; `specs/plan/` tracks the execution.

## Commands
```sh
cargo run                    # Compile and launch the binary
cargo build                  # Compile (debug)
cargo check                  # Check without producing a binary
cargo test                   # Run the tests
cargo fmt                    # Format (rustfmt)
cargo clippy -- -D warnings  # Strict lint (CI)
```

## Code Style
- **Clean Code** + **DDD**: isolate the business logic (git, terminal/PTY, split
  tree) from the UI rendering.
- **Tests**: 3-level loop (`specs/testing.md`) — unit on pure logic,
  business e2e (real git repo / PTY), headless UI e2e via `egui_kittest`. All
  testable logic must be `pub` from the lib; rendering = functions
  `fn(&mut egui::Ui, …)` drivable by kittest.

## Context discipline (exploration)
- **Delegate multi-file surveys to a subagent** (`Explore` / `general-purpose`):
  a "how does X wire across modules" question returns a conclusion, not file
  dumps — keeps large sources out of the main context.
- **Read targeted ranges, never whole large files.** Rust test modules sit at the
  end under `#[cfg(test)]`: read `1..<tests>` when you only need the API surface
  (e.g. `workspace.rs` 1..449, skips ~760 test lines).
- **Prefer LSP over grep→read** for navigation (`documentSymbol`,
  `goToDefinition`, `findReferences`): `rust-analyzer-lsp` is enabled for this
  project. The LSP tool is deferred — `ToolSearch("LSP")` to load its schema.
- **Batch greps** in one call; use `rg -e a -e b`, not `'a|b'` (the rtk hook
  mangles unescaped alternation).

## Rules
- No overengineering: no speculative abstraction, readable local code.
- Always re-read and simplify.
- Simple, readable, extensible.
- When, before coding, an in-depth refactor would improve the code and ease the
  feature's implementation, prefer doing that refactor first.
