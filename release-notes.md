# Release notes

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

## 3.3.1

- Agents wall and phone: each split of a tab now shows its own
  agent's name instead of the last focused one's.
- Phone: selecting terminal text works while the agent is running —
  the selection handles can be dragged and the screen holds still
  until you tap away.

## 3.3.0

- Phone pairing now lasts: a paired phone stays paired across restarts of
  helm for 30 days, and scanning the QR code again lands on its agents.
- New Preferences › Phone: start phone access at launch when the Mac is on
  the network the phone paired on, and revoke paired devices.
- A dot beside **Open with** shows phone access is on and who is connected;
  click it to open the pairing modal.
- Git sidebar: folding Unstaged or Staged gives its room to the other list,
  and the split between them is draggable.
- Going back from an agent started on the phone lands on the agents list.
- Removed the AI rebase (graph menu entry and its Preferences provider).

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
