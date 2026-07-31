# Screenshots

These PNGs use **these exact filenames** — the root `README.md` references them
directly. They are **generated at 2× (retina)** by the headless render tests, not
captured by hand.

| Filename | What it shows | Scenario it represents |
|---|---|---|
| `hero.png` | The whole app: left sidebar + terminal + git sidebar, on a real repo. The money shot. | The three zones at once, a repo with a few changes staged |
| `terminal.png` | Terminal with **keyboard splits** (2–3 panes) and the per-repo **tab bar** visible. | A split layout (`Cmd+D` / `Cmd+Shift+D`), ideally an agent running in one pane |
| `git-graph.png` | The **commit graph** (`Cmd+Shift+G`): branch/tag chips, colored lanes, a selected commit with its detail + files in the right sidebar. | A repo with several branches so the lanes are visible |
| `worktrees.png` | Left sidebar showing a **project group** (root + indented worktrees) and the **Create worktree** modal open with the branch autocomplete. | The `+` modal on the root row |
| `ai-rebase.png` | The **AI rebase** recap modal (current → target, commits to replay, the AI instructions box) — or the running `AI rebase · m:ss` toolbar chip. | The modal opened from a branch chip's context menu |
| `git-staging.png` | The **diff view** with hunk/line staging controls, plus the unstaged/staged/commit sidebar (bonus: the ✨ AI commit message button). | A file diff mid-stage |
| `conflicts.png` | The in-app **conflict editor**: ours/theirs panes over a live merged result, with the Conflicted/Resolved sidebar and Continue/Abort. | A merge stopped on a conflict |
| `agents.png` | Left sidebar with **agent activity badges**: a spinner (Working) and a green dot (Done) on different workspaces. | The sidebar while an agent is mid-turn in one repo and finished in another |
| `agents-terminals.png` | The cross-repo **agents dashboard** — the wall of live agent terminals (generated). | — |
| `pr-list.png` | The **pull-request cockpit**: the *To review* and *Mine* groups with status, reviewers and age. | Two PRs awaiting review (one with changes requested), two authored (one draft) |
| `pr-review-comments.png` | **In-app PR review**: a file diff with anchored comment threads, a reply and the *Ask {agent}* action. | A posted thread on a hunk, plus a draft note in each pool (forge + agent) |
| `preferences.png` | The full-window **Preferences** page (left nav + a settings card). | Appearance or Project section |

**All** of these are deterministic renders, not manual captures — regenerate the
whole set with
`cargo test --features headless-verify --test shots_gen -- --nocapture` (outputs
to `verify-artifacts/shots/`; copy them here, renaming `_` → `-`).

Each `gen_*` test in [`tests/shots_gen.rs`](../../tests/shots_gen.rs) drives the
real widgets headless with curated in-memory fixtures, so the set stays in sync
with the UI and never drifts from a stale capture. The "What to capture" column
documents the intent each shot is composed to convey.
