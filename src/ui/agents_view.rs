//! Cross-repo agents dashboard (specs/agents.md): the central content rendered
//! while the sidebar's Agents entry is selected — a left list of every running
//! (or just-finished) agent across all open repositories (grouped by project)
//! and a right panel mirroring the selected agent's terminal live. Clicking a
//! row body selects it (the panel follows); its discreet jump icon focuses that
//! agent's workspace. Rendering only — the page returns the targeted action, the
//! app applies it.

use serde::{Deserialize, Serialize};

use crate::agent_watch::AgentBadge;
use crate::theme::{self, Palette};
use crate::ui::preferences::{setting_divider, settings_card};
use crate::ui::spinner::{paint_done_dot, Spinner};
use crate::ui::{clickable, paint_icon, with_alpha};

/// Which dashboard layout the central area shows. **List** is the master-detail
/// cockpit (a list + one mirrored terminal); **Columns** is a grid — a fixed-width
/// column per **worktree**, headed by one light project · branch line, holding a
/// status card per agent that expands to its live terminal (specs/agents.md §5).
/// Persisted in `Prefs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentsViewMode {
    #[default]
    List,
    Columns,
}

/// How a Columns card asks the app to draw its pane: the **selected** card shows
/// the `Full` interactive terminal, every other card a read-only `Preview` (a
/// scaled tail of its last lines — a progress glance). The app dispatches on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermView {
    Full,
    Preview,
}

/// Meaningful lines a collapsed card's progress preview shows (its recent
/// conversation, at readable native size, chrome stripped).
pub const AGENT_PREVIEW_LINES: usize = 12;

const CONTENT_PAD_X: f32 = 32.0;
const CONTENT_PAD_Y: f32 = 32.0;
const CONTENT_MAX_WIDTH: f32 = 720.0;
const SUBTITLE_SIZE: f32 = 13.0;

const CARD_PAD_X: f32 = 16.0;
const CARD_HEADER_HEIGHT: f32 = 48.0;
const ROW_HEIGHT: f32 = 60.0;
const GROUP_GAP: f32 = 16.0;
const ROW_RADIUS: u8 = 8;
/// Narrow elision, shared by a list row (name + branch chip) and a column card's
/// status band (name + tab label): both stop short of the right-aligned state
/// caption — reserving a minimum for the second label — and gain a `…` instead of
/// running underneath it. A tab is renamed freely, so at `COLUMN_MIN_WIDTH` it does
/// reach the caption.
const ROW_TEXT_GAP: f32 = 10.0;
const ROW_MIN_NAME_W: f32 = 40.0;
const ROW_MIN_CHIP_W: f32 = 48.0;

/// Columns view: one column per worktree on a single 2D scroll plane (horizontal
/// between columns, vertical down the wall). **One or two** columns fill the
/// viewport (full width / 50-50); **three or more** keep the persisted,
/// **resizable** shared width (drag the handle in any gap, clamped to
/// [`COLUMN_MIN_WIDTH`, `COLUMN_MAX_WIDTH`]) and let the wall scroll — as does a
/// two-up split that would fall below `COLUMN_MIN_WIDTH` on a narrow window.
/// Every column is a **borderless tinted lane** (its project's hue), not a bordered
/// card: its header, its agents' status bands and their terminals are flat rows
/// stacked on it, hairline-separated and flush, so the wall reads as one surface per
/// worktree — strokeless throughout, the active card's spine included.
const COLUMN_MIN_WIDTH: f32 = 420.0;
const COLUMN_MAX_WIDTH: f32 = 1200.0;
const COLUMN_GAP: f32 = 8.0;
/// Slack left when sizing columns to fill the viewport, so sub-pixel rounding never
/// nudges content past the edge into a spurious scrollbar.
const COLUMN_FILL_SLACK: f32 = 2.0;
/// The lane is rounded at the **top** only: its content runs flush to the bottom
/// (the filling terminal ends there, square), so rounding it would clip the pane.
const COLUMN_RADIUS: u8 = 12;
/// Leading inset before the first column — tight so the wall of terminals sits
/// close to the edge (the central panel already adds its own small margin).
const COLUMN_EDGE_PAD: f32 = 8.0;
/// Breathing room between the page-header hairline and the top of the columns,
/// so they don't sit flush against it.
const COLUMN_TOP_MARGIN: f32 = 8.0;
/// The column's single light header — project · branch (+ dirty bar / agent
/// count) on one line (the list view keeps the taller `CARD_HEADER_HEIGHT`).
const COLUMN_HEADER_HEIGHT: f32 = 34.0;
/// Floor for the expanded terminal: it fills the column's leftover height, so this
/// only kicks in when the collapsed siblings leave it less than a workable strip
/// (then the column grows past the viewport and the wall scrolls).
const TERM_MIN_HEIGHT: f32 = 360.0;
/// Also the height of the band's jump target (`CARD_JUMP_HIT` wide): 40 keeps it at
/// the 40×40 floor a dense desktop UI should still afford.
const AGENT_HEADER_HEIGHT: f32 = 40.0;

/// Distinguishing the single **active** (keyboard) card from the other cards: the
/// active one carries a **spine** flush on its left edge — a filled rail running that
/// card alone, its status band down to its terminal, never the whole column — while
/// every other card recedes behind the terminal's own unfocused dim, one single dim
/// level, so a collapsed preview and a mirrored terminal read equally recessed. A rail,
/// not a ring: the wall stays strokeless and the terminal keeps its full width. It is
/// painted in the **column header's own wash** (`project_header_tint`), so it introduces
/// no color of its own — the header's strip simply runs down the side of the card it
/// belongs to, and the wall stays at one color per project.
const AGENTS_ACTIVE_SPINE: f32 = 3.0;
/// Every agent's status band wears its project's hue too — focused or not — so each pane
/// reads as belonging to its column at a glance. Its wash sits **under** the column
/// header's (`HEADER_ICON_TINT`), which stays the firmest strip on the lane, and lifts
/// under the pointer; the hue is the column's throughout, so no band adds a color.
const AGENTS_BAND_TINT: f32 = 0.10;
const AGENTS_BAND_HOVER_TINT: f32 = 0.22;
/// A pane unfolds instead of snapping in when its card takes focus, and the preview it
/// displaces fades in on the same clock. Short enough to read as a transition rather
/// than as a wait.
const AGENT_UNFOLD_TIME: f32 = 0.16;

/// Compact uncommitted-changes ratio bar on a dirty worktree header — same
/// green/red proportion device as the workspace sidebar (`repo_sidebar`).
const STAT_BAR_W: f32 = 26.0;
const STAT_BAR_H: f32 = 3.0;
const STAT_BAR_MIN: f32 = 3.0;
const STAT_BAR_GAP: f32 = 1.5;

