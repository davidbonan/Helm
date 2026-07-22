# helm — Preferences Page

Spec for the full-window **Preferences** page, in the style of **Codex Settings**
(visual reference provided: left nav + card-based content). Replaces the minimal
floating preferences window. Tokens and components: [`design-system.md`](design-system.md)
§4 ; shortcuts: [`keybindings.md`](keybindings.md) §1/§4.

## 1. Intent

- Preferences will host a **growing number of settings**: the surface moves
  from a small floating window to a structured **full-window page** (left nav
  + card-based content), extensible section by section.
- **v1**: the **shell** (page, nav, cards, setting-row grammar) and the
  **migration of the two existing settings** — the theme (design-system §6) and
  the Pull default ([`git.md`](git.md) §10). **No new settings** in v1.
- **Immediate application**: any change takes effect at once and is persisted to
  `prefs.toml` on change — no Save / Cancel button.

## 2. Opening / closing

- `Cmd+,` **toggles** the page: open ⇒ close, closed ⇒ open
  ([`keybindings.md`](keybindings.md) §1).
- The **gear icon** (top-right) opens the page (the `,` badge while holding
  `Cmd` is unchanged, keybindings §5).
- Exit: **← Back to app** row (nav header), `Esc`, or `Cmd+,`.
- The page **covers the whole window** (the 3 zones are no longer rendered)
  but **destroys nothing**: live PTYs, active git workers, central state
  (Terminal/Graph mode, open diff, selections, tabs) **intact** on return.
- Open page = **exclusive active zone** (keybindings §4): the global app
  shortcuts (`Cmd+O`, `Cmd+1..9`, `Cmd+T`, `Cmd+B`, `Cmd+G`,
  `Cmd+Shift+G`…) are **inactive**; only the preferences toggle (`Cmd+,` by
  default) and `Esc` act — except while **recording** a shortcut (§4 Keyboard),
  where both are captured instead.
- `show_preferences` is **not persisted**: the app always reopens on the app.

## 3. Layout

```
┌──────────────┬──────────────────────────────────────────┐
│ ← Back to app│   Appearance                             │
│              │   ┌────────────────────────────────────┐ │
│  ◐ Appearance│   │ Theme                 [Auto|Lt|Dk] │ │
│  ⎇ Git       │   │ Use light, dark, or match…         │ │
│              │   └────────────────────────────────────┘ │
│              │                                          │
│  (fixed nav, │   (scrollable content, stacked cards,    │
│  bg.sidebar) │   bounded width, bg.canvas)              │
└──────────────┴──────────────────────────────────────────┘
```

- **Left nav**: **fixed** width ~240 (non-resizable, no
  persistence), `bg.sidebar`. At the top: the **← Back to app** row. Below: the
  **section items** (Lucide icon + label, "sidebar nav item"
  grammar from design-system §4); active item = `accent.subtle` + `accent`
  text. No category headers as long as the number of sections does not warrant
  them (they will reuse the section-header style from design-system §2); no
  search field in v1 (deferred, §6 — the natural slot is reserved below Back).
- **Content**: `bg.canvas`, vertically scrollable. Section title
  (~24pt, weight 500, `text.primary`), then stacked **setting cards**,
  with **bounded** width (~640pt max) to stay readable full-screen.
- **Setting card** and **setting row**: design-system §4 components —
  a rounded bordered card grouping rows separated by 1px dividers;
  each row = label (`text.primary`) + optional description
  (`text.muted`) on the left, **control** aligned right (segmented, dropdown,
  toggle, field… depending on the setting).

## 4. Sections & settings (v1)

### Appearance

| Setting | Description (UI) | Control | Behavior |
|---------|------------------|----------|--------------|
| **Theme** | Use light, dark, or match your system | Segmented **Auto / Light / Dark** | Immediate application (components re-read tokens, no relayout — design-system §6); persisted (`theme`). |
| **Light theme** | Colors used when the appearance is light | Dropdown of light presets (`theme::PRESETS`, design-system §6): Helm / GitHub Light / Catppuccin Latte / One Light / Tokyo Night Day | Choice of the **family** applied when the resolved appearance is light — chrome + terminal + diff syntax together. Immediate application; persisted (`light_theme`). |
| **Dark theme** | Colors used when the appearance is dark | Dropdown of dark presets: Helm / GitHub Dark / Catppuccin Mocha / One Dark / Tokyo Night | Same for dark appearance; persisted (`dark_theme`). |

### Git

