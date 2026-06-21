---
name: headless-verify
description: Opens the helm app (HelmApp) headless via egui_kittest, drives it and verifies a behavior, then reports PASS/FAIL with evidence (PNG capture + accessibility-tree dump in a per-run subfolder of verify-artifacts/, gitignored). Without an argument, verifies uncommitted changes; with an argument, verifies the given instructions. Ephemeral verification — writes no persistent test in tests/; harness and evidence are suffixed with a unique run-id, safe with multiple sessions in parallel. Use it to quickly confirm that a UI / keyboard / git change works in the real app, without opening a window.
argument-hint: [natural-language test instructions]
---

# headless-verify

Verifies a **helm** behavior by opening the real app (`HelmApp`)
**headless** (egui_kittest renders egui in memory, no window), driving it,
then reporting **PASS/FAIL** with evidence.

- **Without an argument** → the target is the **uncommitted diff** (working tree + index).
- **With an argument** → the target is the **natural-language instruction** passed in.

This is an **ephemeral** verification: the harness is written to a throwaway,
**per-run** file, run, then **deleted**. Nothing is added to `tests/`. The
evidence (PNG + a11y dump) goes into a **per-run subfolder**
`verify-artifacts/<HV_ID>/` (gitignored) — no capture overwrites the previous one.
The throwaway file and the evidence folder share the same **unique run-id**
(`HV_ID`, timestamp + PID): several sessions can run this skill **in
parallel** in the same clone without overwriting or deleting one another.

## Prerequisites (already in place)

- `Cargo.toml` exposes the feature `headless-verify = ["egui_kittest/eframe", "egui_kittest/wgpu"]`
  and a dev-dep `image` (png). The feature is enabled **only** by this skill: the
  default `cargo test` loop stays without wgpu (see `specs/testing.md` §8).
- macOS target (Metal): the wgpu renderer works headless on this machine.

## Procedure

### 0. Mint the run-id (`HV_ID`)

Once, at the start of the run:

```sh
HV_ID="$(date +%Y%m%d_%H%M%S)_$$"; echo "$HV_ID"
```

Reuse the **displayed value, literally**, in every following step
(shell state does not persist between commands). Underscores only: this
run-id becomes a cargo target name. The PID guarantees uniqueness even if two
sessions start within the same second.

**Concurrency rule**: any `tests/headless_verify_scratch_*.rs` file that does not
carry **this** `HV_ID` belongs to another session — never read, overwrite,
or delete it.

### 1. Determine the scenario

- **Argument provided**: that is the test spec. Translate it into concrete
  interactions (clicks, keys) + observable assertions.
- **No argument**: read the uncommitted changes and infer what to exercise.
  ```sh
  git status --short
  git diff
  git diff --cached
  ```
  Map the touched files → interactions:
  - `src/ui/**`, `src/app.rs` → presence of labels, clicks, emitted intents.
  - keyboard / focus → `key_press`.
  - `src/git/**` → visible effect in the Git panel.
  If the diff is empty: report it and run a **smoke test** (the app opens, the
  expected panels are present).

### 2. Verify it compiles

```sh
cargo check --features headless-verify --tests
```
If there is a compile error → **that is the verification result**: report
FAIL with the errors, clean up (step 6), stop.

### 3. Write the ephemeral harness

A file specific to **this run**, **always deleted** afterward:
`tests/headless_verify_scratch_<HV_ID>.rs` (replace `<HV_ID>` with the value from
step 0). Never the bare name `headless_verify_scratch.rs`: it would collide
with parallel sessions. Adapt the body to the scenario. Verified
skeleton (compiles and runs):

