//! Cross-repo agents dashboard (specs/agents.md): the central content rendered
//! while the sidebar's Agents entry is selected — every running (or just-finished)
//! agent across all open repositories, in one of two views. **List** is a master-detail
//! cockpit (a list grouped by project + a panel mirroring the selected agent's
//! terminal). **Terminals** is a header strip of chips over a wall of the mirrored
//! terminals picked from it, laid out by the terminal's own split tree. A row / band
//! click selects; the discreet jump icon focuses that agent's workspace. Rendering
//! only — the page returns the targeted action, the app applies it.

use serde::{Deserialize, Serialize};

use crate::agent_watch::AgentBadge;
use crate::agents_wall::MAX_SHOWN;
use crate::terminal::layout::{Layout, PaneId};
use crate::theme::{self, Palette};
use crate::ui::preferences::{setting_divider, settings_card};
use crate::ui::spinner::{paint_done_dot, Spinner};
use crate::ui::terminal_view::{terminal_tree, PaneDrop, ResizeDrag, GRIP_RESERVE};
use crate::ui::{clickable, paint_icon, with_alpha};

/// Which dashboard layout the central area shows. **List** is the master-detail
/// cockpit (a list + one mirrored terminal); **Terminals** is the wall — a header strip
/// of every running agent over the mirrored terminals of the ones picked from it, up to
/// [`MAX_SHOWN`], on a split tree that resizes and rearranges like a workspace tab's
/// (specs/agents.md §5). Persisted in `Prefs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentsViewMode {
    #[default]
    List,
    /// Persisted as `columns`: the token predates the wall (the view used to be a
    /// column per worktree) and an unknown value would make a whole existing prefs
    /// file fail to parse, falling back to defaults.
    #[serde(rename = "columns")]
    Terminals,
}

const CONTENT_PAD_X: f32 = 32.0;
const CONTENT_PAD_Y: f32 = 32.0;
const CONTENT_MAX_WIDTH: f32 = 720.0;
const SUBTITLE_SIZE: f32 = 13.0;

const CARD_PAD_X: f32 = 16.0;
const CARD_HEADER_HEIGHT: f32 = 48.0;
const ROW_HEIGHT: f32 = 60.0;
const GROUP_GAP: f32 = 16.0;
const ROW_RADIUS: u8 = 8;
/// Narrow elision, shared by a list row (name + branch chip) and a wall tile's status
/// band (name + branch chip + tab label): both stop short of the right-aligned state
/// caption — reserving a minimum for the second label — and gain a `…` instead of
/// running underneath it. A quarter-width tile is narrow, so the tab label is the first
/// to go.
const ROW_TEXT_GAP: f32 = 10.0;
const ROW_MIN_NAME_W: f32 = 40.0;
const ROW_MIN_CHIP_W: f32 = 48.0;

/// Terminals view header: one chip per running agent, wrapping onto further lines and
/// scrolling past `CHIP_STRIP_MAX_ROWS` of them, so even a workspace full of agents
/// leaves the wall its room.
const CHIP_HEIGHT: f32 = 30.0;
const CHIP_GAP: f32 = 8.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_RADIUS: u8 = 8;
const CHIP_ICON_GAP: f32 = 7.0;
const CHIP_NAME_SIZE: f32 = 12.5;
const CHIP_MAX_NAME_W: f32 = 150.0;
const CHIP_MAX_ORIGIN_W: f32 = 190.0;
const CHIP_STRIP_PAD_X: f32 = 12.0;
const CHIP_STRIP_PAD_Y: f32 = 10.0;
const CHIP_STRIP_MAX_ROWS: f32 = 3.0;