| Setting | Description (UI) | Control | Behavior |
|---------|------------------|----------|--------------|
| **Default pull behavior** | Operation run by the Pull button in the graph toolbar | Dropdown, 4 options: **Fetch All** / **Pull (fast-forward if possible)** / **Pull (fast-forward only)** / **Pull (rebase)** (labels from the `git::sync::PullDefault` domain) | **Same setting** as the radio menu of the Pull split-button ([`git.md`](git.md) §10): both surfaces read/write `pull_default` — a change on one side is reflected on the other; persisted; **never triggers** an operation. |
| **AI provider** | CLI used to generate the commit message | Dropdown, 3 options: **Claude Code** / **Codex** / **opencode** (`ai::AiProvider` domain, product names from `display_name`, default Claude) | CLI launched as a subprocess by the "Generate commit message" button of the commit card ([`git.md`](git.md) §5); Claude is pinned to the small/fast **Haiku** model (`commit_model_args`) since summarizing a staged diff is cheap; persisted; **never triggers** generation. |
| **AI instructions** | Extra guidance added to the commit message prompt | **Multiline** full-width text field (below the label — the right slot is too narrow), hint "e.g. Use conventional commits, write in French…" | Free text appended as-is to the generation prompt; persisted on change. |
| **AI rebase provider** | CLI that performs the AI rebase — runs git itself, never pushes | Dropdown, **same labels** as AI provider: **Claude Code** / **Codex** / **opencode** (same `ai::AiProvider` domain, default Claude — the agentic vs `-p` text invocation differs internally but is not user-facing; the row description states which action it drives) | CLI launched by the **Start AI rebase** of the recap modal ([`git.md`](git.md) §9); configured **separately** from the commit-message provider; persisted; **never triggers** a rebase. |

### Keyboard

Customization of the curated rebindable actions
([`keybindings.md`](keybindings.md) §6). **Restore defaults** action at the top
of the section (resets every binding, inert when nothing deviates), then
**three cards** — Global, Terminal, Git — one **row per action**: label
(`text.primary`) + short description (`text.muted`) on the left, the current
shortcut as a **keycap badge** (e.g. `⇧⌘D`) in the control slot; an unbound
action shows a muted `unbound` placeholder.

- **Recording**: clicking the badge arms the row — the badge turns into
  "Press shortcut…". The next non-modifier keydown is captured with its
  modifiers. While recording: `Esc` **cancels the recording** (does not close
  the page), `Backspace`/`Delete` **unbinds** the action, the preferences
  toggle is captured like any combo (not acted on). Clicking elsewhere cancels.
