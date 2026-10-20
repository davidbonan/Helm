# Release notes

## 3.7.3

- **Files** now has its own sidebar, per worktree: toggle it from the folder
  button or `⌘⇧E`. It sits left of the browser, and shows in Graph mode too.
- The right sidebars are now rounded blocks floating off the window edges.
  Their toggles sit in the terminals' title row, and Git's shows the Git icon.
- Phone, Send feedback and Preferences moved to the bottom of the left
  sidebar.

## 3.7.2

- Browser tabs can be reordered: drag a tab along the row to move it.
- `⌘R` right after clicking a page now reloads it, instead of restarting the
  dev server.

## 3.7.1

- **Picture in Picture**: More › Picture in Picture floats the
  browser over the window. Drag it anywhere, resize it from any edge; hover
  it for host, reload and Dock back into the sidebar.
- The browser sidebar now shows per worktree: toggling it in one worktree
  leaves the others as they were.

## 3.7.0

- New **browser** sidebar on the right, one per worktree: toggle it from the
  globe button or `⌘⇧B`. Pages open on the worktree's dev server.
- Folder-style tabs tinted to the page, with a spinner while loading.
- Favorites page with a sidebar: this project's favorites, starred, frequently
  visited, and folders imported from Chrome or Safari (favorites, history,
  cookies). Site icons on tiles and tabs.
- More menu: find in page, zoom, tablet/mobile viewport, Web Inspector, open
  in Chrome, copy URL.
- With the browser focused, `⌘R` reloads the page (`⌘⇧R` bypasses the cache).

## 3.6.0

- New **Files** tab in the right sidebar, beside Git: browse the worktree's
  tree and open any file in a viewer over the center zone.
- Click in the viewer's text to edit the whole file in place, like a hunk in
  the diff.
- Leave agent notes on viewer lines: they join the diff's notes in one batch,
  sent to the agent together.
- File-type icons in the Files tree and the diff and viewer headers.
- Pull requests: right-click a PR to hide it from the Inbox (and show it
  again from its role tab).
- Terminal: Hebrew renders, and accents and other combining marks sit on
  their letter instead of shifting the line.

## 3.5.2

- Phone: access no longer stops by itself. It survives the Mac leaving and
  rejoining its network, even with helm hidden, and follows the Mac's address
  when the router hands it a new one.
- Phone: a weak Wi-Fi or a locked iPhone no longer unpairs the phone — no
  more rescanning the QR code.
- Phone: a lost connection is noticed instead of leaving a frozen screen, and
  the Mac's pane gets its own size back.
- Pull requests: review time tracking and the `helm pr time` command are
  gone — the figure no longer reflected how reviews are done.

## 3.5.1

- Preferences › Agents: a start command prefixed by environment variables
  (`CLAUDE_CONFIG_DIR=~/.claude-perso claude`) no longer shows the
  "helm won't detect … as an agent" warning.

## 3.5.0

- Phone: open the files of an agent's worktree. A new **Files** button in
  the terminal header browses its folders, newest first, and shows images,
  video, audio and text; **Open** hands the file to Safari.

## 3.4.1

- Emoji are drawn in color everywhere — terminal, pull requests, sidebar,
  tabs — with the system emoji font, as in Ghostty.
- Terminal: an emoji followed by a variation selector (⚠️) no longer shows
  a stray box.

## 3.4.0

- New Preferences › Agents: one list of agents — Claude Code, Codex and
  opencode pre-filled — each with its own commands. Add your own: an alias
  or a wrapper works too.
- Commit message and review now pick an agent from that list, and their
  prompts are editable with a **Restore default**. Your previous settings
  are carried over.
- Phone: the pairing modal says when the macOS firewall keeps the phone
  out, with a link to the Firewall settings.
