# helm — Terminal

Spec for the central zone: real terminal emulation, Ghostty-style keyboard
splits, **tabs per repository** (each tab = a split tree).
Shortcuts: [`keybindings.md`](keybindings.md). Visual tokens:
[`design-system.md`](design-system.md).

## 1. Model

Three levels of nesting: **repository → tabs → split tree → panes**.

- **Tabs per repository.** Each repository loaded in the left sidebar has its
  **own set of tabs** (at least one). A **tab** = an independent **pane tree**
  (splits), with its own focus. All tabs of a repository share the **same working
  directory**: they only open additional terminals — the branch, the `cwd` and
  the git panel (right sidebar) remain those of the repository, **identical
  regardless of the active tab**.
- **Active repository / active tab.** Selecting another repository **hides** the
  current set of tabs and **restores** that of the target repository as it was
  left (tabs, trees, ratios, focus, active tab, live content). No process is
  killed on switch. Within the active repository, a single tab is displayed; the
  repository/tab selection shortcuts are defined in
  [`keybindings.md`](keybindings.md) §1.
- **Creation**: when a repository is added, its set starts with **a single tab**,
  itself reduced to **a single pane** whose shell is launched in the repository
  folder. The **New Tab** action opens a **new tab** (a fresh pane in the
  repository folder) and gives it focus. As long as no repository is added, no
  tab exists: the central zone displays a prompt
  ([`overview.md`](overview.md) §3).
- **Binary split tree** (within a tab): a node is either a
  **leaf** (a pane = a PTY + an emulation grid), or a **split**
  (vertical or horizontal orientation, two children, a ratio ∈ ]0,1[).
- A single pane has **focus** per tab; it is the one that receives the keyboard.

## 2. PTY & shell

- Crate: **`portable-pty`** (Wezterm).
- Shell: value of `$SHELL`, fallback `/bin/zsh` (macOS default). Launched as an
  **interactive login shell** (`-l`).
- Initial `cwd`: the workspace repository folder. Panes born from a split
  inherit the **current** `cwd` of the parent pane — retrieved via the head pid
  of the PTY (`proc_pidinfo` / `PROC_PIDVNODEPATHINFO`); failing that, the repository folder.
- Environment: inherited from the helm process; `TERM=xterm-256color`,
  `COLORTERM=truecolor`.
- Resizing: any change to the pane's cell size emits a
  `resize` (rows/columns) to the PTY (ioctl `TIOCSWINSZ` handled by the crate).

## 3. Emulation

- Crate: **`alacritty_terminal`** (grid, VTE parser, scrollback, cursor state,
  ANSI/CSI/OSC sequences). helm only **drives** the backend and **reads**
  the grid for rendering — no custom parser.
- One reader thread per PTY pushes the bytes into the parser; an egui repaint
  is requested when the grid changes (see [`architecture.md`](architecture.md) §3).
- Scrollback: **10,000 lines** per pane by default.
- **Responses to program queries**: the reports emitted by the emulation
  (`Event::PtyWrite` — device attributes, device status, kitty query
  `CSI ? u`) and dynamic color requests (`OSC 10/11/12 ; ?`) are relayed
  to the PTY writer using the active terminal palette. Without this relay,
  programs that probe the terminal wait in vain or disable background-aware
  styling.
- **Kitty keyboard protocol** (enabled in the `alacritty_terminal` config):
  mode push/pop (`CSI > flags u` / `CSI < u`) and response to the
  `CSI ? u` query (Codex/crossterm probes then pushes *disambiguate* mode).
- **Shift+Enter / Option+Enter**: encoded as `CSI 13;2u` and `ESC CR` **without
  negotiation** — kitty/Ghostty convention for combos without legacy encoding.
  Claude Code never pushes the kitty protocol: it parses these sequences
  unconditionally and relies on the terminal to emit them by default (the
  "natively supported" of Ghostty/kitty/WezTerm/Warp).
  Accepted trade-off, identical to those terminals: a program that ignores
  CSI u receives an unknown sequence instead of a `\r`. Other keys
  keep the legacy encoding.
- **IME / dead keys**: when the terminal has focus, helm enables the OS IME
  at the terminal cursor and forwards committed text (`Event::Ime::Commit`) to
  the PTY. This is required for macOS accents/dead keys and literal Markdown
  backticks on non-US layouts.

