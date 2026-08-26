# Release notes

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

## 1.6.1

- `Esc` now leaves an inline edit *without* keeping it: the buffer is dropped
  and the diff comes back exactly as it was. Every other way out still saves —
  `Cmd+S`, a click elsewhere, another hunk, switching file, repo or worktree.
- With `Esc` a real way back, the save after a pause in typing is gone: an open
  buffer only reaches the working tree on an exit that keeps it, so nothing
  lands behind your back.

## 1.6.0

- Fix a file straight from its diff: click a line — or `Cmd+E` on the hovered
  one — and a caret opens right there, the hunk turning into an editor without
  moving a pixel. Context lines are editable too, so *Extend context* widens the
  window; untracked files take a caret like any other.
- No save button to hunt for: the buffer is written to the working tree when you
  step out — `Cmd+S`, `Esc`, a click elsewhere, even switching repo — and after
  a short pause in typing. `Cmd+Z` undoes inside the buffer.
- An edit stays in the section it was made from: from *Staged* the file is
  re-staged for you, and where a caret cannot open (binary, symlink, a file
  sitting in both sections) a toast says why and offers *Open in editor*.
- The Agents *Columns* wall is now one borderless lane per worktree.

## 1.5.2

- Maintenance release: no user-facing changes since 1.5.1.
