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
/// column per project, split into worktree sub-cards, each holding a live terminal
/// per agent (specs/agents.md §5). Persisted in `Prefs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentsViewMode {
    #[default]
    List,
    Columns,
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
/// Narrow-list elision: the agent name and branch chip stop short of the
/// right-aligned state caption (and reserve a minimum chip), gaining a `…`
/// instead of running underneath it.
const ROW_TEXT_GAP: f32 = 10.0;
const ROW_MIN_NAME_W: f32 = 40.0;
const ROW_MIN_CHIP_W: f32 = 48.0;

/// Columns view: one column per project, scrolled horizontally; each agent card
/// carries a fixed-height live terminal stacked vertically. The column width is
/// **shared** across columns and **resizable** (drag the handle in any gap),
/// persisted in `Prefs` and clamped to [`COLUMN_MIN_WIDTH`, `COLUMN_MAX_WIDTH`].
/// Every project is a recessed `bg.sidebar` panel so the columns read apart on
/// the canvas; the worktree cards (`bg.canvas`) then sit a shade lighter on top.
const COLUMN_MIN_WIDTH: f32 = 420.0;
const COLUMN_MAX_WIDTH: f32 = 1200.0;
const COLUMN_GAP: f32 = 8.0;
const COLUMN_RADIUS: u8 = 12;
const COLUMN_PAD: f32 = 6.0;
/// Leading inset before the first column — tight so the wall of terminals sits
/// close to the edge (the central panel already adds its own small margin).
const COLUMN_EDGE_PAD: f32 = 8.0;
/// Breathing room between the page-header hairline and the top of the columns,
/// so they don't sit flush against it.
const COLUMN_TOP_MARGIN: f32 = 8.0;
/// Compact project title atop a column (the list view keeps the taller
/// `CARD_HEADER_HEIGHT`).
const COLUMN_HEADER_HEIGHT: f32 = 34.0;
const CARD_RADIUS: u8 = 8;
/// A terminal card's height is **shared** across agents and **resizable** (drag a
/// card's bottom edge), persisted in `Prefs` and clamped to these bounds.
const TERM_MIN_HEIGHT: f32 = 160.0;
const TERM_MAX_HEIGHT: f32 = 900.0;
/// Header ink alpha of an unfocused agent card: the focused one stays opaque, the
/// others recede (their terminal also self-dims, like an unfocused split pane).
const UNFOCUSED_ALPHA: u8 = 105;
const AGENT_HEADER_HEIGHT: f32 = 32.0;
const WORKTREE_HEADER_HEIGHT: f32 = 36.0;

/// Compact uncommitted-changes ratio bar on a dirty worktree header — same
/// green/red proportion device as the workspace sidebar (`repo_sidebar`).
const STAT_BAR_W: f32 = 26.0;
const STAT_BAR_H: f32 = 3.0;
const STAT_BAR_MIN: f32 = 3.0;
const STAT_BAR_GAP: f32 = 1.5;

/// Each column is tinted with its own hue (cycled from `palette.lane_colors`, the
/// theme-tuned graph palette) so projects read apart at a glance. The hue is mixed
/// against the theme's own base, not applied flat, so it stays balanced in light
/// and dark: a faint wash on the column body, a firmer one on the worktree band.
const COLUMN_TINT_BODY: f32 = 0.10;
const COLUMN_TINT_BAND: f32 = 0.22;

const REPO_ICON_SIZE: f32 = 15.0;
const NAME_SIZE: f32 = 14.0;
const TAB_SIZE: f32 = 12.0;
const CHIP_SIZE: f32 = 11.5;
const DETAIL_SIZE: f32 = 12.0;
const INDICATOR_SIZE: f32 = 16.0;
const JUMP_ICON_SIZE: f32 = 15.0;
const JUMP_ICON_HIT: f32 = 28.0;

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
    /// A terminal card's height handle was dragged: the app stores the new shared
    /// terminal height and persists it (debounced).
    pub set_terminal_height: Option<f32>,
}

