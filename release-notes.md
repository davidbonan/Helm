# Release notes

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

## 1.4.2

- The sidebar count of pull requests awaiting your review now stays fresh from
  anywhere in the app, and is already right on launch instead of only after a
  first visit to the cockpit.

## 1.4.1

- Rename a linked worktree from its context menu: *Rename worktree…*
  previews the destination as you type, and the sidebar entry follows the move
  with its terminals and running agent untouched.
- The Agents *Columns* view now expands one terminal per column instead of a
  single card over the whole wall; the active card keeps its accent ring.

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