/// Each column is tinted with its project's hue (cycled from `palette.lane_colors`,
/// the theme-tuned graph palette) so projects read apart at a glance and a project's
/// worktrees read together. The hue is mixed against the theme's own base, not
/// applied flat, so it stays balanced in light and dark.
const COLUMN_TINT_BODY: f32 = 0.10;
/// Sidebar header icon: a touch firmer than the column body — the tint carries the
/// project color on a much smaller surface.
const HEADER_ICON_TINT: f32 = 0.16;

const REPO_ICON_SIZE: f32 = 15.0;
const NAME_SIZE: f32 = 14.0;
const TAB_SIZE: f32 = 12.0;
const CHIP_SIZE: f32 = 11.5;
const DETAIL_SIZE: f32 = 12.0;
const INDICATOR_SIZE: f32 = 16.0;
const JUMP_ICON_SIZE: f32 = 15.0;
const JUMP_ICON_HIT: f32 = 28.0;
/// On a column card the jump icon's target spans the status band's full height and a
/// comfortable width (dense-desktop 40px minimum); the chip it paints on hover stays
/// small, so the affordance is easy to hit without looking heavy.
const CARD_JUMP_HIT: f32 = 40.0;
const CARD_JUMP_CHIP: f32 = 26.0;
/// Trailing pad of the jump glyph on a column card: `CARD_PAD_X` less 2px — the
/// optical correction that lands an icon on the same right rail as the column
/// header's text, which at equal padding would look short of it.
const CARD_JUMP_PAD: f32 = CARD_PAD_X - 2.0;

/// Left list width when the right terminal panel is shown; below
/// `LIST_WIDTH + PANEL_MIN_WIDTH` the panel folds away (list spans full width).
const LIST_WIDTH: f32 = 440.0;
const PANEL_MIN_WIDTH: f32 = 380.0;
const PANEL_HEADER_HEIGHT: f32 = 44.0;
const PANEL_PAD_X: f32 = 16.0;

/// One agent line of the dashboard — the app builds these from `RepoCaches`.
pub struct AgentRow<'a> {
    /// Project name = the group root's name; a root and its worktrees share it,
    /// so their agents group under one card.
    pub repo: &'a str,
    /// This entry's own branch, shown as a per-row chip — it tells worktree rows
    /// of the same project apart.
    pub branch: Option<&'a str>,
    pub tab: &'a str,
    pub agent: &'a str,
    pub badge: AgentBadge,
    /// State-relative caption built by the app ("Working…", "Finished 2m ago",
    /// "Idle").
    pub detail: String,
    /// Stable per-worktree discriminator (the app's index of this entry's repo in
    /// the workspace): equal ids = same worktree. Splits a project's rows into
    /// worktree sub-cards in the column view; ignored by the list view.
    pub worktree_id: usize,
    /// Project color index (rank of the group root among root projects): tints this
    /// row's column and, via the same index, the project's sidebar header icon.
    pub lane: usize,
    /// Uncommitted line stats `(additions, deletions)` of this row's worktree when
    /// dirty: drives the ratio bar on the column view's worktree header (a
    /// worktree's rows all carry the same value). `None` when clean.
    pub stats: Option<(usize, usize)>,
}

/// What a click on the dashboard targeted: a row body (mirror that agent in the
/// right panel) or its jump icon (focus that agent's workspace).
#[derive(Default)]
pub struct AgentsPageAction {
    pub select: Option<usize>,
    pub jump: Option<usize>,
    /// The view-mode toggle was clicked: the app persists the new mode.
    pub set_view: Option<AgentsViewMode>,
    /// The columns view's width handle was dragged: the app stores the new
    /// shared column width and persists it (debounced, like a sidebar drag).
    pub set_column_width: Option<f32>,
}

/// Cross-repo dashboard for the central area. A centered List/Columns switch —
/// sharing the Terminal/Git switch's design and titlebar placement — tops both
/// views; below it, **List** is the master-detail cockpit (a left list + a right
/// panel mirroring the selected agent's terminal) and **Columns** is a wall of
/// status cards, one fixed-width column per worktree, where **each** column expands
/// one card to its live terminal. `render_terminal(idx, ui)` draws the live pane
/// for the agent at row `idx` — called for the selected row (List) or once per
/// column's expanded card (Columns). Returns the targeted action
/// (select / jump / view).
#[allow(clippy::too_many_arguments)]
pub fn agents_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    view: AgentsViewMode,
    column_width: f32,
    render_terminal: impl FnMut(usize, &mut egui::Ui, TermView),
) -> AgentsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);

    let set_view = crate::ui::agents_view_switch(ui, palette, view == AgentsViewMode::Columns).map(
        |columns| {
            if columns {
                AgentsViewMode::Columns
            } else {
                AgentsViewMode::List
            }
        },
    );
    let mut action = AgentsPageAction {
        set_view,
        ..Default::default()
    };
    let body = ui.available_rect_before_wrap();

    match view {
        AgentsViewMode::List => render_list_view(
            ui,
            palette,
            rows,
            selected,
            body,
            render_terminal,
            &mut action,
        ),
        AgentsViewMode::Columns => render_columns(
            ui,
            palette,
            rows,
            selected,
            body,
            column_width,
            &mut action,
            render_terminal,
        ),
    }
    action
}

/// List view body: the scrollable project list and, when an agent is selected and
/// there's room, a right panel mirroring its terminal live.
fn render_list_view(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    render_terminal: impl FnMut(usize, &mut egui::Ui, TermView),
    action: &mut AgentsPageAction,
) {
    let show_panel =
        !rows.is_empty() && selected.is_some() && rect.width() >= LIST_WIDTH + PANEL_MIN_WIDTH;
    let list_right = if show_panel {
        rect.left() + LIST_WIDTH
    } else {
        rect.right()
    };
    let list_rect =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(rect.left(), list_right), rect.y_range());
    render_list(ui, palette, rows, selected, list_rect, action);

    if show_panel {
        let panel_rect = egui::Rect::from_x_y_ranges(
            egui::Rangef::new(list_right, rect.right()),
            rect.y_range(),
        );
        render_panel(ui, palette, rows, selected, panel_rect, render_terminal);
    }
}