/// Cross-repo dashboard for the central area. A centered List/Columns switch —
/// sharing the Terminal/Git switch's design and titlebar placement — tops both
/// views; below it, **List** is the master-detail cockpit (a left list + a right
/// panel mirroring the selected agent's terminal) and **Columns** is a grid of
/// every agent's live terminal, one fixed-width column per project.
/// `render_terminal(idx, ui)` draws the live pane for the agent at row `idx` —
/// called once (the selected row) in List, once per agent in Columns. Returns the
/// targeted action (select / jump / view).
#[allow(clippy::too_many_arguments)]
pub fn agents_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    view: AgentsViewMode,
    column_width: f32,
    terminal_height: f32,
    render_terminal: impl FnMut(usize, &mut egui::Ui),
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
            terminal_height,
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
    render_terminal: impl FnMut(usize, &mut egui::Ui),
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
                        card_header(ui, palette, &rows[start], end - start, CARD_HEADER_HEIGHT);
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
    mut render_terminal: impl FnMut(usize, &mut egui::Ui),
) {
    ui.painter().vline(
        rect.left(),
        rect.y_range(),
        egui::Stroke::new(1.0, palette.border_subtle),
    );
    let header_bottom = rect.top() + PANEL_HEADER_HEIGHT;
    if let Some(row) = selected.and_then(|i| rows.get(i)) {
        panel_header(ui, palette, row, rect, header_bottom);
    }
    ui.painter().hline(
        rect.x_range(),
        header_bottom,
        egui::Stroke::new(1.0, palette.border_subtle),
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

/// Columns view: a horizontally-scrolled strip of fixed-width project columns.
/// Each column carries the project title, then a vertical scroll of worktree
/// sub-cards, each with a live-terminal card per agent. `render_terminal(idx, ui)`
/// draws the pane for the agent at row `idx`.
#[allow(clippy::too_many_arguments)]
fn render_columns(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    rect: egui::Rect,
    column_width: f32,
    terminal_height: f32,
    action: &mut AgentsPageAction,
    mut render_terminal: impl FnMut(usize, &mut egui::Ui),
) {
    let column_width = column_width.clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH);
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
    let mut area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // Max inner content height a column may take before it scrolls; the column
    // itself hugs its content (a single short card no longer stretches full-height).
    let col_max = (rect.height() - COLUMN_TOP_MARGIN - 2.0 * COLUMN_PAD).max(120.0);
    egui::ScrollArea::horizontal()
        .id_salt("agents_columns")
        .show(&mut area, |ui| {
            ui.add_space(COLUMN_TOP_MARGIN);
            ui.horizontal_top(|ui| {
                ui.add_space(COLUMN_EDGE_PAD);
                for (col_idx, (start, end)) in groups(rows).into_iter().enumerate() {
                    let hue = palette.lane_colors[col_idx % palette.lane_colors.len()];
                    let col = egui::Frame::new()
                        .fill(mix(palette.bg_sidebar, hue, COLUMN_TINT_BODY))
                        .stroke(egui::Stroke::new(1.0, palette.border_subtle))
                        .corner_radius(egui::CornerRadius::same(COLUMN_RADIUS))
                        .inner_margin(COLUMN_PAD)
                        .show(ui, |ui| {
                            // The frame inherits the row's horizontal layout; the column
                            // stacks top-down inside it.
                            ui.vertical(|ui| {
                                ui.set_width(column_width - 2.0 * COLUMN_PAD);
                                project_column(
                                    ui,
                                    palette,
                                    rows,
                                    start,
                                    end,
                                    selected,
                                    hue,
                                    terminal_height,
                                    col_max,
                                    action,
                                    &mut render_terminal,
                                );
                            });
                        });
                    let col_height = col.response.rect.height();
                    column_resize_handle(ui, palette, col_idx, col_height, column_width, action);
                }
            });
        });
}

/// Draggable separator occupying a column's trailing gap: dragging adjusts the
/// shared column width (clamped, persisted by the app). A faint accent rule and
/// the resize cursor surface it on hover; otherwise it's just empty gap.
fn column_resize_handle(
    ui: &mut egui::Ui,
    palette: &Palette,
    col_idx: usize,
    col_height: f32,
    column_width: f32,
    action: &mut AgentsPageAction,
) {
    let (gap, _) = ui.allocate_exact_size(egui::vec2(COLUMN_GAP, col_height), egui::Sense::hover());
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
            egui::Stroke::new(2.0, palette.accent),
        );
    }
    if handle.dragged() {
        action.set_column_width =
            Some((column_width + handle.drag_delta().x).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH));
    }
}

