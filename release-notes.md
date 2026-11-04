# Release notes

## 3.9.0

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

## 3.7.4

- Agent completion notifications now arrive when helm is in the
  background or hidden, not only when its window is in front.
- Phone: hide or show a project from the eye next to the + in the Agents
  title â shared with the sidebar's eye on the Mac.

## 3.7.3

- **Files** now has its own sidebar, per worktree: toggle it from the folder
  button or `ââ§E`. It sits left of the browser, and shows in Graph mode too.
- The right sidebars are now rounded blocks floating off the window edges.
  Their toggles sit in the terminals' title row, and Git's shows the Git icon.
- Phone, Send feedback and Preferences moved to the bottom of the left
  sidebar.

## 3.7.2

- Browser tabs can be reordered: drag a tab along the row to move it.
- `âR` right after clicking a page now reloads it, instead of restarting the
  dev server.

## 3.7.1

- **Picture in Picture**: More âº Picture in Picture floats the
  browser over the window. Drag it anywhere, resize it from any edge; hover
  it for host, reload and Dock back into the sidebar.
- The browser sidebar now shows per worktree: toggling it in one worktree
  leaves the others as they were.
