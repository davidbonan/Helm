# Release notes

## 2.5.0

- The PR review surface keeps track of how long you have spent on each pull
  request: a quiet clock readout in the header, in minutes only, counts while the
  PR is on screen and the window is focused, and picks up where it left off when
  you come back. Totals survive restarts.
- `helm pr time` (or `--json`) lists every PR you have reviewed with the time spent,
  most recent first — with helm open or closed.

## 2.4.3

- The diff shows *what* changed inside a line, not just that the line changed:
  a rewritten line and the one it replaces are aligned word by word, and the
  parts that actually differ take a stronger tint. Two lines too far apart to be
  one rewrite keep their plain red/green, as before.

## 2.4.2

- The Run panel's inline command field survives `Cmd+V`: holding Cmd made the
  `⌘R` hint appear and the field quietly lost its focus, cancelling the edit
  before the paste could land. Paste a launch command, press Enter, done.

## 2.4.1

- Fixes the crashes: the file list no longer reads a large file of the worktree
  to count its lines. It read it whole — every second, for every repository of
  the group — only to throw the result away, and a build rewriting that file
  underneath killed the app on the spot.

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
