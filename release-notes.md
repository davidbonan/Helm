# Release notes

## 1.0.0

- First stable release of helm.

## 0.9.1

- The terminal now renders in JetBrains Mono, Ghostty's default monospace font.

## 0.9.0

- A "What's new" modal now greets you after an update; the notes also live in
  Preferences › Updates.
- Toggle the changed-files lists between flat and tree views, in both the WIP
  panel and commit details.
- New Run strip in the git sidebar launches your server/app, with per-worktree
  port management.
- Preview image files in the diff view, with zoom and pan.
- Pick a project from the Preferences › Project section.
- Notification toasts moved to the bottom-left and now auto-expire.

## 0.8.4

- Cycle between repositories with Ctrl+Tab, alongside Cmd+Ctrl+1..9.

## 0.8.3

- Moved the agents view switch into the titlebar; columns now size themselves.
- Fixed a resize repaint that flipped a finished agent back to Working.

## 0.8.2

- Gated terminal repaints to the visible pane, cutting idle CPU and GPU use.

## 0.8.1

- Agents columns are now resizable in both width and height.
- General performance work across the app.

## 0.8.0

- Added List and Columns view modes to the agents dashboard.
- Added a hide/show control for projects in the sidebar.
- Moved Remove-from-sidebar into the project header menu.
- Long branch chips and state captions now elide in the agents list.

## 0.7.1

- Commit-message generation now follows the project's conventions.

## 0.7.0

- New agents dashboard: a two-pane cockpit with a live terminal panel.
- Per-repo uncommitted line-stats indicator in the sidebar.
- Larger commit input, a centered switch with icons, and tab dividers.
- Inset project separators, hover-reveal chips, and row accent bars.