```rust
#![cfg(feature = "headless-verify")]

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use helm::app::HelmApp;

#[test]
fn headless_verify() {
    // Session folder provided by the skill (HV_SESSION_DIR = verify-artifacts/<HV_ID>);
    // epoch+pid fallback if run by hand, so a parallel run is never overwritten.
    let session_dir = std::env::var("HV_SESSION_DIR").unwrap_or_else(|_| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("verify-artifacts/session-{secs}-{}", std::process::id())
    });
    std::fs::create_dir_all(&session_dir).unwrap();

    // Opens the real app headless (800x600, dark theme by default).
    // `install_fonts` as `run()` does at prod boot: without this call, the
    // Lucide glyphs render as tofu in the captures.
    let mut harness = Harness::new_eframe(|cc| {
        helm::theme::install_fonts(&cc.egui_ctx);
        HelmApp::default()
    });
    harness.run();

    // Text evidence: accessibility tree (visible with --nocapture).
    let tree = format!("{:#?}", harness.root());
    std::fs::write(format!("{session_dir}/headless_verify.a11y.txt"), &tree).unwrap();

    // --- Assertions / interactions SPECIFIC to the scenario ---
    // presence (non-panic):
    assert!(harness.query_by_label("Git").is_some(), "Git panel missing");
    // interaction:
    // harness.get_by_label("Stage all").click();
    // harness.key_press(egui::Key::J);
    // harness.run(); // re-propagate after each event

    // Visual evidence: PNG capture of the headless render.
    let img = harness.render().expect("render wgpu");
    img.save(format!("{session_dir}/headless_verify.png")).expect("save png");
}
```

Notes:
- `#![cfg(feature = "headless-verify")]` keeps the file inert if a `cargo test`
  without the feature sees it before deletion.
- Isolated component rather than the whole app: use `Harness::new_ui(|ui| …)` by
  calling the `pub` render function (see `tests/ui_git_panel.rs`).
- **Scenario requiring an open repo** (populated sidebar, git panel, terminal
  in a repo): `HelmApp::default()` always starts empty, and opening a repo in
  prod goes through the native `rfd` dialog (not drivable headless). Seed via the seam —
  see "Seed an open repo" below.
- One capture per observed state: name each PNG in `session_dir`
  (`{session_dir}/before.png`, `{session_dir}/after.png`) and `render()`/`save()`
  again after an interaction to prove before/after.

### 4. Run headless

The session folder and the cargo target both carry the `HV_ID` from step 0
(the cargo target = the file stem from step 3):

```sh
HV_SESSION_DIR="verify-artifacts/<HV_ID>" \
  cargo test --features headless-verify --test headless_verify_scratch_<HV_ID> -- --nocapture
```

All evidence for this run is written to that folder, without overwriting previous
or parallel runs. If another session is compiling at the same time, cargo may
print `Blocking waiting for file lock on build directory`: it waits, it does
not fail — let it finish.

### 5. Gather the evidence

In this run's session folder (`verify-artifacts/<HV_ID>/`):
- `*.png` — capture(s) of the real render (readable via the Read tool).
- `*.a11y.txt` — accessibility tree.
- `--nocapture` output — assertions / any panics.

### 6. Clean up (always)

Delete the throwaway harness **for this run only**, on success **as well as** failure:
```sh
rm -f tests/headless_verify_scratch_<HV_ID>.rs
```
Never a glob (`rm tests/headless_verify_scratch*`): the other scratch files
belong to in-progress parallel sessions. If a scratch from an obviously
dead session is lingering (old, no cargo still running), **report** it to
the user rather than deleting it.
Do **not** delete `verify-artifacts/`: that is the evidence handed to the user.

### 7. Report

- **PASS / FAIL** + what was verified (tied to the diff or the instruction).
- Path of the PNG(s) + relevant a11y excerpts.
- If an assertion panicked: quote the message; `get_by_*` dumps the full a11y
  tree when a node is missing (direct proof of the FAIL).

## Seed an open repo (without the native rfd dialog)

`HelmApp::default()` starts with an **empty workspace**: without a repo, you can only
verify the empty state, keyboard no-ops, and the git toggle. But opening a repo in prod goes
through `rfd::FileDialog` (native NSOpenPanel) which would **hang** the headless harness. So you
**never** drive the dialog — you inject an already-populated workspace via the seam:

```rust
pub fn HelmApp::with_workspace(workspace: Workspace) -> Self
```

