# helm — Phone access (follow and drive agents from a phone on the LAN)

From the couch, the user follows the agents running in helm and keeps a session
going: read what the agent printed, type the next instruction, answer a
permission prompt. Phone browser only (iPhone Safari), same local network, no app
to install. Module: `remote` (+ `agent_watch` off the UI thread, §4).

## 1. Intent & scope

| In | Out (§10) |
|----|-----------|
| List of the agents helm detects, with their badge | Plain terminals, free commands, creating a worktree from the phone |
| One agent's terminal at the phone's own size (+ scrollback) | Conversation view (transcript-based) |
| Sending an instruction, quick keys for prompts | Non-agent terminals (plain shells, Run strips) |
| Launching a configured agent in a new tab of a workspace project or worktree (§7.2) | Initial prompt at launch (the composer sends the first one) |
| Works with the Mac locked / helm hidden | HTTPS, Tailscale, push notifications |
| Browsing and opening the files of an agent's worktree (§7.3) | Editing, uploading or deleting a file |

**Agents only**: a pane is exposed while `agent_watch` sees an agent in its
foreground (badge ≠ `None`, [`agents.md`](agents.md) §2). The phone never reaches
a plain shell: the attack surface is the agents, not the Mac. A launch (§7.2)
names an agent of the Mac's list and a workspace entry by id: the phone never
sends a command line nor an absolute path. The files it opens (§7.3) are named
relative to an agent's worktree and resolved inside it.

## 2. Entry point — command palette only

Two palette commands ([`keybindings.md`](keybindings.md) §6), one visible at a time:

| Command | Available when | Effect |
|---------|----------------|--------|
| **Open on phone** | access off | Starts access (§3) and opens the **pairing modal** |
| **Open on phone** | access on | Reopens the pairing modal (fresh pairing code, §3.2) |
| **Stop phone access** | access on | Stops access (§3) |

Pairing modal: QR code of the pairing URL, the URL as text (copyable), *Phone
access is on — anyone with this code can type into your agents and read their
files*, the connected
device count, and a **Stop** button. The code is single-use and lives 5 min: once
used or expired, the open modal shows a fresh one. When the macOS firewall keeps
the phone out — *Block all incoming connections*, or helm's own rule set to block
(`socketfilterfw`, no admin rights; re-read every 2 s while open) — a banner says
so with **Open settings ›** (Firewall pane). While access is on, a
**dot** (8 pt) left of the title bar's *Open with* selector says so — `git.added`
while a phone is connected, `text.muted` while access waits for one, both at 45 %
opacity (full under the pointer) — its tooltip naming who is connected (*Phone
access on — iPhone connected* / *— no phone connected*); a click opens the pairing
modal. Hidden while off: starting and stopping stay in the palette. Paired devices and *Start at launch* live in Preferences › *Phone*
([`preferences.md`](preferences.md) §4).

## 3. Access lifecycle & security

Two lifetimes: the **server** (bound while access is on) and the **pairings**
(a phone scans once, its pairing survives stops and restarts of helm until
revoked, §3.2).

**Start**: *Open on phone*, or at launch on the pairing network (§3.4) — pick the
LAN address (§3.1), bind the server, begin the no-sleep activity (§5). **Stop**, on
whichever comes first: *Stop phone access*, the Mac joining a network no phone was
paired on (§3.1), helm quitting. No idle stop: a phone locked on the couch keeps
its access. Stop closes every connection, frees the port and ends the no-sleep
activity; pairings stay.

### 3.1 Address
First up, non-loopback **private IPv4** (`10/8`, `172.16/12`, `192.168/16`) from
`getifaddrs`, `en0` preferred. The server binds **that address only** (never
`0.0.0.0`: no exposure on a VPN or a second interface). **Port persisted**: the
last bound port is tried first, so the phone's bookmark keeps working; taken ⇒
the OS picks one, which is saved. None found ⇒ the command fails with *No local
network*. A phone's cookie belongs to the host's IP: a new IP (DHCP) ⇒ rescan.

