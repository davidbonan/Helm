//! Cross-repo agents dashboard (specs/agents.md): the central content rendered
//! while the sidebar's Agents entry is selected — every running (or just-finished)
//! agent across all open repositories, as a header strip of chips over a wall of the
//! mirrored terminals picked from it, laid out by the terminal's own split tree. A band
//! click selects; the discreet jump icon focuses that agent's workspace. Rendering
//! only — the page returns the targeted action, the app applies it.

use crate::agent_watch::AgentBadge;
use crate::agents_wall::{MAX_SHOWN, PAGES};
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

/// The pager, in the title row over the header strip: one small button per wall page
/// (specs/agents.md §5). Deliberately quiet — it is a place to put a second set-up, not
/// the page's subject: a numbered square, the one on screen filled, a page holding
/// terminals outlined, an empty one left as a bare digit.
const PAGER_BTN: f32 = 20.0;
const PAGER_GAP: f32 = 4.0;
const PAGER_RADIUS: u8 = 6;
const PAGER_FONT: f32 = 11.0;

/// Page header: **one cluster per project**, on a single line that scrolls sideways. A
/// cluster titles its project once — the sidebar's own hue-tinted folder box, then the name
/// — and ranges its agents under it as chips. Repeating `project · branch` on every chip
/// spent the room on the part they all share and left a project's agents as
/// near-identical pills; hoisted into the cluster's title, that room goes to what does
/// tell them apart, one label per line: the worktree's **branch** over the terminal's
/// **tab**.
const CHIP_HEIGHT: f32 = 42.0;
const CHIP_GAP: f32 = 6.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_RADIUS: u8 = 8;
const CHIP_ICON_GAP: f32 = 7.0;
const CHIP_TAB_SIZE: f32 = 11.5;
const CHIP_LINE_GAP: f32 = 2.0;
const CHIP_MAX_BRANCH_W: f32 = 220.0;
const CHIP_MAX_TAB_W: f32 = 220.0;
const CHIP_STRIP_PAD_X: f32 = 12.0;
const CHIP_STRIP_PAD_Y: f32 = 10.0;

/// A cluster's title, on its own line **over** the chips it heads — the project reads as
/// what it is, the heading of a group, rather than as a first item in the same row. The
/// icon box repeats the sidebar's project header (radius and hue), so the two read as the
/// same project.
const GROUP_ICON_BOX: f32 = 20.0;
const GROUP_ICON_BOX_RADIUS: u8 = 6;
const GROUP_ICON_SIZE: f32 = 12.5;
const GROUP_NAME_SIZE: f32 = 12.5;
const GROUP_MAX_NAME_W: f32 = 240.0;
const GROUP_TITLE_H: f32 = 20.0;
const GROUP_TITLE_GAP: f32 = 5.0;
/// Clusters read apart by **proximity** — tight between the chips of one project, roomy
/// between projects — with a hairline in that gap so the grouping still holds when the
/// strip is scrolled with a project's header off screen.
const GROUP_GAP: f32 = 20.0;
/// The hairline runs alongside the **chips row** only, inset from its top and bottom edges:
/// centred on the whole cluster instead, it straddled the title/chips boundary and read as
/// a stray tick. The titles above it are held apart by the gap alone.
const GROUP_DIVIDER_INSET: f32 = 5.0;
/// The scrollbar is floating (it shows only under the pointer), so a chip cut flush at the
/// strip's edge would read as a rendering bug rather than as more chips: both edges fade
/// into the canvas while there is something past them.
const STRIP_FADE_W: f32 = 28.0;
const STRIP_FADE_STEPS: usize = 14;

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
    /// A pager button was clicked: flip the wall to that page.
    pub page: Option<usize>,
    /// The wall's own rect this frame — the app splits it to place a newly shown
    /// terminal, and reads a seam drag against it. `None` while nothing runs.
    pub wall_rect: Option<egui::Rect>,
    /// A seam between two tiles was dragged (the split tree's own resize).
    pub resize: Option<ResizeDrag>,
    /// A tile was dragged onto another by its grip: re-split on that side, or swap.
    pub drop: Option<PaneDrop>,
}