/// A wall tile's status band, over the mirrored pane flush under it. Also the height of
/// the band's jump target (`CARD_JUMP_HIT` wide).
const TILE_BAND_HEIGHT: f32 = 32.0;
/// Every tile's status band wears its project's hue, so a pane reads as belonging to its
/// project at a glance; the band of the tile the keyboard drives wears it firmest, and a
/// band lifts under the pointer. The panes themselves need no other mark: a mirrored
/// terminal dims itself when it isn't the active one, exactly as an unfocused split does.
const AGENTS_BAND_TINT: f32 = 0.08;
const AGENTS_BAND_HOVER_TINT: f32 = 0.16;
const AGENTS_BAND_ACTIVE_TINT: f32 = 0.26;

/// Project hue (cycled from `palette.lane_colors`, the theme-tuned graph palette) worn
/// by a header chip once its agent is on the wall and by the sidebar's project header
/// icon. Mixed against the theme's own base rather than applied flat, so it stays
/// balanced in light and dark.
const HEADER_ICON_TINT: f32 = 0.16;

const REPO_ICON_SIZE: f32 = 15.0;
const NAME_SIZE: f32 = 14.0;
const TAB_SIZE: f32 = 12.0;
const CHIP_SIZE: f32 = 11.5;
const DETAIL_SIZE: f32 = 12.0;
const INDICATOR_SIZE: f32 = 16.0;
const JUMP_ICON_SIZE: f32 = 15.0;
const JUMP_ICON_HIT: f32 = 28.0;
/// On a wall tile the jump icon's target spans the status band's full height and a
/// comfortable width (dense-desktop 40px minimum); the chip it paints on hover stays
/// small, so the affordance is easy to hit without looking heavy.
const CARD_JUMP_HIT: f32 = 40.0;
const CARD_JUMP_CHIP: f32 = 26.0;
/// Trailing pad of the jump glyph on a wall tile: `CARD_PAD_X` less 2px — the optical
/// correction that lands an icon on the same right rail as text at equal padding, which
/// would otherwise look short of it.
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
    /// the workspace): equal ids = same worktree.
    pub worktree_id: usize,
    /// Project color index (rank of the group root among root projects): tints this
    /// agent's header chip and wall band and, via the same index, the project's sidebar
    /// header icon.
    pub lane: usize,
}

/// What a gesture on the dashboard targeted. The app applies it: a select mirrors that
/// agent (List panel) or makes its tile active (Terminals), a jump focuses its pane in
/// its workspace, a toggle puts its terminal on the wall or takes it off, and a
/// resize / drop relayouts the wall.
#[derive(Default)]
pub struct AgentsPageAction {
    pub select: Option<usize>,
    pub jump: Option<usize>,
    /// The view-mode toggle was clicked: the app persists the new mode.
    pub set_view: Option<AgentsViewMode>,
    /// A header chip was clicked: show that agent's terminal on the wall, or hide it
    /// when it is already there.
    pub toggle: Option<usize>,
    /// The wall's own rect this frame — the app splits it to place a newly shown
    /// terminal, and reads a seam drag against it. `None` outside the Terminals view.
    pub wall_rect: Option<egui::Rect>,
    /// A seam between two tiles was dragged (the split tree's own resize).
    pub resize: Option<ResizeDrag>,
    /// A tile was dragged onto another by its grip: re-split on that side, or swap.
    pub drop: Option<PaneDrop>,
}

/// Cross-repo dashboard for the central area. A centered List/Terminals switch —
/// sharing the Terminal/Git switch's design and titlebar placement — tops both views;
/// below it, **List** is the master-detail cockpit (a left list + a right panel
/// mirroring the selected agent's terminal) and **Terminals** is the wall: a header
/// strip of every running agent over the mirrored terminals picked from it.
/// `render_terminal(idx, ui)` draws the live pane of the agent at row `idx` — called for
/// the selected row (List) or once per wall tile (Terminals) — and reports whether it
/// was clicked. Returns the targeted action.
#[allow(clippy::too_many_arguments)]
pub fn agents_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    view: AgentsViewMode,
    wall: &WallView,
    render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
) -> AgentsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);

    let set_view = crate::ui::agents_view_switch(ui, palette, view == AgentsViewMode::Terminals)
        .map(|terminals| {
            if terminals {
                AgentsViewMode::Terminals
            } else {
                AgentsViewMode::List
            }
        });
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
        AgentsViewMode::Terminals => render_terminals(
            ui,
            palette,
            rows,
            selected,
            body,
            wall,
            &mut action,
            render_terminal,
        ),
    }
    action
}