## 4. Rendering (egui)

- The grid is drawn in a **mono** font (SF Mono / JetBrains Mono, ~13pt,
  see design-system §2), cell by cell, with attributes (bold, italic,
  inverse, underline) and colors resolved via the **terminal palette** (§9).
- **Per-cell alignment.** Backgrounds painted as exact full-cell rects (merged
  color runs), text anchored by runs at `col × cell width`; a glyph served by a
  fallback font (advance ≠ cell) is centered alone in its cell instead of
  shifting the end of the line. **Wide** character (CJK, emoji):
  2 cells, its phantom cell is not drawn.
- **Procedural glyphs** (like Ghostty/Kitty): powerline triangles/chevrons
  `U+E0B0–E0B3` and block elements `U+2580–259F` (shades ░▒▓ = alpha fill)
  drawn by the painter, full-cell, seamless — continuous statuslines and progress
  bars, independent of fonts.
- **Mono chain**: SF Mono → **Symbols Nerd Font Mono** (embedded, MIT —
  private-use-area icons of statuslines) → **Apple Symbols** (system: spinner
  braille, miscellaneous symbols) → **Zapf Dingbats** (system: Dingbats block
  U+2700–27BF, Claude Code's ✢✶✻✽ spinner) → egui fonts.
- Cursor: solid block when the pane has focus, hollow outline otherwise.
- **Unfocused split dim** (Ghostty `unfocused-split-opacity`): in a tab with
  several panes, the focused pane stays at full opacity and the others are
  dimmed by a translucent fill of the terminal background — spotlighting the
  active split. A lone pane is always focused, so it is never dimmed.
- Split separators: 1px `border.subtle` (design-system §7).
- **Tab bar**: in the **header of the central zone** (above the split tree, over
  the terminal's width only — it does not overflow onto the git sidebar). It
  lists the tabs of the **active repository**; the active tab is highlighted
  (`accent`), the others recessed. **+** button on the right (**New Tab** action)
  and a close zone per tab. Always visible as soon as a repository is active, even
  with a single tab (for the discoverability of **+**). Style:
  [`design-system.md`](design-system.md) §4.
- **Tab name** (auto): each tab carries a default name derived from the
  **activity of its focused pane**, refreshed at the agent-watch poll (1 s,
  [`agents.md`](agents.md) §2). Display precedence: a **user rename**
  (`rename_tab`, double-click / context menu) wins; otherwise the **auto name**;
  otherwise the positional `Tab N`. The auto name is **sticky** — it survives
  idle periods and only a **new** activity replaces it; clearing a rename reverts
  to it (never straight to `Tab N`). The activity is resolved, in priority order:
  1. the program's **OSC 0/1/2 title** (what the shell or program declares — set
     by shell integrations, ssh, multiplexers, full-screen TUIs);
  2. the **foreground process** — a recognized agent ([`agents.md`](agents.md))
     or the first non-shell command (`vim`, `cargo`, `ssh`…); a bare shell prompt
     names nothing (idle);
  3. the **live folder**, once the shell has left its spawn directory (the repo
     root already names the sidebar entry).
  Runtime-only, like the tabs themselves (§10): auto names are not persisted.
- **Switch Terminal ⇄ Git** (post-MVP): a segmented control in the header of
  the central zone toggles the display between the **terminal** and the **Git
  graph** ([`git.md`](git.md) §9); in Graph mode, the split tree gives way to the
  commit graph. Style: [`design-system.md`](design-system.md) §4.
- **Font zoom**: the mono font size is **global** (shared by all panes and
  repositories); the zoom and reset shortcuts are defined in
  [`keybindings.md`](keybindings.md) §2.

## 5. Splits

- **Vertical split**: creates a new pane on the right. **Horizontal split**:
  creates a new pane at the bottom. The focused pane becomes a split node with two
  leaves, initial ratio **0.5**; **focus moves to the new pane**.
- **Closing**: the focused leaf is removed; its **sibling** takes the entire space
  of the parent node. Focus moves to the sibling (or the nearest descendant).
  Closing the **last leaf** of a tab closes the tab; the case of the very
  last tab is handled in §11.
- **Resizing**: adjusts the ratio of the split containing the focused pane by
  steps of **5 %** in the given direction.
- **Minimum size** of a pane: ~8 columns × 3 rows; a resize
  that would violate this threshold is bounded.
- **Reorganizing (drag & drop)**: hovering a pane reveals a small **drag grip**
  in its top-right corner; dragging it onto another pane drops it according to
  the zone under the pointer — a **directional edge** (left / right / top /
  bottom) **re-splits** the target with the dragged pane on that side, the
  **center** **swaps** the two panes. The dragged pane keeps its PTY and its
  focus follows it; the grip captures only drags, so a plain click still focuses
  the pane below it.

## 6. Focus & navigation

- The focus navigation shortcut defined in [`keybindings.md`](keybindings.md)
  moves focus geometrically to the nearest neighboring pane in the
  given direction (based on on-screen position, not on the tree structure).
- Clicking in a pane gives it focus.

## 7. Selection, copy / paste

- **Mouse reporting.** When the app enables mouse tracking (modes 1000 click /
  1002 button-drag / 1003 any-motion), button **press**, **release** and (under
  1002/1003) button-held **drag** are forwarded to the PTY as mouse reports —
  **SGR** (mode 1006) or legacy **X10** encoding — with the cell under the pointer.
  This is what lets a full-screen TUI (Claude Code) react to a click, e.g. clicking
  a tool to expand it. **Shift** forces the local gesture (selection) and **Cmd**
  keeps the link affordance (§12); both bypass forwarding. Bare hover-motion (1003
  without a button held) is not forwarded (v1).
- Mouse selection (by character; double-click = word; triple-click = line).
- The **Copy** action copies the selection to the macOS clipboard; without a
  selection, it is a no-op (the terminal signal remains available and goes to the PTY).
- The **Paste** action pastes the clipboard into the PTY (bracketed paste if the
  app enabled it).
- **File drop** (Finder): the dropped paths are shell-escaped (backslash
  convention, like Terminal.app), each followed by a space, and pasted into the
  pane **under the pointer** through the paste path (bracketed paste applies —
  TUIs like Claude Code see the drop as a paste). winit surfaces no drop
  position: the mouse is read from CoreGraphics at drop time; without a usable
  position (headless), fallback to the focused pane. A drop outside any pane
  (sidebars, tab bar) is a no-op.

## 8. Scrolling

- Wheel / trackpad and the scroll shortcuts defined in
  [`keybindings.md`](keybindings.md) traverse the scrollback.
- **Wheel forwarded to the application** when it expects it (xterm / alacritty
  semantics): if the app has enabled **mouse reporting** (modes 1000/1002/1003),
  each notch goes to the PTY as a mouse wheel event (**SGR** encoding if mode
  1006, otherwise normal) with the cell under the pointer; otherwise, in **alt
  screen** with **alternate scroll** (mode 1007, active by default — case of a
  full-screen TUI like Claude Code), each notch goes as an arrow ↑/↓ (`ESC O A/B`
  if app cursor, otherwise `ESC [ A/B`). Outside these two cases, the wheel
  traverses the local scrollback. **Shift+wheel** always forces the local
  scrollback.
- Any keystroke (input) brings the view back to the bottom (usual terminal behavior).
- The **Clear Terminal** action empties the scrollback of the focused pane.

## 9. Color palette

The terminal rendering follows the **terminal palette of the active theme**: each
preset of `theme::PRESETS` (design-system §6) embeds its ANSI table +
background/foreground/selection, consistent with its chrome. The table below is
the palette of the **Helm** family (default); the other families (GitHub,
Catppuccin, One, Tokyo Night) reuse the recognized terminal ports of each theme
(`terminal::palette::TermPalette::*`).

> Catppuccin palette **alone** tried then rolled back: visually at odds
> with the chrome that stayed Helm. The Catppuccin colors return via the
> full theme family, chrome included.

| Role | Dark | Light |
|------|--------|-------|
| `background` | `#19222D` | `#FFFFFF` |
| `foreground` | `#ECECEC` | `#1E2030` |
| black / br. black | `#232D3B` / `#5A6374` | `#1E2030` / `#6A6B6E` |
| red / br. red | `#E5484D` / `#FF6369` | `#C42B2B` / `#E5484D` |
| green / br. green | `#46A758` / `#5DC971` | `#297A3A` / `#46A758` |
| yellow / br. yellow | `#E2A03F` / `#F2C55C` | `#9A6700` / `#B7791F` |
| blue / br. blue | `#4F86E8` / `#6E9CEC` | `#2E68D3` / `#4579D8` |
| magenta / br. magenta | `#B05CCC` / `#C77DDB` | `#8E44AD` / `#A65BC2` |
| cyan / br. cyan | `#38A3A5` / `#4FC3C5` | `#1F7A8C` / `#2A93A6` |
| white / br. white | `#C9CACC` / `#ECECEC` | `#D2D4D9` / `#FFFFFF` |

The 256 colors and true-color (24 bits) are rendered as emitted by the PTY;
only the 16 base ANSI colors use the table above.

## 10. Lifecycle

- Creation: when a repository is added, **one tab** (one pane). The **New Tab**
  action adds a tab; closing the last pane of a tab closes the tab (§11).
- Restoration: on repository switch, the in-memory **set of tabs** (trees,
  active tab, focus) is re-displayed — no PTY killed.
- **Cross-session** persistence: the list of repositories is persisted, **not** the
  live terminal sessions (a PTY is not restorable) **nor the number/order
  of tabs**. On restart, each repository starts again with **a fresh tab** (one
  pane). See [`architecture.md`](architecture.md) §4.

## 11. Edge cases

- **Shell process exited** (`exit`): the pane displays a "[process
  exited]" banner; `Enter` relaunches a shell in the same `cwd`.
