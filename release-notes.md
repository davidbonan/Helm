# Release notes

## 1.1.0

- Review pull requests inside helm — a new Pull Requests cockpit lists open
  PRs from GitHub and Bitbucket, with detail, diff, and per-commit changes.
- Comment inline on PR diff lines, reply in threads, leave conversation
  comments, and submit a verdict, all without leaving the app.
- Check out a pull request directly into its own worktree.
- PR navigation is instant now — reviews and diffs are cached per pull request.
- Mark up your working diff with inline review notes and send them to a Claude
  terminal agent.
- Reorganize terminal splits by drag-and-drop.
- Agent cards in the Columns view size themselves to the viewport.
- Done agents appear as clickable rows under the Agents sidebar entry.

## 1.0.3

- Select individual files in the WIP sidebar and stash them one at a time.
- Newly created worktrees now show up in the sidebar on their own while the app is focused.

## 1.0.2

- Jump to the Agents pane with ⌃⌘0; hold Cmd to reveal its shortcut badge.
- Confirm dialogs with the Enter key.
- Polished the workspace sidebar headers, now color-coded per project.

## 1.0.1

- Hold Cmd to reveal a ⌘R badge next to the Run/Relaunch button.
- Agent cards in the Columns view are now collapsible, with a progress preview.
- Polished the first-launch empty state.

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
