# Browser — the browser sidebar

Spec for the **browser sidebar**: a resizable web page on the right of the
terminals, to preview the worktree's dev server next to them.

## 1. Scope

| In v1 | Out of v1 |
|-------|-----------|
| A **browser toggle** in the title bar (and `⌘⇧B`) shows a resizable sidebar between the terminals and the git sidebar | Several browsers side by side, a full-window browser |
| Native engine view (WebKit) over the sidebar, real input, IME, cursor, scroll | Chromium engine (CEF) — the engine port keeps it a drop-in (§2) |
| Toolbar: back, forward, address pill (reload/stop inside), Dev server, Favorites, More (Find in page, page zoom, viewport width, Web Inspector, Open in Chrome, Copy URL) | Downloads, device emulation beyond the width |
| Tabs per worktree: a tab row under the toolbar | Reordering tabs, `target="_blank"` in a new tab |
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
- `target="_blank"` links and `window.open` load **in the same tab**.
- No engine (headless tests): the sidebar draws its toolbar and an empty page area.

## 3. Sidebar

- A **globe toggle** in the title bar, left of the git sidebar's toggle (accent
  while shown), and **Toggle browser sidebar** (`⌘⇧B`) show or hide the browser
  sidebar. Both can stay open with the git sidebar: the browser sits between the
  terminals and it. Hidden, with the git sidebar, in the Agents and PR modes, and
  while no worktree is active.
- Resizable by its left edge, 320 pt at least, the terminals keeping 240 pt; its
  width and whether it shows are kept across launches (560 pt by default).
- Each **worktree has its own browser** (tabs, pages): the first time its sidebar
  shows, it opens on the worktree's default URL (§5). Switching worktree shows the
  other's; a worktree leaving the workspace drops its browser.
- Opening the sidebar gives its page the keyboard (the URL field on the start
  page).

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
    **tab row** (34 pt, at the foot of the bar) lists them all, even a lone one, in a
    fixed order, ended by **New tab** (`+`). A tab: a badge (the site's icon,
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

## 5. Default URL

1. The **last local URL** the worktree's Run strip printed (`http(s)://localhost`,
   `127.0.0.1`, `0.0.0.0` read as `localhost`, `[::1]`, `*.localhost`); network
   addresses are ignored.
2. Else `http://localhost:$PORT/` when the run command uses `$PORT`.
3. Else none: the page stays blank and the URL field takes focus.

## 6. Keyboard

- While the page has focus, a key with `Cmd` goes to **Helm** (tabs, palette,
  sidebars, Run…) — except the text-editing chords, which stay the page's:
  `Cmd+C`, `Cmd+V`, `Cmd+X`, `Cmd+A`, `Cmd+Z`, `Cmd+Shift+Z`, and the app menu's
  `Cmd+Q`, `Cmd+H`, `Cmd+M`.
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