- **Closing the last pane of a tab** closes the tab and gives focus to the
  neighboring tab. **Closing the last tab** of a repository does not erase the
  repository: it starts again with **a fresh tab** (one pane) — the repository is
  never closed via the terminal close action; removing a repository is an action
  of the left sidebar.
- **PTY open error**: the pane displays the error, without crashing the app.
- **Removing a repository** (left sidebar, [`overview.md`](overview.md) §3.1): all
  the PTYs of **all its tabs** are **killed** and its trees discarded. If it was the
  active repository, we switch to the neighbor; if no repository remains, empty state.
- **Killing a PTY kills its entire process tree** — closing a pane/tab,
  removing a repository or quitting the app (Cmd+Q, closing the window): SIGHUP to
  the foreground job (terminal close semantics) then guaranteed SIGKILL of the
  group and the shell. An agent that ignores SIGHUP (e.g. Claude Code) does not
  survive the app.

## 12. Links (Cmd+hover / Cmd+click)

Holding **Cmd**, the link under the pointer becomes actionable (iTerm2/Ghostty
convention): its cells are **underlined** (keeping their foreground color) and
the cursor switches to a pointing hand; **Cmd+click opens it**. Without Cmd,
nothing changes — selection (§7) and click-to-focus (§6) are untouched.

