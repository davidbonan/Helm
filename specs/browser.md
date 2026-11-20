# Browser — the browser sidebar

Spec for the **browser sidebar**: a resizable web page on the right of the
terminals, to preview the worktree's dev server next to them.

## 1. Scope

| In v1 | Out of v1 |
|-------|-----------|
| A **browser toggle** in the title bar (and `⌘⇧B`) shows a resizable sidebar between the terminals and the git sidebar | More than two pages side by side, a separate macOS window |
| **Picture in Picture**: the browser detached into a floating panel over the window (§11) | A separate macOS window, floating over other apps |
| Native engine view (WebKit) over the sidebar, real input, IME, cursor, scroll | Chromium engine (CEF) — the engine port keeps it a drop-in (§2) |
| Full window (§15) and two tabs side by side (§16), neither kept across launches | Split in Picture in Picture, a split of more than two |
| Toolbar: back, forward, address pill (reload/stop inside), Dev server, Favorites, More (Find in page, page zoom, viewport width, Web Inspector, Open in Chrome, Copy URL) | Device emulation beyond the width, a downloads list beyond the tab's shelf |
| Tabs per worktree: a tab row under the toolbar, reordered by dragging; `target="_blank"` and `window.open` in a new tab | A tab dragged out to its own browser |
| Page dialogs (`alert`, `confirm`, `prompt`) on a card, file inputs on the open panel, downloads into Downloads, a failure page (§12–14) | HTTP authentication, a bypass of certificate errors, `beforeunload` prompts |
| Default URL: the server the Run strip printed, else `localhost:$PORT` | Following the server after the first load, element picker, agent bridge |
| One persistent profile shared by every worktree (cookies, storage survive restarts) | Per-worktree profiles, private mode |
| Helm's `Cmd` shortcuts keep working while the page has focus | Tabs persisted across launches (terminals are not either) |
| Favorites on a start page with its own sidebar, a star in the address pill; local pages per project, the rest shared; imported folders kept as a tree; rename, remove | Editing folders, moving favorites, sync |
| Site icons (favicons) on tiles and tabs, Chrome's own brought by its import | Safari's icon cache |
| History of visited pages, suggestions while typing an address | Search-engine suggestions |
| Import from Chrome or Safari: favorites, history, cookies | Passwords, extensions, open tabs |

## 2. Engine

- The page is a **native view** (macOS `WKWebView`) added over the window's content
  view and moved onto the sidebar's page area every frame. Rendering, text input, IME,
  cursor shapes, scroll physics and context menus are the engine's own.
- Helm talks to it through a small **port** (`browser::WebEngine` opens a
  `browser::WebView`: place, load, navigate, page state, focus, inspector). WebKit is
  the one adapter; a CEF adapter implements the same port without touching the sidebar.
- Profile: the engine's default persistent data store, shared by every worktree.
- `target="_blank"` links and `window.open` open **a new tab**, shown right after
  the opener's, its page made by WebKit on the opener's configuration (`window.opener`
  kept for `window.open`, popup logins work). A page calling `window.close()`
  closes its tab (the last one hides the sidebar, §4).
- `<input type="file">` opens the system open panel as a sheet on the window
  (multiple selection and folders as the input allows); cancelled ⇒ nothing chosen.
- No engine (headless tests): the sidebar draws its toolbar and an empty page area.

## 3. Sidebar

- A **globe toggle** in the title bar, left of the git sidebar's toggle (accent
  while shown), and **Toggle browser sidebar** (`⌘⇧B`) show or hide the browser
  sidebar. Both can stay open with the git sidebar: the browser sits between the
  terminals and it, the files sidebar ([`files.md`](files.md) §2) between the
  terminals and the browser. Hidden, with the git sidebar, in the Agents and PR modes, and
  while no worktree is active.
- Resizable by its left edge, 320 pt at least, the terminals keeping 240 pt; its
  width is kept across launches (a 560 pt block by default).
- Each **worktree has its own browser** (tabs, pages) and **its own toggle**: the
  toggle and `⌘⇧B` show or hide the active worktree's sidebar only, kept per
  worktree across launches. The first time its sidebar shows, it opens on the
  worktree's default URL (§5). Switching worktree shows the other's, or none if
  it is hidden there; a worktree leaving the workspace drops its browser.
