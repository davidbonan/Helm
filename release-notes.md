# Release notes

## 1.4.0

- A `helm://open?path=…` URL now opens exactly what `helm <path>` would: a
  subdirectory lands on its working tree instead of being imported as its own
  project.
- A path with no working tree to open is refused on the spot, without leaving a
  half-imported project behind.

## 1.3.0

- Open a repository straight from your terminal: install the shell command once
  (*Preferences › Terminal › Shell command*) and `helm .` brings up the project
  you are standing in, from any subdirectory.
- A worktree path lands on that worktree. An unknown project is imported with
  its whole worktree group; a known one is simply raised and focused.
- helm is now a single instance: a second launch hands its target to the window
  already open instead of starting a second app.
- Other applications reach the same door through the `helm://open?path=…` URL
  scheme — a Raycast script, an Alfred workflow, a link in your notes.

## 1.2.1

- A hardening pass over every Git write path: staging, discard, commit, and
  their confirmations now always act on the repository you pointed at, never the
  one you just switched away from.
- Partial staging writes each file's exact bytes — non-UTF-8 files, files with no
  trailing newline, symlinks, and executables now stage without corruption, and
  renames stage and count as renames.
- Pull (fast-forward if possible) always merges and never silently rebases;
  force-push is pinned to the commit you were shown and refuses if the remote has
  moved; rewording refuses to land on the wrong commit.
- The conflict editor preserves each file's line endings, warns when the file
  changed on disk, reads sides straight from the index, and won't write conflict
  markers into a resolved file.
- Fetch, pull, and push no longer hang on a hidden credential prompt or get
  killed at two minutes — they fail fast with a clear authentication error and
  allow more time for large transfers.
- Checkout reports where it landed, stops stashing for a no-op, and refuses a
  branch already checked out in another worktree; deleting a clean worktree now
  warns about the ignored files it would wipe.
- Smoother diff view (per-file scroll kept, faster redraw) and a workspace
  sidebar that no longer stalls while counting branches and changes.

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

