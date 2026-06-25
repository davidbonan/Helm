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
