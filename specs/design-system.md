# helm — Design system

Visual reference derived from **Codex Desktop**. This
document fixes the *tokens* (colors, typography, spacing) and the grammar of the
components. It does **not** describe the function of helm (terminal + git,
see [`overview.md`](overview.md)): the Codex screenshot is a chat UI; we reuse
only its **aesthetic**, mapped onto our 3-zone layout (§7).

> **Provenance of the values.** The **light** theme is *pixel-sampled*
> from the Codex Desktop screenshot (3102×2090, retina @2x → 1 logical pt = 2 px) via PIL
> (`Image.quantize` median-cut + patch averages). The **dark** theme is
> **proposed/derived** (the Codex screenshot is light); revised toward
> a **navy** dominant then **re-anchored by PIL sampling of
> the dark mockup** of the git sidebar (`bg.*` / `border.subtle` — the first
> derivation was too dark).
> The metrics are measured by detecting color transitions
> and glyph heights — values in logical points, **approximate**.

## 1. Color tokens

Semantic colors (no hardcoded hex in the code: reference the token).

| Token              | Light (sampled)       | Dark (proposed)      | Usage |
|--------------------|-----------------------|----------------------|-------|
| `accent`           | `#2E68D3`             | `#4F86E8`            | Primary actions, link/active, Commit button |
| `accent.hover`     | `#4579D8`             | `#6E9CEC`            | Accent hover / pressed |
| `accent.subtle`    | `#EAF0FA`             | `#1E2A40`            | Light background behind an active element |
| `accent.ai`        | `#7C5CE0`             | `#9D8AF8`            | AI / agent affordances (Sparkles notes, "Ask {agent}") — kept distinct from `accent` so review comments and agent prompts don't blur |
| `bg.canvas`        | `#FFFFFF`             | `#19222D`            | Central zone + right sidebar |
| `bg.sidebar`       | `#DDDEE1`             | `#10171F`            | Left sidebar |
| `bg.surface`       | `#EFEFF0`             | `#1C2531`            | Pills, input toolbar |
| `bg.surface.hover` | `#F6F6F6`             | `#232D3B`            | Row / surface hover |
| `border.subtle`    | `#D2D4D9`             | `#29323F`            | Pill borders, separators |
| `border.input`     | `#AAB3C5`             | `#3A4252`            | Input card outline (bluish) |
| `text.primary`     | `#1E2030`             | `#ECECEC`            | Title, strong text, icons |
| `text.secondary`   | `#42454A`             | `#B4B5B8`            | Nav items, labels |
| `text.muted`       | `#96989C`             | `#8A8B8F`            | Section headers, shortcuts, placeholder |
| `state.disabled`   | `#919294`             | `#6A6B6E`            | Inactive send/commit button |
| `git.added`        | `#248E4E`             | `#5BB97E`            | Plus icon (added/untracked) |
| `git.modified`     | `#B0780C`             | `#D6A53A`            | Pencil icon (modified) |
| `git.deleted`      | `#C5352E`             | `#E06C66`            | Minus icon (deleted), destructive intent (unstage all, discard) |
| `git.renamed`      | `#26828E`             | `#5BB6C9`            | Arrow icon (renamed) |
| `git.conflict`     | `#CC4C2E`             | `#E8835C`            | Exclamation icon (conflict), out-of-limit subject counter |

The `git.*` colors only tint **the status icon** and the **hover intent**
of a pill: at rest the action pills stay neutral
(`bg.surface` + `border.subtle` + `text.secondary`); on hover, background, border and text
take the intent color (added for Stage, deleted for Unstage/Discard).

## 2. Typography

- **UI**: macOS system font — SF Pro (`system-ui` / `-apple-system`).
- **Mono** (terminal, git branch): SF Mono or JetBrains Mono.

