//! Cross-repo agents dashboard (specs/agents.md): the central content rendered
//! while the sidebar's Agents entry is selected — every running (or just-finished)
//! agent across all open repositories, as a header strip of chips over a wall of the
//! mirrored terminals picked from it, laid out by the terminal's own split tree. A band
//! click selects; the discreet jump icon focuses that agent's workspace. Rendering
//! only — the page returns the targeted action, the app applies it.

use crate::agent_watch::AgentBadge;
use crate::agents_wall::MAX_SHOWN;
use crate::terminal::layout::{Layout, PaneId};
use crate::theme::{self, Palette};
use crate::ui::spinner::{done_flash_lift, paint_done_dot, paint_done_flash, Spinner};
use crate::ui::terminal_view::{terminal_tree, PaneDrop, ResizeDrag, GRIP_RESERVE};
use crate::ui::{clickable, paint_icon, with_alpha, TITLEBAR_HEIGHT};

const CONTENT_PAD_X: f32 = 32.0;
const SUBTITLE_SIZE: f32 = 13.0;

const CARD_PAD_X: f32 = 16.0;
/// Narrow elision on a wall tile's status band (project + branch + tab label): each label
/// stops short of the right-aligned state caption — reserving a minimum for the one that
/// follows — and gains a `…` instead of running underneath it. A quarter-width tile is
/// narrow, so the tab label is the first to go.
const ROW_TEXT_GAP: f32 = 10.0;
const ROW_MIN_REPO_W: f32 = 40.0;
const ROW_MIN_BRANCH_W: f32 = 48.0;

/// Ink over a green band: `text_primary` is the theme's own contrast against its
/// background (near-white in dark, near-black in light), and it holds against the green
/// in both. The softer labels step down by alpha rather than to the muted greys, which
/// the green would swallow.
const ON_GREEN_SOFT: u8 = 205;
const ON_GREEN_FAINT: u8 = 165;

/// Page header: one chip per running agent, wrapping onto further lines and scrolling
/// past `CHIP_STRIP_MAX_ROWS` of them, so even a workspace full of agents leaves the
/// wall its room.
const CHIP_HEIGHT: f32 = 30.0;
const CHIP_GAP: f32 = 8.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_RADIUS: u8 = 8;
const CHIP_ICON_GAP: f32 = 7.0;
const CHIP_REPO_SIZE: f32 = 12.5;
const CHIP_MAX_REPO_W: f32 = 190.0;
const CHIP_MAX_BRANCH_W: f32 = 150.0;
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

/// …until a turn lands. Then the band drops the project's hue and goes **green, whole**
/// — the tiles you must come back to have to read from across the room, and a 10px dot
/// on a project-colored strip does not. Far past the hue's own tints on purpose: this
/// one is the signal, not a decoration. The repo still leads the band, so the tile does
/// not lose whose it is.
const DONE_BAND_TINT: f32 = 0.46;
const DONE_BAND_HOVER_TINT: f32 = 0.56;
const DONE_BAND_ACTIVE_TINT: f32 = 0.66;
/// The extra green the arrival lifts the band by, fading over the flash window: the
/// beat is the band brightening and settling, since a green ring on a green band would
/// have nothing to read against.
const DONE_BAND_FLASH_TINT: f32 = 0.3;

/// Project hue (cycled from `palette.lane_colors`, the theme-tuned graph palette) worn
/// by a header chip once its agent is on the wall and by the sidebar's project header
/// icon. Mixed against the theme's own base rather than applied flat, so it stays
/// balanced in light and dark.
const HEADER_ICON_TINT: f32 = 0.16;

const REPO_SIZE: f32 = 14.0;
const TAB_SIZE: f32 = 12.0;
const BRANCH_SIZE: f32 = 11.5;
const DETAIL_SIZE: f32 = 12.0;
const INDICATOR_SIZE: f32 = 16.0;
const JUMP_ICON_SIZE: f32 = 15.0;
/// On a wall tile the jump icon's target spans the status band's full height and a
/// comfortable width (dense-desktop 40px minimum); the chip it paints on hover stays
/// small, so the affordance is easy to hit without looking heavy.
const CARD_JUMP_HIT: f32 = 40.0;
const CARD_JUMP_CHIP: f32 = 26.0;
/// Trailing pad of the jump glyph on a wall tile: `CARD_PAD_X` less 2px — the optical
/// correction that lands an icon on the same right rail as text at equal padding, which
/// would otherwise look short of it.
const CARD_JUMP_PAD: f32 = CARD_PAD_X - 2.0;

/// One agent line of the dashboard — the app builds these from `RepoCaches`.
pub struct AgentRow<'a> {
    /// Project name = the group root's name; a root and its worktrees share it,
    /// so `project · branch` is what tells their agents apart.
    pub repo: &'a str,
    /// This entry's own branch, shown next to the project — it tells worktrees of
    /// the same project apart.
    pub branch: Option<&'a str>,
    pub tab: &'a str,
    pub agent: &'a str,
    pub badge: AgentBadge,
    /// State-relative caption built by the app ("Working…", "Finished 2m ago",
    /// "Idle").
    pub detail: String,
    /// Milliseconds since the turn landed — `Some` only on `Done`, and counted from
    /// the **rising edge** into it (not from the last output, which the silence
    /// window already leaves stale). Drives the one-shot arrival flash; past
    /// [`DONE_FLASH_MS`] it costs nothing.
    pub done_ago_ms: Option<u64>,
    /// Project color index (rank of the group root among root projects): tints this
    /// agent's header chip and wall band and, via the same index, the project's sidebar
    /// header icon.
    pub lane: usize,
}