**The address is followed**, from the accept thread (no frame needed, §4): every
2 s it checks that the bound address is still up. Gone (a lease refused, a Wi-Fi
roam) ⇒ the listener and every socket of that address close, access stays on, and
the server binds the next LAN address as soon as there is one **on a recorded
network** (gateway MAC, §3.4) — the same address back keeps the bookmark and the
cookie. No address yet, or a gateway not answering yet ⇒ it keeps waiting. An
address on a network no phone was paired on ⇒ access stops.

### 3.2 Pairing & sessions
- **Pairing code**: 128 random bits (`arc4random_buf`), hex, **single use**, valid
  **5 min**; minted each time the pairing modal opens or the shown one is spent.
- **Pairing URL** (QR): `http://<ip>:<port>/pair?t=<code>`. A valid code becomes a
  **paired device** and its cookie `helm_session=<device token>; HttpOnly;
  SameSite=Strict; Path=/; Max-Age=2592000`, then a `303` to `/` — the code leaves
  the address bar and the history.
- **Device token**: 256 random bits, hex. helm stores only its **SHA-256**, in
  `phone_devices.toml` (support directory, beside `prefs.toml`) with the device's
  name (from its `User-Agent`: *iPhone*, *iPad*, *Android phone*, else *Browser*),
  pairing date and last visit.
- **Rotation**: every `GET /` (page load, reconnect probe) answers with a new
  token in a fresh cookie; the previous one stays valid **30 s** (the page's own
  requests in flight), then never again. A copied cookie is worth one visit: the
  phone's next load rotates it away, and if the thief rotates first, the phone
  finds itself unpaired — the visible alarm.
- Every other request needs a paired device's cookie (constant-time compare of
  the hashes), else `401`. The WebSocket upgrade also requires
  `Origin == http://<ip>:<port>`.
- **Revocation**: per device or all, from Preferences (§2); a revoked device's
  open sockets close within a read timeout. A device unseen for **30 days** is
  dropped. No cap on the device count.
- **Alerts** (native notification, [`agents.md`](agents.md) §5 backend): *New
  phone paired — <name>*; *<name> is connected from two addresses — revoke it in
  Preferences if one isn't yours*, when a device opens a WebSocket while another
  of its sockets is live from a different IP. Posted from the server thread: they
  fire with helm hidden.

### 3.3 Accepted risk
Plain HTTP: the cookie travels in clear on the Wi-Fi. Acceptable on a home
network, not on a shared one; rotation, the two-address alert and revocation
bound what a sniffed cookie is worth. LAN only, by decision: no off-LAN access,
no Tailscale for now. The pairing modal states it; §10 lists the upgrade paths.

A paired device reads **every file of the worktrees its agents run in** (§7.3),
secrets included (`.env`, keys): what the agent itself can read there, the phone
can. Only `.git` and whatever resolves outside the worktree stay out of reach.

### 3.4 Start at launch
Preference *Start at launch* (`phone_access_at_launch`, off by default). The
networks where a device was paired are recorded by their **gateway MAC**
(`route -n get default` → gateway IP, `arp -n <ip>` → MAC; no location
permission, unlike the SSID). While the preference is on and access is off, helm
checks every 30 s: on a recorded network ⇒ starts access silently (no modal). A
*Stop phone access* holds it off until the next *Open on phone* or helm restart.
Moving back home after the LAN address went away resumes access the same way.

## 4. Architecture — nothing waits on the UI thread

**Constraint**: macOS gives a hidden or minimized app no draw callback, so
`update` never runs (measured, [`cli.md`](cli.md) §9, `ipc.rs` `REPLY_TIMEOUT`).
The control socket parks requests for the UI thread; the phone cannot — it must
work precisely when helm is out of sight.

Today `HelmApp::update_agent_watch` ticks the agent state on the UI thread. It
moves to a **watcher thread** (refactor, first task):

```
[UI] --register/unregister pane, focus--> [agent watcher, 1 s tick] --snapshot--> [UI]
                                                    |                              (reads)
                                                    +--snapshot--> [remote server]
[remote server] --reads grid (lock)--> [Term grid] <--feeds-- [PTY reader]
[remote server] --paste / key bytes--> [PtyWriter]
[remote server] --claim / release size--> [PaneSizing] <--widget size, claim-- [UI]
[remote server] --spawn Pane (launch)--> [Registry pending + watcher extra] --Pane--> [UI adopts]
```

