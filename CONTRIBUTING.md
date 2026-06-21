# Contributing to Helm

Thanks for your interest in Helm. This is a native **macOS** app written in
**Rust** (`eframe` / `egui`), so a working Rust toolchain on macOS is all you
need to get started.

## Getting started

```sh
git clone https://github.com/davidbonan/Helm.git
cd Helm
cargo run            # compile and launch the app
```

The [`rust-toolchain.toml`](rust-toolchain.toml) pins the `stable` channel and
the `rustfmt` + `clippy` components — `rustup` installs them automatically on
first build.

## Architecture

The codebase separates business logic from rendering (Clean Code + DDD):

- **`src/lib.rs`** — testable modules (git, terminal/PTY, split tree, …). All
  testable logic is `pub`.
- **`src/main.rs`** — the thin `eframe` wrapper.
- Rendering lives in `fn(&mut egui::Ui, …)` functions, drivable headlessly.

`specs/` freezes the product intent; `specs/plan/` tracks execution. Read
[`specs/architecture.md`](specs/architecture.md) for the module map and
[`specs/testing.md`](specs/testing.md) for the test strategy before a larger
change.

## Tests

Three levels, all run by `cargo test`: unit (pure logic), business e2e (a real
git repo / PTY), and headless UI e2e via `egui_kittest`.

```sh
cargo test
```

## Before opening a pull request

The CI gate ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs these
three checks — run them locally first:

```sh
cargo fmt                  # format
cargo clippy -- -D warnings # lint (warnings are errors)
cargo test                 # all three test levels
```

Keep changes surgical and matched to the existing style. New behavior that is
testable should ship with a test. For anything touching the UI, keep the
rendering functions headless-drivable so they stay covered by `egui_kittest`.

## License

By contributing, you agree that your contributions are licensed under the same
terms as the project: **MIT OR Apache-2.0** (see [`LICENSE-MIT`](LICENSE-MIT)
and [`LICENSE-APACHE`](LICENSE-APACHE)).