- **Detected (v1)**: `http(s)://` URLs; **file paths** (absolute, `~`, or
  relative), with an optional `:line(:col)` suffix (compiler/grep output, e.g.
  `src/main.rs:42:7`); **OSC 8 hyperlinks** (already parsed by the emulation —
  the contiguous run of cells sharing the same URI forms the link). A bare
  domain (`example.com`) is **not** a link; trailing punctuation
  (`.,;:)]}'">`) is trimmed.
- **Detection is hover-anchored**: the token under the pointer is expanded on
  the **logical line** (soft-wrapped rows joined via the wrap flag, capped at
  8 visual rows) — no regex, no grid scan; it runs only while Cmd is held
  with the pointer over a cell (and at click time).
- **File paths must exist** to be actionable: relative paths and `~` are
  resolved against the **live cwd of the pane's shell** (same `proc_pidinfo`
  mechanism as §2), falling back to the pane's spawn cwd; a candidate that
  does not resolve to an existing file shows no affordance. Limits (v1):
  paths containing spaces are not detected; directories are not links.
- **Actions**: URL → macOS `open` (default browser). File → the configured
  **IDE** ([`preferences.md`](preferences.md) §4 Terminal: VS Code / Cursor /
  Zed, default VS Code), its CLI spawned detached with the file path and line;
  a CLI that fails surfaces an **error toast** naming the command (no silent
  fallback). OSC 8: `http(s)` URI → browser; `file://` URI (percent-decoded) →
  the file route; other schemes are ignored (v1).
- The rendering stays signal-only: the pane emits an `open_link` intent (path
  already resolved and validated); the app executes it
  ([`architecture.md`](architecture.md) §1).