- **Watcher thread** owns the per-pane `PaneAgentState` and ticks every second
  with the same pure state machine ([`agents.md`](agents.md) §3). Per pane it
  holds shareable handles only: `Arc<PaneActivity>`, a **pgid probe** (a `dup` of
  the PTY master fd, `tcgetpgrp` on it — independent of the `Pane`'s lifetime),
  the labels (project, branch, tab) the UI refreshes. The UI sends the focused
  pane (acknowledging a green, §1 of agents.md) and reads the published snapshot
  each frame; badges, dashboard, `Cmd+J`, and completion notifications keep their
  behaviour.
- **Remote registry**: per exposed pane, `SharedTerm` + `PtyWriter` +
  `Arc<PaneActivity>` + `Arc<PaneSizing>` clones, keyed by an opaque id minted at registration. The UI
  registers a pane when it opens and unregisters it when it drops; exposure is
  filtered by the watcher's badge.
- **Server**: `std::net` + `httparse` for the HTTP routes, `tungstenite`
  for the WebSocket, one thread per connection — the threads-and-channels model
  of the rest of helm (architecture §3), no async runtime. Not `tiny_http`: its
  upgrade hides the `TcpStream` behind a `Box<dyn ReadWrite>`, so one thread
  could not both read the phone (read timeout) and push frames. Assets (`index.html`, `app.js`, `app.css`) embedded with
  `include_str!`; nothing loaded from a CDN.

- **Launch without the UI** (§7.2): the server thread spawns the `Pane` itself —
  `Pane` owns its PTY and threads, nothing of egui — then registers it **pending**
  in the registry and as an **extra** of the watcher, so the phone can watch it and
  the badge can light while helm draws no frame. The `Pane` travels to the UI over a
  channel with its entry's key; the next `update` drains it and inserts it as a new
  tab. A pending pane / watcher extra stays until a UI `publish` / `track` lists
  it, or the UI drops it unadopted (entry gone) and forgets it. The UI publishes
  the launch targets beside the panes: the workspace entries (not hidden) and the
  agent list from Preferences; its repaint pacer wakes the UI at launch.

Phone input goes through `Pane`'s write path semantics: bytes stamp
`PaneActivity` input, so replying from the phone **acknowledges** a green exactly
like typing on the Mac.

## 5. Keeping the Mac reachable

While access is on, helm holds an `NSProcessInfo` activity
(`NSActivityUserInitiated`: idle **system** sleep disabled, App Nap off); the
**display** may sleep and the screen may lock. Ended at stop. A lid closed on
battery still sleeps — accepted (§9).

## 6. Wire protocol

HTTP: `GET /pair`, `GET /` (+ assets; rotates the session cookie, §3.2), `GET /ws` (upgrade),
`GET /files` and `GET /file` (§7.3). `GET /` inlines helm's
active theme on `<html>` (`data-theme` + the tokens as CSS variables): the first
paint already wears it. Everything else runs
on the WebSocket, one JSON object per text frame.

Server → phone:

| `type` | Payload | When |
|--------|---------|------|
| `theme` | `{dark, tokens}` — helm's chrome colors (`accent`, `sidebar`, `canvas`, `surface`, `border`, `text`… as `#rrggbb`) | on connect, then when helm switches theme or preset |
| `agents` | `[{id, project, branch, tab, badge}]` | on connect, then on change (watcher tick) |
| `screen` | `{id, cols, rows, fg, bg, lines, cursor, writable, app_scrolls}` | watched pane, on change, ≤ 10 /s — ≤ 60 /s for 600 ms after a phone `send`/`key`/`scroll`, so a swipe reads as motion; `fg`/`bg` = the palette's own, what a blank cell shows; `app_scrolls` = the app takes the wheel ([`terminal.md`](terminal.md) §8) |
| `history` | `{id, first, lines}` | reply to `history`; `first` = next page's `before` |
| `ended` | `{id}` | watched pane dropped |
| `targets` | `{entries: [{id, project, branch, worktree}], agents: [{id, name}]}` | on connect, then when the workspace or the agent list changes |
| `launched` | `{id}` | reply to `launch`: the new pane, watchable at once |
| `launch_failed` | `{message}` | reply to `launch`: unknown entry / agent, spawn error |
| `ping` | — | every 3 s, with a WebSocket ping frame |

**Liveness**: nothing else flows while the agents idle, and a dead TCP link closes
no socket by itself. The page drops a socket silent for 10 s and reconnects; the
server drops a phone that answered no frame for 10 s (the browser answers the
WebSocket ping by itself), which gives the watched pane its size back.

`lines` = rows of **runs** `{t, fg, bg, bold, italic, underline}`, colors
resolved to `#rrggbb` through the pane's `TermPalette`, dim and inverse already
folded into the colors, trailing blanks dropped (`terminal::screen`).
`writable = false` once no agent is in the foreground: the screen stays readable,
input is refused (§1, agents only).

Phone → server:

| `type` | Payload | Effect |
|--------|---------|--------|
| `watch` | `{id, rows, cols}` | start mirroring this pane (one per connection), claim its size (§7.1); while watched, the pane counts as **seen** — acknowledges its green ([`agents.md`](agents.md) §1) |
| `resize` | `{id, rows, cols}` | the phone's screen changed (zoom, rotation): claim that size |
| `unwatch` | — | back to the list: release the size |
| `send` | `{id, text}` | re-claims the size, then `Pane::paste` semantics (bracketed when the mode is on) then `\r` |
| `key` | `{id, key}` | re-claims the size, then one quick key (§7), encoded like the Mac keyboard |
| `history` | `{id, before, count}` | `count` scrollback lines above line `before` |
| `launch` | `{entry, agent, rows, cols}` | spawn the agent in a new tab of that entry, sized for the phone (§7.2) |
| `scroll` | `{id, lines, line, col}` | re-claims the size, then the Mac wheel's bytes (`wheel_bytes`: `lines > 0` = up, cell under the finger, ≤ 100 lines); nothing when the app does not take the wheel |

Change detection: the server snapshots the watched grid every 100 ms (16 ms while
the phone drives) under the short lock and sends only when the screen differs from
the last frame sent. A
`send`/`key`/`scroll` to an id that is not exposed or not writable is dropped.

## 7. Phone UI

Mobile-first, a single page wearing **helm's active theme** (mode and preset, not
the phone's scheme): chrome on `bg.sidebar`, content on `bg.canvas` / the terminal
palette ([`design-system.md`](design-system.md) §1).

- **Agents**: large title, blurred when the list scrolls under it; one rounded card
  per project — branch over tab (the terminal's name, [`terminal.md`](terminal.md) §4; the tab alone when detached; the agent's name tells
  nothing, every one is *Claude*), a state pill (badge +
  *Working* / *Done* / *Idle*, same semantics and colors as the sidebar), chevron.
  Tap ⇒ terminal view. Empty ⇒ *No agent running*.
- **Terminal**: the screen in a monospace `<pre>`, **sized for the phone** (§7.1):
  the agent draws its TUI for the phone's width, no reflow. The reading font
  (12 px by default) sets the size in cells; pinch on the terminal or A−/A+ changes
  it, and once the gesture settles the phone asks for the matching size (`resize`)
  — native page zoom is off, it would scale the header and the dock too. A frame
  wider than the phone (the Mac took the size back) shows fitted to the width until
  the phone claims again. A one-finger swipe the mirror cannot scroll any further
  goes on: to the app as the wheel (`scroll`, one line per text line of travel)
  when `app_scrolls` — a full-screen TUI like Claude Code has no local scrollback —
  else, upward, it requests `history`. With `app_scrolls` the mirror never scrolls
  natively (`overflow-y: hidden`, `touch-action: none`): a few pixels of native
  scroll would have taken the whole gesture from the page. Its lines take no touch
  (`pointer-events: none`): a frame replaces the lines that changed, and a touch
  whose target left the DOM stops reaching the mirror. Released while moving, the
  scroll **glides** on, slowing down like iOS's own (0.998 / ms). Claude Code
  scrolls one line per wheel event, so the content follows the finger 1:1.
- **Text selection**: iOS's own (long press, handles, *Copy* — `navigator.clipboard`
  needs a secure context). While a selection is open in the mirror, frames are
  **held** (the latest one is painted once it closes) and a swipe no longer scrolls
  the app: a replaced line drops the selection on it, and a handle drag is a swipe.