- Opening the sidebar gives its page the keyboard (the URL field on the start
  page).
- A URL Cmd+clicked in a terminal ([`terminal.md`](terminal.md) §12) opens in a
  new tab of its worktree's browser, shown; the sidebar opens if hidden, and a
  browser not yet opened starts on that URL instead of the default one.

## 4. Panel

- **Toolbar** (one row, 40 pt): back · forward (each only while it leads
  somewhere) · **address pill** (up to 640 pt, centered between the two groups) ·
  **Dev server** (server icon) · **Favorites** (star) · **More** (`⋯`). Under
  it, the **find bar** while open, then the **tab row**, always.
  - **Tint**: over a page, the toolbar takes the page's color — its
    `theme-color`, else the background WebKit derives from the page — and fades
    to it in 0.2 s; no hairline under it. Icons and text take the light or dark
    theme's primary text, whichever contrasts more. Start page, blank page, or a
    translucent color: the palette's canvas and the `border.subtle` hairline.
  - **Fold**: once the page scrolls down past its top 16 pt (8 pt of travel), the
    toolbar folds to 26 pt in 0.18 s, its tab row with it: only the page's
    host, small and centered, on the same tint. 30 pt of upward travel, back near the top, a pointer moving
    over it, or a click (which edits the URL) unfolds it; a new page starts
    unfolded. Read from a script WebKit injects in each page (`window.scrollY`):
    a page scrolling an inner container never folds it.
  - Address pill: raised like Safari's — white on a light bar, a lighter shade on
    a dark one; a capsule (fully rounded ends) with no border, set off by a wide
    soft shadow. At rest, a small lock (`https`) or globe icon and the
    page's host, centered — no scheme, no path. Its star shows on hover, or while
    the page is a favorite. A click edits the full URL, selected; `Enter`
    loads what was typed (scheme kept; a local host or IP gets `http://`, a host
    with a dot or a port `https://`; words or a bare name are searched on Google)
    and gives the page focus; `Esc` restores the page URL. While loading,
    an accent glow fills the pill from the left: it eases toward WebKit's progress,
    creeps on while it stalls, runs to full then fades out. The **star**, then
    **reload**, end the pill at rest: the star filled when the page is a
    favorite, a click adds or removes it (§7); reload turns to stop while
    loading, `Shift`-click reloads bypassing the cache.
  - **Tabs**: a worktree's browser holds one or more tabs, each its own page (own view,
    history, scroll, toolbar fold); the address pill is the shown tab's. The
    **tab row** (34 pt, at the foot of the bar) lists them all, even a lone one, in
    order, ended by **New tab** (`+`). A tab: a badge (the site's icon,
    §10, else its letter as on the start page, or a globe in a grey one while it
    has no page; a spinner while its page loads) and the elided title. The shown
    tab is a folder tab: the page's color, its top corners rounded, its foot
    flaring into the page; the row behind it a step toward the ink, a hairline
    between two unshown tabs.
    - Equal shares of the row (less the `+`), up to 200 pt each, while each keeps 80 pt;
      else the shown tab keeps its title (150 pt) and the others their badge
      (30 pt). Tabs that still do not fit scroll sideways, the shown one kept
      in view.
    - A click on a tab shows it, its page taking the keyboard; hover lights it. Its `×` (titled tabs: on hover, always on the shown
      one) or a middle click closes it. Closing the shown tab shows the next
      one, else the previous. Closing the last tab (its `×`, a middle click or
      `Cmd+W`, §6) hides the sidebar and drops its page: the next opening shows
      a tab on the start page.
    - A tab dragged along the row moves: an accent line marks the gap it would
      land in, on the pointer's side of the tab under it; released, it lands
      there. The shown tab stays shown.
  - **New tab** (`+`): a tab after the shown one, on the start page with the
    URL field focused.
  - **Favorites** (star): shows the start page (§7), pressed meanwhile; a second
    click shows the page again. Disabled while there is no page.
  - **Dev server** (server icon): loads the default URL again; its tooltip names
    it. Disabled when there is none. It tells nothing of the server's state.
  - **More** (`⋯`): a menu, its entries disabled while there is no page:
    - **Find in Page** `⌘F`: opens the find bar.
    - **Zoom** `−` · percent · `+`: the page's zoom, per tab, in Safari's steps
      (50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300 %); a click on
      the percent resets it.
    - **Viewport** Desktop · Tablet · Mobile: the page keeps the sidebar's width,
      or 768 pt, or 390 pt, centered in the page area (full height), the room
      on both sides taking the sidebar color. Per tab; no user-agent change.
    - **Web Inspector**: the engine's inspector (WebKit Web Inspector), docked
      inside the sidebar. Right-click *Inspect Element* also works.
    - **Picture in Picture**: the browser onto the floating panel (§11).
    - **Open in Chrome**: the page URL in Google Chrome (its DevTools), else the
      default browser.
    - **Copy URL**: the page URL to the clipboard.
  - **Find bar** (34 pt, the toolbar's tint): a field, previous / next, *No
    matches* when the text is not on the page, and `×`. Typing finds the text;
    `Enter` the next match, `Shift+Enter` the previous, both wrapping;
    `Esc` or `×` closes it and gives the page the keyboard. Per tab.
- **Page area**: below the toolbar. A click in the page or a press in the toolbar
  gives the page the keyboard; a click in a terminal takes it back. While the
  page holds it, no terminal does.
- **Covered**: the native view is drawn above egui, so it is hidden whenever
  something egui draws could fall on it — address suggestions (§8), a modal
  (command palette included), an open popup or context menu, a pane drag,
  Preferences, the Agents and PR modes, another worktree, the sidebar hidden.
  - **Frozen page**: where the sidebar stays drawn (suggestions, modal, popup, drag),
    the page shows frozen beneath: the view is snapshotted, stays up until the
    snapshot is in (200 ms at most, then hides with none), and the snapshot is
    drawn in its place, at its own size from the page area's top-left corner. No
    snapshot: the page area takes the toolbar's tint. Uncovered, the live view
    comes back at once.
  - A view leaving the screen (another tab shown, the start page, the sidebar
    not drawn) hides at once and takes a last snapshot, drawn while it comes back.
  - A page waiting on its dialog (§12) takes no snapshot (its web process is
    blocked): its view hides at once, the page area keeps a picture only if one
    was already up, else its fill.

## 5. Default URL

1. The **last local URL** the worktree's Run strip printed (`http(s)://localhost`,
   `127.0.0.1`, `0.0.0.0` read as `localhost`, `[::1]`, `*.localhost`); network
   addresses are ignored.
