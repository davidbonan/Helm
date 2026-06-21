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

## Rules
- No overengineering: no speculative abstraction, readable local code.
- Always re-read and simplify.
- Simple, readable, extensible.