/// Left column: the scrollable list of project cards (or the empty state). The
/// view switch sits in the titlebar, so this starts straight at the cards.
fn render_list(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    action: &mut AgentsPageAction,
) {
    let column_width = (rect.width() - 2.0 * CONTENT_PAD_X).clamp(0.0, CONTENT_MAX_WIDTH);
    let left = rect.left() + CONTENT_PAD_X;
    let content_inner =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(left, left + column_width), rect.y_range());
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("agents_list")
        .show(&mut content, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(CONTENT_PAD_Y);
            if rows.is_empty() {
                empty_state(ui, palette);
            } else {
                for (start, end) in groups(rows) {
                    settings_card(ui, palette, |ui| {
                        card_header(ui, palette, &rows[start], end - start);
                        for (offset, row) in rows[start..end].iter().enumerate() {
                            setting_divider(ui, palette);
                            let idx = start + offset;
                            let click = agent_row(ui, palette, row, selected == Some(idx));
                            if click.jump {
                                action.jump = Some(idx);
                            } else if click.body {
                                action.select = Some(idx);
                            }
                        }
                    });
                    ui.add_space(GROUP_GAP);
                }
            }
            ui.add_space(CONTENT_PAD_Y);
        });
}

/// Right panel: a divider, a compact header naming the mirrored agent, and the
/// live terminal filling the rest (drawn by `render_terminal` for the selected
/// row). `rect` sits below the page header, so no titlebar inset is re-added.
fn render_panel(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    mut render_terminal: impl FnMut(usize, &mut egui::Ui, TermView),
) {
    ui.painter().vline(
        rect.left(),
        rect.y_range(),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let header_bottom = rect.top() + PANEL_HEADER_HEIGHT;
    if let Some(row) = selected.and_then(|i| rows.get(i)) {
        panel_header(ui, palette, row, rect, header_bottom);
    }
    ui.painter().hline(
        rect.x_range(),
        header_bottom,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let term_rect = egui::Rect::from_min_max(egui::pos2(rect.left(), header_bottom), rect.max);
    let mut panel_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(term_rect)
            .id_salt("agents_terminal_panel")
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if let Some(idx) = selected {
        render_terminal(idx, &mut panel_ui, TermView::Full);
    }
}

/// Compact title bar for the mirrored terminal: agent name, branch chip, tab.
fn panel_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    rect: egui::Rect,
    header_bottom: f32,
) {
    let center_y = (rect.top() + header_bottom) / 2.0;
    let left = rect.left() + PANEL_PAD_X;
    let right = rect.right() - PANEL_PAD_X;
    let painter = ui.painter();
    let name_end = painter
        .text(
            egui::pos2(left, center_y),
            egui::Align2::LEFT_CENTER,
            crate::agent_watch::display_name(row.agent),
            egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())),
            palette.text_primary,
        )
        .right();
    painter.text(
        egui::pos2(right, center_y),
        egui::Align2::RIGHT_CENTER,
        row.tab,
        egui::FontId::proportional(TAB_SIZE),
        palette.text_muted,
    );
    if let Some(branch) = row.branch.filter(|b| !b.is_empty()) {
        branch_chip(ui, palette, branch, name_end + 8.0, center_y, f32::INFINITY);
    }
}

/// Columns view: a wall of fixed-width **worktree** columns on a single 2D scroll
/// plane — horizontal between columns, vertical down the wall. Each column carries
/// one light project · branch header, then a status card per agent. Every column
/// expands **one** card to its live terminal (the selected agent when it lives here,
/// else the column's most urgent one — `column_expanded`), the rest a collapsed
/// preview. `render_terminal(idx, ui)` draws the pane for the agent at row `idx`.
#[allow(clippy::too_many_arguments)]
fn render_columns(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    column_width: f32,
    action: &mut AgentsPageAction,
    mut render_terminal: impl FnMut(usize, &mut egui::Ui, TermView),
) {
    if rows.is_empty() {
        let inner = egui::Rect::from_min_max(
            egui::pos2(rect.left() + CONTENT_PAD_X, rect.top()),
            rect.max,
        );
        let mut e = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        empty_state(&mut e, palette);
        return;
    }
    let columns = worktree_columns(rows);
    let n = columns.len();
    // Width that fills the viewport evenly: the trailing gap lives outside
    // `column_width`, so back it out per column.
    let fill =
        (rect.width() - COLUMN_EDGE_PAD - n as f32 * COLUMN_GAP - COLUMN_FILL_SLACK) / n as f32;
    // Only one or two projects fill the viewport (full width / 50-50). Three or more,
    // or a fill below the minimum (a narrow window), keep the comfortable persisted
    // width and let the wall scroll — an even split would crush the columns, which
    // breaks down on a small screen.
    let fill_viewport = n <= 2 && fill >= COLUMN_MIN_WIDTH;
    let resizable = !fill_viewport;
    let column_width = if fill_viewport {
        fill
    } else {
        column_width.clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH)
    };
    // Each column fills at least the visible viewport height, so its tinted lane
    // reads full-height even when its agents are collapsed; taller content scrolls.
    let col_min_height = (rect.height() - COLUMN_TOP_MARGIN - COLUMN_FILL_SLACK).max(0.0);
    let mut area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // One 2D scroll plane for the whole wall: horizontal between columns, vertical
    // down the wall. No per-column scroll — each column fills the viewport height,
    // then grows with its content, so the tallest column drives the vertical extent.
    egui::ScrollArea::both()
        .id_salt("agents_columns")
        .show(&mut area, |ui| {
            ui.add_space(COLUMN_TOP_MARGIN);
            ui.horizontal_top(|ui| {
                // Gaps are the explicit edge pad + per-column resize handle; drop the
                // inherited item spacing so the fill math lands on the viewport edge.
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(COLUMN_EDGE_PAD);
                for (col_idx, (start, end)) in columns.into_iter().enumerate() {
                    let lane = rows[start].lane;
                    // The lane is a bare tinted surface — no border, no padding: its
                    // header, status bands and terminals are flat full-bleed rows on it,
                    // so the column reads as one surface instead of nested cards.
                    let col = egui::Frame::new()
                        .fill(project_tint(palette, lane))
                        .corner_radius(egui::CornerRadius {
                            nw: COLUMN_RADIUS,
                            ne: COLUMN_RADIUS,
                            sw: 0,
                            se: 0,
                        })
                        .show(ui, |ui| {
                            // The frame inherits the row's horizontal layout; the column
                            // stacks top-down inside it.
                            ui.vertical(|ui| {
                                ui.set_width(column_width);
                                ui.set_min_height(col_min_height);
                                worktree_column(
                                    ui,
                                    palette,
                                    rows,
                                    start,
                                    end,
                                    selected,
                                    col_min_height,
                                    action,
                                    &mut render_terminal,
                                );
                            });
                        });
                    let col_height = col.response.rect.height();
                    column_resize_handle(
                        ui,
                        palette,
                        col_idx,
                        col_height,
                        column_width,
                        resizable,
                        action,
                    );
                }
            });
        });
}

