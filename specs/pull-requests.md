# helm — Pull Requests (workspace PR cockpit)

A **Pull Requests** entry under the Helm section of the left sidebar, **below
Agents**, opens a cross-repo cockpit listing the open PRs that concern the user
across the workspace's repositories: **the PRs they authored** and **the PRs
awaiting their review**. Sources: **GitHub** (via the `gh` CLI) and **Bitbucket
Cloud** (via the REST API). Modules: `pull_requests` (domain) +
`ui::pull_requests_view` (rendering). Mirrors the Agents entry's wiring
(`CentralMode`, sidebar-stays / git-panel-hidden) — see [`agents.md`](agents.md) §5.

## 1. Scope & identity

- **Workspace-scoped.** Only the repositories present in the left sidebar are
  queried. Each repo's `origin` is parsed by `git::forge::parse_remote`
  ([`git/forge.rs`](../src/git/forge.rs)) into a `Forge`
  (`GitHub {owner, repo}` / `Bitbucket {workspace, repo}`); an unrecognized or
  self-hosted host is **skipped silently**. Worktrees of one root share a remote
  ⇒ **deduped by `Forge`** (queried once per remote).
- **Two roles, "me"-relative.** Per forge, "me" is resolved **once per session**
  and cached (GitHub: `gh api user --jq .login`; Bitbucket: `GET /2.0/user`). A
  PR is **Mine** when authored by me, **To review** when I'm a requested
  reviewer. The two are exclusive (no self-review); the list is deduped by
  `(forge, number)`, **Mine wins** if a source ever returns a PR under both.
- Only **open** PRs (including **draft**) are listed; merged/closed are out of
  the list's scope.

## 2. Sidebar entry & badge