| Role                         | Size (~pt)   | Weight  | Notes |
|------------------------------|--------------|---------|-------|
| Central title                | 24           | 500     | "What should we build?" |
| Nav / chat item              | 13           | 400–500 | `text.secondary` |
| Section header               | 11           | 600     | UPPERCASE, letter-spacing +0.04em, `text.muted` |
| Shortcut badge (`⌘1`)        | 12           | 400     | `text.muted`, tabular figures |
| Input placeholder            | 15           | 400     | `text.muted` |
| Pill label                   | 12–13        | 500     | — |
| Terminal                     | 13           | 400     | mono |

*Sizes anchored on the measurement of the title (~22pt cap-height) and a nav item
(~12.5pt); the others are proportional, to be refined on prototype.*

## 3. Spacing & metrics (logical pt, approx)

| Element                          | Value |
|----------------------------------|--------|
| Default window size              | 1280 × 800 (min 900 × 600) |
| Left sidebar width               | ~330 (≈21 % window) → **helm default: 280** (min 200), resizable |
| Right sidebar width              | **default 320** (min 260), resizable |
| Row height (nav/chat)            | ~26–28 |
| Sidebar horizontal padding       | ~12–14 |
| Inter-section spacing            | ~16–20 |
| Input card radius                | ~16–18 |
| Pill / button radius             | ~8 |
| Row radius on hover              | ~6–8 |
| Borders                          | 1 px `border.*` |
| Window frame                     | rounded corners + drop shadow; transparent titlebar (no custom chrome) |

## 4. Components

- **Cursor** — any **enabled** clickable element (buttons, tabs, rows,
  chips, menu items…) shows the **pointer** cursor (`PointingHand`) on
  hover; disabled ⇒ default cursor. Exceptions: resize handles
  (cursor ↔/↕), text selection zones (diff lines, terminal), and
  the **git graph rows** — only their ref **chips** show the
  pointer (a tag, without checkout or menu, keeps the default cursor).
- **Sidebar nav item** — icon (16pt) + label `text.secondary`, row ~26pt,
  hover = `bg.surface.hover` + radius ~7pt. Active = `accent.subtle` + `accent` text.
- **Section header** — uppercase `text.muted`, top margin ~16pt. The
  **PROJECTS** header carries a **`+` action** aligned right (`text.muted`) that
  triggers **Open Folder** (same action as the shortcut defined in
  [`keybindings.md`](keybindings.md)), preceded by an **eye action** opening a
  dropdown that lists **every** project (hidden ones included) with a checkbox to
  **show/hide** it ([`worktrees.md`](worktrees.md) §1).
- **Project header** ([`worktrees.md`](worktrees.md) §1) — **non-selectable** row
  above each project's worktrees: the **project name** (`text.secondary`, elided
  `…`) left-aligned at the shared sidebar indent, and a **right-edge cluster** —
  a **`+`** create-worktree button and the **collapse chevron** (`text.muted` →
  `text.primary` on hover), plus the **aggregate agent dot** (below) when
  collapsed. *Reveal in Finder · Copy path · Hide project · Remove from sidebar*
  live on the header's **right-click** menu. Clicking the header toggles
  collapse; it never takes the active `accent.subtle` selection (it is not
  selectable).
- **Project separator** — a 1px `border.subtle` rule between one project block and
  the next, bleeding into the sidebar's horizontal padding so it spans the panel
  **edge to edge**.
- **Repository row** (worktree) — a **folder** icon for the **main** worktree / a
  **worktree** (folder-with-git) icon for a **linked** worktree, at the shared
  sidebar indent. The **main** row is **single line** — the **branch** as the label
  (`text.secondary`, elided `…`), folder name on hover. A **linked worktree** row
  is **two lines** — the **folder name** as the title (`text.secondary`), the
  **branch** as a dimmer `text.muted` **monospace** caption beneath. The
  **shortcut badge** (`⌘1`–`⌘9`, counting rows only — headers skipped) only
  appears while **holding `Cmd`**, aligned right in `text.muted`, as an overlay
  without pushing the layout (see [`keybindings.md`](keybindings.md) §5).
  `text.muted` icon + explanatory tooltip for a **vanished path**. Active row =
  `accent.subtle` + `accent` text.
