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
no extra fetch — the cache is kept current by the background tick (§6), which
runs whether or not the cockpit is open.

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
  diffstat: Option<(u32, u32)>,                               // ± tally; GitHub-only (§5)
  comment_count: Option<u32>,                                 // comment tally; Bitbucket-only (§5)
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
(§11), which fills the same area, and **Back** returns to the list. All labels are
in **English** ([`design-system.md`](design-system.md) §7) — the design canvas this
round was drawn on is French, but the label language is a frozen decision.

- **Header** — a band on the page's own ground, held apart from the list by the tab
  baseline alone, carrying the **Pull Requests** title and, next to it,
  what the page is holding (*"14 open · 1 draft"*, or *"7 of 14 shown"* while the
  filters narrow it); a **search field** (`model::matches_search`: title, number,
  author, branch, project) with a **clear** ✕ once it holds a query; then the
  **Filters** / **Priority** / **Refresh** (§6) controls. **Filters** opens a
  checkbox per workspace project, so a noisy repo can be muted, and wears a count
  pill while any are off; **Priority** chooses the ordering *inside* each band —
  *Priority* (oldest touched first: a PR that has been sitting is the more urgent
  one) or *Recently updated*. Both are **session state**, not persisted. **Refresh**
  is the page's own housekeeping rather than a view control, so it sits past a
  divider, drops the outline, and carries how long ago the last fetch landed
  (*"· 2 min ago"*). No notification or theme chrome (the theme lives in
  Preferences).
