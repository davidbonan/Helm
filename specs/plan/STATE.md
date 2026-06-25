# Progress state — helm

> **Source of truth for active progress.**
> Conventions and *Definition of Done*: [`README.md`](README.md). Statuses:
> `☐` to do · `◐` in progress · `☑` done+verified · `⊘` blocked · `⏭` deferred.

---

## Active milestone — M-PR · Pull Requests cockpit

Spec: [`specs/pull-requests.md`](../pull-requests.md). Entry below Agents listing
my PRs + PRs to review, from GitHub (`gh`) and Bitbucket Cloud, scoped to the
workspace repos. Counter: **0/9**.

- ☐ **PR1 — Domain model.** `pull_requests::model`: `PullRequest`, `PrRole`,
  `PrState`, `Checks`, `Review`, `Reviewer`, `PrDetail`. Reuse
  `git::forge::{Forge, parse_remote}`. *Files*: `src/pull_requests/model.rs`,
  `src/lib.rs`. *Tests*: unit on role/dedupe-by-`(forge,number)` helpers.
- ☐ **PR2 — GitHub source (pure).** `pull_requests::github`: `gh` arg builders
  (`pr list/view/checkout`, `api user`) + I/O-free `parse_list`/`parse_detail`
  over `--json` fixtures; `gh auth status` availability probe shape. *Files*:
  `src/pull_requests/github.rs`, `tests/fixtures/`. *Tests*: unit on captured
  JSON (open/draft, checks, review, reviewers).
- ☐ **PR3 — Bitbucket source (pure).** `pull_requests::bitbucket`: Cloud `2.0`
  URL builders + Basic-auth header + I/O-free `parse_list`/`parse_detail`.
  Keychain creds via `security` CLI (service `helm.bitbucket`); `bitbucket_email`
  in `Prefs`. *Files*: `src/pull_requests/bitbucket.rs`,
  `src/pull_requests/creds.rs`, `src/persistence.rs`. *Tests*: unit on JSON
  fixtures + URL/header builders.
- ☐ **PR4 — PrRunner + cache.** Detached one-shot runner (architecture §3
  contract: one reply/request, `in_flight`, drain+repaint each frame); fans
  per-`Forge` queries, classifies roles vs cached identity → `Vec<PullRequest>`
  + per-source status; app cache + refresh cadence (entry / manual / ~60 s
  focused tick). *Files*: `src/pull_requests/runner.rs`, `src/app/*`. *Tests*:
  business e2e on command/URL construction (no live network).
- ☐ **PR5 — Sidebar entry + central mode.** `Pull Requests` row below Agents
  (`Icon::GitPullRequest`), To-review count badge; `CentralMode::PullRequests`
  (sidebar stays, git panel hides); `SidebarAction.open_pull_requests`. **No
  keyboard shortcut.** *Files*: `src/ui/repo_sidebar.rs`, `src/ui/mod.rs`,
  `src/app/{mod,render}.rs`. *Tests*: UI e2e (entry renders, click sets mode).
- ☐ **PR6 — Cockpit page.** `ui::pull_requests_view::pull_requests_page`: two-pane
  (list grouped To review / Mine + detail panel: description, checks, reviewers,
  read-only comments; actions Open in browser / Checkout), resizable split
  `Prefs.pr_detail_width`. *Files*: `src/ui/pull_requests_view.rs`, `src/ui/mod.rs`.
  *Tests*: UI e2e on a fixture list (groups, rows, select, action intents).
- ☐ **PR7 — Checkout = worktree.** If a worktree already sits on the PR source
  branch ⇒ activate that row (no git write); else fetch the branch (GitHub
  `pull/<n>/head`, Bitbucket `origin/<source>`) and create a worktree on it via
  `git::worktree::CreateRunner`, then activate it. *Files*:
  `src/pull_requests/runner.rs`, `src/app/render.rs`; reuse `src/git/worktree.rs`.
  *Tests*: unit on fetch-ref/argv build + existing-worktree match.
- ☐ **PR8 — Preferences section.** Preferences → **Pull Requests**: Bitbucket
  email field + Set token (Keychain), GitHub `gh` status line; persist
  `bitbucket_email`, `pr_detail_width`. *Files*: `src/ui/preferences*.rs`,
  `src/persistence.rs`, `specs/preferences.md`. *Tests*: UI e2e on the section.
- ☐ **PR9 — End-to-end verify.** `headless-verify`: open the cockpit on a fixture,
  confirm groups + detail + Open/Checkout actions render and route. Demonstrable
  milestone scenario (DoD).