The harness mounts a **real throwaway git fixture** (same pattern as the business e2e tests,
`specs/testing.md` §4: `tempfile` + `git2`), builds the `Workspace` via the pub API
(`Workspace::new()` + `add(Repo::new(path, is_git))`), then injects it. No machine
state is touched; isolated and parallelizable.

```rust
use helm::app::HelmApp;
use helm::workspace::{Repo, Workspace};

// Fixture: real git repo, predictable name, with a change to populate the status.
let tmp = tempfile::tempdir().unwrap();
let repo_path = tmp.path().join("fixture-repo");
std::fs::create_dir_all(&repo_path).unwrap();
git2::Repository::init(&repo_path).unwrap();
std::fs::write(repo_path.join("README.md"), "hello").unwrap();

let mut workspace = Workspace::new();
let is_git = helm::git::is_repo(&repo_path);
workspace.add(Repo::new(repo_path.clone(), is_git)); // 1st repo ⇒ becomes active

let mut harness = Harness::builder()
    .with_os(egui::os::OperatingSystem::Mac)
    .build_eframe(move |cc| {
        helm::theme::install_fonts(&cc.egui_ctx); // otherwise icons render as tofu
        HelmApp::with_workspace(workspace)
    });
harness.run();

// The repo is open: present in the sidebar + its terminal panel started.
assert!(harness.query_by_label("fixture-repo").is_some());
assert_eq!(harness.state().pane_count(), 1);
// tmp must live until the end of the test (do not drop it before the assertions).
```

Notes:
- **Keep `tmp` (`TempDir`) alive** until the end of the test: its `Drop` deletes the
  repo on disk. The `Workspace` only holds the `PathBuf`, not the guard.
- The active repo **opens a real PTY** (cwd = repo path): the shell prompt and the
  real git status (branch, files) appear — git fixture ⇒ populated git panel.
- **Multi-repo**: several `add(...)` then `workspace.set_active(i)` before
  building the app (exercises `Cmd+1..9`, the switch, the removal).
- **Repo row**: exposed in the a11y tree as a `Button` labeled with the repo
  name (`repo_sidebar.rs`) ⇒ targetable via `query_by_label("<name>")`.

## egui_kittest cheatsheet (verified API, 0.34)

| Need | Call |
|---|---|
| Open the whole app | `Harness::new_eframe(\|_cc\| HelmApp::default())` |
| Force the OS (macOS modifiers) | `Harness::builder().with_os(egui::os::OperatingSystem::Mac).build_eframe(\|_cc\| HelmApp::default())` |
| Isolated component | `Harness::new_ui(\|ui\| git_panel(ui, &status, &mut sink))` |
| Advance one frame | `harness.run()` |
| Presence without panic | `harness.query_by_label("X").is_some()` |
| Get (panic if absent) | `harness.get_by_label("X")` |
| Substring / role | `query_by_label_contains`, `get_by_role(accesskit::Role::Button)` |
| Click | `harness.get_by_label("X").click(); harness.run();` |
| Keyboard | `harness.key_press(egui::Key::J)` / `harness.key_press_modifiers(mods, key)` |
| Read app state | `harness.state()` *(HelmApp fields private → prefer the a11y tree)* |
| Dump a11y tree | `format!("{:#?}", harness.root())` |
| PNG capture | `harness.render().unwrap().save(format!("{session_dir}/x.png")).unwrap()` |

## Guardrails

- **Ephemeral** verification: never commit `tests/headless_verify_scratch_*.rs`
  nor `verify-artifacts/`.
- **Concurrency**: one `HV_ID` per run (step 0), reused for the scratch file,
  the cargo target, and the evidence folder. Never use the bare name
  `headless_verify_scratch.rs`, never touch another `HV_ID`'s scratch.
- **Never** add `headless-verify` to the default test loop (pulls in wgpu).
- Assert on the **accessibility tree** (stable public surface), not on the
  pixel — no snapshot comparison here (testing.md §8).
- Minimal diff: this skill does not modify the application code, it observes it.
