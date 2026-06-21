# helm — Tests & feedback loop

How to verify a change end to end, without launching the window by hand.
Aligned with [`CLAUDE.md`](../CLAUDE.md) (DDD: domain isolated from rendering)
and with [`architecture.md`](architecture.md) §6.

## 1. Three levels

| Level | Target | Where | Example |
|--------|-------|----|---------|
| **Unit** | pure logic (no I/O) | `#[cfg(test)]` within the module | `src/git/status.rs` → `classify` |
| **Business e2e** | domain against a real resource (FS, git repo, PTY) | `tests/it/*.rs` | `tests/it/git_status_e2e.rs`, `tests/it/pty_e2e.rs` |
| **UI e2e** | egui rendering driven headless | `tests/it/*.rs` (egui_kittest) | `tests/it/ui_git_panel.rs` |

Everything runs under `cargo test` — including the UI, which executes **without
a window** (egui_kittest renders egui in memory). No manual step for the loop.

## 2. Structural prerequisite: lib + bin split

Integration tests (`tests/`) only see the **public API of a library crate**.
A `[[bin]]`-only crate exposes nothing. Hence:

- `src/lib.rs` — declares the modules (`app`, `git`, `ui`, …), this is the tested surface.
- `src/main.rs` — thin wrapper: `helm::app::run()`.

Everything that must be tested in e2e must therefore be `pub` from the lib.

## 3. Unit level — pure logic

Within the module, without I/O. The business logic must be extracted into pure
functions (input → deterministic output) to stay testable this way.

```rust
// src/git/status.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_change_is_staged_only() {
        let s = classify(git2::Status::INDEX_MODIFIED);
        assert_eq!(s, Sections { staged: true, unstaged: false });
    }
}
```

## 4. Business e2e level — real resource

One file per domain in `tests/`. We set up a real disposable resource, call the
public API, and assert.

**Git** — real repository in a temporary folder (`tempfile`):

```rust
// tests/git_status_e2e.rs
let tmp = tempfile::tempdir().unwrap();
git2::Repository::init(tmp.path()).unwrap();
std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
let st = helm::git::status::load(tmp.path()).unwrap();
assert!(st.unstaged.iter().any(|f| f.path == "a.txt"));
```

**Terminal** — real PTY (`portable-pty`), pattern for the emulation:

```rust
// tests/pty_e2e.rs — we launch a command, we read what it writes to the PTY
```

`tempfile::tempdir()` removes itself at the end of the test: no residue, tests
parallelizable.

## 5. UI e2e level — egui_kittest

The rendering must be a **free function** taking `&mut egui::Ui` (not a piece
of `eframe::App`), so that kittest calls it directly:

```rust
// src/ui/git_panel.rs
pub fn git_panel(ui: &mut egui::Ui, status: &RepoStatus, intents: &mut Vec<GitIntent>)
```

The test renders this component headless, queries the accessibility tree,
simulates a click, then verifies the effect (here: the emitted intent):

```rust
// tests/ui_git_panel.rs
let intents = Rc::new(RefCell::new(Vec::new()));
let cl = intents.clone();
let mut harness = Harness::new_ui(move |ui| git_panel(ui, &status, &mut cl.borrow_mut()));
harness.run();
harness.get_by_label("Stage all").click();
harness.run();
assert!(intents.borrow().contains(&GitIntent::StageAll));
```

Key points:

- `Harness::new_ui(closure)` runs the closure on each frame; we share the
  state with the outside via `Rc<RefCell<…>>`.
- Queries (`egui_kittest::kittest::Queryable` trait): `get_by_label`,
  `get_by_role`, … The target must be identifiable — a button is by its
  text; for a mute widget, set an accessible `id`/label.
- After an interaction, **call `harness.run()` again** to propagate the event.

## 6. Running the loop

All `tests/*.rs` are modules of a **single** integration binary (`tests/it/`,
entrypoint `tests/it/main.rs`): `cargo test` links once instead of once per
file, which is what keeps the loop fast.

```sh
cargo test                          # everything: unit + business e2e + UI e2e
cargo test --lib                    # only the unit tests (fast)
cargo test --test it ui_git_panel   # a single integration suite (module filter)
cargo clippy --all-targets -- -D warnings   # strict lint (includes the tests)
```

## 7. Extending

- **New business logic** → pure function + unit test in the module; if
  it touches a resource, add a `tests/<domain>_e2e.rs`.
- **New UI component** → `pub` `fn(&mut egui::Ui, …)` function, then a
  kittest test that drives it.
- **Keep `app.rs` thin**: it wires the components to `eframe`; the testable
  logic lives in `git`, `ui`, `terminal`, `workspace`.

## 8. Extension: snapshot tests (optional, not enabled)

egui_kittest can compare a rendering pixel-by-pixel to a reference image
(`snapshot` + `wgpu` features, `harness.snapshot("name")`). Deliberately **not
enabled**: it pulls in `wgpu`, stores versioned PNGs, and stays sensitive to
font rendering. To be considered only to lock down visual fidelity (cf. Codex)
once the design-system is stabilized — in which case enable the feature and
isolate these tests (they only run cleanly on the macOS target).
