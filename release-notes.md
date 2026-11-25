# Release notes

## 3.11.2

- **Pull requests**: a Bitbucket repository that was deleted, renamed or is no
  longer accessible no longer hides every Bitbucket pull request — the others
  still list, and the warning names the repository with Bitbucket's reason.

## 3.11.1

- **Phone**: the back chevron returns to the previous screen again — opening an
  agent no longer reloads the page, and a page reopened on an agent or a file
  goes back up to the agents list.

## 3.11.0

- **Project cards** in the sidebar: each project sits on its own card, with a
  monogram and a colour derived from its name, worn by its worktree icons and
  its active row. Diff stats show as a small pill.
- **Agents under each worktree**: a chevron (on hover) unfolds the worktree's
  agent sessions, hung on a thread in the project colour — each with its state
  and recap; hover shows Claude's last prompt, click focuses its terminal.
- **Browser**: full window (`⌘⇧↩`) and split view with two tabs side by side.
  Page dialogs, file pickers, downloads (with a shelf), links opening a new
  tab, and a failure page offering to start the dev server.
- Browser sidebar: its resize edge now grabs at any height.

## 3.10.0

- Browser: copy, cut, paste, select all and undo (`⌘C` `⌘X` `⌘V` `⌘A` `⌘Z`)
  now work in web pages, and `⌘⌫`, `⌘←` / `⌘→` delete and move by line. The
  menu bar gains an Edit menu.
- Browser sidebar: easier to resize — the whole gap along its left edge grabs it.

## 3.9.1

- **Workflows**: chain Claude Code steps into a reusable
  workflow. Define them in Preferences › Workflows, launch one
  with `⌘P`, fill in its inputs, and follow progress in the pill
  next to the tabs. Each step advances on its own when Claude
  finishes — `/compact` and `/clear` steps included.
- Markdown files get a **Code | Preview** toggle, in the file
  viewer and in the diff.

## 3.8.2

- Phone access: reopening Safari on your phone no longer shows 'This phone
  isn't paired' — the page and its reload stay signed in.

## 3.8.1

- Right-click a file or folder in the Files sidebar: **Copy path**, **Copy
  relative path**, and **Reveal âº** to open it in Finder, your editor, a
  terminal or GitKraken.

## 3.7.7

- Ctrl+Enter now reaches the terminal: in Claude Code it sends your message
  right away instead of queuing it.

## 3.7.6

- Right-click a changed file in the Git sidebar âº **Reveal in Files**: the
  Files sidebar opens, unfolded down to that file and selected.

## 3.7.5

- Cmd+click in a terminal now catches the links agents print:
  markdown links, long URLs and paths the agent wrapped over several lines,
  paths inside `Update(â¦)` or backticks, and `file:12-30` ranges.
- Cmd+click on a URL opens it in a new tab of helm's browser; from the
  Agents view, helm jumps to that agent's worktree first.
- Cmd+click on a folder opens it in your editor.