/// Separator occupying a column's trailing gap. While the wall overflows
/// (`resizable`), dragging adjusts the shared column width (clamped, persisted by
/// the app), surfaced by a faint accent rule and the resize cursor on hover. When
/// columns fill the viewport evenly the width is derived, so it's just empty gap.
fn column_resize_handle(
    ui: &mut egui::Ui,
    palette: &Palette,
    col_idx: usize,
    col_height: f32,
    column_width: f32,
    resizable: bool,
    action: &mut AgentsPageAction,
) {
    let (gap, _) = ui.allocate_exact_size(egui::vec2(COLUMN_GAP, col_height), egui::Sense::hover());
    if !resizable {
        return;
    }
    let handle = ui.interact(
        gap.expand2(egui::vec2(3.0, 0.0)),
        ui.id().with(("agents_col_resize", col_idx)),
        egui::Sense::drag(),
    );
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ui.painter().vline(
            gap.center().x,
            egui::Rangef::new(gap.top() + 10.0, gap.bottom() - 10.0),
            egui::Stroke::new(2.0_f32, palette.accent),
        );
    }
    if handle.dragged() {
        action.set_column_width =
            Some((column_width + handle.drag_delta().x).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH));
    }
}

/// One worktree column: the single light header (project · branch), then its agents
/// stacked flush on the column's lane — no per-agent card frame, a hairline between
/// each. The whole wall is a single 2D scroll plane, so the column hugs its full
/// content height rather than scrolling on its own.
#[allow(clippy::too_many_arguments)]
fn worktree_column<F: FnMut(usize, &mut egui::Ui, TermView)>(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    start: usize,
    end: usize,
    selected: Option<usize>,
    col_min_height: f32,
    action: &mut AgentsPageAction,
    render_terminal: &mut F,
) {
    // The column lays out on explicit spacing alone (headers, bands and terminal
    // strips are allocated at exact heights), so the inherited item spacing would
    // just add slack between them.
    ui.spacing_mut().item_spacing.y = 0.0;
    let col_top = ui.min_rect().top();
    column_header(ui, palette, &rows[start], end - start);
    // The agents stack in a **permanently** inset body: the gutter it leaves on the lane
    // is where the active card paints its focus spine, so the spine never covers a
    // terminal's first column of glyphs and selecting another card shifts no content.
    let body = ui.available_rect_before_wrap();
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(body.left() + AGENTS_ACTIVE_SPINE, body.top()),
                body.max,
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    body_ui.spacing_mut().item_spacing.y = 0.0;
    // Every column expands one card (agents.md §5): the selected agent when it lives
    // here, else the column's most urgent one. That card's terminal swallows the
    // column's leftover height instead of sitting at a fixed strip above an empty
    // lane, so the wall reads as equal-height columns. The collapsed siblings' height
    // isn't statically known (each preview hugs a variable row count), so size the
    // strip from the column *overhead* — every laid-out pixel but the strip — measured
    // the previous frame. Live panes repaint each frame, so the settle is unseen.
    let expanded = column_expanded(rows, start, end, selected);
    let col_id = ui.id().with(("agents_col_fill", start));
    let fill_height = match ui.data(|d| d.get_temp::<f32>(col_id)) {
        Some(overhead) => (col_min_height - overhead).max(TERM_MIN_HEIGHT),
        None => TERM_MIN_HEIGHT,
    };
    for (offset, row) in rows[start..end].iter().enumerate() {
        let idx = start + offset;
        // A hairline is all that separates the stacked agents (under the header, then
        // between each pane and the next status band) — the rows themselves stay flush.
        hairline(&mut body_ui, palette);
        agent_terminal_card(
            &mut body_ui,
            palette,
            row,
            idx,
            idx == expanded,
            selected == Some(idx),
            fill_height,
            action,
            render_terminal,
        );
    }
    let overhead = body_ui.next_widget_position().y - col_top - fill_height;
    ui.data_mut(|d| d.insert_temp(col_id, overhead));
    // The inset body is a child ui: hand its extent back so the lane (and the frame
    // measuring it) grows with the agents stacked inside it.
    ui.advance_cursor_after_rect(body_ui.min_rect());
}

/// The one rule the flattened column uses: a full-bleed hairline between two of its
/// stacked rows, in place of any card frame around them.
fn hairline(ui: &mut egui::Ui, palette: &Palette) {
    let (rule, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    // Full-bleed over the spine gutter the body is inset by: the rule is the only
    // structure left on a flattened column, so stopping 3px short of the lane's edge
    // reads as a nick in it rather than as the gutter.
    ui.painter().hline(
        egui::Rangef::new(rule.left() - AGENTS_ACTIVE_SPINE, rule.right()),
        rule.center().y,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

/// The column's single light header, gathering every project/worktree indication on
/// one line: repo icon + project name, branch, then — right-aligned — the
/// uncommitted ratio bar when the worktree is dirty and the agent count once it
/// holds more than one. Its own row carries the **project hue** at the column's firmest
/// wash — above the lane behind it and the status bands below, which share its base and
/// its hue: once the agents run full-bleed over the lane, this strip is where a project
/// reads at a glance.
fn column_header(ui: &mut egui::Ui, palette: &Palette, first: &AgentRow, count: usize) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), COLUMN_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius {
            nw: COLUMN_RADIUS,
            ne: COLUMN_RADIUS,
            sw: 0,
            se: 0,
        },
        project_header_tint(palette, first.lane),
    );
    let inner = rect.shrink2(egui::vec2(CARD_PAD_X, 0.0));
    let center_y = inner.center().y;
    // Lay the right edge out first: the count and the ratio bar bound the room the
    // branch label gets before it elides.
    let mut right = inner.right();
    if count > 1 {
        let label = format!("{count} agents");
        let font = egui::FontId::proportional(DETAIL_SIZE);
        let width = ui
            .painter()
            .layout_no_wrap(label.clone(), font.clone(), palette.text_muted)
            .size()
            .x;
        ui.painter().text(
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            label,
            font,
            palette.text_muted,
        );
        right -= width + 10.0;
    }
    let dirty = first.stats.filter(|&(a, d)| a > 0 || d > 0);
    if let Some((additions, deletions)) = dirty {
        paint_stat_bar(ui, palette, right, center_y, additions, deletions);
        right -= STAT_BAR_W + 10.0;
    }
    let painter = ui.painter();
    paint_icon(
        painter,
        egui::pos2(inner.left() + REPO_ICON_SIZE / 2.0, center_y),
        REPO_ICON_SIZE,
        lucide_icons::Icon::FolderGit2,
        palette.text_secondary,
    );
    let name_end = painter
        .text(
            egui::pos2(inner.left() + REPO_ICON_SIZE + 10.0, center_y),
            egui::Align2::LEFT_CENTER,
            first.repo,
            egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())),
            palette.text_primary,
        )
        .right();
    let label = first.branch.filter(|b| !b.is_empty()).unwrap_or("—");
    let branch_icon_x = name_end + 12.0;
    paint_icon(
        painter,
        egui::pos2(branch_icon_x + REPO_ICON_SIZE / 2.0, center_y),
        REPO_ICON_SIZE - 1.0,
        lucide_icons::Icon::GitBranch,
        palette.accent,
    );
    let branch_x = branch_icon_x + REPO_ICON_SIZE + 6.0;
    paint_elided(
        painter,
        egui::pos2(branch_x, center_y),
        label,
        egui::FontId::monospace(CHIP_SIZE + 1.0),
        palette.text_secondary,
        right - ROW_TEXT_GAP - branch_x,
    );
    let info = match dirty {
        Some((a, d)) => format!("{} · {label} · +{a} −{d} uncommitted", first.repo),
        None => format!("{} · {label}", first.repo),
    };
    ui.interact(
        rect,
        ui.id().with(("agents_col_header", first.repo, label)),
        egui::Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &info));
}