- **Agent activity dot** (repository row + project-header aggregate —
  [`agents.md`](agents.md)) — at the **right edge of the row**, in the `⌘N` badge
  column (mutually exclusive: the badge only exists while Cmd is held): `Working`
  = **accent arc spinner** ~11pt `accent`; `Done` = solid dot r 3.5pt `git.added`
  (green, existing token) wrapped in a **fading pulse halo**; `Idle` = **hollow
  ring** r 3.5pt `text.muted`; `None` = nothing. The project **header** carries
  the same dot as an **aggregate** (max over its worktrees), visible when the
  group is collapsed. No dot on a `deleting` row (the deletion spinner takes
  precedence). A11y: suffix in the row label ("· agent working / done / idle");
  tooltip for Working/Done only.
- **Input card** — `bg.canvas` background, 1px `border.input` border, radius ~16pt,
  padding ~16pt; **integrated bottom toolbar** (same corners) in `bg.surface`
  containing the pills + mic + send button.
- **Pill / dropdown** — `bg.surface` background, `border.subtle` border, radius ~8pt,
  `text.secondary` label + `text.muted` chevron. Bare **tag** variant (PR labels,
  conversation **Author/Reviewer** role tags) drops the chevron — stays monochrome so
  the `accent.ai` hue is reserved for AI/agent surfaces.
- **Detail card** (PR review detail body + conversation comments — pull-requests.md
  §11) — `bg.surface` fill + `border.subtle` 1px + radius ~10pt + ~12–14pt padding,
  raised over the `bg.canvas` detail (distinct from the `bg.canvas` Settings card,
  which sits on a `bg.surface` page).
- **Primary button** (Commit / Open Folder…) — `accent` background **darkened**
  (×0.85 in light, ×0.70 in dark — derived `Palette::primary_button_fill`,
  same on hover over `accent.hover`; the shared token stays full color
  elsewhere), white text, radius ~8pt; disabled = `state.disabled`.
- **Destructive button** (confirmation modals — Discard / Delete) — white
  label on `git.deleted` background, on the right of the button row; neutral Cancel
  on the left.
- **Modal** (confirmations, create worktree) — popup colors with **~16pt
  padding** between the border and the content; controls inside (buttons,
  inputs, rows) use a **discreet ~6pt radius** instead of the pill default;
  primary/destructive action on the right, neutral Cancel on the left.
- **Tab bar** (terminal zone) — in the header of the central zone, over its
  width only. **Active** tab: `accent` text (+ thin border / `accent.subtle`);
  inactive: `text.secondary` on `bg.surface`, hover `bg.surface.hover`. Separators
  1px `border.subtle`. **+** button on the right (`text.muted` → `text.primary` on hover).
  *Absent from the Codex screenshot (chat UI) — proposed style, to be validated on prototype.*
- **Switch Terminal ⇄ Git** (central zone header, post-MVP) — segmented
  control with two segments ("Terminal" / "Git"), **centered** in the header
  of the central zone, coexisting with the tab bar. **Active** segment: `accent`
  text (+ `accent.subtle`); inactive: `text.secondary` on `bg.surface`, hover
  `bg.surface.hover`; radius ~8pt. Each segment **reserves in internal padding** the
  width of the `⇧⌘G` badge on either side of the label: holding Cmd paints the badge
  in the right reserve of the **target segment** without shifting anything
  ([`keybindings.md`](keybindings.md) §5). Toggles the central display
  terminal ⇄ graph ([`git.md`](git.md) §9). *Proposed, to be validated on prototype.*
- **Commit row (graph)** (post-MVP) — lane node (`accent` / `text.secondary` circle)
  + 1px `border.subtle` edges; short hash (mono §2, `text.muted`), summary
  (`text.primary`, elided `…`) — **no author/date column** (author's
  initials in the node, the detail lives in the right sidebar). Branch/tag
  decorations = pills (§ pill above). Selected row: `accent.subtle` background.
  Graph column **bounded** by default (~6 lanes, excess lanes clipped);
  graph ⇄ message boundary **resizable** on drag (cursor ↔, `border.subtle`
  line on hover — [`git.md`](git.md) §9).