- **Keyboard**: the page is pinned to the visual viewport (height *and*
  `offsetTop`), so the dock rides right above the iOS keyboard. Measured on the
  simulator (iOS 27): WebKit reports the final viewport the moment the keyboard
  starts rising, while it animates its own pan to reveal the field — applied at
  once, the page jumped by the keyboard's height then slid back. So the prompt
  stays transparent 150 ms on focus (iOS skips the pan for it; a single tick no
  longer suffices), and the page height **transitions** (250 ms, the keyboard's
  curve) instead of snapping. Closing, iOS reports the viewport only once the
  keyboard is gone: the prompt's `blur` starts the growth instead.
  Sticks to the bottom while new output arrives, unless the user scrolled up — also
  through the height transition (`ResizeObserver`).
- **Terminal header**: back chevron, project over branch · tab, state
  pill, a **Files** button (§7.3), A−/A+ segmented. Both lines keep their height while empty: on a direct
  load the rows are sized before the agents list fills them.
- **Composer** (bottom, above the keyboard): an input card (`border.input`, accent
  when focused) holding the multi-line field and a round **Send** (`send`); empty
  text + Send = Enter alone. **Dictation** = the iOS keyboard's own mic: Web Speech
  is gated on a secure context, which a `http://` LAN page is not (checked on the
  simulator, §8).