/// Linear per-channel blend of `base` toward `tint` by `t` (0 = base, 1 = tint),
/// in sRGB space — enough for the subtle column washes. Blending against the
/// theme's own base keeps the tint balanced in both light and dark.
fn mix(base: egui::Color32, tint: egui::Color32, t: f32) -> egui::Color32 {
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    egui::Color32::from_rgb(
        lerp(base.r(), tint.r()),
        lerp(base.g(), tint.g()),
        lerp(base.b(), tint.b()),
    )
}

/// The faint column-body wash for project `lane` — the background the Agents
/// columns view tints each column with.
pub(crate) fn project_tint(palette: &Palette, lane: usize) -> egui::Color32 {
    mix(
        palette.bg_sidebar,
        palette.lane_color(lane),
        COLUMN_TINT_BODY,
    )
}

/// Firmer wash for project `lane`, on the same base as a status band: worn by the
/// workspace sidebar's header icon and by a column's header row — the same hue as the
/// project's Agents columns, a touch firmer than their body wash.
pub(crate) fn project_header_tint(palette: &Palette, lane: usize) -> egui::Color32 {
    mix(
        palette.bg_surface_hover,
        palette.lane_color(lane),
        HEADER_ICON_TINT,
    )
}

/// Compact green/red uncommitted ratio bar, vertically centered on `center_y`
/// with its right edge at `right` (mirrors the workspace sidebar's bar).
fn paint_stat_bar(
    ui: &egui::Ui,
    palette: &Palette,
    right: f32,
    center_y: f32,
    additions: usize,
    deletions: usize,
) {
    let rect = egui::Rect::from_min_size(
        egui::pos2(right - STAT_BAR_W, center_y - STAT_BAR_H / 2.0),
        egui::vec2(STAT_BAR_W, STAT_BAR_H),
    );
    let radius = egui::CornerRadius::same((STAT_BAR_H / 2.0) as u8);
    let (green_w, red_w) = crate::ui::git_panel::ratio_bar_widths(
        additions,
        deletions,
        STAT_BAR_W,
        STAT_BAR_GAP,
        STAT_BAR_MIN,
    );
    let painter = ui.painter();
    if green_w > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(green_w, STAT_BAR_H)),
            radius,
            palette.git_added,
        );
    }
    if red_w > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.right() - red_w, rect.top()), rect.max),
            radius,
            palette.git_deleted,
        );
    }
}

