# Release notes

## 3.1.0

- A worktree's right-click menu can now delete it **together with its branch**:
  *Delete worktree and local branch*, or *… and local + remote branch* (also
  removed from the remote it tracks). Nothing is touched if the remote refuses.

## 3.0.1

- **Open on phone** (command palette): scan the QR code with your
  phone on the same Wi-Fi to follow your agents from anywhere in the house —
  the list with their badges, the live terminal, a reply field and quick keys
  (`Esc`, `1`–`3`, arrows, `⇥`, `^C`, `⏎`) to answer permission prompts.
  Keeps working with the Mac locked.
- While you drive an agent from the phone, it redraws at the phone's size; a
  pill on the Mac pane says so, and a click there takes the size back. Pinch
  or A−/A+ on the phone picks the reading size.
- Plain HTTP on your local network: use it on a trusted Wi-Fi. *Stop phone
  access* ends it; it also stops after 2 h with no phone connected.

## 2.8.0

- `Cmd+P` opens the **command palette**: reach the actions spread across
  the sidebar, git toolbar and context menus by typing, no mouse. Commands
  that don't apply to the current worktree are hidden.
- Sub-screens for **Running servers** (stop any live Run strip, across
  projects), **Stashes** and **Checkout branch**; `Esc` goes back.
- On an empty query, your last command and most used ones lead the list
  under **Recent**.

## 2.7.0

- The pull requests list opens on **Inbox**: only what needs you — PRs waiting on
  your review, plus your own still in review. Drafts, red builds, changes
  requested and ready-to-merge stay under the role tabs.
- A PR you already approved moves to **Waiting on the author** — merging is their
  move, not yours.
- Bigger, more legible rows: taller lines, larger avatars, readable initials, and
  a review verdict badge with a verdict-colored ring you can actually spot.

## 2.6.0

- `Cmd+J` jumps straight to the first agent that has finished — the top green row
  under **Agents** in the sidebar. Landing there clears its green, so pressing
  again takes you to the next finished one. Rebindable in Preferences › Keyboard;
  hold `Cmd` to see the `⌘J` hint on the row.

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