/// Cross-repo dashboard for the central area: a **header strip** listing every running
/// agent — one chip apiece, carrying its live state indicator, grouped under its project —
/// over a **wall** of the
/// ones picked from it, at most [`MAX_SHOWN`]. A chip click shows or hides that agent's
/// terminal; the shown ones are laid out by the **terminal's own split tree**, so the
/// wall's seams resize and its panes rearrange exactly like a workspace tab's
/// (terminal.md §5). `render_terminal(idx, ui)` mirrors the live pane of the agent at
/// row `idx`, once per wall tile, and reports whether it was clicked. Returns the
/// targeted action. The page owns the whole central area, titlebar included: the only
/// thing in the title row is the **pager**, which rides the band this page would otherwise
/// leave empty — `workspace_shown` places it, since with the workspace sidebar hidden the
/// macOS traffic lights and the sidebar toggle float over that corner.
#[allow(clippy::too_many_arguments)]
pub fn agents_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    selected: Option<usize>,
    wall: &WallView,
    workspace_shown: bool,
    render_terminal: impl FnMut(usize, &mut egui::Ui) -> bool,
) -> AgentsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let mut action = AgentsPageAction::default();
    // Nothing to page through while no agent runs, and the empty state reads better
    // under a bare title row.
    if !rows.is_empty() {
        wall_pager(ui, palette, rect, wall, workspace_shown, &mut action);
    }
    ui.add_space(f32::from(TITLEBAR_HEIGHT));
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
    // Two agents of one worktree running the same tool carry the same tab title, so the
    // labels alone cannot tell them apart: the tie-break is computed once for the frame and
    // worn by both the chip and the band, which must not disagree.
    let ordinals = duplicate_ordinals(rows);
    let header_bottom = agent_chips(ui, palette, rows, &ordinals, rect, wall, &mut action);
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
        &ordinals,
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
    /// The page on screen — the one the tree and the slots above belong to.
    pub page: usize,
    /// How many terminals each page holds, the pager's own state.
    pub page_counts: [usize; PAGES],
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

/// The pager: [`PAGES`] buttons, one wall composition apiece (specs/agents.md §5).
/// Flipping parks the current set-up whole — its agents and its geometry — and brings the
/// other one back as it was left. It rides the **title row**, which this page leaves empty
/// otherwise: a row of its own cost 30px of header to carry four 20px buttons.
fn wall_pager(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    wall: &WallView,
    workspace_shown: bool,
    action: &mut AgentsPageAction,
) {
    let row = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.right(), rect.top() + f32::from(TITLEBAR_HEIGHT)),
    );
    let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(row));
    // On the chips' own rail, unless the title row's floating controls need more room —
    // with the workspace sidebar hidden they reach over this corner.
    let left =
        row.left() + crate::ui::titlebar_content_inset(ui, workspace_shown).max(CHIP_STRIP_PAD_X);
    // Placed by hand rather than by a layout: a horizontal layout would add its own
    // spacing before the first button and drop them at the row's top.
    for nth in 0..PAGES {
        let button = egui::Rect::from_min_size(
            egui::pos2(
                left + nth as f32 * (PAGER_BTN + PAGER_GAP),
                row.center().y - PAGER_BTN / 2.0,
            ),
            egui::Vec2::splat(PAGER_BTN),
        );
        if pager_button(&mut row_ui, palette, button, nth, wall) {
            action.page = Some(nth);
        }
    }
}

/// One pager button: its number, filled while it is the page on screen, outlined while it
/// holds terminals it is not showing, bare when that page is empty.
fn pager_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    nth: usize,
    wall: &WallView,
) -> bool {
    let count = wall.page_counts.get(nth).copied().unwrap_or(0);
    let active = wall.page == nth;
    let response = ui
        .allocate_rect(rect, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    let fill = match (active, hovered) {
        (true, _) => palette.bg_surface,
        (false, true) => palette.bg_surface_hover,
        (false, false) => egui::Color32::TRANSPARENT,
    };
    let border = if active {
        with_alpha(palette.accent, 110)
    } else if count > 0 {
        palette.border_subtle
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(PAGER_RADIUS),
        fill,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        (nth + 1).to_string(),
        egui::FontId::new(PAGER_FONT, theme::medium_family(ui.ctx())),
        match (active, count > 0) {
            (true, _) => palette.text_primary,
            (false, true) => palette.text_secondary,
            (false, false) => with_alpha(palette.text_muted, 140),
        },
    );
    let label = format!("Wall page {}", nth + 1);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &label));
    response
        .on_hover_text(match count {
            0 => format!("{label} — empty"),
            1 => format!("{label} — 1 terminal"),
            n => format!("{label} — {n} terminals"),
        })
        .clicked()
}

