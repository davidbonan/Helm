# Release notes

## 3.2.0

- Start a new agent from the phone: the **+** on the Agents screen
  picks a project or worktree and an agent, then opens it in a new tab on
  the Mac — even with helm hidden or the Mac locked.
- Choose which agents the phone can start (Claude Code, Codex and opencode
  by default) in Preferences › Agents › Phone launch.
- Fixed a freeze when the last terminal closed while phone access was on.

## 3.1.3

- New `/` button next to the phone's quick keys: a menu that sends
  Claude Code's `/clear`, `/compact` and `/model` in one tap.

## 3.1.2

- The phone page now wears helm's theme (dark or light, following the Mac) with a
  cleaner layout: one card per project, state pills, branch over tab.
- Swiping scrolls a full-screen agent like Claude Code smoothly on the iPhone, and
  watching an agent on the phone acknowledges its green badge.
- Opening the keyboard on the phone no longer makes the page jump.
- New `⌫` quick key, repeating while held, to clear a prompt `⇥` filled in.

## 3.1.1

- Much lighter on CPU with large repositories and big workspaces: the git status
  refresh no longer scans the whole working tree twice per second. Change counts
  of the inactive repos in the sidebar now refresh every 30 s (instantly when you
  come back to the app); the active repo stays live.

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
