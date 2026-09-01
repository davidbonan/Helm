# Release notes

## 2.4.0

- A pull request opens at once. The changed files come from the repository
  itself when the listed head and base commits are already there — a PR opened
  before, a branch you work on — with no round trip to the remote at all
  (≈ 3.4 s → 35 ms); when they are not, a single fetch brings both tips instead
  of two in a row.
- The PR body, checks and conversation paint as soon as the forge returns them;
  inline comments, review threads and commits load beside them and fill in a
  moment later, under a *Loading comments…* row — instead of everything waiting
  for the last call (first paint ≈ 2.5 s → 0.9 s on GitHub).
- Refreshing an open PR keeps its threads on screen until the fresh detail has
  fully landed — no blank in between.
- File diffs of a review are computed by a small pool, the file you are on
  first, rather than one thread per file.

## 2.3.0

- Annotating a diff now sends. `Enter` still queues the note for the batch;
  **⌘↩** — and the editor's **Send review** button — validate it *and* hand the
  whole batch to the agent, without the detour through the recap pill. A review
  comment destined for GitHub / Bitbucket never leaves on a keystroke: it is
  posted publicly on submit, so it keeps `Enter` alone.
- Every comment surface of a review now reads as one object — the note editor,
  the reply editor, the inline threads and the Conversation blocks all wear the
  same shape: the text, a rule, and an action bar carrying each control with its
  own shortcut beside it.
- An inline thread is **one block**: the comment, its replies nested on a rail,
  and a single bar for the whole thread — instead of a card per comment with the
  Reply and Resolve buttons floating underneath. Answering replaces the bar in
  place rather than splitting the thread in two.
- A **resolved** thread folds to a single line — the tally and the first words —
  and opens when you ask it to, so what is settled no longer pushes the code
  apart.
- Comment cards no longer run off the right edge on a file with long lines: they
  were as wide as the longest line in the diff, which put **Resolve** and **Send
  review** past the window, out of reach.

## 2.2.2

- Terminal glyphs stay in their cell: Claude Code's ✻ spinner, its ⏺ bullets and
  the emoji were drawn at their own size and spilled ink over the characters
  next to them. Anything wider than the grid is now shrunk into it.
- The bundled terminal face is JetBrains Mono **Nerd Font** — the statusline
  private-use icons and the braille spinners are drawn by the mono face itself,
  on the grid, instead of being borrowed oversized from a symbol font. Menlo
  backs it for the Dingbats, where Claude Code takes its spinner.
- The Pull Requests header stands on the page instead of over it: the tab
  baseline alone separates it from the list.

## 2.2.1

- `helm run` works again from the installed `helm` command: the symlink in
  `/usr/local/bin` was read as an unbundled build, so every `helm run …`
  answered "helm is not running" while the app was listening on another socket.

## 2.2.0

- Ask helm from the terminal whether a dev server is already running:
  `helm run status` answers for the worktree you are standing in — state, port,
  command — and `helm run list` covers every worktree helm knows. `start` /
  `stop` / `relaunch` drive the Run strip without touching the window, and
  `helm run logs -n 40` tails what the server printed, so a stack trace is one
  command away. Every one of them takes `--json`.
- `helm init claude` teaches Claude Code those commands in one shot: the rules
  land in `~/.claude/HELM.md`, linked from your `CLAUDE.md`. Your agent then asks
  helm before spawning a second server on a port nobody assigned it — and reuses
  the one already running, in the strip where you can watch it. Re-run it after
  an update to refresh the rules.
- The Agents wall comes in four pages: a set of terminals you arranged stays
  arranged when you need another pair, and the four-terminal cap counts per page.
  The pager rides the title row instead of a row of its own, which hands 30px of
  header back to the wall.
- Pull requests open on a redesigned browse list: stacks carry a numbered spine
  and their own foldable header, the author's avatar leads beside the state, the
  assigned reviewers sit on the right edge, and CI and ± fold into the row's
  flags rather than holding always-blank columns.
- A two-finger swipe to the right leaves a review and goes straight back to the
  list.

## 2.1.1

- The Agents strip groups its chips by project: the project is named once, as a
  header over the chips that belong to it, instead of being repeated on every
  one of them. Each chip spends that room on what actually tells your agents
  apart — its branch over the tab it runs in.
- Two agents in the same worktree running the same tool are numbered (`#1`,
  `#2`), on the chip and on its tile on the wall, so identical terminals can be
  told apart.
- The strip is one row that scrolls sideways now, so it keeps the same height
  whether one agent runs or twenty and the wall keeps the rest of the window.
  Scroll into a project and its header stays pinned on the left.

## 2.1.0

- A pull request's Files tab is one continuous scroll now: every file's diff
  stacks in a single column and the rail becomes its table of contents instead
  of a second list. Each file opens on a full-bleed header strip, and a hunk's
  actions sit on a hairline rather than inside a card.
- Descriptions and comments render like the forge renders them: GFM tables come
  out as tables, images are drawn in place — fetched off the UI thread, cached,
  and openable full-surface with zoom and pan — and columns take the width their
  content asks for.
- The conversation reads as one page: prose held at its measure, the Reviewers /
  Checks / Labels rail one gutter to the right, and body text at full contrast —
  in dark mode everything not bold used to read as disabled.
- `Esc` steps out one stage at a time — composer, file, list, then the cockpit —
  instead of closing everything from anywhere.
- Links open where you click them in the prose, and Bitbucket repo images load
  instead of failing with a 401.

## 2.0.2

- The Run panel's output can be selected and copied: drag over it, double-click
  a word, triple-click a line, then `Cmd+C`. A server error no longer has to be
  retyped to be shared — lift it straight out of the strip.

## 2.0.1

- A finished agent now turns its whole tile band green on the Agents wall,
  instead of marking it with a small dot — the tile you have to come back to
  reads from across the screen. The moment a turn lands, the band brightens
  once and settles.
- Agent chips and tile bands lead with the project instead of the agent's name:
  `helm-studio · main` rather than a wall of identical `Claude` labels. The
  agent's name and its tab moved to the chip's hover text.

## 2.0.0

- The Agents dashboard is one view now — a wall of live terminals you compose.
  A header strip lists every running agent as a chip carrying its state, its
  name and `project · branch`: click one to put its terminal on the wall, click
  again to take it off. Four at a time is the cap; past it the remaining chips
  read disabled and say so on hover.
- The wall is the terminal's own layout, so the workspace splits carry over —
  drag a seam to resize, drag a tile's grip onto another to re-split or swap,
  and the focus/resize chords drive it. Showing an agent splits the roomiest
  tile across its longer axis: one fills the wall, two sit side by side, and a
  wall you rearranged keeps its shape.
- The `List | Terminals` switch is gone with the grouped agent list and the
  per-card conversation preview. The dashboard always opens on the wall, and
  which terminals you watch is a choice instead of something derived.