- **Graph actions toolbar** (post-MVP) — row of buttons at the top of
  the graph view (above the column headers), aligned left, separated
  from the graph by 1px `border.subtle`: Lucide icon + `text.secondary` label,
  hover `bg.surface.hover`, radius ~8pt. **Pull** = **split-button**: main
  zone (default action) + chevron separated by an internal 1px opening the
  radio menu of the default (§ pill/dropdown). Disabled action = `state.disabled`
  + tooltip; operation in progress = spinner in place of the icon. **Dismissible**
  error banner below the toolbar (`text.primary` on `bg.surface`,
  `border.subtle` border, close cross). ([`git.md`](git.md) §10.)
  *Proposed, to be validated on prototype.*
- **Git sidebar cards** (post-MVP) — the right sidebar (status mode)
  stacks **two cards with no border or background of their own** on `bg.canvas` (the
  sidebar edges form the frame; gap ~10pt): the **main card** (header
  "Git" + branch chip + icons, summary bar, Unstaged/Staged sections in
  **two fixed-height blocks** — same height at 0 entries, internal scroll) and the
  **commit card** detached at the bottom. Header and summary are separated by 1px
  full-width rules; between rows, dimmed 1px separators
  ([`git.md`](git.md) §3).
- **Diff ratio bar** — track ~56×6pt on the right of the summary bar: two
  segments `git.added` / `git.deleted` proportional to the totals **+A** / **−D**
  (minimum readable width for a non-zero side; no change ⇒ empty
  track).
- **File list** (Unstaged/Staged sections of the git sidebar **and**
  commit detail) — **common** style (`ui::file_list`): full-width rows
  ~34pt with square corners **touching** (the breathing room lives in the row height — the
  hover highlight fills the whole line), **internal margin ~10pt** (same
  inset as the summary bars — the icon falls at the same x in all lists),
  dimmed 1px separators; colored status icon (~15pt), elided path `…` (folder
  `text.muted`, file `text.primary`),
  stats `+N`/`−N` in fixed columns aligned right (`git.added` /
  `git.deleted`; binary ⇒ no stat — the sidebar hides them on hover in
  favor of the action pills). Hover = `bg.surface.hover`; selection =
  `accent.subtle` (dark) / `bg.surface` (light) + 3pt accent bar on the
  left. **No scrolling on click** (a clicked row is already visible);
  only keyboard navigation ↑/↓ brings the row into the viewport by the shortest
  path, without centering.
- **Framed input with integrated counter** (commit card) — `bg.canvas` background,
  1px `border.subtle` border, radius **~6pt** (discreet rounding, mockup); the counter
  "n / limit" (`text.muted`, `git.conflict` beyond the indicative limit)
  is **integrated into the frame** (right of the field; under the text for the textarea).
  Label above (`text.secondary`), `git.deleted` asterisk for the required
  field.
- **Commit button** (commit card) — full width, **~34pt tall**, **primary
  button** fill (darkened accent, § above; disabled
  `state.disabled`). **Near-square** corners (~4pt, ≈ 0.06 × the height in the
  mockup), git-branch icon + white centered label.
- **Preferences page** (post-MVP) — **full-window** page in two
  zones ([`preferences.md`](preferences.md)): fixed **left nav** ~240 on
  `bg.sidebar` — **← Back to app** row at the top (arrow icon + `text.secondary`
  label, hover `bg.surface.hover`), then section items
  ("sidebar nav item" grammar above, Lucide icon + label, active =
  `accent.subtle` + `accent` text); **content** on `bg.canvas`, scrollable —
  section title (~24pt, 500, `text.primary`) + stacked settings cards
  (max width ~640pt, gap ~16). *Proposed, to be validated on prototype.*
- **Settings card** (Preferences page) — `bg.canvas` background container, 1px
  `border.subtle` border, radius ~12, internal padding ~16; groups setting
  rows separated by full-width 1px `border.subtle` rules.