### Next actions
- Start **PR1** (model) — no I/O, unblocks PR2/PR3 parsers.

### Blockers
- none

### Open questions
- none

---

## Completed milestone — ☑ M-RC · In-diff code review → Send to Claude

In-diff annotation flow: **Review mode → click a line → text field + Validate
under the line → annotate across multiple files (comments accumulate per repo) →
header button `Send to {agent} (N)`** opens a **new terminal tab** running
`claude "<prompt>"` in the active worktree, prompt = aggregated file/line/code/note.
Works on **both** Git WIP and Commit Détail (read-only lines made annotable in
review mode). Locked: multi-file accumulation (app-level store), **dedicated**
pref `review_agent_command` (default `claude`), launch in a **new tab**.
In-memory only except `review_agent_command` (persisted). Counter: **6/6**.

- ☑ **RC1 — Domain + prompt.** `review::{LineComment, build_review_prompt}` —
  pure markdown grouped by file (BTreeMap), line-ref `new_lineno` else
  `old_lineno`, code + note. *Files*: `src/review.rs`, `src/lib.rs`. *Tests*:
  unit (grouping order, line-ref fallback, multi-file aggregation).
- ☑ **RC2 — Pref `review_agent_command`.** Scalar in `Prefs` before
  `keybindings`, `#[serde(default = "…")]` → `"claude"`; Preferences AI card row
  (reuse `run_command_row` singleline, persist via `action.ai_changed`). *Files*:
  `src/persistence.rs`, `src/ui/preferences.rs`, `src/app/render.rs`. *Tests*:
  round-trip (default/empty/custom) + UI e2e on the row.
- ☑ **RC3 — Review domain store.** `review::{FileComments, add_comment,
  delete_comment, count, ReviewIntent}` — pure store helpers (upsert by line,
  delete+purge empty file, total count). *Files*: `src/review.rs`. *Tests*: unit
  on add/delete/purge/count. (HelmApp/DiffViewState/DiffReview wiring folded into
  RC4 — those private fields only become live with RC4's rendering.)
- ☑ **RC4 — diff_view rendering + wiring.** Introduces `HelmApp.{review:
  HashMap<RepoKey, FileComments>, review_mode}` + `apply_review_intent`
  (toggle/save/delete/clear; `SendToAgent` no-op stub → RC5),
  `DiffViewState.{active_comment, comment_buffer}` (+ `clear()`), `struct
  DiffReview<'a>` 8th param of `diff_view()` as `Option<&mut DiffReview>` (both
  call-sites; `None` at the test sites). Header: Review toggle + `Send (N)` +
  Clear. `diff_line` returns `Option<DiffLineAction>`; rows allocate height
  before the clip check (reserved off-screen); per-line loop renders saved note
  block (+ Delete pill) + inline `TextEdit` editor at `content_left`. Click in
  review mode → `DiffLineAction::OpenComment` (incl. read-only lines), per-line
  stage hover-pill suppressed; first `Esc` cancels editor. *Files*:
  `src/app/mod.rs`, `src/ui/diff_view.rs`, `src/app/render.rs`. *Tests*: UI e2e
  (click line → type → Validate → `SaveComment` intent; `Send (1)` badge count) +
  `headless-verify` (toggle → click → type → Validate → saved note + badge).
- ☑ **RC5 — Apply + spawn.** `apply_review_intent::SendToAgent` →
  `send_review_to_agent(ctx)`: build prompt (`build_review_prompt`) → `add_tab`
  → pre-insert the agent pane under `(run_key, new_tab_id)` at the fresh tab's
  focus `PaneId` (render's `or_insert_with` then a no-op) → rename tab to the
  agent command → `central_mode = Terminal`, `diff = None`. `pty::agent_command`
  (program + prompt argv, no `-l`/`-c`) + `open_agent_terminal`. *Files*:
  `src/app/{mod,render}.rs`, `src/terminal/pty.rs`. *Tests*: unit pty
  argv/cwd/TERM; in-crate e2e (seeded comments → `send_review_to_agent` adds an
  active tab carrying a `Live` agent pane + flips central mode, clears diff).
- ☑ **RC6 — End-to-end verify.** `headless-verify`: review WIP + Commit Détail,
  multi-file, Send opens a new tab with a live pane (stub agent in test).
  Demonstrable milestone scenario (DoD).

### Next actions (M-RC)
- **M-RC complete** (6/6). Review the worktree branch `m-rc`, then merge into
  `main`.

### Open questions (M-RC)
- Keep comments after Send (current plan) vs clear automatically — Clear button
  provided either way.