/// The header strip: one cluster per project, in workspace order (so a project's worktrees
/// stay adjacent) — its **title over** the row of its agents' chips — scrolling
/// **sideways** past the strip's width, which keeps the strip's height fixed however many
/// agents run and leaves the wall all the rest. Returns the strip's bottom edge.
fn agent_chips(
    ui: &mut egui::Ui,
    palette: &Palette,
    rows: &[AgentRow],
    ordinals: &[Option<usize>],
    rect: egui::Rect,
    wall: &WallView,
    action: &mut AgentsPageAction,
) -> f32 {
    let strip = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.right(), rect.top() + strip_height()),
    );
    let mut strip_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // Where each cluster ended up, to pin the title of the one the strip is scrolled into.
    let mut spans: Vec<ClusterSpan> = Vec::new();
    let out = egui::ScrollArea::horizontal()
        .id_salt("agents_chips")
        .show(&mut strip_ui, |ui| {
            ui.add_space(CHIP_STRIP_PAD_Y);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.add_space(CHIP_STRIP_PAD_X);
                for (nth, group) in project_groups(rows).into_iter().enumerate() {
                    if nth > 0 {
                        group_divider(ui, palette);
                    }
                    let lead = &rows[group.start];
                    let mut title = egui::Rect::NOTHING;
                    let cluster = ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, GROUP_TITLE_GAP);
                        title = group_title(ui, palette, lead.repo, lead.lane);
                        ui.horizontal(|ui| {
                            for idx in group {
                                let shown = wall.shows(idx);
                                if agent_chip(
                                    ui,
                                    palette,
                                    &rows[idx],
                                    ordinals[idx],
                                    shown,
                                    !shown && wall.full,
                                ) {
                                    action.toggle = Some(idx);
                                }
                            }
                        });
                    });
                    spans.push(ClusterSpan {
                        repo: lead.repo,
                        lane: lead.lane,
                        title,
                        right: cluster.response.rect.right(),
                    });
                }
                ui.add_space(CHIP_STRIP_PAD_X);
            });
            ui.add_space(CHIP_STRIP_PAD_Y);
        });
    let scrolled = out.state.offset.x > 1.0;
    if scrolled {
        paint_edge_fade(ui.painter(), strip, true, palette.bg_canvas);
    }
    if out.content_size.x - out.state.offset.x - out.inner_rect.width() > 1.0 {
        paint_edge_fade(ui.painter(), strip, false, palette.bg_canvas);
    }
    // Scrolled into a cluster, past its own title: its chips would be left saying which
    // branch but not whose. The title follows them instead, pinned at the strip's edge on
    // its own line — clear of the chips, which is what putting it above them buys. Spans do
    // not overlap, so at most one pins.
    let pin_x = strip.left() + CHIP_STRIP_PAD_X;
    if let Some(span) = spans
        .iter()
        .filter(|_| scrolled)
        .find(|span| span.title.left() < pin_x - 0.5 && span.right > pin_x)
    {
        pin_group_title(ui, palette, strip, span);
    }
    strip.bottom()
}

/// The strip's fixed height: a cluster's title line over one row of chips.
fn strip_height() -> f32 {
    2.0 * CHIP_STRIP_PAD_Y + GROUP_TITLE_H + GROUP_TITLE_GAP + CHIP_HEIGHT
}

/// Where a cluster landed on the strip this frame: its title's rect and the right edge of
/// its last chip, in screen space, for the pinning test.
struct ClusterSpan<'a> {
    repo: &'a str,
    lane: usize,
    title: egui::Rect,
    right: f32,
}