- **Quick keys** row: `Esc` · `↑` · `↓` · `⇥` · `⇧⇥` · `⌫` · `^C` ·
  `⏎` — enough to answer Claude Code's permission menus and switch its mode.
  `⌫` repeats while held: it clears a prompt `⇥` filled in.
  Pinned at the row's right end, outside its scroll: a **`/`** button opening a
  menu of Claude Code commands — `/clear`, `/compact`, `/model` — a tap sends it like the
  composer (`send`); a tap elsewhere closes the menu.
  Encoded by the same byte table as the Mac terminal (`key_bytes`, moved from
  `ui::terminal_view` to the terminal domain so `remote` does not import the UI).
- **Reconnect**: on socket loss, a socket silent for 10 s (§6) or
  `visibilitychange` back to visible (iOS suspends background tabs), reconnect
  and re-`watch`; a `401` shows *This
  phone isn't paired — on your Mac, run Open on phone and scan the code* (revoked,
  dropped after 30 days, rotated away, or a new IP). Hidden, the page closes its socket: the
  phone stops driving (§7.1).

### 7.1 One PTY, two screens — the latest to act sizes it

A PTY has a single size and the agent draws for it, so the Mac and the phone
**take turns** (tmux's `window-size latest`), arbitrated by `terminal::sizing`:

| Event | PTY size |
|-------|----------|
| Phone `watch` / `resize` / `send` / `key` / `scroll` | the phone's rows × cols (bounded 8–300 × 20–500) |
| Click, keystroke, paste or mouse input on the pane **on the Mac** | the Mac widget's size |
| Phone `unwatch`, socket closed, page hidden | the Mac widget's size |
| Mac window or split resized while the phone drives | recorded, applied when the Mac gets the turn back |

The phone thread resizes itself (`TIOCSWINSZ` on a `dup` of the master fd): it
works while helm draws no frame (§4). The rows ignore the iOS keyboard, so
opening it does not resize the agent. While the phone drives, the Mac shows the
narrower grid in its pane and a pill at the pane's top right — Smartphone icon +
*Sized for your phone — click to take it back* (`bg.surface`, `border.subtle`,
`text.secondary`, radius 8) — so the narrow grid does not read as a glitch.
Clicking it, like any click in the pane, takes the size back.

### 7.2 Launching an agent

- **Entry**: a **+** button at the right of the *Agents* large title; hidden when
  the Mac's agent list is empty.
- **Sheet** (bottom, `bg.surface`, rounded top): *Project* — one row per entry,
  project name, worktrees indented under their project with their branch; then
  *Agent* — one chip per configured agent, by name; **Start** (accent) at the
  bottom. The last entry and agent used are preselected (`localStorage`, per
  phone); a vanished one falls back to the first.
- **Start** ⇒ `launch`; on `launched` the sheet closes on the terminal view of the
  new pane (`watch` right away); on `launch_failed` the message shows in the sheet.
- **History**: the sheet is its own entry (`#/new`); *Cancel*, a backdrop tap or a
  back swipe close it; `launched` replaces it with the pane, so back from the new
  pane lands on the agents list.
- **On the Mac**: a login shell in the entry's directory, into which the agent's
  command is typed (as *Send to {agent}* does: exiting the agent leaves a shell). The
  new tab joins the entry's tabs **without** becoming active; its label follows the
  usual auto-naming. Until the UI adopts it, the phone labels it with the agent's
  name.
- **Before the agent shows**: a launched pane is watchable at once, **read-only**
  (`writable = false`, notice *Starting <agent>…*) until the watcher sees the agent
  in its foreground; a command that fails (`command not found`) stays readable there. It enters the agents list
  with its badge, like any other.

### 7.3 Files of the worktree

What the agent wrote — a screenshot, a video, a report — opens on the phone.

- **Scope**: the worktree of an **exposed agent's** pane (its workspace entry's
  directory), all of it, gitignored files included. Never `.git`, never a path
  that resolves outside the worktree (`..`, a symlink leaving it). Symlinks are
  not listed. Read only.