/// The wall's live composition, handed in by the app (which owns the `AgentWall`): the
/// split tree to lay the tiles out with — `None` while nothing is shown — the row each
/// slot mirrors, and whether every slot is taken.
pub struct WallView<'a> {
    pub layout: Option<&'a Layout>,
    pub slots: &'a [(PaneId, usize)],
    pub full: bool,
}

impl WallView<'_> {
    fn row_of(&self, slot: PaneId) -> Option<usize> {
        self.slots
            .iter()
            .find(|(id, _)| *id == slot)
            .map(|(_, row)| *row)
    }

    fn shows(&self, row: usize) -> bool {
        self.slots.iter().any(|(_, r)| *r == row)
    }
}

/// List view body: the scrollable project list and, when an agent is selected and
/// there's room, a right panel mirroring its terminal live.
fn render_list_view(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
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
    mut render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
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
        render_terminal(idx, &mut panel_ui);
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

/// Terminals view: a **header strip** listing every running agent — one chip apiece,
/// carrying its live state indicator — over a **wall** of the ones picked from it, at
/// most [`MAX_SHOWN`]. A chip click shows or hides that agent's terminal; the shown
/// ones are laid out by the **terminal's own split tree**, so the wall's seams resize
/// and its panes rearrange exactly like a workspace tab's (terminal.md §5).
/// `render_terminal(idx, ui)` mirrors the pane of the agent at row `idx` and reports
/// whether it was clicked.
#[allow(clippy::too_many_arguments)]
fn render_terminals(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    wall: &WallView,
    action: &mut AgentsPageAction,
    render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
) {
    if rows.is_empty() {
        let inner = egui::Rect::from_min_max(
            egui::pos2(rect.left() + CONTENT_PAD_X, rect.top()),
            rect.max,
        );
        let mut empty = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        empty_state(&mut empty, palette);
        return;
    }
    let header_bottom = agent_chips(ui, palette, rows, rect, wall, action);
    ui.painter().hline(
        rect.x_range(),
        header_bottom,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), header_bottom + 1.0), rect.max);
    // The app needs the wall's own rect to place a newly shown terminal (it splits the
    // roomiest tile) and to turn a seam drag into a ratio.
    action.wall_rect = Some(body);
    render_wall(
        ui,
        palette,
        rows,
        selected,
        body,
        wall,
        action,
        render_terminal,
    );
}

/// The header strip, in workspace order (so a project's worktrees stay adjacent):
/// a chip per running agent, wrapping onto further lines and scrolling past
/// `CHIP_STRIP_MAX_ROWS` so a busy workspace never eats the wall. Returns the strip's
/// bottom edge.
fn agent_chips(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    rect: egui::Rect,
    wall: &WallView,
    action: &mut AgentsPageAction,
) -> f32 {
    let max_height =
        CHIP_STRIP_MAX_ROWS * (CHIP_HEIGHT + CHIP_GAP) - CHIP_GAP + 2.0 * CHIP_STRIP_PAD_Y;
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + CHIP_STRIP_PAD_X, rect.top()),
        egui::pos2(rect.right() - CHIP_STRIP_PAD_X, rect.bottom()),
    );
    let mut strip = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let used = egui::ScrollArea::vertical()
        .id_salt("agents_chips")
        .max_height(max_height)
        .show(&mut strip, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(CHIP_STRIP_PAD_Y);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, CHIP_GAP);
                for (idx, row) in rows.iter().enumerate() {
                    let shown = wall.shows(idx);
                    if agent_chip(ui, palette, row, shown, !shown && wall.full) {
                        action.toggle = Some(idx);
                    }
                }
            });
            ui.add_space(CHIP_STRIP_PAD_Y);
        })
        .content_size
        .y;
    rect.top() + used.min(max_height)
}