/// Repaints a scrolled-past cluster's title at the strip's left edge, over an opaque
/// backdrop that fades out along the title's own line — the pinned copy is decoration, so
/// it claims no accessibility node of its own.
fn pin_group_title(ui: &mut egui::Ui, palette: &Palette, strip: egui::Rect, span: &ClusterSpan) {
    let pin_x = strip.left() + CHIP_STRIP_PAD_X;
    let width = group_title_width(ui, palette, span.repo);
    let band = egui::Rect::from_min_max(
        egui::pos2(strip.left(), strip.top()),
        egui::pos2(pin_x + width + CHIP_GAP, span.title.bottom() + 1.0),
    );
    ui.painter().rect_filled(band, 0, palette.bg_canvas);
    paint_edge_fade(
        ui.painter(),
        egui::Rect::from_min_max(
            band.right_top(),
            egui::pos2(band.right() + STRIP_FADE_W, band.bottom()),
        ),
        true,
        palette.bg_canvas,
    );
    paint_group_title(
        ui,
        palette,
        span.repo,
        span.lane,
        egui::Rect::from_min_size(
            egui::pos2(pin_x, span.title.top()),
            egui::vec2(width, span.title.height()),
        ),
    );
}

/// Consecutive rows sharing a project, one cluster apiece. The app orders agents by
/// workspace position, where a root and its worktrees sit adjacent, so grouping runs of
/// equal names is enough — and a project that somehow came in split simply gets two
/// clusters rather than a chip filed under the wrong header.
fn project_groups(rows: &[AgentRow]) -> Vec<std::ops::Range<usize>> {
    let mut groups: Vec<std::ops::Range<usize>> = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        match groups.last_mut() {
            Some(open) if rows[open.start].repo == row.repo => open.end = idx + 1,
            _ => groups.push(idx..idx + 1),
        }
    }
    groups
}

/// `#n` for each agent whose project, branch **and** tab all match another's — five
/// `Claude Code` tabs of one worktree are otherwise one chip repeated five times, and the
/// labels have nothing left to tell them apart with. `None` on a unique triple, which is
/// the common case: the mark only shows where it is needed.
fn duplicate_ordinals(rows: &[AgentRow]) -> Vec<Option<usize>> {
    let same =
        |a: &AgentRow, b: &AgentRow| a.repo == b.repo && a.branch == b.branch && a.tab == b.tab;
    rows.iter()
        .enumerate()
        .map(|(idx, row)| {
            let twins = rows.iter().filter(|other| same(row, other)).count();
            (twins > 1).then(|| rows[..idx].iter().filter(|prev| same(row, prev)).count() + 1)
        })
        .collect()
}

/// The terminal as a chip and a band name it: its tab, plus the `#n` tie-break when an
/// identical tab of the same worktree runs elsewhere on the strip.
fn tab_label(row: &AgentRow, ordinal: Option<usize>) -> String {
    match ordinal {
        Some(n) => format!("{} #{n}", row.tab),
        None => row.tab.to_owned(),
    }
}

/// A cluster's title: the project's name once, behind the sidebar's own hue-tinted folder
/// box, over the chips it heads. It is a label, not a control — the chips are what clicks.
/// Returns the rect it took, which is where the pinned copy comes from.
fn group_title(ui: &mut egui::Ui, palette: &Palette, repo: &str, lane: usize) -> egui::Rect {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(group_title_width(ui, palette, repo), GROUP_TITLE_H),
        egui::Sense::hover(),
    );
    paint_group_title(ui, palette, repo, lane, rect);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, repo));
    rect
}

/// The room a cluster title takes: its icon box, then the project's name up to
/// [`GROUP_MAX_NAME_W`].
fn group_title_width(ui: &egui::Ui, palette: &Palette, repo: &str) -> f32 {
    let name_w = ui
        .painter()
        .layout_no_wrap(
            repo.to_owned(),
            group_name_font(ui.ctx()),
            palette.text_primary,
        )
        .size()
        .x
        .min(GROUP_MAX_NAME_W)
        .ceil();
    GROUP_ICON_BOX + CHIP_ICON_GAP + name_w
}

fn group_name_font(ctx: &egui::Context) -> egui::FontId {
    egui::FontId::new(GROUP_NAME_SIZE, theme::medium_family(ctx))
}