- **Validation** (keybindings.md §6): a combo without `Cmd`/`Ctrl`/`Alt`, a
  reserved combo (`Cmd+1..9`, `Cmd+Ctrl+1..9`, `Esc`) or a combo already bound
  to another rebindable action is **refused** — the row stays armed and shows
  an inline error (`status.error`) naming the holder ("Already used by
  *Split right*"); a valid capture closes the recording and applies.
- **Row affordances**, visible only when the row deviates from its default:
  **reset** (back to default) and **✕ unbind**. Hover-revealed, like the other
  per-row secondary controls.
- **Immediate application**: the app rebuilds the keymap (routing and `Cmd`
  badges read it at once) and persists on change (`keybindings`, §5) — intents
  pattern, the page never writes prefs itself.

### Terminal

| Setting | Description (UI) | Control | Behavior |
|---------|------------------|----------|--------------|
| **Editor** | IDE opened by a Cmd+click on a file link in the terminal ([`terminal.md`](terminal.md) §12) | Dropdown, 3 options: **VS Code** / **Cursor** / **Zed** (`links::Editor` domain, product names from `label`, default VS Code) | Opens the file (with its line) in the chosen IDE's CLI — `code`/`cursor -g {file}:{line}`, `zed {file}:{line}` — spawned detached; a CLI that fails surfaces an error toast naming it (no silent fallback). Persisted on change (`editor`); **never opens** anything by itself. |
| **Shell command** | Run `helm <path>` in a terminal to open a repository or worktree ([`cli.md`](cli.md) §7) | **Install** button when absent, **Replace** when a foreign `helm` holds the path, the install directory as a read-only status when it is ours; outside a bundle, the dev-mode note | Symlinks `/usr/local/bin/helm` to the binary **inside** the bundle, so an in-place update ([`update.md`](update.md) §5) keeps it working. Intent pattern — the page writes no file; the app links and toasts the outcome. A non-writable directory returns the exact `sudo ln -sf …` to run; a real file at that path is never replaced. Nothing persisted (the link on disk is the state). |

### Pull Requests

Sources and credentials of the PR cockpit ([`pull-requests.md`](pull-requests.md)
§3). Two cards: **GitHub** (read-only status, `gh` owns the token) and
**Bitbucket** (status + the email/token creds). Opening the section warms the
same cache the cockpit uses, so the status lines show live state; while the
first fetch is in flight they read **Checking…**.

| Setting | Description (UI) | Control | Behavior |
|---------|------------------|----------|--------------|
| **GitHub** | Pull requests are read through the gh CLI | Read-only status line | The last fetch's GitHub status: **Connected** when `gh auth status` succeeds, else the inline hint (*"Install gh and run `gh auth login`"*). No secret is stored — `gh` owns the token. |
| **Bitbucket** | Connection status of the Bitbucket source | Read-only status line | **Connected** when the email + Keychain token authenticate, else the hint (missing creds, *"Bitbucket token invalid or expired"*, unreachable). |
| **Email** | Bitbucket account email used for Basic auth | Full-width text field | The non-secret account email; persisted (`bitbucket_email`). Empty ⇒ the Bitbucket source stays off. |
| **API token** | Stored in the macOS Keychain, never written to disk | Masked field + **Save** button | **Save** writes the token to the Keychain (`security`, service `helm.bitbucket`) and re-fetches; the token never reaches `prefs.toml`. |

### Project

A **project picker** replaces the section title: a title-sized dropdown of every
workspace project (folder names, sidebar order). It opens on the project you're
on (§5) and switches to configure any other without leaving it. With no
repository open the section shows "Open a repository to configure it." Settings
are **personal** (per project root in `prefs.toml`), not checked into the repo —
see [`worktrees.md`](worktrees.md) §6 for the worktree semantics and the
ACE-on-pull rationale.

| Setting | Description (UI) | Control | Behavior |
|---------|------------------|----------|--------------|
| **Worktrees base** | Base folder new worktrees for this project are created under | Full-width path field (hint = default `<root>.worktrees`) + **Choose…** native folder picker | Empty ⇒ default; absolute used verbatim; relative resolved against the root. The create modal previews the resolved destination; a missing base is created on first worktree. Persisted on change (`project_settings`). |
| **Post-create script** | Bash run in the new worktree's terminal right after creation | **Multiline** monospace field, hint `e.g. npm install && cp "$HELM_PROJECT_ROOT/.env" .` | Typed into the new worktree's **first terminal** (live, fire-and-forget) with `HELM_WORKTREE_PATH` / `HELM_WORKTREE_BRANCH` / `HELM_PROJECT_ROOT` / `HELM_SOURCE_BRANCH` exported. Empty/whitespace ⇒ no-op. Persisted on change. |

The picker and field edits flow through the **intents** pattern (the page never
opens `rfd` itself); the app applies, persists, and drops a project's entry once
both settings are cleared.

## 5. State & persistence

- **Active section** and the Project picker's **selected project**: **session**
  memory (not persisted); the section defaults to Appearance, and the picker is
  (re)seeded to the active project each time the page opens.
- `prefs.toml` fields: `theme` and `pull_default` (existing) + `light_theme` /
  `dark_theme` (theme families, default `"helm"`; an unknown id falls back to
  Helm at resolution time without rewriting the TOML) + `ai_provider` (kebab-case,
  default `"claude"`) / `ai_instructions` (default empty) + `ai_rebase_provider`
  (kebab-case, default `"claude"`) + `editor_command` (editor template for the
  terminal's file links, default `"code -g {file}:{line}"`, empty = macOS
  `open` — [`terminal.md`](terminal.md) §12) + `keybindings` (table `action-id = "combo"`,
  e.g. `split-right = "cmd+shift+x"`: **only deviations** from the defaults,
  `""` = unbound, unknown id / unparsable combo ignored at resolution without
  rewriting the TOML — keybindings.md §6) + `bitbucket_email` (Bitbucket account
  email, default empty; the paired token lives in the macOS Keychain, **never**
  in the TOML — [`pull-requests.md`](pull-requests.md) §3) + `pr_detail_width`
  (PR cockpit detail-panel width) + `project_settings` (array-of-tables
  keyed by project `root`: optional `worktree_base` + `post_create`; an entry
  with neither is dropped, orphans whose project left the workspace are purged).
- The rendering logic stays as pure `fn(&mut egui::Ui, …)` functions
  driven by a state + **intents** (testing.md §5) — no pref writes
  in the UI, the app applies and persists.

## 6. Extensibility (out of v1 scope)

The grammar (nav + cards + control-slot rows) is designed to
host the following additions **without rework** — each one will come via a
product decision / dedicated milestone:

- **Search settings** field at the top of the nav (filters sections + rows by
  label/description).
- Expected new sections: General, etc.; the **Terminal** section (§4) will
  host the persisted font / size.
- **Category** headers in the nav as the number of sections grows.

## 7. Edge cases

- **No repository open** ⇒ page accessible (settings are independent of the
  workspace); the global empty state reappears on return.
- **Theme change** ⇒ the page itself switches palette immediately.
- **Git operation in progress** ⇒ the page stays usable; changing the Pull
  default during an op runs nothing (a default never triggers execution).
- **Window resize**: fixed nav, fluid content; the minimum
  window width (900, design-system §3) guarantees readability.