/// One agent inside its worktree column. Collapsed to a status band — a state
/// indicator + name + tab + state-caption + jump icon — over a **read-only preview**
/// (the pane's last lines, scaled to fit, drawn via `TermView::Preview`) when
/// `expanded` is false. Each column expands **one** card to a mirrored live terminal
/// (read / scroll / type), filling the column's leftover height; the **active** one
/// (the single keyboard target, `selected_agent`) carries the left spine and its band
/// wash, the other columns' expanded terminals stay dimmed until clicked. Band and pane are
/// flat and flush — the card is a segment of the column, not a card on it. Clicking
/// the header or the preview **selects** the card. The jump icon (same right-edge
/// affordance as the list row) focuses that pane in its workspace. Each expanded
/// card's terminal gets a unique `id_salt` so it owns its own focus.
#[allow(clippy::too_many_arguments)]
fn agent_terminal_card<F: FnMut(usize, &mut egui::Ui, TermView)>(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    idx: usize,
    expanded: bool,
    active: bool,
    fill_height: f32,
    action: &mut AgentsPageAction,
    render_terminal: &mut F,
) {
    let (hrect, response, hovered) = clickable(
        ui,
        egui::vec2(ui.available_width(), AGENT_HEADER_HEIGHT),
        true,
    );
    // The agent's status band: a flat full-bleed row that sets the status apart from the
    // pane flush below it, tinted with the project's hue like the column header above it
    // (a wash below it, so the header stays the column's firmest strip) and lifting under
    // the pointer to signal it expands on click. The active card is marked by its spine
    // alone — no wash on top of this one, so every band reads the same. Its wash is
    // painted at the end: a collapsed card's preview lifts the band with it, and that is
    // only known once the preview below is laid out.
    let band_bg = ui.painter().add(egui::Shape::Noop);
    // Content rides the **column header's** rail, not the body's: the body is
    // permanently inset by the spine gutter, so back it out here — otherwise every agent
    // name hangs a few px right of its project name and the column's left edge wobbles
    // down the stack.
    let inner = egui::Rect::from_min_max(
        egui::pos2(hrect.left() - AGENTS_ACTIVE_SPINE + CARD_PAD_X, hrect.top()),
        egui::pos2(hrect.right() - CARD_PAD_X, hrect.bottom()),
    );
    // Jump-to-workspace affordance at the right edge, mirroring the list row's
    // external-link icon: clicking it focuses this agent's pane in its workspace. The
    // glyph lands on the header meta's right rail (`CARD_JUMP_PAD`); its hit box takes
    // the band's full height and a comfortable width — the icon it paints is small, but
    // the target isn't.
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(
            hrect.right() - CARD_JUMP_PAD - JUMP_ICON_SIZE / 2.0,
            inner.center().y,
        ),
        egui::vec2(CARD_JUMP_HIT, hrect.height()),
    );
    let on_icon = ui.rect_contains_pointer(icon_rect);
    if on_icon {
        ui.painter().rect_filled(
            egui::Rect::from_center_size(icon_rect.center(), egui::Vec2::splat(CARD_JUMP_CHIP)),
            egui::CornerRadius::same(6),
            palette.bg_surface_hover,
        );
    }
    // Centered on the header's repo-icon center (not on its own box), so the state
    // indicator and the folder icon above it share one rail despite differing in size.
    let indicator = egui::Rect::from_center_size(
        egui::pos2(inner.left() + REPO_ICON_SIZE / 2.0, inner.center().y),
        egui::Vec2::splat(INDICATOR_SIZE),
    );
    paint_indicator(ui, palette, row.badge, indicator, |c| c);
    let painter = ui.painter();
    // Same icon → label step as the column header, so the agent name starts exactly
    // under the project name.
    let text_x = inner.left() + REPO_ICON_SIZE + 10.0;
    let detail_font = egui::FontId::proportional(DETAIL_SIZE);
    let detail_w = painter
        .layout_no_wrap(row.detail.clone(), detail_font.clone(), palette.text_muted)
        .size()
        .x;
    let content_right = icon_rect.left() - 8.0 - detail_w - ROW_TEXT_GAP;
    let name_w = paint_elided(
        painter,
        egui::pos2(text_x, inner.center().y),
        crate::agent_watch::display_name(row.agent),
        egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())),
        palette.text_primary,
        (content_right - text_x - ROW_TEXT_GAP - ROW_MIN_CHIP_W).max(ROW_MIN_NAME_W),
    );
    let tab_x = text_x + name_w + 9.0;
    paint_elided(
        painter,
        egui::pos2(tab_x, inner.center().y),
        row.tab,
        egui::FontId::proportional(TAB_SIZE),
        palette.text_muted,
        content_right - tab_x,
    );
    painter.text(
        egui::pos2(icon_rect.left() - 8.0, inner.center().y),
        egui::Align2::RIGHT_CENTER,
        &row.detail,
        detail_font,
        detail_color(palette, row.badge),
    );
    paint_icon(
        painter,
        icon_rect.center(),
        JUMP_ICON_SIZE,
        lucide_icons::Icon::ExternalLink,
        if on_icon {
            palette.text_secondary
        } else {
            palette.text_muted
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!(
                "{} in {} — {}",
                crate::agent_watch::display_name(row.agent),
                row.repo,
                row.tab
            ),
        )
    });
    let on_icon_click = response
        .interact_pointer_pos()
        .is_some_and(|p| icon_rect.contains(p));
    if response.clicked() {
        if on_icon_click {
            action.jump = Some(idx);
        } else {
            action.select = Some(idx);
        }
    }

    // How far this card is unfolded. egui snaps a value the first time it sees an id, so
    // opening the page paints no animation — only a later expand does — and the value
    // retargets mid-run when the pointer walks through cards.
    let unfold = ui.ctx().animate_bool_with_time_and_easing(
        ui.id().with(("agent_unfold", idx)),
        expanded,
        AGENT_UNFOLD_TIME,
        egui::emath::easing::cubic_out,
    );
    let mut card_hovered = hovered;
    if !expanded {
        // Collapsed: a read-only progress preview (the pane's last lines, scaled to
        // fit), flush under the band. Clicking it — like the header — selects the card
        // so it expands. It arrives on the clock the pane it displaces leaves on.
        let preview = ui
            .scope(|ui| {
                ui.multiply_opacity(1.0 - unfold);
                render_terminal(idx, ui, TermView::Preview);
            })
            .response
            .rect;
        let hit = ui
            .interact(
                preview,
                ui.id().with(("agent_preview", idx)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        // Band and preview are one target, so they light as one: hovering the preview
        // lifts the band above it instead of leaving the taller half of a clickable
        // card unlit.
        card_hovered |= hit.hovered();
        if hit.clicked() {
            action.select = Some(idx);
        }
    } else {
        // Fill the column's leftover height (`fill_height`, sized by the caller from
        // the column overhead and already floored at `TERM_MIN_HEIGHT`).
        let (strip, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), fill_height),
            egui::Sense::hover(),
        );
        // The pane is laid out at its final rect from the first frame — the grid, and so
        // the PTY, is sized from that rect, so animating it would resize the shell every
        // frame. It is *revealed* instead: its own base fills the strip and a clip runs
        // down it, unfolding the pane out from under its band.
        ui.painter().rect_filled(strip, 0, palette.bg_canvas);
        let mut term_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(strip)
                .id_salt(("agent_term", idx))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        if unfold < 1.0 {
            term_ui.multiply_opacity(unfold);
            let revealed = egui::Rect::from_min_max(
                strip.min,
                egui::pos2(strip.right(), strip.top() + unfold * strip.height()),
            );
            term_ui.set_clip_rect(revealed.intersect(ui.clip_rect()));
        }
        render_terminal(idx, &mut term_ui, TermView::Full);
        // The hovered terminal owns one wheel axis (vertical scrollback, or horizontal
        // under Shift — terminal.md §8); it reads the delta without consuming it, so
        // swallow just that axis to stop the wall's 2D scroll plane from scrolling in
        // tandem, while the cross axis still reaches it.
        if ui.rect_contains_pointer(strip) {
            ui.ctx().input_mut(|i| {
                if i.modifiers.shift {
                    i.smooth_scroll_delta.x = 0.0;
                } else {
                    i.smooth_scroll_delta.y = 0.0;
                }
            });
        }
    }
    // The lane reserves a gutter left of every card so the active spine never covers a
    // pane's first column of glyphs — but left bare it reads as a notch beside the column
    // header, which spans the lane edge to edge. So the card covers it: the band's wash
    // down the band…
    let gutter = egui::Rect::from_min_max(
        egui::pos2(hrect.left() - AGENTS_ACTIVE_SPINE, hrect.top()),
        egui::pos2(hrect.left(), ui.next_widget_position().y),
    );
    ui.painter().set(
        band_bg,
        egui::Shape::rect_filled(
            egui::Rect::from_min_max(gutter.min, hrect.max),
            0,
            mix(
                palette.bg_surface_hover,
                palette.lane_color(row.lane),
                if card_hovered {
                    AGENTS_BAND_HOVER_TINT
                } else {
                    AGENTS_BAND_TINT
                },
            ),
        ),
    );
    // …and the pane's own base down the rest of it. A theme pairs its chrome and its
    // terminal palette on one background (`bg_canvas` — asserted in `theme`), so the
    // gutter beside a pane is seamless whichever preset is on.
    ui.painter().rect_filled(
        egui::Rect::from_min_max(egui::pos2(gutter.left(), hrect.bottom()), gutter.max),
        0,
        palette.bg_canvas,
    );
    if active {
        // The focus mark: a spine down **this card's** left edge — its status band to its
        // pane's bottom — so the focus reads on the pane the keyboard drives, not on the
        // column it sits in, and no frame boxes in a terminal. It takes over the gutter,
        // which covers no content.
        ui.painter()
            .rect_filled(gutter, 0, project_header_tint(palette, row.lane));
    }
}