fn paint_group_title(ui: &egui::Ui, palette: &Palette, repo: &str, lane: usize, rect: egui::Rect) {
    let icon_box = egui::Rect::from_center_size(
        egui::pos2(rect.left() + GROUP_ICON_BOX / 2.0, rect.center().y),
        egui::Vec2::splat(GROUP_ICON_BOX),
    );
    ui.painter().rect(
        icon_box,
        egui::CornerRadius::same(GROUP_ICON_BOX_RADIUS),
        project_header_tint(palette, lane),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        icon_box.center(),
        GROUP_ICON_SIZE,
        lucide_icons::Icon::Folders,
        palette.text_secondary,
    );
    paint_elided(
        ui.painter(),
        egui::pos2(icon_box.right() + CHIP_ICON_GAP, rect.center().y),
        repo,
        group_name_font(ui.ctx()),
        palette.text_primary,
        rect.right() - icon_box.right() - CHIP_ICON_GAP,
    );
}

/// The hairline between two clusters, centred in the gap that already separates them and
/// running the height of the chips it stands between.
fn group_divider(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(GROUP_GAP, GROUP_TITLE_H + GROUP_TITLE_GAP + CHIP_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().vline(
        rect.center().x.round(),
        rect.bottom() - CHIP_HEIGHT + GROUP_DIVIDER_INSET..=rect.bottom() - GROUP_DIVIDER_INSET,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

/// Fades one edge of the strip into the canvas, hard at the edge and gone
/// [`STRIP_FADE_W`] in — the cue that the strip scrolls that way.
fn paint_edge_fade(painter: &egui::Painter, strip: egui::Rect, left: bool, bg: egui::Color32) {
    let step = STRIP_FADE_W / STRIP_FADE_STEPS as f32;
    for nth in 0..STRIP_FADE_STEPS {
        let fade = 1.0 - nth as f32 / STRIP_FADE_STEPS as f32;
        let x = if left {
            strip.left() + nth as f32 * step
        } else {
            strip.right() - (nth + 1) as f32 * step
        };
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, strip.top()),
                // Overlapping by a hair: adjacent slices must not leave a seam of canvas
                // showing through the gradient.
                egui::vec2(step + 1.0, strip.height()),
            ),
            0,
            with_alpha(bg, (fade * 255.0) as u8),
        );
    }
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

/// One chip of a project's cluster: state indicator, then the two things that tell one of
/// that project's agents from the next, a line each — its worktree's **branch** in mono
/// over the **tab** of the terminal it runs in, quieter, `#n` appended when an identical tab
/// of the same worktree also runs. Two lines rather than one row of labels: a branch and a
/// tab side by side read as one long string, stacked they read as two facts.
/// The project is the cluster's title, and the agent's own name
/// is not painted (a strip of `Claude` labels says nothing); both ride on the hover text.
/// Filled in the project's hue while the agent is **on the wall** (clicking takes it off),
/// quiet otherwise. With every slot taken the remaining chips read **disabled** and say so
/// on hover — hiding one is the way to make room (agents.md §5). Returns true when clicked.
fn agent_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    ordinal: Option<usize>,
    shown: bool,
    blocked: bool,
) -> bool {
    let name = crate::agent_watch::display_name(row.agent);
    let branch_font = egui::FontId::monospace(BRANCH_SIZE);
    let tab_font = egui::FontId::proportional(CHIP_TAB_SIZE);
    let branch = row.branch.filter(|b| !b.is_empty());
    let tab = tab_label(row, ordinal);
    // A blocked chip is legible but visibly out of reach: the whole chip fades, ink and
    // indicator alike.
    let fade = |c: egui::Color32| if blocked { with_alpha(c, 110) } else { c };
    // Rounded up: the measured width doubles as the width the label is then truncated at,
    // and a fractional one lands on the edge case where the last glyph gets elided away.
    let measure = |text: &str, font: egui::FontId, max: f32| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, palette.text_primary)
            .size()
            .x
            .min(max)
            .ceil()
    };
    let branch_w = branch.map_or(0.0, |b| measure(b, branch_font.clone(), CHIP_MAX_BRANCH_W));
    let tab_w = measure(&tab, tab_font.clone(), CHIP_MAX_TAB_W);
    // As wide as its longer line.
    let width = 2.0 * CHIP_PAD_X + INDICATOR_SIZE + CHIP_ICON_GAP + branch_w.max(tab_w);
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
    // The two lines sit as one block centred against the indicator, so the chip reads as a
    // single object rather than two rows that happen to share a surface. On a detached
    // worktree there is no branch and the tab stands alone, centred.
    let line_height = |text: &str, font: egui::FontId| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, palette.text_primary)
            .size()
            .y
    };
    let painter = ui.painter();
    let text_x = indicator.right() + CHIP_ICON_GAP;
    let tab_ink = fade(palette.text_muted);
    let Some(branch) = branch else {
        paint_elided(
            painter,
            egui::pos2(text_x, rect.center().y),
            &tab,
            tab_font,
            tab_ink,
            tab_w,
        );
        return chip_outcome(response, shown, blocked, &name, row, &tab);
    };
    let (top_h, bottom_h) = (
        line_height(branch, branch_font.clone()),
        line_height(&tab, tab_font.clone()),
    );
    let top_y = rect.center().y - (top_h + CHIP_LINE_GAP + bottom_h) / 2.0 + top_h / 2.0;
    let bottom_y = top_y + top_h / 2.0 + CHIP_LINE_GAP + bottom_h / 2.0;
    paint_elided(
        painter,
        egui::pos2(text_x, top_y),
        branch,
        branch_font,
        fade(if shown {
            palette.text_primary
        } else {
            palette.text_secondary
        }),
        branch_w,
    );
    paint_elided(
        painter,
        egui::pos2(text_x, bottom_y),
        &tab,
        tab_font,
        tab_ink,
        tab_w,
    );
    chip_outcome(response, shown, blocked, &name, row, &tab)
}