/// Where an agent runs: `project · branch`, or the project alone on a detached worktree.
/// The dashboard is cross-repo, so the project is what tells two agents on `main` apart —
/// it rides with the branch on every chip and every wall band.
fn origin(row: &AgentRow) -> String {
    match row.branch.filter(|b| !b.is_empty()) {
        Some(branch) => format!("{} · {branch}", row.repo),
        None => row.repo.to_owned(),
    }
}

/// One header chip: state indicator, agent name, and where it runs. Filled in the
/// project's hue while the agent is **on the wall** (clicking takes it off), quiet
/// otherwise. With every slot taken the remaining chips read **disabled** and say so on
/// hover — hiding one is the way to make room (agents.md §5). Returns true when clicked.
fn agent_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    shown: bool,
    blocked: bool,
) -> bool {
    let name = crate::agent_watch::display_name(row.agent);
    let name_font = egui::FontId::new(CHIP_NAME_SIZE, theme::medium_family(ui.ctx()));
    let origin_font = egui::FontId::monospace(CHIP_SIZE);
    let origin = origin(row);
    // A blocked chip is legible but visibly out of reach: the whole chip fades, ink and
    // indicator alike.
    let fade = |c: egui::Color32| if blocked { with_alpha(c, 110) } else { c };
    let measure = |text: &str, font: egui::FontId, max: f32| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, palette.text_primary)
            .size()
            .x
            .min(max)
    };
    let name_w = measure(&name, name_font.clone(), CHIP_MAX_NAME_W);
    let origin_w = measure(&origin, origin_font.clone(), CHIP_MAX_ORIGIN_W);
    let width =
        2.0 * CHIP_PAD_X + INDICATOR_SIZE + CHIP_ICON_GAP + name_w + CHIP_ICON_GAP + origin_w;
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, CHIP_HEIGHT), !blocked);
    let fill = if shown {
        project_header_tint(palette, row.lane)
    } else if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(CHIP_RADIUS),
        fill,
        egui::Stroke::new(
            1.0_f32,
            if shown {
                with_alpha(palette.accent, 110)
            } else {
                palette.border_subtle
            },
        ),
        egui::StrokeKind::Inside,
    );
    let indicator = egui::Rect::from_center_size(
        egui::pos2(
            rect.left() + CHIP_PAD_X + INDICATOR_SIZE / 2.0,
            rect.center().y,
        ),
        egui::Vec2::splat(INDICATOR_SIZE),
    );
    paint_indicator(ui, palette, row.badge, indicator, fade);
    let painter = ui.painter();
    let text_x = indicator.right() + CHIP_ICON_GAP;
    let painted = paint_elided(
        painter,
        egui::pos2(text_x, rect.center().y),
        &name,
        name_font,
        fade(if shown {
            palette.text_primary
        } else {
            palette.text_secondary
        }),
        name_w,
    );
    paint_elided(
        painter,
        egui::pos2(text_x + painted + CHIP_ICON_GAP, rect.center().y),
        &origin,
        origin_font,
        fade(palette.text_muted),
        origin_w,
    );
    let label = format!("{name} · {origin} · {}", row.tab);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, shown, &label));
    if blocked {
        response.on_hover_text(format!(
            "{MAX_SHOWN} terminals at most — hide one to make room"
        ));
        return false;
    }
    response.clicked()
}

