# Release notes

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

## 1.5.1

- Walking the Git file list with the arrow keys no longer stutters on large
  files: a diff now colours what the viewport shows straight away and finishes
  the rest over the following frames.
- Clicking a file in the Git sidebar brings up its diff at once instead of
  waiting behind the background refresh — up to half a second saved on a large
  repository.
- *Open in editor* now re-focuses the Zed window that already holds the project
  and adds the file as a tab, instead of opening a second window and reloading
  the whole workspace.

## 1.5.0

- Clean up several worktrees in a row: a *Delete worktree from disk* clicked
  while another removal is still running no longer vanishes without a trace.
  Each row now carries its own spinner, and two removals finishing together
  both land.
- A collapsed card in the Agents *Columns* view previews a few more lines of
  its conversation.

## 1.4.3

- Agent completion banners now come from helm itself: they show up under *helm*
  in System Settings › Notifications and can be allowed through a Focus mode,
  which until now swallowed them silently.