- **Tabs** — **Open · To review · Mine · Drafts** (`model::ListTab`), each with its
  count in a pill (tinted on the open tab; a tab reading `0` is as much of an answer
  as one reading `14`). Every fetched PR is open by construction (§1), so the tabs
  are views over the same cache: no extra query, and **no Merged tab** (merged PRs
  are out of the fetch's scope).
- **List**, grouped by **what each PR is waiting on** rather than by role or date
  (`model::ActionGroup`, in this order): **Waiting on your review** ·
  **Ready to merge** · **Waiting on the author** · **In review**. First match wins,
  so a PR blocked on its author never masquerades as reviewable and a review the
  user still owes outranks an approval someone else already gave. Each band is a
  colored section header (glyph + uppercase label + count pill + a rule out to the
  column edge) over the band's **blocks**. The whole list is centered in a reading
  column capped at 1280pt, and closes on a quiet **"End of list · N pull requests"**.
- **Blocks** (`model::list_blocks`) — a band's rows sit in bordered cards rather than
  running full-bleed: one card per **stack**, one for everything loose, ordered by
  where their first row falls under the chosen sort. A **stack** is a chain of PRs
  each targeting the previous one's **source** branch in the same repo; it gets its
  own header — glyph, **STACK**, size pill, repo, `→` its base, and the one
  instruction that matters, *"Merge bottom-up — start at #1"* — and a chevron that
  folds it to that header alone (session state, keyed by repo + base).
- **Row** — a `gutter · author · main · flags · comments · reviewers` grid. The
  **gutter** holds the open / draft / changes-requested **state icon** for a loose PR;
  inside a stack it holds the **spine** and this PR's **rank badge**, numbered from the
  base. The **author** avatar sits right beside it: whose PR this is belongs with what
  it is, not across the row from it. The
  **main** column leads with the PR's **tracker key** (`model::issue_key`, read off
  the branch then the title) then the title, over a meta line
  `#number · author · age · project · source → dest`, where the branch flow is a chip
  and the project drops out inside a stack (its header names it once). A row hanging
  off an earlier rank rather than the one above says so — **↳ off #N** — so a
  branching stack still reads as a flat numbered list. In the **Waiting on the
  author** band the row reads a notch quieter. On the right, the **flags** —
  *Review first* (the base of a stack), *Changes requested*, *Checks failing* /
  *running*, *Draft*, amber **blocks N** (`model::blocked_count`: how many listed PRs
  target this one's source branch) — then the **comment** tally and, on the right edge,
  the **assigned reviewers**: overlapping avatars, each badged with where it stands
  (green check approved, red minus changes requested; a reviewer who has not ruled
  wears none, an empty badge being itself a verdict), the rest collapsing into a `+N`
  disc. Verdicts are ordered first, changes-requested leading, so the reviewer standing
  in the way is never the one that falls behind the overflow. The cluster is
  right-aligned in a fixed slot, so the clusters line up down the list. In
  **Ready to merge** an inline **Merge** button precedes them.
  Clicking a row **selects** it (→ the §11 review surface).
- **Per-forge gaps in the row tallies** (§4): the **± tally** is GitHub-only
  (`gh pr list` returns the scalars; Bitbucket would need one `diffstat` request per
  PR) and the **comment tally** is Bitbucket-only (its list payload carries
  `comment_count`, whereas `gh pr list --json comments` would pull every comment
  *body* of every PR). Neither holds a column the other forge would always leave
  blank: the ± rides the **meta line** and CI folds into the **flags** — and a green
  build, being no news, raises none. Each is simply absent on the forge that cannot
  supply it cheaply, the way `labels` already is.
- **Merge** — from the inline button on a ready-to-merge row, or from the review
  surface header (§11). Both raise a **confirmation modal** naming the repo, the
  branch flow and the forge; confirmed, a gated `PrMergeRequest` posts off-thread
  (GitHub `gh pr merge --merge`, Bitbucket `POST …/pullrequests/{id}/merge` with
  `merge_strategy: merge_commit`). **No strategy picker** — squash vs rebase is a
  repository policy the cockpit does not own — and the **source branch is kept**
  (`--delete-branch` / `close_source_branch` deliberately absent: helm may hold a
  worktree on that branch, §7). On success the merged PR's cached review is dropped,
  the surface returns to the list and the list re-fetches; on failure the forge's own
  message surfaces.
- **Detail** of the selection: the **diff-centric review surface** (§11).

Empty / edge states, all a centered glyph over a headline and a line saying what to
do about it: no recognized-forge repo ⇒ *"No GitHub or Bitbucket repository in your
workspace"*; nothing open ⇒ *"No pull requests"*; the filters leaving nothing ⇒
*"No pull request matches these filters"*. A source unavailable ⇒ its inline hint
(§3) while the other source still lists.

## 6. Fetching, refresh & threading

A one-shot **`PrRunner`** (detached thread per request, gated by `in_flight`)
follows the established runner contract (architecture §3): **one reply per
request**, drained every frame, `request_repaint` on each event — no streaming,
so the unbounded channel stays sound. It fans the per-`Forge` queries, classifies
roles against the cached identity, and returns a `Vec<PullRequest>` + per-source
status. **Detail** (on selection) and **checkout** (§7) are separate gated
requests.

Refresh happens on a **cold cache** (first frame after launch), on a **manual
Refresh** button, and on a **slow background tick** while the window is
**focused** — network is heavier than the worktree / git ticks, so the cadence is
deliberately conservative (rate limits): **~60 s** while the cockpit is on
screen, **~180 s** from any other zone. The tick is **not** gated on the page
being open: the sidebar badge (§2) is read from the terminal, so a cache
refreshed only by the cockpit would be stale by construction. A change to the
workspace repo set re-queries. Network error / offline ⇒ keep the last good
cache, flag it **stale**; **never wipe rows** on a failed refresh.

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
- **M-PR6 (Tour-1 redesign)**: `ActionGroup::of` band classification (draft /
  changes-requested / red CI outrank a review owed, which outranks an approval),
  `ListTab::accepts`, `matches_search`, `blocked_count` (unit); the `gh pr merge` /
  Bitbucket merge URL + body builders (unit); UI e2e — the actionability bands, the
  tab filter, the search field, a ready-to-merge row's inline **Merge**, the surface
  header's **Merge**, the popover verdict group driving the submit label, and
  **Hide tests** filtering the rail's file list.
- **M-PR9 (list blocks & stacks)**: `list_blocks` (a chain numbers base-first, a
  branching stack notes its `off #N`, loose rows collect into one block that keeps the
  band's order, a cycle degrades to loose rows, no linking across repos), `issue_key`
  (branch before title; `UTF-8` and a mid-word run are not tickets) and `row_tags`
  (order, a green build raising nothing, the blocks tally) and `reviewers_by_verdict`
  (a changes-requested reviewer never falls behind the `+N`) — all unit; UI e2e — a
  stack lists under its header and folds away, a lone PR gets no header, a stacked row
  still selects by its own title, the footer counts what the filters let through, and
  the search field's **clear**.
- **Swipe back & the slide (§11)**: the recognizer's thresholds, the vertical and
  leftward rejections, firing while the fingers are still down, and the momentum run
  both completing a short flick and never firing twice (unit);
  `note_h_scroll_room` / `h_scroll_owns_swipe` composition — a scrolled surface under
  the pointer claims the swipe, one back at its left edge or away from the pointer does
  not, and a read clears the flag (unit); UI e2e — a two-finger swipe right returns to
  the list once the slide lands, a scroll does not, a mouse wheel never reads as a
  swipe, and `Esc` / **Back** still hand the app back after their travel. **Not
  covered**: the veto reaching a real scrolled diff band end to end — the harness would
  not move the band's own scroll offset.
- **M-PR3 (cache & richer reviewing)**: the bounded review-cache LRU + the
  `should_refresh_pr` throttle predicate (unit); the GitHub / Bitbucket commit and
  `diff_hunk` parsers + the reply / issue-comment arg & body builders (unit); the
  per-commit delta on a throwaway repo (business e2e); UI e2e — a commit-band
  selection, an inline center card with its snippet emitting `select_file`, the
  reply editor (overlay + center) emitting `ReplyToThread`, and the conversation
  composer / card reply emitting `PostConversationComment`.

## 10. Accepted limitations / out of scope (v1)

- **Merging is supported in-app** (§5), with a confirmation and no strategy picker
  — always a plain merge commit, source branch kept. Line comments, **replies to
  existing threads**, **conversation comments** (add + reply), and approve /
  request-changes / comment reviews are in-app too (§11).
- **No module graph.** A map of the touched modules needs an import graph helm does
  not build, and git alone yields changed files, not dependencies. Inventing edges
  from folder nesting would be a lie. The **Graph ⇄ Files** toggle that once carried
  a disabled Graph segment is **gone** from the rail — a permanently inert control at
  the top of the list was worse than no control; it returns when there is real data
  behind it.
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

- **Surface header.** A **full-width band above both panes**, on the page's own
  ground and held apart by the tab baseline alone, so the PR's identity
  and the review actions stay put whatever the body shows. Three rows: **Back** ·
  the **title** · `#number` · a **health cluster** (passing checks · unresolved
  threads · mergeability) · a hairline · icon-only **Open in browser** / **Checkout**
  · **Finish review** · the **Merge** button (§5); then the identity line (state chip,
  author avatar, `source → dest`, **Created** relative age via `model::relative_age`
  over `PrDetail.created_at`); then the **tabs**.
  The right cluster reads in three groups, spaced apart and split by the hairline:
  **read-only status**, then **navigation**, then the two **actions** — the health
  icons are not buttons and must not sit flush against ones that are.
- **Finish review.** An outlined header button badged with the pending draft count,
  opening the **composer as a popover**: the **verdict group** (Comment-only review ·
  Request changes · Approve) with a caption naming the selection, the summary field
  and **Submit**, whose label names what will be sent. Verdict and submit are one
  control group — the header carries no verdict selector of its own, and there is
  exactly one place a review leaves the app. The popover is the composer's only home:
  it is reachable from every tab, unlike a rail footer.
- **Tabs.** **Conversation · Files**, each with its count, stored per PR url. An
  **opened file *is* the Files tab** — picking one from the conversation lands on the
  column, scrolled to that file; leaving Files drops the current-file mark. There is
  **no Commits tab**: its only real function
  was scoping the diff to one commit, which the Files toolbar's commit dropdown
  already does, and it listed the commits that dropdown lists anyway.
  - **Conversation** — the PR detail: a **meta-row** (reviewer
    avatar cluster + neutral **label** pills — labels are GitHub-only, monochrome so
    the `accent.ai` hue stays reserved for AI surfaces), the **body in a card**
    (`bg_surface` + `border_subtle`) **rendered as markdown** (an in-house
    `pulldown-cmark` renderer, as are the comment bodies — it controls font size,
    line-height and letter-spacing so prose blocks don't read as a dense wall), then
    Checks, then the **conversation card** (below). No author block over the body:
    author, branches and age are the surface header's second line, and repeating them
    under the tabs reads as a rendering slip rather than as context. On `bg_canvas`, in
    the commit-detail visual language. **Cards bounded, prose bounded inside them**:
    the cards run to ~1200px (a step and a half past the measure — a surface three
    times wider than its text reads as a rendering slip), and the *text* inside stops
    at its reading measure of ~900px. The measure is held by laying the galley out from
    the job: handed a bare `LayoutJob`, egui's `Label` relayouts it at the width of the
    `Ui` and the bound is lost. Tables, fenced blocks and images keep the card's width —
    those gain from it. The **scroll area reaches ~16px past the column, into the
    gutter, and does not auto-shrink**: egui floats the bar against the area's right
    edge, so an area that ends where the cards end puts the bar on their border — and
    `auto_shrink` (on by default) pulls it right back there however far the rect is told
    to reach, which is the trap. Held open, the bar clears the cards by ~24px and they
    keep an even margin on both sides.
  - **Prose ink.** Body text is `text_primary`, **strong** runs the *medium* face
    (~500) rather than a brighter ink — emphasis carried by colour means everything
    that isn't bold sits at `text_secondary`, which reads as disabled and greys a whole
    description in dark mode. Quotes stay muted, links accent.
  - **Metadata rail.** Everything *about* the PR lives in a rail (~300px) full height,
    **one ~32px gutter to the right of the conversation column**, the pair **centered in
    the pane** the way a forge centers its review page: anchored to opposite edges they
    drift into two islands separated by a hole wider than the card itself, so what a
    wide window leaves over is split between the two sides instead. Below ~1530px the
    pair fills the pane and the rail lands flush right. **No rule between the two** —
    the gutter and the cards' own borders already say where one ends and the other
    starts, and a hairline there only fences off a column that is already set apart. It
    carries **Reviewers** (avatar, name, and the verdict on the right: a tick once
    approved, a cross for changes requested,
    *Awaiting review* while it is still owed), **Checks**, **Labels**. It stands down
    when the conversation would be left under ~520px, and Checks then falls back into
    the column; the meta-row over the body gives way to the rail rather than naming the
    reviewers and labels twice.
  - **Markdown coverage.** Headings, emphasis, code (inline + fenced), lists, quotes,
    **GFM tables** and **images** — a review body states its results in a table and
    shows them in screenshots, and both were being flattened into a paragraph of
    pipes. A table spans the **card**, not the prose measure, and its columns are
    **sized on what their content asks for**, then scaled to that width: shared evenly,
    a *Step* column of single digits takes the same quarter as the sentence beside it,
    which then wraps over three lines for nothing. What a column asks for is its widest
    unwrapped cell, floored so rows line up under their headers and capped so one long
    line (a URL, a stack trace) can't claim the table; a cell holding an image asks for
    the image's share, since its text says nothing about the thumbnail below it. Scaling
    to the available width is what keeps a content-sized table from pushing past the
    pane and taking the horizontal scroll with it. An **image inside a table cell** is a
    picture there too — hung under the cell's text, scaled to its column: a smoke-test
    table carries its evidence in the Evidence column, and naming the file there is not
    the evidence. Until the bytes
    land (or if they never do) the placeholder names it — its alt text, else the URL's
    file name — and says why.
  - **Links.** A link run is clickable where it sits in the prose: the renderer records
    each run's range, hit-tests it on the laid-out galley (one box per row it wraps
    over) and names the clicked URL to the app, which owns the opening. Bitbucket's
    smart-link attribute run — `{: data-inline-card='' }` right after a link, which the
    parser hands back as prose without the attribute-list extension — is dropped.
  - **Image viewer.** In the flow an embedded picture is a thumbnail; clicking it opens
    it **full-surface** over the review: scroll or pinch zooms (1× fit to 8×, about the
    pointer), drag moves it, double click resets, and `Esc` / a click on the backdrop /
    the ✕ closes. The viewer owns `Esc` while it is up, so that press never also drops
    the file or leaves the review.
  - **Bitbucket repo downloads.** A screenshot linked from a comment normally sits under
    `bitbucket.org/{ws}/{repo}/downloads/{file}`, which answers **401** to the API
    credentials: the website host is not the API. Such a URL is rewritten to
    `api.bitbucket.org/2.0/repositories/{ws}/{repo}/downloads/{file}`, which redirects to
    a signed link curl follows **without** the header (it drops it off-host). Every other
    URL is fetched as written.
  - **Embedded images** are fetched off-thread by URL (`PrReviewRequest::Image`,
    `curl`), decoded with the `image` crate already in use for diff previews, and kept
    as a texture in a **URL-keyed cache** — one fetch per asset however many bodies
    link it. Until it lands (or if it fails) the body shows a muted placeholder naming
    the image, which is also what asks for the fetch. The forge credentials only
    travel to the **forge's own hosts** (`github.com`, `bitbucket.org`): an image URL
    can name any host on the internet. A raw `<img src="…">` is read too — a pasted
    screenshot often arrives as HTML — but the bodies' HTML is not parsed further.
  - **Files** — the rail + diff pane (below).
- **Conversation card.** Everything the conversation *is*, in **one** card
  (`bg_surface` + `border_subtle`), drawn to the `PR Conversation.dc.html` design
  canvas in helm's own palette tokens (the canvas carries the Directskills MUI
  palette — §5's frozen decisions hold: EN labels, helm's palette):
  - **Head** — the title, the **total** comment tally as plain text, and the
    **Oldest | Newest** order toggle boxed as one segmented control (persisted per PR,
    shown only past one comment), closed by a hairline.
  - **One list of threads.** PR-level comments and line-anchored threads sit in the
    **same** list; there is **no separate *Inline comments* band**. A thread is one
    object whether it hangs on the PR or on a line, and two bands made a reviewer read
    "what is still open" twice on the same screen. Grouping: PR-level comments nested
    by `parent_id`, then one thread per (file, anchor), oldest-first.
  - **Open work first** — a **Needs attention · N** section over one raised block per
    unresolved thread: its `path:line`, the code it hangs on (the click target that
    opens the file at that line), the comments (root at full weight, replies under a
    thread rail), then **Reply** — plus **Resolve** when the forge gives the thread a
    handle (a GitHub issue comment has none).
  - **Resolved threads fold into one block** — a header (tick, *Resolved*,
    `N threads · M comments · K files`, Show / Hide) over **one row per thread**:
    `path:line`, its comment tally, a **one-line elided excerpt** of the root, the
    participants' avatars (at most 3) and the last activity. A row **opens in place**
    to the snippet, the comments, a *Resolved · last reply …* note and
    **Reply** / **Reopen**. The block's open state is session state per PR, each row's
    per thread id. The forges do not say **who** resolved a thread (neither GitHub's
    `isResolved` nor Bitbucket's `resolution` carries an actor into the model), so the
    note states what is known rather than naming a resolver helm never fetched.
  - **Composer** — one object at the foot: the field and its action bar
    (*Markdown supported* · **Comment**) inside a single frame beside the user's
    avatar. The button fills with accent only once the draft holds non-blank text.
- **Files tab: one continuous column.** The center stacks **every** changed file's
  diff, one band under the next, in the rail's order — a review is read as one
  document, not opened file by file. The **rail is that column's table of contents**:
  a row click scrolls to its band (and marks it the current file), it never swaps the
  center for a single diff. Each band is the ordinary diff renderer under **band
  chrome** (`diff_view_band`): its path + ± header carries a **fold chevron** instead
  of Close, and it scrolls **horizontally only** — the column owns the vertical axis.
  A band is **flat**: no card, no outline, **no rounded corners**. What tells two
  files apart is a **full-bleed header strip** — `bg_surface_hover` (the quietest fill
  that reads as a header; `bg_surface` is a hair off the canvas) closed top and bottom
  by a hairline, running edge to edge, with 10pt of air between bands. A rounded,
  outlined card around a wall of code reads as a heavy object boxing in the review;
  a bar reads as a seam, which is all this has to be.
  A band whose diff has not landed keeps its header over a muted *Loading diff…* (or
  its own error): a per-file failure never takes the column down, and it is a **line,
  not a spinner** — dozens of animations would repaint the app for as long as the
  fetches trickle in. The rail's **filters gate both lists**, so the contents and the
  column always cover the same files.
  A slim **toolbar** heads the pane: the PR's `N files · +A −B` tally, the **commit
  scope** dropdown (*All commits* or one commit → `commit^..commit`), and a
  **thread navigator** pinned right (*Thread i / N*, prev/next, scrolling to the file
  that carries the thread). The navigator is **hidden below two threads** — a
  navigator for a single item is noise, and the file row already flags it.
  Two consequences the shape forces, and how they are paid for:
  - **State is per file.** A `DiffViewState` caches one file's highlighting and holds
    its open editors, so the app keys them **by path** (`PrReview.file_views`); the
    conversation tab keeps its own. This is what made the earlier attempt fall back to
    an accordion.
  - **Off-screen bands are not laid out.** A band whose height was measured and which
    sits over a screenful away only **reserves that height**; laying out thousands of
    rows every frame is what made a continuous view unaffordable. The height is kept
    per (file, width), so a resize re-measures instead of reserving a stale one.
  Every file's diff is fetched for the range (`ensure_range_diffs`), not just a
  selected one — the column shows them all.

- **Rail.** The rail **sits on the left**, under the surface header, and **only in
  the Files tab** — Conversation has no use for a file list and takes the full width.
  It carries the **Files changed** band and the file rows, nothing else: no PR-level
  actions, no title, no detail, no composer. There is **no Graph ⇄ Files toggle**;
  the graph needs an import graph helm does not build (§10), and a permanently
  disabled half-control was occupying the rail's first row.
  The **Files changed** band reuses the shared Flat ⇄ Tree file-view toggle
  (`Prefs.git_file_view`): Flat shows full paths, Tree groups files under collapsible
  directory rows (`git::file_tree`). Next to the count sit the two **list filters**,
  as icon + count chips: **unread only** (`EyeOff`) and **hide tests**
  (`FlaskConical`). Filters live beside the list they filter, not in the diff pane
  across the split; the ± totals live in the toolbar, which always has room for them
  where a usefully narrow rail does not. File rows show only quiet monochrome icons
  when they carry forge-review draft comments or agent notes.
  The rail **collapses** via the header toggle (`PanelRight*`) or **⌘G** (the
  git-sidebar key, rebound here since the standard git sidebar is suppressed in the
  PR cockpit), persisted in `Prefs.pr_rail_collapsed`; the split width stays
  `Prefs.pr_detail_width` (dragging the split **right** widens it now that the rail
  is on the left). On row hover the gutter shows
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
  ±counts, quiet review/agent icons); every one of them is diffed in the column, each
  fetched once per (range, file) and cached. Picking a row scrolls to its band and
  marks it viewed for that review session. The header's icon-only unread filter chip
  filters the list down to files not yet opened; when the filter hides every row, the
  list shows **All files viewed**. The surface opens on the **Conversation** tab;
  `Esc` over the column clears the current-file mark, then returns to the list
  (**Back**). Binary / oversize blobs degrade as elsewhere (git.md).
- **`Esc` cascade.** One step per press, innermost first: an **open editor or
  composer** takes it (an inline editor rolls its buffer back, a reply/comment field
  closes; a focused always-on composer just gives up its focus) and the key **stops
  there**; else the current-file mark; else back to the list; and only from the list
  does `Esc` leave the cockpit for the terminal. A press that closes a comment field
  never also closes the panel behind it.
- **Swipe back (macOS trackpad).** A **two-finger swipe rightward** over the review
  returns to the list, the way every macOS document surface goes back. It goes
  *straight* to the list rather than mirroring the `Esc` cascade: the file mark only
  moves the rail's highlight in a column that already shows every diff, so a first
  swipe spent on it would read as a dead gesture. Recognized off `MouseWheel` events
  in **points** with a real `TouchPhase` — a mouse wheel reports lines and never
  matches, and any modifier held disqualifies the run. It fires when the run has
  travelled ≥64pt right and at least twice as far horizontally as vertically —
  **mid-swipe, on the event that crosses the threshold, not on release**: macOS trails
  a flick with a momentum run whose end lands a second or more later, and a surface
  that waits for it answers long after the fingers have left the trackpad. That
  momentum is the **same gesture**: a run beginning within 250ms of the last one ending
  continues it, inheriting its distance (so a short flick completes on its coast) and
  its verdict (so a spent run cannot fire twice, and one already disqualified stays
  so). **A horizontally-scrolled surface under the pointer wins the swipe**: a diff
  band whose long code line has been pushed off its left edge claims rightward swipes
  over it until it is back home, the same precedence Safari applies before it will
  navigate.
- **Back slides.** Every way out of the review — the gesture, `Esc`, the header's
  **Back** — plays the same **220ms** ease-out: the surface travels off to the right
  over the list, which is already drawn underneath at rest, and the app is handed back
  to the list only when the travel ends. The leaving surface carries a hairline over a
  short shadow on its left edge; both pages sit on `bg_canvas`, so without one it would
  slide off as a seam nobody can see. The list underneath **does not parallax**: it is
  centered on its own measure, and shifting it crops the page header off the left edge
  for the length of the animation. Mid-slide neither surface answers a click, and the
  review cannot start a second slide out of the first.
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
- **Forge submission (write).** The rail footer composer carries an optional
  **summary** and a primary button whose label names what will be sent — the
  `ReviewVerdict` itself is chosen in the surface header's verdict group (above)
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

- **Per-commit view (read).** The Files toolbar's **commit scope** dropdown lists the
  PR's commits **oldest-first** (short sha · subject) under *All commits*, and is the
  only place they are listed; selecting one recomputes the changed files +
  diffs over `commit^..commit` (explicit base/head — the cache key includes it),
  while **All commits** restores the three-dot PR diff. Reuses `pr_changed_files` /
  `pr_file_diff` with the commit's oids (local once the head is fetched).
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
limitations for **GitHub + Bitbucket Cloud** reviews; **merging** is covered by §5.
