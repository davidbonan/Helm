# Progress state — helm

> **Source of truth for active progress.**
> Conventions and *Definition of Done*: [`README.md`](README.md). Statuses:
> `☐` to do · `◐` in progress · `☑` done+verified · `⊘` blocked · `⏭` deferred.

---

## ☑ Milestone — M-Amend · Amend HEAD's commit message

Spec: [`specs/git.md`](../git.md) §9 (commit detail) + §1 (decided write). In-place
**reword of `HEAD`** from the commit detail panel: double-click the message block ⇒
inline editor (subject + description, prefilled) with **Amend** / **Cancel**.
Message-only (tree + author preserved, committer refreshed); HEAD-only; blocked mid
merge/rebase. Counter: **1/1**.

- ☑ **T1 — Amend.** Domain `commit::amend_message` (`Commit::amend`; guards clean
  state + non-empty message + existing `HEAD`); `GitCommand::AmendMessage(String)`
  routed through `send_then_reload_graph`; `select_head_after_amend` one-shot
  re-selects the new `HEAD` in `on_graph` (oid changes on reword). UI:
  `commit_detail::message_block` double-click → editor (egui temp keyed by oid) →
  `GitIntent::AmendMessage`. Tests: 3 domain (reword / preserve / blank) + 4 UI e2e
  (open-prefilled / emit / cancel / read-only-off-HEAD).
  *Files*: `src/git/commit.rs`, `src/git/worker.rs`, `src/app/git_session.rs`,
  `src/app/render.rs`, `src/ui/commit_detail.rs`, `src/ui/git_panel.rs`,
  `src/app/keys.rs`, `src/ui/mod.rs`.

---

## ☑ Milestone — M-PR5 · PR detail → mockup parity

Spec: [`specs/pull-requests.md`](../pull-requests.md) §4/§11 +
[`design-system.md`](../design-system.md) §4. Brought the **PR detail**
(center, no file open) up to the sent mockup: a **carded** layout (author block /
meta-row / description card / conversation cards), **reviewers + labels + role
pills**, a **Created … ago** age and an **Oldest/Newest** conversation toggle.
Locked decisions: **labels = GitHub-only** (Bitbucket Cloud has no PR labels →
empty); **linked issue / conversation filter / reactions = cut** (no backing — the
Jira key already leads the title); reviewers wired from the existing
`PullRequest.reviewers` (GitHub-populated, **BB empty in v1**). Counter: **7/7**.

- ☑ **T1 — Reviewers cluster + role pills.** Detail meta-row reuses
  `reviewer_stack` (allocated rect → painter); `comment_role` derives **Author** /
  **Reviewer** neutral tags on conversation cards (never `accent.ai`).
  *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **T2 — Labels (GitHub fetch).** `PullRequest.labels: Vec<String>`; GitHub
  `LIST_FIELDS += labels`, `parse_pr` maps `o["labels"][].name`; Bitbucket empty.
  Rendered as neutral pills in the meta-row (`neutral_pill`, §4 pill grammar), row
  hidden when reviewers + labels both empty.
- ☑ **T3 — Created date.** `PrDetail.created_at`; GitHub `DETAIL_FIELDS +=
  createdAt`, Bitbucket `created_on`. `model::relative_age` (pure, `now` injected)
  renders "Created … ago", right-aligned in the author row. (The list *Updated*
  column was later widened + humanized the same way — M-PR4 T2.)
- ☑ **T4 — Carded detail layout.** Author block + right-aligned Created age, a
  reviewers + labels meta-row, the **body in a `bg.surface` card**, conversation
  **comment cards** (1px `border.subtle` + radius `CARD_RADIUS`). **Token nitpicks
  needed no code change** — heading casing (band titles are 13pt strong primary, not
  the sidebar uppercase-muted header), body contrast, `#number` chip cursor and the
  verdict segmented fill already match §4/the mockup; documented the detail card in
  `design-system.md` §4.
- ☑ **T5 — Conversation sort.** `conversation_header` carries an **Oldest|Newest**
  toggle (`ui.data_temp` per PR url), shown with >1 comment; reverses the
  oldest-first list. Filter / reactions / linked-issue **cut**.
- ☑ **T6 — Body render fix.** `\r` stripped from body + comment text in both parsers
  (`github::parse_detail`, `bitbucket::parse_body`/`parse_comments`). The mockup's
  stray leading `·` was its own sample data (the header's `author · src → dest` is
  intentional), not a render leak.
- ☑ **T7 — Spec + STATE + verify.** Folded into `pull-requests.md` §4
  (`labels`/`created_at`) + §11 (layout), `design-system.md` §4 (tag variant + detail
  card); counter recomputed; `gen_pr_detail` screenshot reviewed (cards, meta-row,
  role tags, Created age, sort toggle all present).

### Next actions (M-PR5)
- Done. Bitbucket reviewers now wired (M-PR4 T2); labels stay GitHub-only.

**Post-milestone polish** (live-render gaps the mockup screenshots surfaced):
- ☑ **Markdown bodies + readability.** Description, conversation cards and inline-comment
  threads were leaking raw markdown (`## …`, `**…**`, `` `…` ``) and, once rendered via
  `egui_commonmark`, read too dense. Replaced by an in-house renderer
  (`pull_requests_view::markdown`, on `pulldown-cmark` — already in the tree via
  `egui_commonmark`) that builds `LayoutJob`s carrying controlled size (`MD_TEXT_SIZE`),
  intra-line leading (`MD_LINE_HEIGHT`) and **letter-spacing** (`MD_LETTER_SPACING`) —
  none of which `egui_commonmark` 0.23's plain `RichText` output can set. Handles
  headings, bold/italic/strike/inline-code, bullet + ordered lists, block quotes and
  fenced code blocks; `pulldown-cmark` keeps `cookie_name` / `upsertInDatabase` from
  italicizing on intraword underscores. The `CommonMarkCache` plumbing is gone.
- ☑ **Reply on every conversation card.** Was Bitbucket-only (the pill needed a forge
  `id`); GitHub conversation comments (`github::parse_detail` → `id: None`) had none.
  `ConversationEdit::Reply` now keys by **conversation index** (stable across the
  Oldest/Newest reversal) and `conversation_reply_block` takes the optional `parent`:
  threaded forges nest via `parent: Some(id)`, flat (GitHub) posts a new top-level
  comment (`parent: None`). Spec §11 updated.