- **Routes**, both behind the session cookie, `404` for a pane with no agent or a
  path out of scope:

| Route | Answer |
|-------|--------|
| `GET /files?pane=<id>&path=<dir>` | `{entries: [{name, dir, size, modified_ms}], total}` — one directory, **newest first**, at most 500 entries (`total` counts them all) |
| `GET /file?pane=<id>&path=<file>` | the file's bytes; `Range` honoured (`206`, `416`) — iOS plays no video or audio without it |

  `path` is relative to the worktree, percent-encoded; empty = its root.
  `Content-Type` from the extension for what a browser shows natively (images,
  video, audio, PDF, HTML); anything else is `text/plain` when its first bytes are
  UTF-8 text, `application/octet-stream` otherwise. `X-Content-Type-Options:
  nosniff` always. HTML and SVG go out with `Content-Security-Policy: sandbox
  allow-scripts`: opened as a page they run in an origin of their own, so a
  script in a worktree file reaches neither the cookie nor the WebSocket (its
  `Origin` is refused, §3.2).
- **Files view** (`#/pane/<id>/files/<dir>`): header with back chevron, *Files*
  over the directory's path; one card of rows — folder or file icon, name, size ·
  age, a chevron on folders. A folder opens as a new history entry (back goes up);
  a file opens the viewer. By folder rather than one flat list of recent files: a
  build directory (`target/`, `node_modules/`) would fill such a list by itself.
  Empty ⇒ *Empty folder*; more than 500 ⇒ *Showing the 500 most recent of <total>*.
- **Viewer** (`#/pane/<id>/file/<path>`): header with back chevron, the file's
  name and **Open** — the raw file in a new tab, Safari's own viewer (zoom, PDF,
  rendered HTML, share sheet). Body by `Content-Type`: image fitted to the width,
  video and audio with the native controls (`playsinline`), text in a monospace
  block (first 256 KB, said so when cut); anything else *No preview — Open it in
  Safari*.
- **The mirror stays watched** while the files are open: the pane keeps the
  phone's size, no redraw on the way back.

## 8. Testing