/// What a gesture on the dashboard targeted. The app applies it: a select makes that
/// agent's tile active, a jump focuses its pane in its workspace, a toggle puts its
/// terminal on the wall or takes it off, and a resize / drop relayouts the wall.
#[derive(Default)]
pub struct AgentsPageAction {
    pub select: Option<usize>,
    pub jump: Option<usize>,
    /// A header chip was clicked: show that agent's terminal on the wall, or hide it
    /// when it is already there.
    pub toggle: Option<usize>,
    /// The wall's own rect this frame — the app splits it to place a newly shown
    /// terminal, and reads a seam drag against it. `None` while nothing runs.
    pub wall_rect: Option<egui::Rect>,
    /// A seam between two tiles was dragged (the split tree's own resize).
    pub resize: Option<ResizeDrag>,
    /// A tile was dragged onto another by its grip: re-split on that side, or swap.
    pub drop: Option<PaneDrop>,
}

/// Cross-repo dashboard for the central area: a **header strip** listing every running
/// agent — one chip apiece, carrying its live state indicator — over a **wall** of the
/// ones picked from it, at most [`MAX_SHOWN`]. A chip click shows or hides that agent's
/// terminal; the shown ones are laid out by the **terminal's own split tree**, so the
/// wall's seams resize and its panes rearrange exactly like a workspace tab's
/// (terminal.md §5). `render_terminal(idx, ui)` mirrors the live pane of the agent at
/// row `idx`, once per wall tile, and reports whether it was clicked. Returns the
/// targeted action. The page owns the whole central area, titlebar included — nothing
/// sits in the title row, so it only clears the macOS traffic lights.
pub fn agents_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    wall: &WallView,
    render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
) -> AgentsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    ui.add_space(f32::from(TITLEBAR_HEIGHT));
    let mut action = AgentsPageAction::default();
    let rect = ui.available_rect_before_wrap();

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
        return action;
    }
    let header_bottom = agent_chips(ui, palette, rows, rect, wall, &mut action);
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
        &mut action,
        render_terminal,
    );
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
/// The dashboard is cross-repo, so the project is what tells two agents on `main` apart.
/// Spelled out for screen readers and hover text; the chips and bands paint the two
/// halves themselves, so the project can lead in its own weight.
fn origin(row: &AgentRow) -> String {
    match row.branch.filter(|b| !b.is_empty()) {
        Some(branch) => format!("{} · {branch}", row.repo),
        None => row.repo.to_owned(),
    }
}

/// The branch as it trails the project on a chip or a band — empty on a detached
/// worktree, where the project stands alone.
fn branch_suffix(row: &AgentRow) -> String {
    match row.branch.filter(|b| !b.is_empty()) {
        Some(branch) => format!("· {branch}"),
        None => String::new(),
    }
}

/// One header chip: state indicator, then where the agent runs — the project leading,
/// its branch trailing. The agent's own name is not painted (a wall of `Claude` chips
/// says nothing); it rides on the hover text with the tab. Filled in the project's hue
/// while the agent is **on the wall** (clicking takes it off), quiet otherwise. With
/// every slot taken the remaining chips read **disabled** and say so on hover — hiding
/// one is the way to make room (agents.md §5). Returns true when clicked.
fn agent_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    shown: bool,
    blocked: bool,
) -> bool {
    let name = crate::agent_watch::display_name(row.agent);
    let repo_font = egui::FontId::new(CHIP_REPO_SIZE, theme::medium_family(ui.ctx()));
    let branch_font = egui::FontId::monospace(BRANCH_SIZE);
    let branch = branch_suffix(row);
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
    let repo_w = measure(row.repo, repo_font.clone(), CHIP_MAX_REPO_W);
    let branch_w = if branch.is_empty() {
        0.0
    } else {
        CHIP_ICON_GAP + measure(&branch, branch_font.clone(), CHIP_MAX_BRANCH_W)
    };
    let width = 2.0 * CHIP_PAD_X + INDICATOR_SIZE + CHIP_ICON_GAP + repo_w + branch_w;
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
    paint_indicator(ui, palette, row.badge, row.done_ago_ms, indicator, fade);
    let painter = ui.painter();
    let text_x = indicator.right() + CHIP_ICON_GAP;
    let painted = paint_elided(
        painter,
        egui::pos2(text_x, rect.center().y),
        row.repo,
        repo_font,
        fade(if shown {
            palette.text_primary
        } else {
            palette.text_secondary
        }),
        repo_w,
    );
    if !branch.is_empty() {
        paint_elided(
            painter,
            egui::pos2(text_x + painted + CHIP_ICON_GAP, rect.center().y),
            &branch,
            branch_font,
            fade(palette.text_muted),
            branch_w - CHIP_ICON_GAP,
        );
    }
    let label = format!("{name} · {} · {}", origin(row), row.tab);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, shown, &label));
    if blocked {
        response.on_hover_text(format!(
            "{MAX_SHOWN} terminals at most — hide one to make room"
        ));
        return false;
    }
    response
        .on_hover_text(format!("{name} · {}", row.tab))
        .clicked()
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