/// One project column: title + count, then a vertical scroll of its worktree
/// sub-cards.
#[allow(clippy::too_many_arguments)]
fn project_column<F: FnMut(usize, &mut egui::Ui)>(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    start: usize,
    end: usize,
    selected: Option<usize>,
    hue: egui::Color32,
    terminal_height: f32,
    col_max: f32,
    action: &mut AgentsPageAction,
    render_terminal: &mut F,
) {
    card_header(ui, palette, &rows[start], end - start, COLUMN_HEADER_HEIGHT);
    let (rule, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rule.x_range(),
        rule.center().y,
        egui::Stroke::new(1.0, palette.border_subtle),
    );
    ui.add_space(8.0);
    // Hug the worktree cards, scrolling only once they outgrow the column's share
    // of the page (so one short card leaves no empty lane below it).
    let scroll_max = (col_max - COLUMN_HEADER_HEIGHT - 1.0 - 8.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt(("agents_col", start))
        .max_height(scroll_max)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (ws, we) in worktree_runs(rows, start, end) {
                worktree_card(
                    ui,
                    palette,
                    rows,
                    ws,
                    we,
                    selected,
                    hue,
                    terminal_height,
                    action,
                    render_terminal,
                );
                ui.add_space(GROUP_GAP);
            }
        });
}

/// One worktree sub-card: a branch header, then a live-terminal card per agent.
/// `hue` tints the header band so the card carries its column's color.
#[allow(clippy::too_many_arguments)]
fn worktree_card<F: FnMut(usize, &mut egui::Ui)>(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    ws: usize,
    we: usize,
    selected: Option<usize>,
    hue: egui::Color32,
    terminal_height: f32,
    action: &mut AgentsPageAction,
    render_terminal: &mut F,
) {
    // Raised `bg.surface` card so the column's tinted lane reads as the surface
    // behind it (unlike the list view, whose cards sit on the canvas).
    egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(CARD_RADIUS))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            worktree_header(ui, palette, &rows[ws], we - ws, hue);
            for (offset, row) in rows[ws..we].iter().enumerate() {
                let idx = ws + offset;
                setting_divider(ui, palette);
                agent_terminal_card(
                    ui,
                    palette,
                    row,
                    idx,
                    selected == Some(idx),
                    terminal_height,
                    action,
                    render_terminal,
                );
            }
        });
}