/// The wall itself: one mirrored terminal per shown agent, laid out by
/// [`terminal_tree`] — the workspace's own split renderer, so the seams between tiles
/// drag to resize and each tile's grip drags onto another to re-split or swap
/// (terminal.md §5). The reorg it reports rides back on the action; the app applies it
/// to the wall's tree.
#[allow(clippy::too_many_arguments)]
fn render_wall(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    wall: &WallView,
    action: &mut AgentsPageAction,
    mut render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
) {
    let mut body = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .id_salt("agents_wall")
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let Some(layout) = wall.layout else {
        empty_wall(&mut body, palette);
        return;
    };
    let mut clicked = None;
    let out = terminal_tree(&mut body, layout, palette, |tile, slot, _focused| {
        let Some(idx) = wall.row_of(slot) else {
            return false;
        };
        let Some(row) = rows.get(idx) else {
            return false;
        };
        let hit = wall_tile(
            tile,
            palette,
            row,
            idx,
            selected == Some(idx),
            action,
            &mut render_terminal,
        );
        if hit {
            clicked = Some(idx);
        }
        hit
    });
    action.select = action.select.or(clicked);
    action.resize = out.resize;
    action.drop = out.drop;
}

/// One tile: a compact status band — state indicator, agent name, branch chip, tab,
/// state caption, jump icon — over the agent's **live terminal**, flush under it. The
/// band wears the project's hue, firmest on the tile the keyboard drives (the mirrored
/// pane dims itself when it isn't the active one, as an unfocused split does), and lifts
/// under the pointer. Clicking the band or the terminal makes the tile active; the jump
/// icon focuses that pane in its workspace. Returns whether the tile was clicked.
fn wall_tile<F: FnMut(usize, &mut egui::Ui) -> bool>(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    idx: usize,
    active: bool,
    action: &mut AgentsPageAction,
    render_terminal: &mut F,
) -> bool {
    let rect = ui.available_rect_before_wrap();
    let band = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(
            rect.right(),
            (rect.top() + TILE_BAND_HEIGHT).min(rect.bottom()),
        ),
    );
    let response = ui
        .interact(band, ui.id().with("agent_band"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let tint = if active {
        AGENTS_BAND_ACTIVE_TINT
    } else if response.hovered() {
        AGENTS_BAND_HOVER_TINT
    } else {
        AGENTS_BAND_TINT
    };
    ui.painter().rect_filled(
        band,
        0,
        mix(palette.bg_surface_hover, palette.lane_color(row.lane), tint),
    );
    // Jump-to-workspace affordance, the list row's external-link icon: its hit box takes
    // the band's full height, the chip it paints on hover stays small. It stops short of
    // the tile's top-right corner, which the tree's own drag grip owns (`GRIP_RESERVE`) —
    // the two affordances must not sit on top of each other.
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(
            band.right() - GRIP_RESERVE - CARD_JUMP_PAD - JUMP_ICON_SIZE / 2.0,
            band.center().y,
        ),
        egui::vec2(CARD_JUMP_HIT, band.height()),
    );
    let on_icon = ui.rect_contains_pointer(icon_rect);
    if on_icon {
        ui.painter().rect_filled(
            egui::Rect::from_center_size(icon_rect.center(), egui::Vec2::splat(CARD_JUMP_CHIP)),
            egui::CornerRadius::same(6),
            palette.bg_surface_hover,
        );
    }
    let indicator = egui::Rect::from_center_size(
        egui::pos2(
            band.left() + CARD_PAD_X + INDICATOR_SIZE / 2.0,
            band.center().y,
        ),
        egui::Vec2::splat(INDICATOR_SIZE),
    );
    paint_indicator(ui, palette, row.badge, indicator, |c| c);
    let painter = ui.painter();
    let detail_font = egui::FontId::proportional(DETAIL_SIZE);
    let detail_w = painter
        .layout_no_wrap(row.detail.clone(), detail_font.clone(), palette.text_muted)
        .size()
        .x;
    let content_right = icon_rect.left() - 8.0 - detail_w - ROW_TEXT_GAP;
    let text_x = indicator.right() + 10.0;
    let name = crate::agent_watch::display_name(row.agent);
    let origin = origin(row);
    // Agent, then where it runs (`project · branch` — the wall is cross-repo, so two
    // agents on `main` need their project), then the tab. A narrow tile elides from the
    // right, so the least identifying label is the first to go.
    let name_end = text_x
        + paint_elided(
            painter,
            egui::pos2(text_x, band.center().y),
            &name,
            egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())),
            palette.text_primary,
            (content_right - text_x - 8.0 - ROW_MIN_CHIP_W).max(ROW_MIN_NAME_W),
        );
    painter.text(
        egui::pos2(icon_rect.left() - 8.0, band.center().y),
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
    let origin_x = name_end + 9.0;
    let tab_x = origin_x
        + paint_elided(
            painter,
            egui::pos2(origin_x, band.center().y),
            &origin,
            egui::FontId::monospace(CHIP_SIZE),
            palette.text_secondary,
            (content_right - origin_x).max(0.0),
        )
        + 9.0;
    paint_elided(
        painter,
        egui::pos2(tab_x, band.center().y),
        row.tab,
        egui::FontId::proportional(TAB_SIZE),
        palette.text_muted,
        (content_right - tab_x).max(0.0),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{name} in {origin} — {}", row.tab),
        )
    });
    let on_icon_click = response
        .interact_pointer_pos()
        .is_some_and(|p| icon_rect.contains(p));
    if response.clicked() && on_icon_click {
        action.jump = Some(idx);
        return false;
    }
    // The pane fills whatever the band leaves; it is the tile's own child ui, so its
    // egui state (focus, scroll) hangs off the slot the tree salted, not off a row index
    // that shifts as agents come and go.
    let term_rect = egui::Rect::from_min_max(egui::pos2(rect.left(), band.bottom()), rect.max);
    let mut term_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(term_rect)
            .id_salt("agent_term")
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let term_clicked = render_terminal(idx, &mut term_ui);
    response.clicked() || term_clicked
}

