# Feedback — Specification

> In-app **bug report / suggestion** filed as a **GitHub issue** on the helm
> repo (`davidbonan/Helm`) without leaving helm.

## 1. Goal

A discreet entry point to report a bug or suggest an improvement from the app:
a top-right icon opens a small modal (kind + description); **Send** opens the
GitHub "new issue" form in the browser, pre-filled, ready for the user to submit.

## 2. Entry point

- **Icon** in the top-right action row (`ui::top_right_actions`), left of the
  Preferences gear: a `lucide::Bug` glyph, same icon-button shape as the gear.
- A click opens the **feedback modal** (single `Modal::Feedback`, exclusive
  with the other modals like every confirmation surface).

## 3. Modal

- **Type** — a dropdown (`egui::ComboBox`): **Suggestion** / **Bug**
  (defaults to Bug).
- **Description** — a multiline text box (required).
- **Send** — disabled while the description is blank (trimmed). **Cancel** /
  `Esc` / click outside dismiss without sending.
- On Send the modal closes immediately; the outcome surfaces as a toast
  (success "Opening GitHub…", or the failure detail).

## 4. Delivery

- **Channel**: a GitHub `issues/new` URL handed to macOS **`open`** (no HTTP, no
  embedded token) — the user's browser opens the pre-filled new-issue form on
  `davidbonan/Helm`, which they review and submit (signed in to GitHub). A
  server-side POST was rejected: it would require shipping a credential in the
  distributed `.app`, which is extractable and abusable.
- **Transport**: `open` invoked through `git::cli` (`run_program`). The
  `title` / `body` / `labels` are percent-encoded (RFC 3986 unreserved set;
  spaces `%20`, newlines `%0A`), so the whole URL is a single arg (no shell, no
  temp file).
- **Title**: the first non-blank line of the description.
- **Labels**: `bug` (Bug) / `enhancement` (Suggestion) — the default GitHub
  repo labels.
- **Body**: the full description plus an automatic metadata footer
  `— helm <version> · macOS <productVersion>` (app version from
  `update::current_version`, OS from `sw_vers`).

## 5. Execution model

- Opening the browser is **synchronous and instant** (LaunchServices hands off
  at once), so there is no worker thread: `open_issue` runs on the UI thread and
  its `Result` (Ok / `FeedbackError`) becomes a toast on the spot.

## 6. Domain isolation

- `feedback` module: `FeedbackKind`, `open_issue` / `open_with` (seam for tests),
  `FeedbackError` — no egui dependency.
- `ui::feedback_modal`: pure rendering `fn(&mut egui::Ui, …) ->
  FeedbackModalAction`, the page state (`FeedbackPage`) owned by `HelmApp`.