/// One column per **worktree**: consecutive rows sharing project *and*
/// `worktree_id`. A project's worktrees carry the same `repo` name and arrive
/// adjacent (workspace order), so their columns end up side by side under the same
/// project hue.
fn worktree_columns(rows: &[AgentRow]) -> Vec<(usize, usize)> {
    runs(rows, |a, b| {
        a.repo == b.repo && a.worktree_id == b.worktree_id
    })
}

/// The row a column expands to its live terminal: the globally-selected agent
/// when it lives in this column, otherwise the column's most urgent agent
/// (Working > Done > Idle, ties by workspace order — the global default rule).
/// So every column shows one live terminal — the selected one active (keyboard +
/// spine), the rest a live glance (agents.md §5). `start < end` always
/// (a column is a non-empty group), so the fallback never yields `start` spuriously.
fn column_expanded(rows: &[AgentRow], start: usize, end: usize, selected: Option<usize>) -> usize {
    if let Some(s) = selected.filter(|s| (start..end).contains(s)) {
        return s;
    }
    (start..end)
        .max_by_key(|&i| (rows[i].badge, std::cmp::Reverse(i)))
        .unwrap_or(start)
}

/// Consecutive `[start, end)` ranges sharing the same project — the list view's
/// grouping: a root and its worktrees carry the same `repo` name and arrive adjacent
/// (workspace order), so a linear scan splits them.
fn groups(rows: &[AgentRow]) -> Vec<(usize, usize)> {
    runs(rows, |a, b| a.repo == b.repo)
}

/// Splits the rows into maximal `[start, end)` runs whose members `same` as the run's
/// first row.
fn runs(rows: &[AgentRow], same: impl Fn(&AgentRow, &AgentRow) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..rows.len() {
        if !same(&rows[i], &rows[start]) {
            out.push((start, i));
            start = i;
        }
    }
    if !rows.is_empty() {
        out.push((start, rows.len()));
    }
    out
}

/// Project header of a list card: repo name and the agent count.
fn card_header(ui: &mut egui::Ui, palette: &Palette, first: &AgentRow, count: usize) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CARD_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    let inner = rect.shrink2(egui::vec2(CARD_PAD_X, 0.0));
    let painter = ui.painter();
    paint_icon(
        painter,
        egui::pos2(inner.left() + REPO_ICON_SIZE / 2.0, inner.center().y),
        REPO_ICON_SIZE,
        lucide_icons::Icon::FolderGit2,
        palette.text_secondary,
    );
    let name_font = egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx()));
    let name_x = inner.left() + REPO_ICON_SIZE + 10.0;
    painter.text(
        egui::pos2(name_x, inner.center().y),
        egui::Align2::LEFT_CENTER,
        first.repo,
        name_font,
        palette.text_primary,
    );
    let label = if count == 1 {
        "1 agent".to_owned()
    } else {
        format!("{count} agents")
    };
    ui.painter().text(
        egui::pos2(inner.right(), inner.center().y),
        egui::Align2::RIGHT_CENTER,
        label,
        egui::FontId::proportional(DETAIL_SIZE),
        palette.text_muted,
    );
}

/// Paints `text` left-anchored, vertically centered at `pos`, elided with `…`
/// past `max_width`. Returns the painted width so the caller can place what
/// follows (the branch chip after the name) without overlapping it.
fn paint_elided(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: impl ToString,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> f32 {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::text::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    let galley = painter.layout_job(job);
    let size = galley.size();
    painter.galley(egui::pos2(pos.x, pos.y - size.y / 2.0), galley, color);
    size.x
}

/// Pill carrying the project's branch (mono, design-system §2), `bg.surface`
/// fill + `border.subtle` stroke so it reads as a chip on the canvas card. The
/// branch is elided past `max_text_width` so a long name never pushes the chip
/// under the row's right-aligned caption.
fn branch_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    branch: &str,
    left: f32,
    center_y: f32,
    max_text_width: f32,
) {
    let font = egui::FontId::monospace(CHIP_SIZE);
    let mut job = egui::text::LayoutJob::single_section(
        branch.to_owned(),
        egui::text::TextFormat::simple(font, palette.text_secondary),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_text_width.max(0.0));
    let galley = ui.painter().layout_job(job);
    let size = galley.size();
    let pad_x = 7.0;
    let height = 18.0;
    let chip = egui::Rect::from_min_size(
        egui::pos2(left, center_y - height / 2.0),
        egui::vec2(size.x + 2.0 * pad_x, height),
    );
    ui.painter().rect(
        chip,
        egui::CornerRadius::same(5),
        palette.bg_surface,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(chip.left() + pad_x, center_y - size.y / 2.0),
        galley,
        palette.text_secondary,
    );
}

/// Outcome of a click on an agent row: its body selects, its jump icon focuses.
struct RowClick {
    body: bool,
    jump: bool,
}