| Level | What |
|-------|------|
| Unit | grid → `screen` runs (colors, attributes, wide chars, cursor); history paging; pairing code single-use + 5 min, device token hash match, rotation grace (30 s, injected clock), 30-day drop, `User-Agent` → name, `phone_devices.toml` round-trip; `Origin` checks; gateway MAC parsed from `route` / `arp` output; address pick over fixture interfaces; quick-key → bytes; registry keeps a pending pane until a publish lists it or it is forgotten |
| Unit (files) | `Range` → whole / part / unsatisfiable (`a-b`, `a-`, `-n`, past the end, several ranges ignored) |
| Business e2e (files) | server on `127.0.0.1`, an agent pane on a real directory: `GET /files` lists newest first without `.git` nor a symlink; `GET /file` serves an image with its type, a `Range` as `206` with `Content-Range`, an unknown extension as text; HTML goes out sandboxed; `..`, `.git` and a symlink leaving the worktree answer `404`; a plain shell's pane answers `404`; no cookie answers `401`; the address going away closes the listener and the sockets, back ⇒ served again on the same origin, access never stopped; an unrecorded network stops access |
| Business e2e | watcher ticks a real PTY with the `fake_agent_named` fixture with **no UI frame**; server on `127.0.0.1`: pair → cookie → the code is spent → `GET /` rotates the cookie, the new one works → a server restarted on the same store accepts it → a revoked device gets `401` and its socket closes → `agents` lists the fake agent → `send` reaches the PTY → a plain shell pane is never listed; `launch` of a fake agent with **no UI frame** → `launched` → the pane is listed with its badge → `send` reaches it; unknown entry / agent → `launch_failed`; an idle phone receives `ping`; a phone that stops reading is dropped within 10 s |
| App unit | *Start at launch*: on a recorded network access starts, elsewhere not, after a manual Stop not; a drained launch lands as a new, non-active tab of its entry; an entry gone meanwhile drops the pane and forgets it |
| UI e2e (kittest) | palette shows *Open on phone* / *Stop phone access* by state; pairing modal renders QR + URL + device count; Preferences › *Phone* lists devices, Revoke / Revoke all, the *Start at launch* toggle |
| Simulator | `.claude/skills/mobile`: `examples/phone_preview` (real server, fake agents, `--light`, `--loopback`) opened in the iOS simulator's Safari, screenshots — rendering and theme, not taps or the keyboard |
| Manual | iPhone Safari on the LAN: pair, follow a live Claude Code turn, answer a permission prompt, lock the Mac 10 min then resume |

## 9. Accepted limitations

- Plain HTTP on the LAN (§3.3).
- Every turn change is a SIGWINCH: the agent redraws, and what it had already
  printed at the other width stays in the scrollback at that width (history lines
  wider than the phone scroll sideways).
- Two phones on one agent: the last to act sizes it; one leaving hands the size to
  the Mac until the other acts.
- Scrollback read while the agent keeps printing drifts by the lines scrolled
  in meanwhile (history is addressed by grid line); back at the bottom, it resets.
- No notification on the phone: the user opens the page to check.
- A configured agent whose program is not on the watchlist ([`agents.md`](agents.md)
  §2) never gets a badge: the phone keeps it read-only and never lists it
  (Preferences warns, [`preferences.md`](preferences.md) §4).
- Mac asleep (lid closed on battery, manual sleep) ⇒ unreachable until wake.
- Files: an HTML file opened in Safari loads none of the worktree files it links
  to (its sandboxed origin sends no cookie): self-contained pages only. A pane
  whose agent exited no longer serves its worktree. *Open* from a Home Screen web
  app may land in a browser sheet without the cookie (§9, last item).
- One LAN address: moving the Mac to a network no phone was paired on stops
  access; *Start at launch* resumes it back on a recorded network — at the next
  frame helm draws (the check runs on the UI thread, §4).
- The Mac's IP changes (DHCP) ⇒ access follows it (§3.1), but the cookie no longer
  matches the host: rescan. A static lease on the router avoids it.
- Access started on a network with no pairing yet stops if its address goes away.
- An iOS Home Screen web app may keep its own cookies, apart from Safari
  (unverified on a device): then pair from inside it.

## 10. Out of scope (possible follow-ups)

- **Conversation view** from Claude Code's session transcript (bubbles, native
  mobile reading), mapped to a pane through a `SessionStart` hook.
- **HTTPS / Tailscale** (encrypted, off-LAN, prerequisite for Web Push).
- **Push notifications** on agent completion.
- **Tappable paths** in the mirror, opening the viewer (§7.3) on the file named.