- ☑ **Reply + composer restyle (mockup parity).** The Reply affordance is now a quiet
  neutral pill (`reply_pill` → `pill_button` with a message-square glyph), and the
  standalone "Add a comment" pill became an **always-visible composer bar** at the foot
  of the band — avatar, input field bound to `DiffViewState::conversation_add_buffer`,
  and a filled-accent **Comment** button, raising the same parent-less
  `PostConversationComment`. The button is right-anchored (`right_to_left` so the field
  can't push it off the band) and reads solid accent throughout, only submitting once
  the draft is non-blank. No paperclip/emoji icons (no backend → dead decoration).
  `ConversationEdit::Add` / `open_conversation_add` dropped.
- ☑ **Composer avatar = real current user.** The composer avatar shows the signed-in
  user's initials, resolved per forge from the PR runner's identity pass: GitHub via
  `gh api user --jq '.name // .login'` (`github::current_name_args`), Bitbucket via
  `display_name`/`nickname` off the `/2.0/user` reply
  (`bitbucket::parse_current_user_display_name`). `PrReply` carries `github_name` /
  `bitbucket_name` (only `Some` on the first reply that resolves identity); the app
  keeps them in `HelmApp::pr_user_github` / `pr_user_bitbucket` and feeds the one
  matching `pr.forge_kind` into `PrReviewView::current_user`. `None` ⇒ the plain dot.
- ☑ **Loaders for list + detail fetches.** Cold list/refresh showed nothing (the cache
  reads `Absent`/`Absent` before the first reply ⇒ the misleading "No repository" empty
  state); detail opened to empty body/checks/conversation shells reading as "nothing
  here". Browse list now takes a `PrSourceHints::loading` (`!cache.loaded ||
  runner.busy()`): the refresh icon becomes a spinner and the body shows a centered
  "Loading pull requests…" instead of the empty state. Review center takes
  `PrReviewView::detail_loading` (`detail.is_none() && detail_error.is_none()`): a
  spinner + "Loading pull request…" stands in for the detail sections. Mirrors the
  existing `files_loading`/`diff_loading` rail placeholders.
- ☑ **Comment-card overhaul + code snippets + resolved threads.** Every comment card
  (overlay, center inline, conversation) now carries the **avatar + author** in
  `text.primary` (no more per-author hue), an **Author/Reviewer** role tag, a relative
  **age** (`model::relative_age`), **threaded replies indented**, a **selectable** markdown
  body (the card no longer swallows clicks — the snippet is the open target), and a
  **multi-row composer**. Code-anchored cards embed a **few-line code snippet**
  (`detail::code_snippet`): a numbered gutter + `+`/`-`/space signs tinted green/red/neutral.
  Source: GitHub `diff_hunk` → `model::hunk_snippet` (kept to the `INLINE_SNIPPET_LINES`
  tail ending at the anchor); Bitbucket (no hunk) windows the loaded diff, else nothing.
  **Resolve/Reopen** toggle on inline threads → `ReviewIntent::ResolveThread`: GitHub via a
  GraphQL `reviewThreads` join (`databaseId → (PRRT node id, isResolved)`,
  `apply_thread_resolution`) + `resolve/unresolveReviewThread` mutation on the node id;
  Bitbucket via `POST …/comments/{id}/resolve` (resolve) / `DELETE` (reopen, 204).
  `PrComment`/`ThreadComment` gained `created_at` / `resolved` / `thread_id`. *Files*:
  `src/pull_requests/{model,github,bitbucket,runner}.rs`, `src/app/mod.rs`, `src/review.rs`,
  `src/ui/{detail,pull_requests_view,diff_view}.rs`. *Tests*: domain (`hunk_snippet`,
  `relative_age`), I/O (`parse_review_threads`/`apply_thread_resolution`/`resolve_thread_args`,
  BB resolution parse + `resolve_comment_url`), UI e2e (snippet open target, resolve pill →
  `ResolveThread`). **Live forge round-trips unverified** (no creds/network here).
- ☑ **Conversation replies nest by `parent_id`.** The center Conversation band listed
  every `path.is_none()` comment as a flat top-level card, ignoring `parent_id` — so a
  Bitbucket conversation reply stood as its own card. `conversation_section` now groups
  comments into threads by walking the `parent_id` chain (GitHub stays flat: id/parent
  both None ⇒ one comment per thread), rendering the root then its replies **indented**
  with **one Reply affordance per thread** (nesting under the root id). *Files*:
  `src/ui/pull_requests_view.rs`. *Test*: `conversation_reply_nests_under_its_parent`.
- ☑ **Inline-comment code preview on the conversation page.** Bitbucket inline comments
  carry no forge `diff_hunk`, and the conversation page has no file open, so the center
  inline cards showed only the bare "Open …" link. `poll_pr_review` now prefetches the
  local diff of every commented file lacking a hunk (`ensure_comment_diffs`, deduped via
  `PrReview.comment_diff_requests`), and the view passes the current-range diffs
  (`PrReviewView.comment_diffs`) so `inline_snippet` windows `source_lines` for any
  commented file — not just the open one. *Files*: `src/app/mod.rs`, `src/app/render.rs`,
  `src/ui/pull_requests_view.rs`. *Test*: `inline_comment_card_windows_comment_diff_when_no_hunk`.
- ☑ **Accordion resolved threads + richer snippets.** Resolved threads (center inline +
  conversation) collapse to a one-line **"Resolved · N comments"** summary row (tick +
  chevron), re-opening on click — per-thread expand state in `DiffViewState.expanded_resolved`
  (keyed by the root comment id; `is_resolved_expanded`/`toggle_resolved`); no forge/parser
  change. Snippets gained **syntax highlighting** (`detail::code_snippet` highlights the code
  via `syntax_highlight::highlight_buffer` from the file path; the `+`/`-` sign keeps its kind
  colour) and more context: `INLINE_SNIPPET_LINES` 4→8, and the Bitbucket fallback now windows
  the loaded diff's **hunk** (`diff_window_snippet`, colored add/delete) instead of flat
  `source_lines` when a hunk covers the anchor. *Files*: `src/ui/{detail,pull_requests_view,diff_view}.rs`.
  *Test*: `resolved_inline_thread_collapses_and_reopens_on_click`.
- ☑ **Unified comment-card grammar (conversation + inline + overlay).** The three comment
  surfaces shared no visual language; they now wear one **Detail card** grammar (`bg.surface`
  + `border.subtle` + 10pt radius + 12pt padding) via `comment_frame`. A thread renders as a
  single card — root at full weight, replies a notch in under a left **thread-rail** with a
  lighter avatar (`detail::author_avatar_small`, 20pt) — through the shared `thread_members` /
  `comment_meta_row` helpers; the **age** moved to the card's right edge, and the Reply/Resolve
  controls moved **inside** the card foot. Spacing unified on a `GAP_XS/SM/MD` (4/8/12) scale,
  the Oldest|Newest toggle became a real **segmented control** (`order_segment`, `accent.subtle`
  active), and the diff-overlay `thread_card` dropped its tinted-pill+left-edge look for the same
  card. *Files*: `src/ui/{detail,pull_requests_view,diff_view}.rs`. No new test (covered by the
  existing 30 PR-view render/label tests).
- ☑ **Comment-card readability polish.** Screenshot review showed the grammar was correct but
  still read flat. Each comment now lays out as an **avatar gutter + text column** (`comment_block`
  / `comment_meta_line`): the body aligns under the author instead of sliding back under the
  avatar, and replies nest by a wider gutter under a `border.input` thread-rail on the root
  avatar's centre. The conversation **composer** (`conversation_add_block`) is a quiet single-line
  field (surface fixed to `bg.surface` via `extreme_bg_color`) with a compact Comment button below
  it. The whole detail (overview, description, checks, comment threads) spans the full panel
  width. Section headers reach **parity** — both *Conversation* and *Inline comments* carry a
  `count_chip`; comment **age** brightened to `text.secondary`; author avatars desaturated a touch
  (`detail::muted_lane`). *Files*: `src/ui/{detail,pull_requests_view}.rs`. Verified headlessly
  (31 PR-view render/label tests + a one-off wgpu screenshot, since removed).

### Blockers / Open questions (M-PR5)
- **Bitbucket reviewers**: resolved in M-PR4 T2 (`fields=+values.participants` on the
  list query → parsed from `participants`). **Labels** stay GitHub-only — Bitbucket
  Cloud has no PR labels (genuinely absent, nothing to fetch).
- **List *Updated* column**: resolved in M-PR4 T2 — humanized via `model::relative_age`
  after the column was widened.

---

## ☑ Milestone — M-PR4 · PR list redesign (full-width table)

Spec: [`specs/pull-requests.md`](../pull-requests.md) §5 (reconciled to a mockup).
Turns the browse list from compact two-line rows into a **full-width column
table** per role group, in English. Locked design decisions: **EN** labels, **no
tabs** (the two role groups are the only split), **full-width list** (master →
detail, unchanged §11 surface), header = **title + Refresh** only (no global
search / notifications / theme / `+`), **no filters / sort**; each group is a
**rounded card** (section title + count above it, hairline row separators);
**Status = a plain colored label** (state + review decision) — no pill, CI checks
live in the §11 detail (matched to the sent mockup). Counter: **2/2**.

- ☑ **T1 — Browse list → carded column table.** `render_list` gains a page header
  (`Pull Requests` title + Refresh button → `PullRequestsPageAction.refresh`);
  each group renders a **card** (reserved backdrop shape + `border.subtle` stroke)
  holding a column-header row + table rows with hairline separators. Per-section
  columns — **To review**: Title (+ branch) · Project · Author · Reviewer ·
  Status · Updated · ›; **Mine**: Title (+ branch) · Project · Reviewers · Status
  · Updated · ›. Cell helpers: `columns` (per-role x-ranges), `paint_avatar` /
  `reviewer_stack` (overlapping initials avatars + `+N`, ring tinted by reviewer
  state), `pr_status` (state/review → colored label). *Files*:
  `src/ui/pull_requests_view.rs`, `src/app/render.rs`. *Tests*: PR-cockpit UI e2e
  (groups, rows, select + Refresh intents); `headless-verify` (`shots_gen::gen_pr_list`).
- ☑ **T2 — Calibration + relative age + BB reviewers (feedback).** `columns` now
  caps Title (`TITLE_MIN_W`/`TITLE_MAX_W`) and spreads the fixed data columns with
  an even computed gap to the right edge (was right-anchored → Title hogged all
  slack on wide windows); widened **Updated** (46→96) and humanized it via
  `model::relative_age(updated_at, now)` (falls back to raw on unparseable input).
  Bitbucket reviewers now populate: `role_filtered_url` adds `fields=+values.participants`,
  `parse_pr` reads the roster + per-reviewer decision from `participants`
  (`aggregate_review` collapses it for the Status column). *Files*:
  `src/ui/pull_requests_view.rs`, `src/pull_requests/bitbucket.rs`. *Tests*:
  `bitbucket::parse_list_reads_reviewers_and_review_from_participants`; wide
  `gen_pr_list` shot reviewed.

### Next actions (M-PR4)
- Done. Proceed to M-PR5 (PR detail parity) when ready.

### Blockers / Open questions (M-PR4)
- none.

---

## ☑ Milestone — M-PR3 · PR review: cache & richer reviewing

Spec: [`specs/pull-requests.md`](../pull-requests.md) §5/§6/§11. Makes the cockpit
navigation instant (per-PR cache + diff cache), adds a **per-commit** view,
surfaces **inline comments in the center with code context**, and lifts the §10
limits on **inline replies** and **conversation comments** (add + reply).
Counter: **9/9** — complete, pending review + merge of the `m-pr` worktree branch.

- ☑ **T1 — Per-PR review cache.** Replace `pr_review: Option<PrReview>` with a
  bounded (~8) `HashMap<PrReviewKey, PrReview>` + the active key; `open_pr_review`
  adopts a cached entry **without re-running the runners** (instant), else builds
  + fetches. Preserve per PR: `draft`, `agent_notes`, `summary`, `verdict`,
  `selected_file`, `diff_view`, `detail`, `files`. Re-open re-fetches only when the
  entry is older than ~60 s, swapping on arrival without touching drafts. Pure
  `LruOrder<PrReviewKey>` helper (touch/evict) kept out of the rendering layer.
  *Files*: `src/app/mod.rs`, a small pure LRU module. *Tests*: unit on the LRU;
  UI e2e — PR A → open file → PR B → back to A shows no spinner, drafts intact.
- ☑ **T2 — Diff cache within a PR.** `PrReview.diffs: HashMap<(Oid,Oid,String),
  FileDiff>` instead of the single `diff`/`diff_path` slot (key includes base/head
  — required by T5); `ensure_selected_diff` fetches on miss only, the poll handler
  warms the cache even when the user has switched away. *Files*: `src/app/mod.rs`,
  `src/app/render.rs`. *Tests*: in-crate app — A→B→A across cached files fires no
  fetch; a cache miss fires exactly one.
- ☑ **T3 — Throttle the focus-regained list refetch.** Pure predicate
  `should_refresh_pr(cold, repos_changed, focus_regained, age, min_age)` gates the
  `focus_regained` branch by a 30 s min age on `last_pr_poll`; cold / repos_changed
  fire unconditionally and the periodic 60 s tick is kept at the call site. *Files*:
  `src/app/mod.rs`. *Tests*: unit on the predicate.
- ☑ **T4 — Commits in the detail fetch.** `PrCommit { sha, short, subject, author }`
  + `PrDetail.commits` (oldest-first). GitHub: `commits` added to `DETAIL_FIELDS` +
  `parse_commit` (oid→7-char short, `messageHeadline`, first author name/login).
  Bitbucket: paginated `commits_url` + `parse_commits` (hash, first message line,
  `display_name`/raw), reversed to oldest-first in the runner. *Files*:
  `src/pull_requests/model.rs`, `github.rs`, `bitbucket.rs`, `runner.rs`. *Tests*:
  unit on both fixtures.
- ☑ **T5 — Per-commit diff.** Commit band in the rail (above Files changed);
  selecting a commit recomputes files+diff over `commit^..commit` (explicit
  base/head), "All commits" = the current three-dot diff. Reuses
  `pr_changed_files`/`pr_file_diff` with other oids (local after `fetch
  pull/N/head`); `selected_commit` state in `PrReview`; a files request taking
  explicit base/head. *Files*: `runner.rs`, `mod.rs`, `pull_requests_view.rs`.
  *Tests*: business e2e (single-commit delta on a throwaway repo); UI e2e (band +
  selection).
- ☑ **T6 — Inline comments in the center, with code context (read).** `PrComment`
  gains `context: Option<String>` (GitHub `diff_hunk` from the already-fetched
  comments payload — no extra request; Bitbucket: window derived from the loaded
  `FileDiff`, else `None`). A new **Inline comments** section in the center detail,
  grouped per file: each card = a small monochrome code snippet over the thread;
  clicking opens the file at that line (`select_pr_file` + scroll). The diff overlay
  stays. *Files*: `model.rs` (`context` + `forge_threads`/parse), `github.rs`
  (capture `diff_hunk`), `bitbucket.rs` (fallback), `pull_requests_view.rs`.
  *Tests*: unit on `diff_hunk` parse; UI e2e — inline card with snippet in the
  center, click emits `select_file`.
- ☑ **T7 — Reply to an inline comment (write).** Thread `id` plumbed to the cards
  (`review::ThreadComment` gains `id`); a reply editor on **both** renders (diff
  overlay **and** T6 center card). Post: GitHub `POST pulls/{n}/comments/{id}/
  replies`; Bitbucket `{content.raw, parent:{id}}`. Detail refetches on success.
  *Files*: `review.rs`, `model.rs`, `github.rs`, `bitbucket.rs`, `runner.rs`,
  `mod.rs`, `diff_view.rs`/`pull_requests_view.rs`. *Tests*: unit on reply builders;
  UI e2e emits the reply intent from both places.
- ☑ **T8 — Conversation comments: add + reply (write).** Standalone composer in the
  Conversation section + reply on top-level cards. GitHub `POST issues/{n}/comments`;
  Bitbucket `POST .../comments` (no inline, `parent` for the reply). `ReviewIntent::
  PostConversationComment { parent: Option<u64>, body }` unifies add (`None`) and
  card reply (`Some(id)`); the runner gained `PrPostKind::Conversation` +
  `request_conversation`/`post_conversation`, reusing the §11 success-refetch path.
  *Files*: `review.rs`, `github.rs`, `bitbucket.rs`, `runner.rs`, `mod.rs`,
  `diff_view.rs`, `pull_requests_view.rs`. *Tests*: unit on `issue_comment_args`; UI
  e2e add (`conversation_composer_emits_post_conversation_comment`) + reply
  (`conversation_card_reply_emits_nested_post_conversation_comment`).
- ☑ **T9 — Spec + STATE + verify.** Folded M-PR3 into `pull-requests.md`: §4 model
  (`PrComment.context`, `PrCommit`, `PrDetail.commits`), §5 (inline-context cards +
  per-commit view in the center), §6 (bounded per-PR review cache + per-file diff
  cache → instant revisit; focus-regained 30 s throttle), §10 (replies &
  conversation comments lifted out of scope), §11 (M-PR3 block: per-commit view,
  inline comments in center, thread reply, conversation add/reply) + §9 test
  catalogue. `headless-verify` PASS — the review detail (inline-context card,
  Conversation composer + replies, commit band) renders and the conversation-card
  reply emits a nested `PostConversationComment`.

### Next actions (M-PR3)
- **Review then merge** the `m-pr` worktree branch into `main` (the milestone loop
  does not merge/push — user's call).

### Blockers / Open questions (M-PR3)
- none. Note: T7/T8 extend the frozen §10/§11 scope — fold into §11 in T9. Cache
  diff key (T2) must include `(base,head)` or the per-commit view (T5) serves a
  stale diff; T6/T7/T8 can share one `PrCommentRunner` (reply inline, reply/add
  conversation) + the success refetch.

---

## ☑ Milestone — M-PR2 · In-app PR review (diff · line comments · submit · Ask Claude)

Spec: [`specs/pull-requests.md`](../pull-requests.md) §11. Turns the read-only
detail panel into a **diff-centric review surface**: PR diff without cloning,
in-diff line comments, **Submit review** (Comment / Approve / Request changes) on
GitHub **and** Bitbucket Cloud, and **Ask Claude** on an existing thread. Reuses
the M-RC review engine (`review.rs`, `ui::diff_view`). Counter: **7/7** — complete,
pending review + merge of the `m-pr` worktree branch.

- ☑ **T1 — PR diff producer (domain).** `git::diff::pr_changed_files` +
  `pr_file_diff` over the three-dot `merge-base(base,head)..head` range (I/O-free).
  *Files*: `src/git/diff.rs`. *Tests*: business e2e on a throwaway repo (delta +
  three-dot isolation).
- ☑ **T2 — Wire PR detail fetch.** Base/head resolution in the matched workspace
  repo; changed-files + per-file diff as gated requests on the detail runner path.
  *Files*: `src/app/*`, `src/pull_requests/runner.rs`.
- ☑ **T3 — Detail panel → diff-centric.** Rail of changed files + lazy diff via
  `ui::diff_view`; header keeps Open / Checkout. *Files*:
  `src/ui/pull_requests_view.rs`, `src/app/{mod,render}.rs`.
- ☑ **T4 — Inline existing comments.** Posted threads overlay the diff anchored at
  their line, read-only, via `review::ForgeThreads`. *Files*: `src/review.rs`,
  `src/ui/diff_view.rs`.
- ☑ **T5 — Draft + post review (write).** Footer composer (verdict + summary +
  Submit (N)); `model::draft_comments` flattens the draft store; gated
  `PrPostRunner` posts GitHub `…/reviews` (`gh api --input -`) and Bitbucket inline
  comments + `approve`/`request-changes` (`curl`, Keychain auth); success resets
  the draft and refetches. *Files*: `src/pull_requests/{model,github,bitbucket,
  runner}.rs`, `src/ui/{pull_requests_view,diff_view}.rs`, `src/app/{mod,render}.rs`.
  *Tests*: builder units + 2 business e2e (GitHub payload, Bitbucket bodies/URLs) +
  UI e2e (Submit emits the intent).
- ☑ **T6 — Ask Claude on a thread.** `ReviewIntent::AskAgentOnThread { file, line }`
  from a per-thread **Ask {agent}** pill; the app builds the prompt from that
  thread and launches the agent in the PR worktree (shared `launch_pr_agent`).
  *Files*: `src/review.rs`, `src/ui/diff_view.rs`, `src/app/mod.rs`. *Tests*: UI
  e2e (pill emits the anchored intent).
- ☑ **T7 — Spec + STATE + verify.** `specs/pull-requests.md` §11 (+ §4/§5/§9/§10
  reconciled); this block; full `cargo fmt` + `clippy --all-targets -D warnings` +
  `cargo test` green.

### ☑ Review-surface fix pass (post-T7)
- ☑ **Rail in commit-detail language.** `review_rail` mirrors `ui::commit_detail`:
  author avatar + name + `source → dest`, PR body, a **Files changed** band (count
  chip + ±totals + ratio bar), file rows with separators, bold section titles.
  *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **Conversation = top-level only.** The rail's Conversation lists comments with
  no `path`/`line`; inline ones stay anchored in the diff (`ForgeThreads`).
  *Files*: `src/ui/pull_requests_view.rs`. *Tests*: UI e2e (inline author absent
  from the rail).
- ☑ **Header clears the global buttons.** PR cockpit reserves `TITLEBAR_HEIGHT`
  like the other central modes. *Files*: `src/app/render.rs`.
- ☑ **Ask Claude on a worktree-less branch.** `PendingPrAsk` defers the prompt
  across checkout + create, then `resume_pending_pr_ask` launches the agent in a
  new tab once the worktree is live and any post-create script is consumed.
  *Files*: `src/app/{mod,render}.rs`.

### ☑ Review-surface fix pass v2 (user feedback)
- ☑ **Rail moved to the RIGHT, collapsible.** `render_review` now fills the main
  area with the diff and puts the changed-files rail on the right (commit-detail's
  place); a header toggle (`PanelRightClose`/`Open`) collapses it, persisted as
  `Prefs::pr_rail_collapsed`. Resize handle drag inverted (drag left widens).
  *Files*: `src/ui/pull_requests_view.rs`, `src/app/{mod,render}.rs`,
  `src/persistence.rs`. *Tests*: UI e2e (toggle intent; collapsed hides files).
- ☑ **Per-line icon back to hover-only.** Reverted `DiffReview.always_comment`;
  the gutter button paints on hover only, as before. *Files*: `src/ui/diff_view.rs`,
  `src/ui/pull_requests_view.rs`, `src/app/render.rs`.
- ☑ **Comment affordance reads as a comment.** Gutter icon `Sparkles` → 
  `MessageSquarePlus`; the Sparkles now marks only the separate Send-to-agent pill,
  so the per-line action no longer reads as "AI". *Files*: `src/ui/diff_view.rs`.

### ☑ Review-surface fix pass v3 (user feedback)
- ☑ **Header less prominent, on canvas.** `review_header` paints on
  `palette.bg_canvas` (was `bg_sidebar`); PR title `14.5` → `13.5`.
  *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **Rail = commit-detail background, hideable like the git sidebar.** `review_rail`
  + `review_composer` on `bg_canvas` (was `bg_sidebar`), matching `commit_detail`.
  In the PR cockpit the standard git toggle is suppressed, so **⌘G** now flips
  `pr_rail_collapsed` (alongside the header toggle). *Files*:
  `src/ui/pull_requests_view.rs`, `src/app/render.rs`.
- ☑ **Both per-line affordances.** The hover gutter now shows **two** icons:
  `MessageSquarePlus` (slot 0 → forge draft comment) and `Sparkles` (slot 1 →
  `ReviewIntent::AskAgentOnLine` → `ask_claude_on_line` → `launch_pr_agent`, quoting
  the line). PR review only (`DiffReview.line_agent`); working-tree/commit diffs keep
  the single comment icon. *Files*: `src/review.rs`, `src/ui/diff_view.rs`,
  `src/app/mod.rs`, `src/ui/pull_requests_view.rs`. *Tests*: UI e2e
  (`clicking_the_line_sparkles_emits_ask_agent_on_line`).

### ☑ Review-surface fix pass v4 (user feedback)
- ☑ **Gutter note icon is surface-aware.** Working-tree / commit diffs show the
  `Sparkles` (AI) glyph (the note only feeds the agent); the PR diff shows
  `MessageSquarePlus` (the note is a **forge review comment**, posted to GitHub /
  Bitbucket via *Submit review (N)*, and also batchable to the agent via *Send to
  {agent}*). Picked from `DiffSurface::forge_review()`. *Files*: `src/ui/diff_view.rs`.
- ☑ **Per-line direct agent launch dropped — PR notes batch like the others.** The
  PR diff no longer has a second `Sparkles` button that launches the agent on one
  line; its gutter is the single note button, recording a draft comment handed off
  as a batch via *Send to {agent}*. Removed `ReviewIntent::AskAgentOnLine`,
  `DiffSurface::line_agent`/`claude_button`/`AskLineAgent`, `ask_claude_on_line`,
  `line_agent_prompt`. The existing-thread *Ask {agent}* pill (`AskAgentOnThread`)
  is unchanged. *Files*: `src/review.rs`, `src/ui/diff_view.rs`, `src/app/mod.rs`.
  *Tests*: UI e2e (`pr_surface_note_button_records_a_draft_comment`).

### ☑ Review-surface fix pass v5 (user feedback)
- ☑ **PR diff has two separate note pools.** A forge pool (`PrReview.draft` →
  `MessageSquarePlus`, slot 0 → *Submit review (N)* → GitHub / Bitbucket) and an
  agent pool (`PrReview.agent_notes` → `Sparkles`, slot 1 → *Send to {agent}* recap
  + whole-PR *Ask Claude*), kept apart so forge review comments are never forced
  through the agent. New `review::ReviewPool { Agent, Forge }` tags
  `SaveComment`/`DeleteComment`; `DiffReview` gained `forge: Option<&FileComments>`,
  `DiffViewState.active_comment` a pool key, `comment_block`/`save_note`/
  `open_inline_editor` a `pool` arg; `apply_pr_review_intents` routes by pool and
  `ask_claude_on_pr` now reads `agent_notes` (not the forge draft). Working-tree /
  commit diffs keep the single agent `Sparkles` pool (`forge: None`). *Files*:
  `src/review.rs`, `src/ui/diff_view.rs`, `src/app/mod.rs`, `src/app/render.rs`,
  `src/ui/pull_requests_view.rs`, `specs/pull-requests.md`. *Tests*: UI e2e
  (`pr_forge_button_records_a_forge_pool_comment`,
  `pr_agent_button_records_an_agent_pool_note`).

### ☑ Review-surface fix pass v6 (user feedback)
- ☑ **Rail toggle moved to the title bar.** The changed-files rail collapse button
  now lives in `top_right_actions` (the slot the suppressed git toggle vacates in a
  Helm central mode), same `sidebar_toggle` glyph + ⌘G shortcut as the other
  sidebars. `root_layout`/`top_right_actions` thread `pr_rail_collapsed` +
  `&mut pr_toggle_rail`; the in-view header toggle and `PullRequestsPageAction::toggle_rail`
  are gone. *Files*: `src/ui/mod.rs`, `src/app/render.rs`, `src/ui/pull_requests_view.rs`.
- ☑ **Full-width header band removed.** `review_header` + `rail_toggle_button`
  deleted; the diff now fills the whole surface. Later passes moved Back, the PR
  title and PR-level actions into the center detail, leaving rail collapse scoped
  to the changed-files list + composer. *Files*: `src/ui/pull_requests_view.rs`.
  *Tests*: UI e2e (collapsed rail hides files; toggle-intent test dropped).
- ☑ **Homogeneous button radius.** New `theme::RADIUS_BUTTON = 4` (matches the git
  sidebar commit button); the PR back / action / verdict / Submit buttons drop their
  hardcoded `6.0`. *Files*: `src/theme.rs`, `src/ui/pull_requests_view.rs`.

### ☑ Review-surface fix pass v7 (user feedback)
- ☑ **No file is force-opened; Close ≠ Back.** The review surface opens with **no
  file selected** (the "select a file" placeholder) instead of auto-selecting the
  first changed file. The diff's **Close** (and `Esc` over the diff) now clears the
  selection back to that placeholder **without leaving the surface**, distinct from
  **Back** (which still returns to the list). New
  `PullRequestsPageAction::close_file` + `App::close_pr_file`; dropped the
  auto-select in `poll_pr_review`. *Files*: `src/ui/pull_requests_view.rs`,
  `src/app/mod.rs`, `src/app/render.rs`, `specs/pull-requests.md`. *Tests*: UI e2e
  (`closing_the_open_file_emits_close_not_back`).

### ☑ Review-surface fix pass v8 (user feedback)
- ☑ **PR detail moved to the center; rail kept lean.** With no file open the center
  area now hosts the PR **detail** (author + `source → dest`, body, Checks,
  conversation) instead of an empty placeholder; the right rail keeps only the PR
  heading + actions (Back / Open / Checkout), the **Files changed** band + file
  list and the composer. Selecting a file still swaps the center to its diff. Pure
  view re-layout (new `review_detail`), no app/state change. *Files*:
  `src/ui/pull_requests_view.rs`, `specs/pull-requests.md`. *Tests*: UI e2e updated
  (`detail_conversation_lists_only_top_level_comments`,
  `collapsed_rail_hides_the_changed_files_but_keeps_the_center_area`).

### ☑ Review-surface fix pass v9 (user feedback)
- ☑ **Back + title moved to the center; rail header dropped.** The center detail
  now leads with a **Back** control and the PR **title**; the rail's old header row
  (Back + PR state icon + `#number` + title) is gone, so the sidebar gains vertical
  space for the Files changed band + file list + composer. Back lives in the detail
  (a diff's **Close** returns there). Pure view re-layout — `review_detail` gained
  the header + an `action` param. *Files*:
  `src/ui/pull_requests_view.rs`, `specs/pull-requests.md`. *Tests*: covered by the
  existing PR-cockpit UI e2e (Back/Open/Checkout still resolve).

### ☑ Review-surface fix pass v7 (user feedback)
- ☑ **Rail reaches the window top like a real side panel.** `render_review` now
  receives the full-height central rect; the rail frame (its divider + background)
  spans to the title strip while the diff/detail and the rail's scroll content inset
  past `TITLEBAR_HEIGHT` (where the floating toggle/feedback/prefs icons sit). The
  browse list keeps its inset body. *Files*: `src/ui/pull_requests_view.rs`.

### ☑ Comment-card restyle (user feedback)
- ☑ **Three distinct comment identities, compact, below the line.** The diff's
  per-line comment surfaces share one tinted-card-with-left-edge grammar in three
  inks: forge review draft = `accent` (MessageSquarePlus), agent note = new
  `accent.ai` violet (Sparkles), fetched PR thread = neutral `text.muted` (avatar +
  author). The inline composer dropped its card+header wrapper back to a bare field
  + compact Delete/validate footer; the saved note card dropped its title +
  right-anchored `Lnn`, and comment blocks are no longer indented to the code column
  — the body now flows left-aligned directly under its line. New
  `theme::Palette.accent_ai` (+ every preset); `pool_style`/`comment_block` lost
  their `agent` arg. *Files*: `src/theme.rs`, `src/ui/diff_view.rs`,
  `specs/design-system.md`. *Tests*: existing diff_view + PR-view UI e2e (a11y labels
  unchanged); on-demand shot `gen_pr_review_comments` (`tests/shots_gen.rs`).

### ☑ PR review polish pass (user feedback)
- ☑ **Rail file list plugged into Flat ⇄ Tree.** PR changed files reuse
  `Prefs.git_file_view`, `file_list::view_toggle` and `git::file_tree` with
  session-only collapsed directories. *Files*: `src/ui/pull_requests_view.rs`,
  `src/app/render.rs`.
- ☑ **Composer is a true segmented control.** Comment / Approve / Request changes
  render as one segmented control; the primary button label names the submitted
  action and disables empty Comment reviews. *Files*: `src/ui/pull_requests_view.rs`.
- ☑ **Review signals are calmer per file.** Viewed state moved to an icon-only
  unread filter chip in the Files changed header; rows keep only muted forge-draft
  and agent-note icons with no counts. Existing-thread Ask action moved inside the
  thread card. *Files*: `src/ui/{pull_requests_view,diff_view}.rs`. *Tests*: PR
  rail UI e2e + diff thread intent e2e.
- ☑ **PR-level actions live in the center detail.** Open in browser / Checkout
  moved out of the rail and only appear when no file diff is open.
- ☑ **Center detail header compacted.** The PR detail now starts with a full-width
  header: Back on the left, title + `author · source → dest`, compact
  Open-in-browser / Checkout actions on the right, and a `#number` chip.

### Next actions (M-PR2)
- **Review then merge** the `m-pr` worktree branch into `main` (the milestone loop
  does not merge/push — user's call).

### Blockers / Open questions (M-PR2)
- none

---

## ☑ Milestone — M-PR · Pull Requests cockpit

Spec: [`specs/pull-requests.md`](../pull-requests.md). Entry below Agents listing
my PRs + PRs to review, from GitHub (`gh`) and Bitbucket Cloud, scoped to the
workspace repos. Counter: **9/9** — complete, pending review + merge of the
`m-pr` worktree branch.

- ☑ **PR1 — Domain model.** `pull_requests::model`: `PullRequest`, `PrRole`,
  `PrState`, `Checks`, `Review`, `Reviewer`, `PrDetail`. Reuse
  `git::forge::{Forge, parse_remote}`. *Files*: `src/pull_requests/model.rs`,
  `src/lib.rs`. *Tests*: unit on role/dedupe-by-`(forge,number)` helpers.
- ☑ **PR2 — GitHub source (pure).** `pull_requests::github`: `gh` arg builders
  (`pr list/view/checkout`, `api user`) + I/O-free `parse_list`/`parse_detail`
  over `--json` fixtures; `gh auth status` availability probe shape. *Files*:
  `src/pull_requests/github.rs`, `tests/fixtures/`. *Tests*: unit on captured
  JSON (open/draft, checks, review, reviewers).
- ☑ **PR3 — Bitbucket source (pure).** `pull_requests::bitbucket`: Cloud `2.0`
  URL builders + Basic-auth header + I/O-free `parse_list`/`parse_detail`.
  Keychain creds via `security` CLI (service `helm.bitbucket`); `bitbucket_email`
  in `Prefs`. *Files*: `src/pull_requests/bitbucket.rs`,
  `src/pull_requests/creds.rs`, `src/persistence.rs`. *Tests*: unit on JSON
  fixtures + URL/header builders.
- ☑ **PR4 — PrRunner + cache.** Detached one-shot runner (architecture §3
  contract: one reply/request, `in_flight`, drain+repaint each frame); fans
  per-`Forge` queries, classifies roles vs cached identity → `Vec<PullRequest>`
  + per-source status; app cache + refresh cadence (entry / manual / ~60 s
  focused tick). *Files*: `src/pull_requests/runner.rs`, `src/app/*`. *Tests*:
  business e2e on command/URL construction (no live network).
- ☑ **PR5 — Sidebar entry + central mode.** `Pull Requests` row below Agents
  (`Icon::GitPullRequest`), To-review count badge; `CentralMode::PullRequests`
  (sidebar stays, git panel hides); `SidebarAction.open_pull_requests`. **No
  keyboard shortcut.** *Files*: `src/ui/repo_sidebar.rs`, `src/ui/mod.rs`,
  `src/app/{mod,render}.rs`. *Tests*: UI e2e (entry renders, click sets mode).
- ☑ **PR6 — Cockpit page.** `ui::pull_requests_view::pull_requests_page`: two-pane
  (list grouped To review / Mine + detail panel: description, checks, reviewers,
  read-only comments; actions Open in browser / Checkout), resizable split
  `Prefs.pr_detail_width`. *Files*: `src/ui/pull_requests_view.rs`, `src/ui/mod.rs`,
  `src/persistence.rs`, `src/app/{mod,render}.rs`.
  *Tests*: UI e2e on a fixture list (groups, rows, select, action intents).
- ☑ **PR7 — Checkout = worktree.** If a worktree already sits on the PR source
  branch ⇒ activate that row (no git write); else fetch the branch (GitHub
  `pull/<n>/head`, Bitbucket `origin/<source>`) and create a worktree on it via
  `git::worktree::CreateRunner`, then activate it. *Files*:
  `src/pull_requests/runner.rs` (`CheckoutRunner` + `fetch_refspec`/`match_pr_root`/
  `matching_worktree`), `src/app/{mod,render}.rs`; reuse `src/git/worktree.rs`.
  *Tests*: unit on fetch-ref/argv build + existing-worktree match.
- ☑ **PR8 — Preferences section.** Preferences → **Pull Requests**: Bitbucket
  email field + Save token (Keychain), GitHub/Bitbucket `gh` status lines from the
  PR cache (section-open tick warms it); persist `bitbucket_email`,
  `pr_detail_width`. *Files*: `src/ui/preferences.rs`, `src/app/{mod,render}.rs`,
  `specs/preferences.md`. *Tests*: UI e2e on the section (nav opens it, statuses
  surface, email change + Save intents).
- ☑ **PR9 — End-to-end verify.** `headless-verify` on a fixture cockpit: To
  review / Mine groups + rows, the selection's detail (description, checks,
  reviewers, comments) and the Open-in-browser / Checkout actions all render and
  route their intents. Demonstrable milestone scenario (DoD).

- ☑ **Hardening — branch review pass.** Resilience + correctness fixes across the
  PR feature: per-source `Option<Vec<PullRequest>>` reply so a transient GitHub or
  Bitbucket failure keeps last-good rows + flags `PrCache.stale` (never blanks the
  cockpit); Bitbucket token off `curl` argv (stdin `--config -`) + connect/max
  timeouts + page-follow `next` pagination; `Esc` on a loaded PR diff no longer
  discards the draft; PR runner drained on the Preferences page; checkout from the
  review surface targets `review.pr`; selection reconciled after refresh; cockpit
  surfaces §5 empty / source-unavailable banners; rail-width NaN clamp; re-query on
  workspace repo-set change. *Files*: `src/pull_requests/{runner,bitbucket}.rs`,
  `src/ui/pull_requests_view.rs`, `src/app/{mod,render}.rs`. *Tests*: `PrCache`
  apply resilience units + the existing PR/UI suites. **Dropped:** GitHub
  team-requested reviews → To-review (per-repo `gh` role queries already cover
  direct requests; team expansion deferred).

### Next actions
- **Review then merge** the `m-pr` worktree branch into `main` (the milestone
  loop does not merge/push — user's call). Next milestone afterwards.

### Blockers
- none

### Open questions
- none

---

## Completed milestone — ☑ M-RC · In-diff code review → Send to Claude

In-diff annotation flow (no review "mode"): each diff line carries a borderless
**note (✦) icon** beside its stage `+`; clicking it opens an **inline editor**
(Enter / click-outside validates, Shift+Enter newline; ✕ deletes the comment, ✓
validates), a saved note shows as a **clickable card** (click re-opens its
editor) → annotate across multiple files (comments accumulate per repo). The
header **recap chip (✦ N)** opens a **popover** listing every note grouped by file
(each shows its truncated code anchor, edits in place, deletes) with a
right-anchored **✦ Send to {agent}** footer that opens a **new terminal tab**
running
`claude "<prompt>"` in the active worktree (prompt = aggregated
file/line/code/note) and **clears** the repo's comments (the tab is the only
signal). Works on **both** Git WIP and Commit Détail (every line annotable).
Locked: multi-file accumulation (app-level store), **dedicated** pref
`review_agent_command` (default `claude`), launch in a **new tab**.
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
- ☑ **RC4 — diff_view rendering + wiring (no review "mode").** Introduces
  `HelmApp.review: HashMap<RepoKey, FileComments>` + `apply_review_intent`
  (save/delete; `SendToAgent` no-op stub → RC5), `DiffViewState.{active_comment,
  popover_edit, popover_buffer, note_focus}` (+ `clear()`), `struct
  DiffReview<'a> { comments, agent, intents }` 8th param of `diff_view()` as
  `Option<&mut DiffReview>` (both call-sites; `None` at the test sites). Each
  diff line carries a per-line note (✦) icon beside its stage `+` (gutter widened
  via `LINE_ACTION_W`/`LINE_ACTION_GAP`); click → `DiffLineAction::OpenComment`
  (incl. read-only lines). `diff_line` returns `Option<DiffLineAction>`; rows
  allocate height before the clip check. Per-line loop renders an inline
  `note_editor` (Enter validates, Shift+Enter newline via `input_mut` event
  filtering; ✕/✓ icons under the field) or a saved-note `note_card` (clickable →
  re-opens its editor); `note_focus` one-shot autofocus. Header: recap chip
  (✦ N) → `CloseOnClickOutside` popover (per-file notes, each edit-in-place +
  delete, `Send to {agent}` footer). First `Esc` cancels editor/popover-edit.
  *Files*: `src/app/mod.rs`, `src/ui/diff_view.rs`, `src/app/render.rs`. *Tests*:
  UI e2e (click ✦ → type → Validate → `SaveComment`; chip → popover lists notes,
  delete, `Send to {agent}` → `SendToAgent`).
- ☑ **RC5 — Apply + spawn.** `apply_review_intent::SendToAgent` →
  `send_review_to_agent(ctx)`: build prompt (`build_review_prompt`) → `add_tab`
  → pre-insert the agent pane under `(run_key, new_tab_id)` at the fresh tab's
  focus `PaneId` (render's `or_insert_with` then a no-op) → rename tab to the
  agent command → **clear the repo's comments** (`self.review.remove(&key)`) →
  `central_mode = Terminal`, `diff = None`. `open_agent_terminal` spawns an
  **interactive login shell** with the prompt exported (`HELM_REVIEW_PROMPT`)
  then feeds `pty::agent_invocation` (`{program} "$HELM_REVIEW_PROMPT"`): the
  agent runs as a **shell job**, so Ctrl+C / exit drops back to a usable prompt
  instead of a dead pane. *Files*: `src/app/{mod,render}.rs`,
  `src/terminal/pty.rs`. *Tests*: unit on the fed invocation; in-crate e2e
  (seeded comments → `send_review_to_agent` adds an active tab carrying a `Live`
  agent pane + flips central mode, clears diff).
- ☑ **RC6 — End-to-end verify.** `headless-verify`: review WIP + Commit Détail,
  multi-file, Send opens a new tab with a live pane (stub agent in test).
  Demonstrable milestone scenario (DoD).

### Next actions (M-RC)
- **M-RC complete** (6/6). Review the worktree branch `m-rc`, then merge into
  `main`.

### Open questions (M-RC)
- none — Send now clears the repo's comments automatically (opening the tab is
  the only signal); the standalone Clear control was dropped.