- **Setting row** (Preferences page) — min height ~56pt: on the left,
  **label** 13pt weight 500 `text.primary` + optional **description** 12pt
  `text.muted` below; on the right (vertically centered), the **control** —
  segmented (§6), pill/dropdown, toggle, field… depending on the setting.
- **Account badge** (Codex, top-right) — **not reused** in helm
  (standard transparent macOS titlebar, §3).

## 5. Iconography

Thin linear icons (stroke ~1.5pt), monochrome, aligned on `text.muted`
at rest and `text.primary`/`accent` in the active state. SF Symbols style.

## 6. Theme: mode + presets

- Three modes: **Auto** (default, follows macOS `NSApp.effectiveAppearance`),
  **Light**, **Dark**.
- Persisted preference; all components read the §1 tokens, so the toggle
  only changes a *palette*, not the layout.
- **Theme presets**: the §1 tokens are the **Helm** family
  (default). The `theme::PRESETS` registry embeds other families as light/dark
  pairs — **GitHub**, **Catppuccin** (Latte/Mocha), **One**, **Tokyo
  Night** — each carrying its chrome tokens, its **terminal palette**
  ([`terminal.md`](terminal.md) §9) and its **syntect** theme for diff
  syntax highlighting (`Palette.syntax`, two-face set): a single choice recolors
  the whole interface consistently. The chrome tokens of the non-Helm families
  are **derived from the official palettes** of each theme (Primer, Catppuccin,
  One Half, folke/tokyonight), mapped onto the §1 grammar.
- The user chooses **one family per mode** (`light_theme` / `dark_theme`,
  persisted); the resolved appearance (mode + system) selects the variant.
  Unknown id ⇒ **Helm** fallback without rewriting the prefs.
- **Location**: **Preferences** page, Appearance section — mode segmented control
  + Light/Dark theme dropdowns ([`preferences.md`](preferences.md) §4,
  [`keybindings.md`](keybindings.md) §1).

## 7. Mapping to the helm layout

The Codex screenshot shows neither terminal nor git panel; here is how to lay
the aesthetic onto the 3 zones of [`overview.md`](overview.md) §3:

> **Label language**: the entire UI is in **English** — the labels cited
> below are the strings actually rendered.

- **Left sidebar (git repositories)**: Codex sidebar style 1:1 — `bg.sidebar`,
  **PROJECTS** section header, then per project a **non-selectable header** + its
  **worktree rows** ([`worktrees.md`](worktrees.md) §1), active row in
  `accent.subtle`, `⌘1..9` badges aligned right **only while holding `Cmd`**.
- **Center (terminal)**: chrome in `bg.canvas`, **tab bar** in the header
  (§4), split separators in 1px `border.subtle`. The **terminal rendering** keeps
  its own palette (background/ANSI), independent of the chrome's light/dark toggle;
  palette defined in [`terminal.md`](terminal.md) §9.
- **Right sidebar (git, two cards)**: on `bg.canvas`, two cards
  **without border** (§4). **Main card**: header = git-branch icon +
  "Git" + branch chip (mono, §2) + **Discard all** / **Refresh** icons;
  summary bar = "N files changed" + totals **+A** / **−D** (`git.added` /
  `git.deleted`) + ratio bar (§4); **Unstaged (N) / Staged (N)** sections
  collapsible into **two fixed-height blocks** (same height at 0 entries, internal
  scroll) — **colored** status icon (`git.*`) per file, stats `+N` / `−N`
  aligned right at rest, replaced on hover by the
  **Stage / Unstage / Discard** pills (neutral at rest, tinted to the intent on
  hover); global actions **Stage All / Unstage All** in the section header,
  **Discard all** in the header (all destructive actions — discard
  file/all — go through a **confirmation modal**, git.md §3). **Commit
  card**: labels **Commit message** (* required) / **Description (optional)**,
  framed inputs with integrated counter 72 / 1000 (§4); full-width
  **Commit N files** button (§4, near-square, git-branch icon; disabled via
  `state.disabled` as long as nothing is staged or the summary is empty). No
  Commit & Push (git.md §3).