/// Branch header of a worktree sub-card: a git-branch icon + branch label, then
/// (when the worktree is dirty) the uncommitted ratio bar and the agent count.
/// `hue` is the column's color, washed into the band.
fn worktree_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    first: &AgentRow,
    count: usize,
    hue: egui::Color32,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), WORKTREE_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    // Column-tinted band with the card's top corners: reads as a sub-header over
    // the agent rows below it, carrying the column's color.
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius {
            nw: CARD_RADIUS,
            ne: CARD_RADIUS,
            sw: 0,
            se: 0,
        },
        mix(palette.bg_surface_hover, hue, COLUMN_TINT_BAND),
    );
    let inner = rect.shrink2(egui::vec2(CARD_PAD_X, 0.0));
    paint_icon(
        ui.painter(),
        egui::pos2(inner.left() + REPO_ICON_SIZE / 2.0, inner.center().y),
        REPO_ICON_SIZE - 1.0,
        lucide_icons::Icon::GitBranch,
        palette.accent,
    );
    let label = first.branch.filter(|b| !b.is_empty()).unwrap_or("—");
    ui.painter().text(
        egui::pos2(inner.left() + REPO_ICON_SIZE + 10.0, inner.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::monospace(CHIP_SIZE + 1.0),
        palette.text_secondary,
    );
    let count_label = if count == 1 {
        "1 agent".to_owned()
    } else {
        format!("{count} agents")
    };
    let count_font = egui::FontId::proportional(DETAIL_SIZE);
    let count_w = ui
        .painter()
        .layout_no_wrap(count_label.clone(), count_font.clone(), palette.text_muted)
        .size()
        .x;
    ui.painter().text(
        egui::pos2(inner.right(), inner.center().y),
        egui::Align2::RIGHT_CENTER,
        &count_label,
        count_font,
        palette.text_muted,
    );
    let dirty = first.stats.filter(|&(a, d)| a > 0 || d > 0);
    if let Some((additions, deletions)) = dirty {
        paint_stat_bar(
            ui,
            palette,
            inner.right() - count_w - 10.0,
            inner.center().y,
            additions,
            deletions,
        );
    }
    let info = match dirty {
        Some((a, d)) => format!("{label} · +{a} −{d} uncommitted"),
        None => label.to_owned(),
    };
    ui.interact(
        rect,
        ui.id().with(("agents_wt", label)),
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

/// One agent's sub-sub-card: a state indicator + name + tab + state-caption +
/// jump icon header, then a resizable-height live terminal that fills the card
/// width edge to edge, then a bottom drag handle for the shared height. The
/// focused agent is spotlighted by keeping its header opaque while the others dim
/// (their terminal self-dims too, like an unfocused split pane) — no outline.
/// Every card gets a unique `id_salt` so each terminal owns its own focus.
#[allow(clippy::too_many_arguments)]
fn agent_terminal_card<F: FnMut(usize, &mut egui::Ui)>(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    idx: usize,
    focused: bool,
    terminal_height: f32,
    action: &mut AgentsPageAction,
    render_terminal: &mut F,
) {
    let ink = |c: egui::Color32| {
        if focused {
            c
        } else {
            with_alpha(c, UNFOCUSED_ALPHA)
        }
    };
    ui.add_space(8.0);
    let (hrect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), AGENT_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    let inner = hrect.shrink2(egui::vec2(CARD_PAD_X, 0.0));
    // Jump-to-workspace affordance at the right edge, mirroring the list row's
    // external-link icon: focuses this agent's pane in its workspace (the app
    // handles `action.jump`).
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(inner.right() - JUMP_ICON_HIT / 2.0, inner.center().y),
        egui::vec2(JUMP_ICON_HIT, JUMP_ICON_HIT),
    );
    let jump = ui.interact(
        icon_rect,
        ui.id().with(("agent_card_jump", idx)),
        egui::Sense::click(),
    );
    let on_icon = jump.hovered();
    if jump.clicked() {
        action.jump = Some(idx);
    }
    jump.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!(
                "Open {} in workspace",
                crate::agent_watch::display_name(row.agent)
            ),
        )
    });
    let indicator = egui::Rect::from_center_size(
        egui::pos2(inner.left() + INDICATOR_SIZE / 2.0, inner.center().y),
        egui::vec2(INDICATOR_SIZE, INDICATOR_SIZE),
    );
    paint_indicator(ui, palette, row.badge, indicator, ink);
    let painter = ui.painter();
    let name_end = painter
        .text(
            egui::pos2(inner.left() + INDICATOR_SIZE + 12.0, inner.center().y),
            egui::Align2::LEFT_CENTER,
            crate::agent_watch::display_name(row.agent),
            egui::FontId::new(NAME_SIZE, theme::medium_family(ui.ctx())),
            ink(palette.text_primary),
        )
        .right();
    painter.text(
        egui::pos2(name_end + 9.0, inner.center().y),
        egui::Align2::LEFT_CENTER,
        row.tab,
        egui::FontId::proportional(TAB_SIZE),
        ink(palette.text_muted),
    );
    painter.text(
        egui::pos2(icon_rect.left() - 8.0, inner.center().y),
        egui::Align2::RIGHT_CENTER,
        &row.detail,
        egui::FontId::proportional(DETAIL_SIZE),
        ink(detail_color(palette, row.badge)),
    );
    if on_icon {
        painter.rect_filled(
            icon_rect.shrink(5.0),
            egui::CornerRadius::same(6),
            palette.bg_surface_hover,
        );
    }
    paint_icon(
        painter,
        icon_rect.center(),
        JUMP_ICON_SIZE,
        lucide_icons::Icon::ExternalLink,
        if on_icon {
            palette.text_secondary
        } else {
            ink(palette.text_muted)
        },
    );

    ui.add_space(6.0);
    let (strip, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), terminal_height),
        egui::Sense::hover(),
    );
    let mut term_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip)
            .id_salt(("agent_term", idx))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    render_terminal(idx, &mut term_ui);
    // The hovered terminal owns one wheel axis (vertical scrollback, or horizontal
    // under Shift — terminal.md §8); it reads the delta without consuming it, so
    // swallow just that axis to stop the column's vertical ScrollArea from scrolling
    // in tandem, while the cross axis still reaches the columns' horizontal scroll.
    if ui.rect_contains_pointer(strip) {
        ui.ctx().input_mut(|i| {
            if i.modifiers.shift {
                i.smooth_scroll_delta.x = 0.0;
            } else {
                i.smooth_scroll_delta.y = 0.0;
            }
        });
    }
    terminal_resize_handle(ui, palette, idx, terminal_height, action);
}