/// Agents are running but the wall is empty (nothing picked yet, or everything hidden):
/// the header strip is the way in, so say so rather than showing a bare canvas.
fn empty_wall(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
        paint_icon(
            ui.painter(),
            rect.center(),
            36.0,
            lucide_icons::Icon::LayoutGrid,
            palette.text_muted,
        );
        ui.add_space(14.0);
        ui.label(
            egui::RichText::new("No terminal on the wall")
                .size(16.0)
                .family(theme::medium_family(ui.ctx()))
                .color(palette.text_primary),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!(
                "Pick an agent above to watch it live — up to {MAX_SHOWN} at once."
            ))
            .size(SUBTITLE_SIZE)
            .color(palette.text_muted),
        );
    });
}

/// Linear per-channel blend of `base` toward `tint` by `t` (0 = base, 1 = tint), in
/// sRGB space — enough for the subtle project washes. Blending against the theme's own
/// base keeps the tint balanced in both light and dark.
fn mix(base: egui::Color32, tint: egui::Color32, t: f32) -> egui::Color32 {
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    egui::Color32::from_rgb(
        lerp(base.r(), tint.r()),
        lerp(base.g(), tint.g()),
        lerp(base.b(), tint.b()),
    )
}

/// Wash for project `lane` on a status-band base: worn by a header chip whose agent is
/// on the wall and by the workspace sidebar's project header icon — one hue per project,
/// the same one its wall bands carry.
pub(crate) fn project_header_tint(palette: &Palette, lane: usize) -> egui::Color32 {
    mix(
        palette.bg_surface_hover,
        palette.lane_color(lane),
        HEADER_ICON_TINT,
    )
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
/// under the row's right-aligned caption. Returns the pill's width, so a caller
/// laying out a third label (a wall tile's tab) knows where the chip ends.
fn branch_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    branch: &str,
    left: f32,
    center_y: f32,
    max_text_width: f32,
) -> f32 {
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
    chip.width()
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
    fn the_wall_maps_its_slots_to_rows() {
        let slots = [(PaneId(3), 2), (PaneId(1), 0)];
        let wall = WallView {
            layout: None,
            slots: &slots,
            full: false,
        };
        assert_eq!(wall.row_of(PaneId(3)), Some(2));
        assert_eq!(wall.row_of(PaneId(9)), None);
        assert!(wall.shows(0));
        assert!(!wall.shows(1));
    }
}