/// One agent line: state indicator, agent name over its tab, a branch chip, the
/// state caption, and a discreet "open in workspace" icon. The body selects the
/// row (mirrored in the panel); the icon jumps to that agent's workspace.
fn agent_row(ui: &mut egui::Ui, palette: &Palette, row: &AgentRow, selected: bool) -> RowClick {
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(ui.available_width(), ROW_HEIGHT), true);
    let hl = rect.shrink2(egui::vec2(4.0, 4.0));
    if selected {
        ui.painter().rect_filled(
            hl,
            egui::CornerRadius::same(ROW_RADIUS),
            with_alpha(palette.accent, 28),
        );
    } else if hovered {
        ui.painter().rect_filled(
            hl,
            egui::CornerRadius::same(ROW_RADIUS),
            palette.bg_surface_hover,
        );
    }
    let inner = rect.shrink2(egui::vec2(CARD_PAD_X, 0.0));
    let indicator = egui::Rect::from_center_size(
        egui::pos2(inner.left() + INDICATOR_SIZE / 2.0, inner.center().y),
        egui::vec2(INDICATOR_SIZE, INDICATOR_SIZE),
    );
    paint_indicator(ui, palette, row.badge, indicator, |c| c);

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(inner.right() - JUMP_ICON_HIT / 2.0, inner.center().y),
        egui::vec2(JUMP_ICON_HIT, JUMP_ICON_HIT),
    );
    let on_icon = ui.rect_contains_pointer(icon_rect);
    if on_icon {
        ui.painter().rect_filled(
            icon_rect.shrink(5.0),
            egui::CornerRadius::same(6),
            palette.bg_surface_hover,
        );
    }

    let text_x = inner.left() + INDICATOR_SIZE + 14.0;
    let detail_font = egui::FontId::proportional(DETAIL_SIZE);
    let detail_w = ui
        .painter()
        .layout_no_wrap(row.detail.clone(), detail_font.clone(), palette.text_muted)
        .size()
        .x;
    let content_right = icon_rect.left() - 8.0 - detail_w - ROW_TEXT_GAP;
    let has_branch = row.branch.filter(|b| !b.is_empty());
    let name_max = if has_branch.is_some() {
        (content_right - text_x - 8.0 - ROW_MIN_CHIP_W).max(ROW_MIN_NAME_W)
    } else {
        (content_right - text_x).max(0.0)
    };
    let painter = ui.painter();
    let name_w = paint_elided(
        painter,
        egui::pos2(text_x, inner.center().y - 9.0),
        crate::agent_watch::display_name(row.agent),
        egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())),
        palette.text_primary,
        name_max,
    );
    let name_end = text_x + name_w;
    paint_elided(
        painter,
        egui::pos2(text_x, inner.center().y + 10.0),
        row.tab,
        egui::FontId::proportional(TAB_SIZE),
        palette.text_muted,
        (content_right - text_x).max(0.0),
    );
    paint_icon(
        painter,
        icon_rect.center(),
        JUMP_ICON_SIZE,
        lucide_icons::Icon::ExternalLink,
        if on_icon {
            palette.text_secondary
        } else {
            palette.text_muted
        },
    );
    painter.text(
        egui::pos2(icon_rect.left() - 8.0, inner.center().y),
        egui::Align2::RIGHT_CENTER,
        &row.detail,
        detail_font,
        detail_color(palette, row.badge),
    );
    if let Some(branch) = has_branch {
        let chip_text_max = (content_right - (name_end + 8.0) - 14.0).max(0.0);
        branch_chip(
            ui,
            palette,
            branch,
            name_end + 8.0,
            inner.center().y - 9.0,
            chip_text_max,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!(
                "{} in {} — {}",
                crate::agent_watch::display_name(row.agent),
                row.repo,
                row.tab
            ),
        )
    });
    let on_icon_click = response
        .interact_pointer_pos()
        .is_some_and(|p| icon_rect.contains(p));
    RowClick {
        body: response.clicked() && !on_icon_click,
        jump: response.clicked() && on_icon_click,
    }
}

/// State dot: a spinner while working, a green dot once finished, a hollow grey
/// dot when idle (same visual language as the sidebar badge). `ink` dims the
/// colors when this agent's card is unfocused (identity otherwise).
fn paint_indicator(
    ui: &egui::Ui,
    palette: &Palette,
    badge: AgentBadge,
    rect: egui::Rect,
    ink: impl Fn(egui::Color32) -> egui::Color32,
) {
    match badge {
        AgentBadge::Working => {
            Spinner::new()
                .size(INDICATOR_SIZE)
                .color(ink(palette.accent))
                .paint_at(ui, rect);
        }
        AgentBadge::Done => {
            paint_done_dot(ui, rect.center(), 5.0, ink(palette.git_added));
        }
        _ => {
            ui.painter().circle_stroke(
                rect.center(),
                4.0,
                egui::Stroke::new(1.5_f32, ink(palette.text_muted)),
            );
        }
    }
}

fn detail_color(palette: &Palette, badge: AgentBadge) -> egui::Color32 {
    match badge {
        AgentBadge::Working => palette.accent,
        AgentBadge::Done => palette.git_added,
        _ => palette.text_muted,
    }
}

/// Shown when no repository has a detected agent: a calm, left-aligned hint.
fn empty_state(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(40.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
    paint_icon(
        ui.painter(),
        rect.center(),
        40.0,
        lucide_icons::Icon::Bot,
        palette.text_muted,
    );
    ui.add_space(16.0);
    ui.label(
        egui::RichText::new("No agents running")
            .size(16.0)
            .family(theme::medium_family(ui.ctx()))
            .color(palette.text_primary),
    );
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "Launch Claude, Codex, or another agent in a terminal and it shows up here.",
        )
        .size(SUBTITLE_SIZE)
        .color(palette.text_muted),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(repo: &'static str, agent: &'static str, badge: AgentBadge) -> AgentRow<'static> {
        AgentRow {
            repo,
            branch: None,
            tab: "Tab 1",
            agent,
            badge,
            detail: String::new(),
            worktree_id: 0,
            lane: 0,
            stats: None,
        }
    }

    #[test]
    fn groups_split_on_repo_change() {
        let rows = [
            row("a", "claude", AgentBadge::Working),
            row("a", "codex", AgentBadge::Idle),
            row("b", "aider", AgentBadge::Done),
        ];
        assert_eq!(groups(&rows), vec![(0, 2), (2, 3)]);
        assert_eq!(groups(&[]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn columns_split_per_worktree() {
        let mut rows = [
            row("helm", "claude", AgentBadge::Working),
            row("helm", "codex", AgentBadge::Done),
            row("helm", "aider", AgentBadge::Idle),
        ];
        rows[2].worktree_id = 1;
        // Two agents in the same worktree share a column; the other worktree gets its own.
        assert_eq!(worktree_columns(&rows), vec![(0, 2), (2, 3)]);
        assert_eq!(worktree_columns(&[]), Vec::<(usize, usize)>::new());
        // Two projects whose entries happen to carry the same id still split.
        let other = [
            row("helm", "claude", AgentBadge::Idle),
            row("api", "codex", AgentBadge::Idle),
        ];
        assert_eq!(worktree_columns(&other), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn column_expanded_prefers_selection_then_urgency() {
        let rows = [
            row("a", "claude", AgentBadge::Idle),   // 0
            row("a", "codex", AgentBadge::Working), // 1
            row("a", "aider", AgentBadge::Working), // 2
            row("b", "amp", AgentBadge::Done),      // 3
        ];
        // A selection inside the column wins outright, even over a more urgent sibling.
        assert_eq!(column_expanded(&rows, 0, 3, Some(0)), 0);
        // No selection here ⇒ most urgent, ties broken to the earliest row (idx 1 over
        // the equally-Working idx 2), regardless of a selection in another column.
        assert_eq!(column_expanded(&rows, 0, 3, None), 1);
        assert_eq!(column_expanded(&rows, 0, 3, Some(3)), 1);
        // A lone-agent column always expands that agent.
        assert_eq!(column_expanded(&rows, 3, 4, None), 3);
    }
}