2. Else `http://localhost:$PORT/` when the run command uses `$PORT`.
3. Else none: the page stays blank and the URL field takes focus.

## 6. Keyboard

- While the page has focus, a key with `Cmd` goes to **Helm** (tabs, palette,
  sidebars, Run…) — except the text-editing chords, which stay the page's:
  `Cmd+C`, `Cmd+V`, `Cmd+X`, `Cmd+A`, `Cmd+Z`, `Cmd+Shift+Z`, `Cmd+Delete`,
  `Cmd+Fn+Delete`, `Cmd+arrows` (`Shift` or not), and the app menu's `Cmd+Q`,
  `Cmd+H`, `Cmd+M`. An Edit menu (Undo, Redo, Cut, Copy, Paste, Select All)
  carries the clipboard chords to the page.
- Every other key goes to the page. The terminal shortcuts (`Cmd+D`,
  `Cmd+Alt+arrows`…) are inert meanwhile: no terminal has focus.
- While the browser has the keyboard (its page, its URL or find field), **New
  Tab** (`Cmd+T`) opens a browser tab instead of a helm tab, and **Close pane**
  (`Cmd+W`) closes the shown tab; on the last tab it hides the sidebar, never a
  terminal. **Run** (`Cmd+R`) reloads the shown page, never the server;
  `Cmd+Shift+R` (fixed) reloads it bypassing the cache.
  **Zoom in / out / reset** (`Cmd+=`, `Cmd+-`,
  `Cmd+0`) zoom the shown page instead of the terminals' font, and `Cmd+F`
  (fixed) opens its find bar (§4).

## 7. Favorites — start page

