# Release notes

## 1.2.0

- Amend the last commit's message right from the commit panel.
- Rebasing a diverged branch that has no commits of its own now just moves it
  onto the target, with no commits to replay.
- Click anywhere on the Run panel header to collapse or expand it; its collapsed
  state is now remembered per worktree.

## 1.1.3

- Fixed approving, requesting changes on, and resolving comments in Bitbucket
  pull requests — these actions no longer fail with a server error.

## 1.1.2

- The Pull Requests list now shows each PR's source → target branch, and
  stacked PRs nest as a tree under their base.
- Deleting a PR comment no longer leaves a "deleted" placeholder behind.

## 1.1.1

- The pull-request detail view is now full-width, with Markdown-rendered
  comments and clearer nested reply threads.
- Polished comment cards for readability.

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