A **Pull Requests** row sits **directly below the Agents row** under the Helm
section ([`agents.md`](agents.md) §5), shown once the workspace has a project
(hidden on the empty first-launch state). Icon: `Icon::GitPullRequest`.
Selecting it sets `CentralMode::PullRequests`: the central area shows the
cockpit, the **project sidebar stays** (the row highlighted), the **per-repo git
panel hides** — the same handshake as Agents (`!agents_active` guard generalized
to "a Helm central mode is open"). Reachable by **click** — **no keyboard
shortcut** (unlike Agents' `⌃⌘0`).

**Badge** = the count of **To review** PRs (the actionable ones) as a small count
pill; **0 ⇒ nothing**. Authored PRs do **not** raise the badge (informational,
not a call to action). The badge reads the cached aggregate, so showing it costs
no extra fetch.

## 3. Sources & authentication

| Source | Transport | Auth | Availability probe |
|---|---|---|---|
| **GitHub** | `gh` CLI (`gh pr list` / `gh pr view` / `gh pr checkout`, `--json`) | gh's own (`gh auth login`) | `gh auth status` exit 0 |
| **Bitbucket Cloud** | `curl` → `api.bitbucket.org/2.0` (the `update.rs` idiom) | Basic `email:token` | token present + `GET /2.0/user` → 200 |

Consistent with the repo's **no-HTTP-crate** convention (architecture §5):
GitHub data comes from `gh … --json` (auth, pagination and fork handling for
free), Bitbucket from a `curl` shell-out parsed with the already-present
`serde_json`. **No new runtime dependency.**

- **GitHub** carries **no stored secret** — `gh` owns the token. `gh` absent or
  unauthenticated ⇒ the GitHub source contributes nothing and the page shows a
  one-line hint (*"Install gh and run `gh auth login`"*).
- **Bitbucket** needs an **API token / app password**. The non-secret **email**
  is stored in `Prefs.bitbucket_email`; the **token** lives in the macOS
  **Keychain** (`security` CLI, service `helm.bitbucket`) — **never** in
  `prefs.toml`. Both are set from Preferences → **Pull Requests**
  ([`preferences.md`](preferences.md)). Missing creds ⇒ one-line hint; 401 ⇒
  *"Bitbucket token invalid or expired"*.

## 4. Data model (`pull_requests::model`)

```
PullRequest {
  forge_kind, repo_label, number, title,
  role: Mine | ToReview,
  state: Open | Draft,
  author, source_branch, dest_branch, url, updated_at,
  checks: Passing | Failing | Pending | None,
  review: Approved | ChangesRequested | Pending | None,
  reviewers: Vec<Reviewer>,
  labels: Vec<String>,                                        // GitHub labels; empty on Bitbucket (§11 meta-row)
}
PrComment { author, body, path, old_lineno, new_lineno, id, parent_id, context }  // a fetched thread comment (§11)
PrCommit  { sha, short, subject, author }                     // one PR commit, oldest-first (§11)
PrDetail  { body, comments: Vec<PrComment>, check_runs: Vec<CheckRun>, commits: Vec<PrCommit>, created_at }  // lazy, on selection; created_at ISO-8601 (§11 "Created" age)
ReviewVerdict = Comment | Approve | RequestChanges            // submit composer (§11)
DraftComment  { path, line, body }                            // a postable line comment (§11)
```

Pure mappers `github::parse_list` / `parse_detail` and
`bitbucket::parse_list` / `parse_detail` turn raw JSON into these — **I/O-free,
unit-tested on fixtures**; the runner (§6) owns the shell-out. `forge_kind` /
`repo_label` come from the `Forge` that produced the query.

## 5. The cockpit (`ui::pull_requests_view::pull_requests_page`)

A **full-width list page** owning the central area (like the Agents dashboard):
no side-by-side detail pane — selecting a PR **navigates** to its review surface
(§11), which fills the same area, and **Back** returns to the list. The header
carries only the **Pull Requests** title and a **Refresh** button (§6) — no
global search, notification or theme chrome (the theme lives in Preferences), and
**no tabs** (the two role groups below are the only split). All labels are in
**English** ([`design-system.md`](design-system.md) §7).

- **List**, two **grouped sections** — **To review** then **Mine** (each section
  title carries a count and sits **above** a **rounded card**) — each card is a
  **column table** with a header row and hairline row separators. The shared
  columns are **Title** (the PR title with its **`source → dest` branch** beneath,
  led by the open / draft **state icon**), **Project** (repo chip), **Status** and
  **Updated** (relative age); **To review** adds an **Author** and a **Requested
  reviewer** column, **Mine** a **Reviewers** column (stacked avatars, `+N`
  overflow). **Status** is a single **plain colored label** encoding state +
  review decision (*Open* / *In review* / *Approved* / *Changes requested* /
  *Draft*) — no pill fill; the CI **checks** (✓ / ✗ / •) live in the §11 review
  detail, not the list. A trailing chevron marks the row → detail affordance.
  **No** project / reviewer / status **filter** or sort control —
  workspace-scoped + grouped is the only ordering. A PR **stacked** on another
  listed PR — its **target** branch is that PR's **source** in the same repo —
  **nests** under it as an **indented tree** (base first, `├`/`└` gutter
  connectors); an unstacked group renders flat as before. Clicking a row
  **selects** it (→ the §11 review surface).
- **Detail** of the selection: header (title · `#number` · state), `source →
  dest`, author, **checks** + **reviewers**, and the **diff-centric review
  surface** (§11) — the PR's changed files and their diffs, with in-diff comments,
  **inline-comment cards with code context** and a **per-commit** view in the
  center, and a submit composer. PR-level actions: **Open in browser** (reuses
  `terminal::links::open_url` on the PR's `url`), **Checkout** (§7), and **Ask
  Claude** (§11). **Merging** stays out of scope (opens the browser).

Empty / edge states: no recognized-forge repo ⇒ *"No GitHub or Bitbucket
repository in your workspace"*; a source unavailable ⇒ its inline hint (§3) while
the other source still lists.

## 6. Fetching, refresh & threading

A one-shot **`PrRunner`** (detached thread per request, gated by `in_flight`)
follows the established runner contract (architecture §3): **one reply per
request**, drained every frame, `request_repaint` on each event — no streaming,
so the unbounded channel stays sound. It fans the per-`Forge` queries, classifies
roles against the cached identity, and returns a `Vec<PullRequest>` + per-source
status. **Detail** (on selection) and **checkout** (§7) are separate gated
requests.

Refresh happens on **first entry** to the page (cold / stale cache), on a
**manual Refresh** button, and on a **slow background tick** (~60 s) while the
page is open **and the window is focused** — network is heavier than the
worktree / git ticks, so the cadence is deliberately conservative (rate limits).
A change to the workspace repo set re-queries. Network error / offline ⇒ keep the
last good cache, flag it **stale**; **never wipe rows** on a failed refresh.

Opened PRs are held in a small **bounded per-PR review cache** (LRU, ~8 entries),
each carrying its own **per-file diff cache** keyed by `(base, head, path)`, so
re-opening a PR — or switching back to one — is **instant**: no re-run of the
detail / diff runners, drafts and selection intact, with a background re-fetch
only when the cached entry has aged past ~60 s (swapped in on arrival, drafts
untouched). The **focus-regained** refetch of the list is throttled by a 30 s
minimum age so a quick refocus doesn't re-hit the network; cold / repos-changed
still fire unconditionally.

## 7. Checkout (open the PR branch as a worktree)

From the detail panel, **Checkout** brings the PR's **source branch** up as a
**worktree** of the **matched workspace repo** (the repo whose `Forge` owns the
PR), reusing the worktree machinery ([`worktrees.md`](worktrees.md) §6,
`git::worktree`):

- **Already checked out** — if a worktree of the project (main or a linked one)
  already sits on the PR's source branch, **Checkout just activates that row**
  (no git write): the sidebar selects it and the central area returns to its
  terminal.
- **Otherwise** — helm **creates a new worktree** on that branch under the
  project's worktrees base (`<root>.worktrees/<branch>` by default, honoring the
  per-project base — worktrees.md §6), then activates it. The branch is **fetched
  first** so a not-yet-local PR branch resolves, **forks included**: GitHub
  `git fetch origin pull/<number>/head:<branch>`, Bitbucket
  `git fetch origin <source>`. An existing same-name local branch is reused.

Runs off-thread via the existing `git::worktree::CreateRunner` (architecture §3
runner contract). On failure a one-line error surfaces and no partial worktree is
left (the create path already rolls back, worktrees.md §6); on success the new
worktree row appears and is selected, and discovery (worktrees.md §4) keeps it in
sync.

## 8. Persistence

New `Prefs` scalars (architecture §4), placed **before** the `keybindings` /
`projects` tables: `bitbucket_email: String` (default `""`) and
`pr_detail_width: f32`. The Bitbucket **token is not persisted in TOML** —
Keychain only (§3). Identity and PR lists are **session caches**, not persisted.

## 9. Tests (testing.md — 3 levels)

- **Unit (pure)**: `github::parse_list` / `parse_detail` and
  `bitbucket::parse_list` / `parse_detail` on captured JSON fixtures (open /
  draft, checks, review decision, reviewers); role classification + dedupe by
  `(forge, number)`; badge = To-review count; the `gh` / `curl` arg & URL
  builders.
- **Business e2e**: command & URL construction against the model (no live
  network — no creds in CI; live calls are out of scope).
- **UI e2e (`egui_kittest`)**: `pull_requests_page` on a fixture list renders the
  two groups + rows; selecting a row emits `select`; **Open in browser** /
  **Checkout** / **Submit review** emit their actions; clicking a
  changed file emits `select_file`; the diff renders an existing thread read-only
  and its **Ask {agent}** pill emits `AskAgentOnThread`; the sidebar entry renders
  and its click sets `CentralMode::PullRequests`.
- **M-PR2 (write)**: the three-dot `pr_changed_files` / `pr_file_diff` delta on a
  throwaway repo; `draft_comments` flattening (blank notes dropped) into the
  GitHub review payload and the Bitbucket inline-comment / verdict-URL builders.
- **M-PR3 (cache & richer reviewing)**: the bounded review-cache LRU + the
  `should_refresh_pr` throttle predicate (unit); the GitHub / Bitbucket commit and
  `diff_hunk` parsers + the reply / issue-comment arg & body builders (unit); the
  per-commit delta on a throwaway repo (business e2e); UI e2e — a commit-band
  selection, an inline center card with its snippet emitting `select_file`, the
  reply editor (overlay + center) emitting `ReplyToThread`, and the conversation
  composer / card reply emitting `PostConversationComment`.

## 10. Accepted limitations / out of scope (v1)

- **No merging** from the app (opens the browser). Line comments, **replies to
  existing threads**, **conversation comments** (add + reply), and approve /
  request-changes / comment reviews **are** supported in-app (§11).
- **Bitbucket Cloud only** — no Server / Data Center (different API base & auth).
- **Workspace-scoped** — no global account search (a PR to review on a repo not
  in the sidebar is not shown).
- **GitHub via `gh` only** (no raw-PAT path); **Bitbucket via email + token**
  (no OAuth).
- Checkout opens/creates a **worktree** on the PR branch; a **Bitbucket
  cross-fork** source (branch on another workspace) isn't fetchable from `origin`
  ⇒ Checkout falls back to **Open in browser**.
- **GitLab / self-hosted** forges unsupported (`parse_remote` ⇒ skipped).

## 11. In-app review (M-PR2/M-PR3 — diff, line & inline comments, replies, conversation, submit, Ask Claude)

The detail panel is a **diff-centric review surface**: the changed files of the
PR shown **without cloning the branch**, each diff annotatable, with a composer to
**submit a review** and a way to **hand a thread to the agent**. It reuses the
M-RC review engine (`review.rs`, `ui::diff_view`) so the in-diff comment UX is the
same one as commit/working-tree review.

- **Layout.** The **center** area shows the selected file's diff, or — when **no
  file is open** — the PR **detail**: a compact full-width header with **Back**,
  the PR **title**, `author · source → dest`, compact **Open in browser** /
  **Checkout** PR-level actions and a `#number` chip, followed by the author block
  (avatar, name, `source → dest`) with a right-aligned **Created** relative age
  (`model::relative_age` over `PrDetail.created_at`), a **meta-row** (the reviewer
  avatar cluster + neutral **label** pills — labels are GitHub-only, monochrome so
  the `accent.ai` hue stays reserved for AI surfaces), the **body in a card**
  (`bg_surface` + `border_subtle`) **rendered as markdown** (an in-house
  `pulldown-cmark` renderer, as are the comment bodies — it controls font size,
  line-height and letter-spacing so prose blocks don't read as a dense wall), then
  Checks and the **Conversation** — each top-level comment a card with an **Author** /
  **Reviewer** role tag and a quiet **Reply** pill, ordered by an **Oldest | Newest**
  toggle (persisted per PR), closed by an always-visible **Add a comment** composer
  bar (the signed-in user's avatar + field + filled **Comment** button). The detail uses the
  commit-detail visual language on `bg_canvas`. The **rail sits on the right** —
  the commit-detail sidebar's place — carrying only a **Files changed** band, the
  file rows and the composer; it never holds PR-level actions, the title or
  detail. The **Files changed** band reuses the shared
  Flat ⇄ Tree file-view toggle (`Prefs.git_file_view`): Flat shows full paths,
  Tree groups files under collapsible directory rows (`git::file_tree`). File rows
  show only quiet monochrome icons when they carry forge-review draft comments or
  agent notes; opened state stays out of the rows and is exposed through a compact
  icon-only unread filter chip carrying the unread count in the header. The rail **collapses**
  via the header toggle
  (`PanelRight*`) or **⌘G** (the git-sidebar key, rebound here since the standard
  git sidebar is suppressed in the PR cockpit), persisted in `Prefs.pr_rail_collapsed`;
  the split width stays `Prefs.pr_detail_width`. On row hover the gutter shows
  **two** review-note buttons feeding **two separate pools** (`ReviewPool`): a
  `MessageSquarePlus` button (slot 0 — the **forge** pool, posted to GitHub /
  Bitbucket on the exact submit action label) and the `Sparkles` button (slot 1 — the
  **agent** pool, batched to the agent via the *Send to {agent}* recap pill). Each
  opens its own inline note editor on the line; the pools never cross, so a forge
  review comment is **never** forced through the agent. Both batch — never one line
  at a time. (The working-tree / commit diffs keep only the agent `Sparkles`.)
- **Diff producer (domain, I/O-free).** `git::diff::pr_changed_files(repo, base,
  head)` + `pr_file_diff(...)` compute the PR delta over the **three-dot**
  `merge-base(base, head)..head` range — only the PR's own changes, never the
  destination branch's drift. Base/head are the PR's `dest`/`source` tips resolved
  in the matched workspace repo; an unfetched head ⇒ the diff is unavailable with
  a one-line hint (Checkout §7 fetches it).
- **Changed files + diff (read).** The rail lists the changed files (path, kind,
  ±counts, quiet review/agent icons); selecting one loads its diff lazily (its own
  gated request) and marks it viewed for that review session. The header's
  icon-only unread filter chip filters the list down to files not yet opened; when the
  filter hides every row, the list shows **All files viewed**. The
  surface opens with **no file selected**, so the center shows the **PR detail**;
  the diff's **Close** (or `Esc` over it) clears the selection back to that detail
  **without leaving the surface** — distinct from **Back**, which returns to the
  list. Binary / oversize blobs degrade as elsewhere (git.md).
- **Existing threads (read).** Posted PR comments overlay the diff **anchored at
  their line**, read-only, via `review::ForgeThreads` (author + body cards with a
  compact in-card **Ask {agent}** action on the thread). They are **never edited**
  locally; the editable draft stores stay `FileComments`.
- **Two draft pools (write).** The forge pool (`PrReview.draft`) and the agent pool
  (`PrReview.agent_notes`) are independent `FileComments` stores; the diff routes a
  `SaveComment`/`DeleteComment` to one by its `ReviewPool`. The forge pool feeds the
  composer's **Submit review (N)** (count = `review::count(draft)`); the agent pool
  feeds the diff's **Send to {agent}** recap and the whole-PR **Ask Claude** prompt
  — so review comments destined for the forge are never sent to the agent.
- **Forge submission (write).** The rail footer composer carries a **segmented
  verdict control** (`ReviewVerdict`: Comment · Approve · Request changes), an
  optional **summary**, and a primary button whose label names what will be sent
  (`Submit 1 comment`, `Approve`, `Request changes + 2 comments`, etc.; an empty
  Comment review reads **Nothing to submit** and is disabled). On submit,
  `model::draft_comments` flattens the forge pool to
  `DraftComment { path, line, body }` (blank notes dropped) and a gated
  **`PrPostRunner`** posts off-thread:
  - **GitHub** — one call to `POST repos/{repo}/pulls/{n}/reviews` via
    `gh api … --input -`, body `{event, body?, comments[]}` (event
    `APPROVE`/`REQUEST_CHANGES`/`COMMENT`; summary & comments omitted when empty).
  - **Bitbucket** — one `curl` per inline comment
    (`POST …/pullrequests/{id}/comments`, `{content.raw, inline{path,to}}`), then
    the summary comment if any, then the verdict (`…/approve` /
    `…/request-changes`; Comment posts nothing extra). Basic `email:token` from
    the Keychain (§3).

  On success the draft/summary/verdict reset and the detail **refetches** so the
  posted thread reappears; on failure a one-line error surfaces and the draft is
  kept.
- **Ask Claude on a thread.** Each existing thread shows an **Ask {agent}** pill
  emitting `ReviewIntent::AskAgentOnThread { file, line }`. The app builds a
  prompt from that thread's comments and launches the agent in the PR's worktree
  (resolved/created as in §7) — the same launch path as the whole-PR **Ask
  Claude**, scoped to one thread.

**M-PR3 — richer reviewing** (folded into the same surface):

- **Per-commit view (read).** A **commit band** above *Files changed* lists the
  PR's commits **oldest-first** (short sha · subject · author); selecting one
  recomputes the changed files + diffs over `commit^..commit` (explicit
  base/head — the cache key includes it), while **All commits** restores the
  three-dot PR diff. Reuses `pr_changed_files` / `pr_file_diff` with the commit's
  oids (local once the head is fetched).
- **Inline comments in the center (read).** Line-anchored threads are also
  surfaced in the center **detail**, grouped per file: each card shows a small
  monochrome **code-context** snippet (GitHub `diff_hunk` straight from the
  comments payload — no extra request; Bitbucket a window derived from the loaded
  `FileDiff`, else none) above the thread, and clicking it **opens the file at
  that line**. The diff overlay anchored on the row stays.
- **Reply to a thread (write).** Each existing thread — in the diff overlay **and**
  the center inline card — carries a **reply** editor: GitHub
  `POST …/pulls/{n}/comments/{id}/replies`, Bitbucket `{content.raw, parent:{id}}`.
  The detail refetches on success so the reply reappears.
- **Conversation comments (write).** The center **Conversation** section closes with
  an always-visible composer bar (the current user's avatar + field + filled **Comment**
  button, parent-less; the avatar's initials come from the forge identity — `gh api user`
  for GitHub, `/2.0/user` `display_name` for Bitbucket)
  plus a **Reply** under **every** top-level card:
  GitHub `POST issues/{n}/comments` (flat — issue comments don't thread, so the
  reply posts a new top-level comment), Bitbucket `POST …/comments` (the reply nests
  via `parent` when the card carries a forge id). Posting reuses the same gated
  **`PrPostRunner`** + success-refetch as the submit / reply paths (the draft is
  untouched — only a submitted *review* clears it).

This supersedes the §10 "no posting / approving / requesting changes / replying"
limitations for **GitHub + Bitbucket Cloud** reviews; only **merging** remains out
of scope (opens the browser).