/// One tile: a compact status band — state indicator, project, branch, tab, state
/// caption, jump icon — over the agent's **live terminal**, flush under it. The
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
    let done = row.badge == AgentBadge::Done;
    let (hue, tint) = if done {
        (
            palette.git_added,
            if active {
                DONE_BAND_ACTIVE_TINT
            } else if response.hovered() {
                DONE_BAND_HOVER_TINT
            } else {
                DONE_BAND_TINT
            } + DONE_BAND_FLASH_TINT * done_flash_lift(ui, row.done_ago_ms),
        )
    } else {
        (
            palette.lane_color(row.lane),
            if active {
                AGENTS_BAND_ACTIVE_TINT
            } else if response.hovered() {
                AGENTS_BAND_HOVER_TINT
            } else {
                AGENTS_BAND_TINT
            },
        )
    };
    ui.painter()
        .rect_filled(band, 0, mix(palette.bg_surface_hover, hue, tint));
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
    // Everything the band carries flips ink over the green — the dot first, which is the
    // one thing painted in that very color. The band's own beat replaces the ring.
    paint_indicator(ui, palette, row.badge, None, indicator, |c| {
        if done {
            palette.text_primary
        } else {
            c
        }
    });
    let (strong, soft, faint) = if done {
        (
            palette.text_primary,
            with_alpha(palette.text_primary, ON_GREEN_SOFT),
            with_alpha(palette.text_primary, ON_GREEN_FAINT),
        )
    } else {
        (
            palette.text_primary,
            palette.text_secondary,
            palette.text_muted,
        )
    };
    let painter = ui.painter();
    let detail_ink = if done {
        soft
    } else if row.badge == AgentBadge::Working {
        palette.accent
    } else {
        palette.text_muted
    };
    let detail = painter.layout_no_wrap(
        row.detail.clone(),
        egui::FontId::proportional(DETAIL_SIZE),
        detail_ink,
    );
    let detail_w = detail.size().x;
    let content_right = icon_rect.left() - 8.0 - detail_w - ROW_TEXT_GAP;
    let text_x = indicator.right() + 10.0;
    let name = crate::agent_watch::display_name(row.agent);
    let branch = branch_suffix(row);
    // The project leads (the wall is cross-repo: it is what says which tile is whose),
    // then its branch, then the tab. A narrow tile elides from the right, so the least
    // identifying label is the first to go.
    let repo_end = text_x
        + paint_elided(
            painter,
            egui::pos2(text_x, band.center().y),
            row.repo,
            egui::FontId::new(REPO_SIZE, theme::medium_family(ui.ctx())),
            strong,
            (content_right - text_x - 8.0 - ROW_MIN_BRANCH_W).max(ROW_MIN_REPO_W),
        );
    let detail_size = detail.size();
    painter.galley(
        egui::pos2(
            icon_rect.left() - 8.0 - detail_w,
            band.center().y - detail_size.y / 2.0,
        ),
        detail,
        detail_ink,
    );
    paint_icon(
        painter,
        icon_rect.center(),
        JUMP_ICON_SIZE,
        lucide_icons::Icon::ExternalLink,
        if on_icon { soft } else { faint },
    );
    let tab_x = if branch.is_empty() {
        repo_end + 9.0
    } else {
        let branch_x = repo_end + 9.0;
        branch_x
            + paint_elided(
                painter,
                egui::pos2(branch_x, band.center().y),
                &branch,
                egui::FontId::monospace(BRANCH_SIZE),
                soft,
                (content_right - branch_x).max(0.0),
            )
            + 9.0
    };
    paint_elided(
        painter,
        egui::pos2(tab_x, band.center().y),
        row.tab,
        egui::FontId::proportional(TAB_SIZE),
        faint,
        (content_right - tab_x).max(0.0),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{name} in {} — {}", origin(row), row.tab),
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

/// Paints `text` left-anchored, vertically centered at `pos`, elided with `…`
/// past `max_width`. Returns the painted width so the caller can place what
/// follows (the origin after the name) without overlapping it.
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

/// State dot: a spinner while working, a green dot once finished, a hollow grey
/// dot when idle (same visual language as the sidebar badge). A turn that *just*
/// landed also gets its one-shot ring (`done_ago_ms`). `ink` dims the colors when
/// this agent's chip is out of reach (identity otherwise).
fn paint_indicator(
    ui: &egui::Ui,
    palette: &Palette,
    badge: AgentBadge,
    done_ago_ms: Option<u64>,
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
            if let Some(elapsed) = done_ago_ms {
                paint_done_flash(ui, rect.center(), 5.0, ink(palette.git_added), elapsed);
            }
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