- Kept with the history in helm's support dir (`browser.json`). A favorite
  starred on a **local page** (the hosts of §5) belongs to the project it was
  starred in (the worktree's group root): listed, starred and suggested only
  there. Every other favorite is shared by all projects. Imported favorites keep
  their source and folder path.
- Adding a favorite keeps the page's title, else its host; a URL is a favorite
  once per project (once for the shared ones).
- The **start page** fills the page area while the tab has no page yet, or after
  the toolbar's **Favorites** button; the native view is hidden meanwhile. Loading
  anything (tile, address, Dev server), navigating (back, forward, reload) or
  **Favorites** again leaves it.
- **Sidebar** (220 pt, the sidebar color, a hairline on its right), top to bottom:
  - a **filter** field;
  - **This project** (the Dev server, §5, then the project's local favorites),
    **Starred** (the shared favorites starred by hand), **Frequently visited**
    (the 12 most visited pages of the history);
  - one group per import source, headed *From Chrome* / *From Safari* (*Imported*
    for favorites imported before the source was kept): its folder tree,
    always unfolded, 16 pt deeper per level; favorites at a source's root sit in
    an *Unsorted* row;
  - at its foot, **Import from Chrome** · **Import from Safari** (§9).
  - A row: icon, name, count; a click shows its section, the shown row raised
    (white on light, a lighter shade on dark) with an accent icon.
- **Section**: its path above (*helm-studio*, *From Chrome › Bookmarks Bar*…)
  muted, its name and count, then its tiles. Typing in the filter shows
  *Results* instead: every favorite of the project and the shared and imported
  ones whose title or URL contains the text (case-insensitive); emptying it
  shows the section again. An empty section says how it fills (*Star a local
  page to keep it with this project.*, *No favorites yet. Star a page, or
  import them from your browser.*, *The pages you visit most will show here.*).
- The section shown is kept per tab; a new tab opens on **This project** when it
  has a tile, else **Starred**.
- **Narrow** page area (under 560 pt): no sidebar; a section picker (the shown
  section, `▾`, a menu of the same rows) and the filter head the section.
- **Tiles**: a grid of equal columns, 180 pt at least, gap 12 pt; a tile is
  64 pt high. Its badge: the site's icon (§10), else the title's initial (the
  host's when untitled) on a hue drawn from the host — the Dev server's is its
  server icon on the accent; its title, up to two lines; under it, muted, the
  host, or `:port/path` for a local page.
  - Click opens it in the tab; `Cmd`-click or a middle click opens it in a new
    tab after the shown one, in the background.
  - Right-click menu: **Open**, **Open in New Tab**, **Copy URL**, **Rename…**,
    **Remove from Favorites** (the last two not on the Dev server or a history
    page). **Rename** edits the title in place: `Enter` keeps it, `Esc` or
    clicking away drops it.
- **Import status** (§9): a banner atop the section while an import runs, then
  what came in or why nothing did; `×` dismisses it. Full Disk Access missing:
  the banner holds **Open Privacy Settings**.

## 8. History & suggestions

- Each `http(s)` page that finishes loading counts a visit (URL, title, count,
  last time). The 5000 most visited (then most recent) are kept. Visits are written
  with the app's periodic save and on quit; a favorite change is written at once.
- While the address pill is edited with text, a card hangs under the pill with up
  to **8 suggestions**: favorites first, then history, whose URL or title contains
  the text (case-insensitive), history ranked by visits then recency. The card is
  the pill's width (360 pt at least, within the page area), 6 pt below it, rows of
  32 pt; as many rows as fit. It takes the toolbar's colors: the pill's fill (white
  on a light bar), its shadow widened, the bar's ink; a star (accent) marks a
  favorite, a clock a visit. `↑`/`↓` select, `Enter` loads the selection (none ⇒
  the typed text, §4), a click loads one. The page shows frozen under the card
  (§4 *Frozen page*); over the start page, the start page stays around it.

## 9. Import

- A one-shot copy, no sync, run in the background: the start page's banner shows
  *Importing from Chrome…*, then what came in (counts) or why nothing did.
- **Favorites**: each keeps its source and folder path (§7; Safari's Reading List skipped),
  appended after the existing ones; a URL already there or not `http(s)`
  (`chrome://`, `javascript:`) skipped. **History**: merged
  by URL (highest count, latest visit), `http(s)` only. **Cookies**: into the
  engine's persistent store (§2), expired ones skipped; the count is what the
  engine accepted.
- **Chrome**: its last-used profile (`Local State`), else `Default`. Cookies are
  decrypted with the *Chrome Safe Storage* Keychain item: macOS asks once; refused ⇒
  favorites and history still come in, cookies are reported skipped. Its
  `Favicons` database brings the icons of the imported favorites (§10).
- **Safari**: its files sit behind macOS privacy. Without **Full Disk Access** for
  helm the import stops with that message and an **Open Privacy Settings** button.
- A browser without data on this Mac ⇒ *No Chrome data found* / *No Safari data
  found*.

## 10. Site icons

- One icon per site (scheme-less host and port: `localhost:3000` and
  `localhost:6006` are two sites), kept as a 64 px PNG in helm's support dir
  (`favicons/`), read back at launch.
- A page that finishes loading reports its icon: the largest declared
  `<link rel="icon">` or `apple-touch-icon`, else `/favicon.ico`. A site without a
  kept icon has it fetched in the background, once per launch (`curl`, 5 s);
  PNG, ICO, JPEG, GIF, WebP and BMP are read, anything else (SVG) leaves the
  letter badge.
- Chrome's import brings the icons its `Favicons` database holds for the
  imported favorites, the largest bitmap of each.
- Shown on the start page's tiles and the tab row's badges.

## 11. Picture in Picture

The worktree's browser detached from the sidebar into a **floating panel** over
helm's window: the page alone, to keep an eye on it while the terminals take the
full width.

- **Mode**: a worktree's browser shows either as the sidebar or as the panel.
  **More › Picture in Picture** (sidebar) detaches it, the sidebar hiding; the
  panel's **Dock** button puts it back. No shortcut. The globe toggle and `⌘⇧B`
  show or hide the browser in its current mode.
- **Per worktree**: each worktree keeps its mode and its panel's frame, across
  launches. Switching worktree shows the other's browser in its own mode.
- **Hidden** where the sidebar is (§3): the Agents and PR modes, Preferences, no
  active worktree.
- **Panel**: the shown tab's page fills it, no toolbar, no tab row; a 4 pt rim
  (the toolbar's tint), rounded corners, a wide soft shadow. The start page
  (§7) shows there too, in its narrow layout.
  - Frame: 480 × 320 pt by default, in the terminals' bottom-right corner, 16 pt
    in. Moved and resized freely (edges and corners of the rim), 240 × 160 pt at
    least, anywhere in the window below the title bar; a shrinking window pushes
    it back in, then shrinks it.
  - It covers whatever lies under it (terminals, sidebars); a click in it gives
    the page the keyboard (§6 unchanged), a click elsewhere takes it back.
- **Bar**: while the pointer is over the panel or the bar, a 26 pt bar hangs
  **above** the panel's top edge, outside the page (the native view is drawn
  over egui, §4): the page's host centered, **reload** (stop while loading),
  **Dock**. Dragging the bar moves the panel. The panel keeps room above it for
  the bar.
- **Covered**: as the sidebar (§4 *Covered*, *Frozen page*): a modal, popup,
  suggestions or pane drag freezes the page; a pane drag under the panel shows it
  frozen, not hidden.
- **Keyboard**: §6 applies while the panel's page has focus, but for `Cmd+F`:
  the panel has no find bar. Dialogs (§12) and the failure page (§14) show in
  the panel too; the downloads shelf (§13) does not (files still land). `Cmd+T` opens a tab shown in the panel; `Cmd+W` on
  the last tab hides the browser, the mode kept.

## 12. Page dialogs

- `alert`, `confirm` and `prompt` (any frame) show a **card** over the shown tab's
  page area, its page frozen (§4 *Covered*) under a light scrim that takes the
  clicks: **{site} says** (the page's host and port; *This page says* without
  one), the message (wrapping, scrolling past 260 pt), a field for `prompt`
  (seeded with its default, focused), then **Cancel** (not for `alert`) and
  **OK**, right-aligned. 420 pt wide at most, 16 pt below the top of the page area.
- `Enter` answers OK (the field's text for `prompt`), `Esc` Cancel; the page
  takes the keyboard back. While the card is up, the browser keeps the keyboard
  (no terminal gets `Enter`).
- A dialog of a tab not shown waits until the tab is shown. A tab closing (or
  its page dropped) answers Cancel.

## 13. Downloads

- A response the page cannot show, one sent as `Content-Disposition: attachment`,
  or a link with `download` is **saved** instead of loaded: into the user's
  Downloads folder, under its suggested name's last segment, numbered like
  Finder while taken (`report (1).csv`). The page stays where it was.
- **Shelf**: at the foot of the sidebar's page area, a row (34 pt) per download
  of the shown tab, the 3 latest at most: status icon, file name, *Downloading…*
  or its percent (and an accent bar along the row's foot), *Done* with **Show in
  Finder**, or the failure's reason. A finished or failed row's `×` dismisses
  it; the shelf goes with its last row. The page ends above it.

## 14. Failure page

- A load that fails before showing anything (nothing listening, host not found,
  timeout, offline, TLS), or a page whose web process dies, shows a **failure
  page** in the page area instead of the view: an icon for the reason, *Can't
  reach {site}* (*This page stopped working* for a crash), the reason in words,
  the URL, and **Try again**. The address pill shows the failed URL, the toolbar
  its untinted colors.
- **Try again**, reload and `Cmd+R` load the failed URL again; any new load
  leaves the page.
- Nothing listening on the worktree's own server (§5) while its Run strip is not
  running ⇒ **Run server** first: runs it as `⌘R` does (git sidebar shown, strip
  unfolded; no command yet ⇒ its editor opens). The page does not reload by itself.
- A navigation replaced by another, or turned into a download, is no failure.

## 15. Full window

The worktree's browser over the whole window, under a thin title strip: the
page at full size, the rest of helm a shortcut away.

- **Toggle**: `Cmd+Shift+Enter` (rebindable, keybindings §6) and the toolbar's
  **Full window** button (left of More, its icon a diagonal expand; pressed look
  while full). The shortcut shows the browser first when hidden. `Esc` does not
  leave it (pages use it). Not kept across launches; per worktree.
- **Motion**: the browser grows out of where it was (the sidebar block, the
  floating panel, else the window's right edge) to the whole window in 0.24 s
  (ease out), its corners squaring and a soft shadow lifting it meanwhile, the
  window behind dimmed; leaving runs it backwards. The page shows frozen during
  the motion (§4 *Covered*), live at both ends.
- **Title strip** (the title bar's height, the toolbar's tint so both read as one
  bar): the traffic lights' room, then the project, a muted `/`, and the worktree's
  branch in a chip; at the right, **Exit full window** with its keycap.
- **Underneath**: the layout stays as it was (the browser sidebar's block empty),
  hidden; terminals keep running. Modals, popups and Preferences cover it as they
  cover the sidebar; the Agents and PR modes hide it (the mode is kept).
- Picture in Picture leaves for full window and comes back after it.

## 16. Two tabs side by side

- **Split**: two tabs of a worktree's browser shown side by side, the tab shown
  and its **partner**. One split per browser.
  - A tab's right-click menu: **Split with Current Tab** (the tab joins the shown
    one, on its right), **Unsplit**, **Close Tab**. Or a tab dragged onto the right
    side of the page area: a drop zone lights up (the right half, *Open on the
    right*); released, the tab joins the shown one there.
  - The two tabs sit next to each other in the row: shown, they make one folder
    tab of two compartments, a hairline between them; unshown, a faint capsule
    groups them.
  - Showing either tab shows both; showing another tab shows it alone (the split
    stays). Closing either tab, or **Unsplit**, ends it.
- **Layout**: the page area becomes two rounded cards (10 pt) on the toolbar's
  tint, 6 pt apart and in from its edges; between them a divider, its grip shown
  on hover, dragged to share the width (each card 320 pt at least; halves at
  first). Where both cannot keep 320 pt (a narrow sidebar), only the shown tab
  shows, the split kept.
- **Active half**: the one the toolbar, the find bar, zoom and the keyboard act
  on — the last one clicked. An accent ring (2 pt) circles it. A click in the
  other half makes it active.
- Each half shows its own start page, failure page or dialog; the downloads
  shelf is the active tab's.
- Not in Picture in Picture (the shown tab alone).