/// A chip's accessibility label, tooltip and click, shared by its one- and two-line forms.
/// The tooltip spells out what the chip leaves to its cluster: the agent's own name, and the
/// project the chip sits under.
fn chip_outcome(
    response: egui::Response,
    shown: bool,
    blocked: bool,
    name: &str,
    row: &AgentRow,
    tab: &str,
) -> bool {
    let label = format!("{name} · {} · {tab}", origin(row));
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, shown, &label));
    if blocked {
        response.on_hover_text(format!(
            "{MAX_SHOWN} terminals at most on a page — hide one, or use another page"
        ));
        return false;
    }
    response.on_hover_text(&label).clicked()
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
    ordinals: &[Option<usize>],
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
            ordinals.get(idx).copied().flatten(),
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
#[allow(clippy::too_many_arguments)]
fn wall_tile<F: FnMut(usize, &mut egui::Ui) -> bool>(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &AgentRow,
    ordinal: Option<usize>,
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
    let tab = tab_label(row, ordinal);
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
        &tab,
        egui::FontId::proportional(TAB_SIZE),
        faint,
        (content_right - tab_x).max(0.0),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{name} in {} — {tab}", origin(row)),
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
            page: 0,
            page_counts: [2, 0, 0, 0],
        };
        assert_eq!(wall.row_of(PaneId(3)), Some(2));
        assert_eq!(wall.row_of(PaneId(9)), None);
        assert!(wall.shows(0));
        assert!(!wall.shows(1));
    }

    fn row<'a>(repo: &'a str, branch: &'a str, tab: &'a str) -> AgentRow<'a> {
        AgentRow {
            repo,
            branch: Some(branch),
            tab,
            agent: "claude",
            badge: AgentBadge::Idle,
            detail: String::new(),
            done_ago_ms: None,
            lane: 0,
        }
    }

    #[test]
    fn one_cluster_per_run_of_rows_sharing_a_project() {
        let rows = [
            row("helm", "main", "Tab 1"),
            row("helm", "agents", "Tab 1"),
            row("api", "main", "Tab 1"),
            row("helm", "main", "Tab 2"),
        ];
        // The last row is a project that came back after another: it opens its own cluster
        // rather than being filed under the earlier `helm` header, which is far away.
        assert_eq!(project_groups(&rows), vec![0..2, 2..3, 3..4]);
    }

    #[test]
    fn only_agents_no_label_can_tell_apart_get_an_ordinal() {
        let rows = [
            row("helm", "main", "Claude Code"),
            row("helm", "main", "Claude Code"),
            row("helm", "agents", "Claude Code"),
            row("helm", "main", "Claude Code"),
        ];
        assert_eq!(
            duplicate_ordinals(&rows),
            vec![Some(1), Some(2), None, Some(3)],
            "the three twins are numbered in workspace order; the other worktree's own \
             tab is unique and stays unmarked"
        );
        assert_eq!(tab_label(&rows[1], Some(2)), "Claude Code #2");
        assert_eq!(tab_label(&rows[2], None), "Claude Code");
    }
}