/// Draggable strip along a terminal card's bottom edge: dragging adjusts the
/// shared terminal-card height (clamped, persisted by the app), and doubles as the
/// card's bottom padding. Resize cursor + an accent rule surface it on hover.
fn terminal_resize_handle(
    ui: &mut egui::Ui,
    palette: &Palette,
    idx: usize,
    terminal_height: f32,
    action: &mut AgentsPageAction,
) {
    let (bar, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
    let handle = ui.interact(
        bar.expand2(egui::vec2(0.0, 3.0)),
        ui.id().with(("agent_term_resize", idx)),
        egui::Sense::drag(),
    );
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        ui.painter().hline(
            egui::Rangef::new(bar.left() + 24.0, bar.right() - 24.0),
            bar.center().y,
            egui::Stroke::new(2.0, palette.accent),
        );
    }
    if handle.dragged() {
        action.set_terminal_height =
            Some((terminal_height + handle.drag_delta().y).clamp(TERM_MIN_HEIGHT, TERM_MAX_HEIGHT));
    }
}

/// Splits a project's `[start, end)` rows into per-worktree runs (consecutive
/// rows sharing `worktree_id`, which arrive adjacent in workspace order).
fn worktree_runs(rows: &[AgentRow], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut s = start;
    for i in (start + 1)..end {
        if rows[i].worktree_id != rows[s].worktree_id {
            out.push((s, i));
            s = i;
        }
    }
    if end > start {
        out.push((s, end));
    }
    out
}

/// Consecutive `[start, end)` ranges sharing the same project — a root and its
/// worktrees carry the same `repo` name and arrive adjacent (workspace order),
/// so a linear scan splits them.
fn groups(rows: &[AgentRow]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..rows.len() {
        if rows[i].repo != rows[start].repo {
            out.push((start, i));
            start = i;
        }
    }
    if !rows.is_empty() {
        out.push((start, rows.len()));
    }
    out
}

/// Project header inside a card: repo name, branch chip, and the agent count.
/// `height` lets the column view run a more compact band than the list card.
fn card_header(ui: &mut egui::Ui, palette: &Palette, first: &AgentRow, count: usize, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
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
        egui::Stroke::new(1.0, palette.border_subtle),
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
                egui::Stroke::new(1.5, ink(palette.text_muted)),
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
    fn worktree_runs_split_on_worktree_id() {
        let mut rows = [
            row("helm", "claude", AgentBadge::Working),
            row("helm", "codex", AgentBadge::Done),
            row("helm", "aider", AgentBadge::Idle),
        ];
        rows[2].worktree_id = 1;
        assert_eq!(worktree_runs(&rows, 0, 3), vec![(0, 2), (2, 3)]);
        // A single worktree ⇒ one run spanning the whole project slice.
        assert_eq!(worktree_runs(&rows, 0, 2), vec![(0, 2)]);
        assert_eq!(worktree_runs(&rows, 0, 0), Vec::<(usize, usize)>::new());
    }
}
