use std::collections::HashSet;
use std::path::Path;

use crate::agent_watch::AgentBadge;
use crate::git::worktree::{path_for_branch, WorktreeSource};
use crate::keybindings::{Action, Keymap};
use crate::theme::{Palette, SHORTCUT_BADGE_SIZE};
use crate::ui::spinner::{paint_done_dot, paint_pinwheel, Spinner};
use crate::ui::{arrow_nav_pressed, ArrowNav, MAX_SHORTCUT, SECTION_TOP_MARGIN};

/// One line of the project sidebar (worktrees.md §1): a non-selectable project
/// header, or a selectable worktree row sitting under it.
pub enum SidebarItem<'a> {
    Header(ProjectHeader<'a>),
    Row(RepoRow<'a>),
}

/// Non-selectable project header (worktrees.md §1): the project identity (root
/// folder name) and its group-level actions, above the main and linked rows.
pub struct ProjectHeader<'a> {
    /// Workspace entry index of the group root: the collapse toggle, `+`
    /// create-worktree, the ⋯ menu and the block drag handle all target it.
    pub root: usize,
    /// Project (root folder) name, elided when too long.
    pub name: &'a str,
    pub path: &'a str,
    pub collapsed: bool,
    /// Project color index (rank among root projects): tints the header icon to
    /// match this project's column in the Agents view.
    pub lane: usize,
    /// The root is present on disk and can host a new linked worktree.
    pub can_create_worktree: bool,
    /// Aggregate agent activity over the project's worktrees (max), shown at the
    /// right edge when the group is collapsed (specs/agents.md §1).
    pub agent: AgentBadge,
}

pub struct RepoRow<'a> {
    /// Workspace entry index — selection, intents and the drag payload.
    pub index: usize,
    /// Folder name, shown on hover (the branch is the visible label).
    pub name: &'a str,
    pub path: &'a str,
    pub missing: bool,
    /// Main worktree (the root's own working tree): solid ● icon, pinned first,
    /// never reordered. A linked worktree is `false`: hollow ○ ring, draggable.
    pub main: bool,
    /// Current branch (HEAD), the row's single-line label; the folder name stands
    /// in when it is absent (detached / unreadable).
    pub branch: Option<&'a str>,
    /// Delete worktree running on a dedicated thread (worktrees.md §6): greyed-out
    /// row, spinner in place of the icon, click and context menu inert.
    pub deleting: bool,
    /// AI agent activity in the entry's terminals (specs/agents.md): replaces the
    /// row icon — spinner (working), green (finished), grey ring (idle).
    pub agent: AgentBadge,
    /// Uncommitted line stats `(additions, deletions)` when the worktree is dirty:
    /// a `+N −M` diffstat at the row's right edge, hidden while Cmd is held so the
    /// `⌃⌘N` shortcut can take that column. `Some((0, 0))` (dirty with no countable
    /// lines) falls back to a small dot; `None` is a clean worktree.
    pub stats: Option<(usize, usize)>,
}

/// An indented child row under the Agents entry: one agent pane currently in the
/// `Done` state (specs/agents.md §5). `index` is its position in `caches.agents`,
/// consumed by `HelmApp::focus_agent` to jump to that pane.
pub struct DoneAgentRow {
    pub index: usize,
    /// Worktree branch (the row's locator), or `None` when detached/unreadable.
    pub branch: Option<String>,
    pub tab: String,
}

/// One project (group root) in the "Projects" eye dropdown that toggles sidebar
/// visibility. Every project is listed — hidden ones included — so a hidden
/// project can be brought back from the only surface that still shows it.
pub struct ProjectVisibility<'a> {
    /// Workspace entry index of the group root: the toggle target.
    pub root: usize,
    pub name: &'a str,
    pub hidden: bool,
}

/// Signals emitted by the sidebar in a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct SidebarAction {
    pub select: Option<usize>,
    pub remove: Option<usize>,
    pub reveal: Option<usize>,
    pub delete_worktree: Option<usize>,
    pub create_worktree: Option<usize>,
    /// Disclosure chevron clicked on a group root: fold/unfold its worktrees.
    pub toggle_collapse: Option<usize>,
    /// Show/hide a project: the eye-dropdown checkbox or the header's "Hide
    /// project" menu, carrying the group root's index.
    pub toggle_hidden: Option<usize>,
    /// A row was dropped onto another by drag-and-drop: reorder request resolved
    /// by `Workspace::reorder`.
    pub reorder: Option<Reorder>,
    pub open: bool,
    /// The cross-repo Agents entry was clicked: switches the central area to the
    /// dashboard (specs/agents.md §5).
    pub open_agents: bool,
    /// The Pull Requests entry was clicked: switches the central area to the
    /// cockpit (specs/pull-requests.md §2).
    pub open_pull_requests: bool,
    /// A Done-state agent child row under the Agents entry was clicked, carrying its
    /// index in `caches.agents`: focus that pane (specs/agents.md §5).
    pub focus_agent: Option<usize>,
}

/// A drag-and-drop reorder: move the block rooted at `from` to land relative to
/// `anchor` (`after` = below it). The domain (`resolve_reorder`) decides whether
/// the move is legal and where the block actually lands.
#[derive(Clone, Copy)]
pub struct Reorder {
    pub from: usize,
    pub anchor: usize,
    pub after: bool,
}

/// Drag payload carried while a sidebar row is dragged: its workspace entry index.
#[derive(Clone, Copy)]
struct DragRow(usize);

/// Delete worktree modal (worktrees.md §6): dirty ⇒ forced confirmation,
/// locked/error ⇒ refusal with a reason.
pub enum DeletePrompt {
    Dirty { label: String, files: usize },
    Refused { label: String, reason: String },
}

/// Outcome of the modal, consumed by `HelmApp`.
#[derive(Default)]
pub struct DeleteModalAction {
    pub confirm: bool,
    pub dismiss: bool,
}

/// Modal selection: an existing source by index, or the on-the-fly new branch
/// (worktrees.md §6). The new-branch row sits last in keyboard order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateSelection {
    Source(usize),
    NewBranch,
}

pub struct CreateWorktreePrompt<'a> {
    pub root_label: &'a str,
    /// Root of the worktree group: destination preview (`path_for_branch`).
    pub root: &'a Path,
    /// Per-project worktree base (worktrees.md §6); `None` ⇒ `<root>.worktrees`.
    pub base: Option<&'a Path>,
    pub sources: &'a [WorktreeSource],
    pub selected: Option<CreateSelection>,
    /// Base a fly-created branch starts from (root HEAD label); the "Create
    /// branch … from <base>" row shows only when this is set.
    pub base_branch: &'a str,
    /// Lowercased names already taken (local + remote local-names): the
    /// new-branch row is offered only for a query absent from this set.
    pub taken: &'a HashSet<String>,
    pub error: Option<&'a str>,
    pub loading: bool,
    pub busy: bool,
}

/// Mutable view state of the create-worktree modal, owned by `HelmApp` for the
/// modal's lifetime (same pattern as the graph `BranchEditor`).
#[derive(Default)]
pub struct CreateWorktreeState {
    /// Autocomplete filter typed in the source-branch input.
    pub query: String,
    /// One-shot focus placed on the filter input at opening.
    pub focused: bool,
    /// Destination folder, pre-filled with the selected branch name.
    pub name: String,
    /// The user typed a custom name: stop following the selection.
    pub name_edited: bool,
}

impl CreateWorktreeState {
    /// Destination folder actually used: the custom name, or the name the
    /// current selection follows (a branch name) when the field is empty/untouched.
    pub fn effective_name<'a>(&'a self, follow: &'a str) -> &'a str {
        let name = self.name.trim();
        if name.is_empty() {
            follow
        } else {
            name
        }
    }
}

#[derive(Default)]
pub struct CreateWorktreeModalAction {
    pub select: Option<CreateSelection>,
    pub create: bool,
    pub dismiss: bool,
}

const ROW_HEIGHT: f32 = 31.0;
/// Linked-worktree row (worktrees.md §3): two stacked lines — the folder name as
/// the title, the branch as a caption beneath it — so it stands taller than the
/// single-line root row.
const ROW_HEIGHT_TWO_LINE: f32 = 44.0;
/// Project header band (worktrees.md §1): a touch shorter than a row so the
/// header reads as a heading, not a selectable line.
const HEADER_HEIGHT: f32 = 28.0;
/// Breathing room above every project header but the first, separating one
/// project block from the previous one (worktrees.md §1).
const PROJECT_GAP: f32 = 8.0;
/// Width of the primary bar marking the active row, drawn at the panel's left edge.
const ROW_ACCENT_BAR: f32 = 2.0;
const ROW_PAD_X: f32 = 8.0;
/// Inset for worktree rows so their icon lines up with the project header's name
/// column — one icon plus its gap past the header icon (worktrees.md §1).
const ROW_INDENT: f32 = ICON_SIZE + ICON_GAP;
const CHEVRON_SIZE: f32 = 12.0;
const ICON_SIZE: f32 = 13.0;
const ICON_RING_WIDTH: f32 = 1.5;
const ICON_GAP: f32 = 10.0;
/// Bordered frame around the project-header icon (worktrees.md §1): a small box
/// setting the project identity apart from the nested worktree row icons below.
const HEADER_ICON_BOX: f32 = 22.0;
const HEADER_ICON_BOX_RADIUS: u8 = 6;
const NAME_SIZE: f32 = 15.0;
/// Branch caption under a linked-worktree row (worktrees.md §3): a dimmer
/// monospace line, smaller than the folder-name title above it.
const BRANCH_SIZE: f32 = 12.0;
const TWO_LINE_GAP: f32 = 1.0;
const HEADER_NAME_SIZE: f32 = 13.5;
const BADGE_COL_W: f32 = 38.0;
const CREATE_COL_W: f32 = 24.0;
/// Width reserved after the project name for the collapse chevron that trails it.
const CHEVRON_COL_W: f32 = 22.0;
/// Gap between the project name and the chevron that follows it.
const HEADER_CHEVRON_GAP: f32 = 6.0;
/// Width reserved at the header's right edge for the aggregate agent dot.
const HEADER_AGENT_COL_W: f32 = 14.0;
const PLUS_SIZE: f32 = 15.0;
const PLUS_HIT: f32 = 20.0;
/// Eye toggle next to the "Projects" header opening the show/hide dropdown.
const EYE_SIZE: f32 = 14.0;
/// Agent activity dot (specs/agents.md) at the right edge of the row, in the ⌃⌘N
/// badge column — mutually exclusive: ⌃⌘N only appears while Cmd is held.
const AGENT_DOT_RADIUS: f32 = 3.5;
const AGENT_SPINNER_SIZE: f32 = 11.0;
/// Uncommitted indicator at the row's right edge: a fixed-width green/red ratio bar
/// with a tiny `+N −M` caption beneath it, so an over-long worktree name stays
/// readable. Shares the ⌃⌘N badge column and yields it while Cmd is held; a dirty
/// change with no countable lines falls back to a dot.
const DIRTY_DOT_RADIUS: f32 = 3.0;
const STAT_BAR_W: f32 = 22.0;
const STAT_BAR_H: f32 = 3.0;
/// Min width of each non-zero half of the bar so a lopsided diff still shows both.
const STAT_BAR_MIN: f32 = 3.0;
const STAT_BAR_GAP: f32 = 1.5;
const STAT_BAR_TEXT_GAP: f32 = 2.0;
/// Very small caption under the bar.
const STAT_SIZE: f32 = 9.0;
/// Gap between the `+N` and `−M` halves of the caption.
const STAT_GAP: f32 = 4.0;

#[allow(clippy::too_many_arguments)]
pub fn repo_sidebar(
    ui: &mut egui::Ui,
    palette: &Palette,
    items: &[SidebarItem],
    // Child-flag of every workspace entry (folded or not): `resolve_reorder` reads
    // the full layout, so it mirrors the entry order, not the visible items.
    child_flags: &[bool],
    // Every project (group root), hidden ones included: the eye dropdown's source,
    // distinct from `items` which already drops hidden projects.
    projects: &[ProjectVisibility],
    active: Option<usize>,
    agents_badge: AgentBadge,
    agents_active: bool,
    // Agent panes in the `Done` state, shown as indented child rows under the Agents
    // entry (specs/agents.md §5).
    done_agents: &[DoneAgentRow],
    // Count of PRs awaiting my review — the Pull Requests entry's badge; 0 ⇒ none
    // (specs/pull-requests.md §2).
    pr_to_review: usize,
    pr_active: bool,
    keymap: &Keymap,
    out: &mut SidebarAction,
) {
    // While a Helm central mode (dashboard / PR cockpit) owns the central area, its
    // entry is the selected row — no repo/worktree row may read as active alongside it.
    let active = if agents_active || pr_active {
        None
    } else {
        active
    };
    // Open Folder (header) stays on lone Cmd; the project badge is ⌃⌘N, so it
    // tolerates Ctrl to stay visible throughout the chord (keybindings §1, §5).
    let cmd_held = ui.input(|i| {
        let m = i.modifiers;
        m.command && !m.shift && !m.alt && !m.ctrl
    });
    let cmd_digits = ui.input(|i| {
        let m = i.modifiers;
        m.command && !m.shift && !m.alt
    });
    let open_folder = keymap.shortcut_for(Action::OpenFolder).map(|s| s.display());

    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD_X);
        ui.label(crate::ui::section_label(palette, "Helm"));
    });
    if !items.is_empty() {
        // Wrapped like the repo rows: the `⌃⌘0` badge's `new_child` only exists while
        // Cmd is held, so a stable id scope keeps it from shifting the following
        // header/scroll auto-ids when Cmd toggles (see the row loop below).
        ui.push_id("agents_entry", |ui| {
            agents_entry(ui, palette, agents_badge, agents_active, cmd_digits, out);
        });
        for row in done_agents {
            agent_child_row(ui, palette, row, out);
        }
        pull_requests_entry(ui, palette, pr_to_review, pr_active, out);
    }

    sidebar_header(
        ui,
        palette,
        projects,
        !items.is_empty(),
        cmd_held,
        open_folder.as_deref(),
        out,
    );

    if items.is_empty() {
        open_folder_row(ui, palette, open_folder.as_deref(), out);
        return;
    }

    let scroll_id = ui.id().with("repo_active_scroll");
    let prev = ui
        .data_mut(|d| d.get_temp::<usize>(scroll_id))
        .unwrap_or(usize::MAX);
    let cur = active.unwrap_or(usize::MAX);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // The drop side (above/below the hovered item) follows the live pointer;
            // `dnd_*_payload` only fire on the item under it, so this is `Some` then.
            let pointer = ui.input(|i| i.pointer.interact_pos());
            // Visible items in render order — feeds the drop insertion line, drawn
            // after the loop once every rect is known.
            let mut placed: Vec<(usize, egui::Rect)> = Vec::new();
            let mut hovering: Option<(usize, usize, bool)> = None;
            let mut released: Option<(usize, usize, bool)> = None;
            // The `⌘1..9` numbering counts rows only — project headers are skipped
            // (worktrees.md §7).
            let mut visible_pos = 0usize;
            let mut seen_header = false;
            for item in items.iter() {
                // A stable id_salt per item: without it, the ⌃⌘N badge's `new_child`
                // (created only while Cmd is held) shifts the auto-ids of the following
                // items, which egui flags in debug with a red box + a relayout pass.
                let (anchor, response) = match item {
                    SidebarItem::Header(header) => {
                        if seen_header {
                            ui.add_space(PROJECT_GAP);
                        }
                        seen_header = true;
                        let response = ui
                            .push_id(("header", header.root), |ui| {
                                project_header(ui, palette, header, out)
                            })
                            .inner;
                        (header.root, response)
                    }
                    SidebarItem::Row(row) => {
                        let shortcut = (visible_pos < MAX_SHORTCUT).then_some(visible_pos);
                        visible_pos += 1;
                        let response = ui
                            .push_id(("row", row.index), |ui| {
                                repo_row(
                                    ui,
                                    palette,
                                    row,
                                    active == Some(row.index),
                                    cmd_digits,
                                    shortcut,
                                    out,
                                )
                            })
                            .inner;
                        if active == Some(row.index) && prev != cur {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        (row.index, response)
                    }
                };
                let below = pointer.is_some_and(|p| p.y > response.rect.center().y);
                if let Some(drag) = response.dnd_hover_payload::<DragRow>() {
                    hovering = Some((drag.0, anchor, below));
                }
                if let Some(drag) = response.dnd_release_payload::<DragRow>() {
                    released = Some((drag.0, anchor, below));
                }
                placed.push((anchor, response.rect));
            }
            draw_project_guides(ui, palette, items, &placed);
            if let Some((from, anchor, after)) = released {
                if crate::workspace::resolve_reorder(child_flags, from, anchor, after).is_some() {
                    out.reorder = Some(Reorder {
                        from,
                        anchor,
                        after,
                    });
                }
            } else if let Some((from, anchor, after)) = hovering {
                if let Some((_, _, insert_at)) =
                    crate::workspace::resolve_reorder(child_flags, from, anchor, after)
                {
                    draw_drop_line(ui, palette, &placed, insert_at);
                }
            }
        });
    ui.data_mut(|d| d.insert_temp(scroll_id, cur));
}

/// Light tree guide tying each project header to its nested worktree rows: a faint
/// vertical spine in the indent gutter, dropping from below the header icon with a
/// short square tick branching to each row, ending at the last row — so the
/// grouping reads as a tree rather than a bare indent. Drawn in one pass after the
/// loop from the laid-out rects (`placed` mirrors `items`).
fn draw_project_guides(
    ui: &egui::Ui,
    palette: &Palette,
    items: &[SidebarItem],
    placed: &[(usize, egui::Rect)],
) {
    // Gap left between the guide tick and the row icon it points at: a short tick
    // branching off the spine, stopping well before the icon.
    const GUIDE_ICON_GAP: f32 = 15.0;
    // Breathing room below the header icon box before the spine starts.
    const GUIDE_SPINE_GAP: f32 = 9.0;
    let stroke = egui::Stroke::new(1.0, palette.border_subtle);
    let painter = ui.painter();
    let mut i = 0;
    while i < items.len() {
        if !matches!(items[i], SidebarItem::Header(_)) {
            i += 1;
            continue;
        }
        let header_rect = placed[i].1;
        // Spine sits at the header icon's left edge so it reads as a bracket beside
        // the icon column, not a line through it; the tick stops short of the row
        // icon so the guide stays clearly detached from it.
        let guide_x = header_rect.left() + ROW_PAD_X;
        let tick_x = header_rect.left() + ROW_PAD_X + ROW_INDENT - GUIDE_ICON_GAP;
        let mut centers = Vec::new();
        let mut j = i + 1;
        while j < items.len() && matches!(items[j], SidebarItem::Row(_)) {
            centers.push(placed[j].1.center().y);
            j += 1;
        }
        if let Some(&last) = centers.last() {
            let spine_top = header_rect.center().y + HEADER_ICON_BOX / 2.0 + GUIDE_SPINE_GAP;
            painter.vline(guide_x, spine_top..=last, stroke);
            for &cy in &centers {
                painter.hline(guide_x..=tick_x, cy, stroke);
            }
        }
        i = j;
    }
}

/// Non-selectable project header (worktrees.md §1): the project name at the shared
/// sidebar indent, then a right-edge cluster — a `+` create-worktree button and the
/// collapse chevron in the slot the ⋯ overflow menu used to hold — with the
/// aggregate agent dot at the very edge when collapsed. Reveal/Copy and Remove from
/// sidebar live on the band's right-click menu. The whole band is the block drag
/// handle and toggles collapse on a body click.
fn project_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    header: &ProjectHeader,
    out: &mut SidebarAction,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(width, HEADER_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.dnd_set_drag_payload(DragRow(header.root));
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let cy = rect.center().y;

    // The aggregate dot only shows when the group is folded (the rows carry their
    // own dots when expanded); its column is reserved only then so the buttons do
    // not jump between states.
    let agent_col = if header.collapsed && header.agent != AgentBadge::None {
        HEADER_AGENT_COL_W
    } else {
        0.0
    };
    // The chevron and the `+` button are affordances revealed only while the header
    // is hovered; their columns stay reserved unconditionally so the name truncation
    // — and the chevron's resting place right after it — never shift on hover.
    let hovered = response.contains_pointer();
    let create_reserve = if header.can_create_worktree {
        CREATE_COL_W
    } else {
        0.0
    };
    let create_response = (header.can_create_worktree && hovered).then(|| {
        let center = egui::pos2(
            rect.right() - ROW_PAD_X - agent_col - CREATE_COL_W / 2.0,
            cy,
        );
        let response = ui
            .interact(
                egui::Rect::from_center_size(center, egui::vec2(PLUS_HIT, PLUS_HIT)),
                ui.id().with(("create_worktree", header.root)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Create worktree");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Create worktree")
        });
        response
    });

    // The project icon shares the row icons' column (the shared sidebar indent), so
    // header and worktree icons line up; the name follows in the label column.
    let icon_left = rect.left() + ROW_PAD_X;
    let icon_center = egui::pos2(icon_left + ICON_SIZE / 2.0, cy);
    let icon_box = egui::Rect::from_center_size(icon_center, egui::Vec2::splat(HEADER_ICON_BOX));
    ui.painter().rect_filled(
        icon_box,
        egui::CornerRadius::same(HEADER_ICON_BOX_RADIUS),
        crate::ui::agents_view::project_header_tint(palette, header.lane),
    );
    ui.painter().rect_stroke(
        icon_box,
        egui::CornerRadius::same(HEADER_ICON_BOX_RADIUS),
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    crate::ui::paint_icon(
        ui.painter(),
        icon_center,
        ICON_SIZE,
        lucide_icons::Icon::Folders,
        palette.text_secondary,
    );
    let name_left = icon_left + ICON_SIZE + ICON_GAP;
    // Reserve room after the name for the trailing chevron, plus the right-edge `+`
    // and aggregate-dot columns, so a long name truncates instead of colliding.
    let name_avail =
        (rect.right() - ROW_PAD_X - agent_col - create_reserve - CHEVRON_COL_W - name_left)
            .max(0.0);
    let mut job = egui::text::LayoutJob::single_section(
        header.name.to_owned(),
        egui::text::TextFormat::simple(
            egui::FontId::proportional(HEADER_NAME_SIZE),
            palette.text_secondary,
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(name_avail);
    let galley = ui.painter().layout_job(job);
    let name_width = galley.size().x;
    ui.painter().galley(
        egui::pos2(name_left, cy - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );

    if hovered {
        let chevron = if header.collapsed {
            lucide_icons::Icon::ChevronRight
        } else {
            lucide_icons::Icon::ChevronDown
        };
        let chevron_center = egui::pos2(
            name_left + name_width + HEADER_CHEVRON_GAP + CHEVRON_SIZE / 2.0,
            cy,
        );
        crate::ui::paint_icon(
            ui.painter(),
            chevron_center,
            CHEVRON_SIZE,
            chevron,
            palette.text_secondary,
        );
    }
    if let Some(create_response) = &create_response {
        let color = if create_response.hovered() {
            palette.text_secondary
        } else {
            palette.text_muted
        };
        crate::ui::paint_icon(
            ui.painter(),
            create_response.rect.center(),
            PLUS_SIZE,
            lucide_icons::Icon::Plus,
            color,
        );
    }
    if agent_col > 0.0 {
        paint_agent_dot(
            ui,
            palette,
            egui::pos2(rect.right() - ROW_PAD_X - AGENT_DOT_RADIUS - 2.0, cy),
            header.agent,
        );
    }

    let create_clicked = create_response
        .as_ref()
        .is_some_and(egui::Response::clicked);
    if create_clicked {
        out.create_worktree = Some(header.root);
    } else if response.clicked() {
        out.toggle_collapse = Some(header.root);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, header.name.to_owned())
    });
    let response = response.on_hover_text(if header.collapsed {
        "Expand worktrees"
    } else {
        "Collapse worktrees"
    });
    egui::Popup::context_menu(&response)
        .style(crate::theme::menu_style)
        .show(|ui| {
            if ui.button("Reveal in Finder").clicked() {
                out.reveal = Some(header.root);
                ui.close();
            }
            if ui.button("Copy path").clicked() {
                ui.ctx().copy_text(header.path.to_owned());
                ui.close();
            }
            ui.separator();
            // A rendered header is always a visible project — this only ever hides.
            if ui.button("Hide project").clicked() {
                out.toggle_hidden = Some(header.root);
                ui.close();
            }
            if ui.button("Remove from sidebar").clicked() {
                out.remove = Some(header.root);
                ui.close();
            }
        });
    response
}

/// Cross-repo Agents nav row pinned above the project list (specs/agents.md §5):
/// shown once the workspace has a project (hidden on the first-launch empty state,
/// where no agent can run), highlighted while the dashboard is open, badged at the
/// right edge by the workspace-wide max badge (spinner = working, green dot = done).
fn agents_entry(
    ui: &mut egui::Ui,
    palette: &Palette,
    badge: AgentBadge,
    is_active: bool,
    // While Cmd is held, the `⌃⌘0` shortcut badge takes the right column from the
    // activity badge — slot 0 of the positional family, like the repo rows' ⌃⌘N
    // (keybindings §5).
    cmd_held: bool,
    out: &mut SidebarAction,
) {
    let width = ui.available_width();
    let (rect, response, hovered) = crate::ui::clickable(ui, egui::vec2(width, ROW_HEIGHT), true);
    if is_active || hovered {
        paint_row_highlight(ui, palette, rect, is_active);
    }
    let color = if is_active {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let icon_left = rect.left() + ROW_PAD_X;
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(icon_left + ICON_SIZE / 2.0, rect.center().y),
        ICON_SIZE,
        lucide_icons::Icon::Bot,
        color,
    );
    let label = ui.painter().layout_no_wrap(
        "Agents".to_owned(),
        egui::FontId::proportional(NAME_SIZE),
        color,
    );
    ui.painter().galley(
        egui::pos2(
            icon_left + ICON_SIZE + ICON_GAP,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        color,
    );
    if cmd_held {
        let mut shortcut = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(rect.right() - ROW_PAD_X - BADGE_COL_W, rect.top()),
                    egui::pos2(rect.right() - ROW_PAD_X, rect.bottom()),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        shortcut.label(
            egui::RichText::new("⌃⌘0")
                .size(SHORTCUT_BADGE_SIZE)
                .color(palette.text_muted),
        );
    } else {
        let dot_center = egui::pos2(
            rect.right() - ROW_PAD_X - AGENT_DOT_RADIUS - 2.0,
            rect.center().y,
        );
        match badge {
            AgentBadge::Working => {
                paint_pinwheel(
                    ui,
                    dot_center,
                    AGENT_SPINNER_SIZE,
                    3,
                    &palette.lane_colors,
                    Some(palette.accent),
                );
            }
            AgentBadge::Done => {
                paint_done_dot(ui, dot_center, AGENT_DOT_RADIUS, palette.git_added);
            }
            AgentBadge::Idle | AgentBadge::None => {}
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, is_active, "Agents")
    });
    if response.clicked() {
        out.open_agents = true;
    }
}

/// Cross-repo Pull Requests nav row, pinned directly below the Agents entry
/// (specs/pull-requests.md §2): shown once the workspace has a project,
/// highlighted while the cockpit owns the central area, badged at the right edge
/// with the count of PRs awaiting my review (0 ⇒ no badge). Click only — no
/// keyboard shortcut, unlike Agents.
fn pull_requests_entry(
    ui: &mut egui::Ui,
    palette: &Palette,
    to_review: usize,
    is_active: bool,
    out: &mut SidebarAction,
) {
    const COUNT_BADGE_SIZE: f32 = 11.0;
    let width = ui.available_width();
    let (rect, response, hovered) = crate::ui::clickable(ui, egui::vec2(width, ROW_HEIGHT), true);
    if is_active || hovered {
        paint_row_highlight(ui, palette, rect, is_active);
    }
    let color = if is_active {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let icon_left = rect.left() + ROW_PAD_X;
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(icon_left + ICON_SIZE / 2.0, rect.center().y),
        ICON_SIZE,
        lucide_icons::Icon::GitPullRequest,
        color,
    );
    let label = ui.painter().layout_no_wrap(
        "Pull Requests".to_owned(),
        egui::FontId::proportional(NAME_SIZE),
        color,
    );
    ui.painter().galley(
        egui::pos2(
            icon_left + ICON_SIZE + ICON_GAP,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        color,
    );
    if to_review > 0 {
        let galley = ui.painter().layout_no_wrap(
            to_review.to_string(),
            egui::FontId::proportional(COUNT_BADGE_SIZE),
            palette.lane_node_text,
        );
        let text_size = galley.size();
        let pill_h = COUNT_BADGE_SIZE + 5.0;
        let pill_w = (text_size.x + 10.0).max(pill_h);
        let right = rect.right() - ROW_PAD_X;
        let pill = egui::Rect::from_min_max(
            egui::pos2(right - pill_w, rect.center().y - pill_h / 2.0),
            egui::pos2(right, rect.center().y + pill_h / 2.0),
        );
        ui.painter().rect_filled(pill, pill_h / 2.0, palette.accent);
        ui.painter().galley(
            pill.center() - text_size / 2.0,
            galley,
            palette.lane_node_text,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, is_active, "Pull Requests")
    });
    if response.clicked() {
        out.open_pull_requests = true;
    }
}

/// Indented child row under the Agents entry: one `Done`-state agent pane
/// (specs/agents.md §5). Clicking it focuses that pane — which acknowledges the
/// green, so the row clears on the next watch tick (an inbox of finished turns).
fn agent_child_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &DoneAgentRow,
    out: &mut SidebarAction,
) {
    let width = ui.available_width();
    let (rect, response, hovered) = crate::ui::clickable(ui, egui::vec2(width, ROW_HEIGHT), true);
    if hovered {
        paint_row_highlight(ui, palette, rect, false);
    }
    let icon_left = rect.left() + ROW_PAD_X + ROW_INDENT;
    let dot_center = egui::pos2(icon_left + ICON_SIZE / 2.0, rect.center().y);
    paint_done_dot(ui, dot_center, AGENT_DOT_RADIUS, palette.git_added);

    let label_left = icon_left + ICON_SIZE + ICON_GAP;
    let label_avail = (rect.right() - ROW_PAD_X - label_left).max(0.0);
    let text = match &row.branch {
        Some(branch) => format!("{branch} · {}", row.tab),
        None => row.tab.clone(),
    };
    let label = text.clone();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
    });
    let mut job = egui::text::LayoutJob::single_section(
        text,
        egui::text::TextFormat::simple(
            egui::FontId::proportional(NAME_SIZE),
            palette.text_secondary,
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(label_avail);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(label_left, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );

    if response.clicked() {
        out.focus_agent = Some(row.index);
    }
}

/// Insertion indicator drawn during a drag: an accent line at the top edge of the
/// row sitting at `insert_at`, or below the last row when the block lands at the end.
fn draw_drop_line(
    ui: &egui::Ui,
    palette: &Palette,
    placed: &[(usize, egui::Rect)],
    insert_at: usize,
) {
    let Some((_, first)) = placed.first() else {
        return;
    };
    let y = placed
        .iter()
        .find(|(index, _)| *index == insert_at)
        .map(|(_, rect)| rect.top())
        .or_else(|| placed.last().map(|(_, rect)| rect.bottom()))
        .unwrap_or(first.top());
    ui.painter().hline(
        first.left()..=first.right(),
        y,
        egui::Stroke::new(2.0, palette.accent),
    );
}

fn sidebar_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    projects: &[ProjectVisibility],
    show_add: bool,
    cmd_held: bool,
    open_folder: Option<&str>,
    out: &mut SidebarAction,
) {
    ui.add_space(SECTION_TOP_MARGIN);
    ui.horizontal(|ui| {
        ui.add_space(ROW_PAD_X);
        ui.label(crate::ui::section_label(palette, "Projects"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Empty state: the labelled "Open Folder…" nav row right below is the add
            // affordance, so the redundant header "+" is dropped until a project exists.
            if show_add {
                let (rect, plus, hovered) =
                    crate::ui::clickable(ui, egui::vec2(PLUS_HIT, PLUS_HIT), true);
                let color = if hovered {
                    palette.text_secondary
                } else {
                    palette.text_muted
                };
                crate::ui::paint_icon(
                    ui.painter(),
                    rect.center(),
                    PLUS_SIZE,
                    lucide_icons::Icon::Plus,
                    color,
                );
                let hover = match open_folder {
                    Some(badge) => format!("Add a project · {badge}"),
                    None => "Add a project".to_owned(),
                };
                let plus = plus.on_hover_text(hover);
                plus.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add a project")
                });
                if plus.clicked() {
                    out.open = true;
                }
                if let Some(badge) = open_folder.filter(|_| cmd_held) {
                    ui.label(
                        egui::RichText::new(badge)
                            .size(SHORTCUT_BADGE_SIZE)
                            .color(palette.text_muted),
                    );
                }
            }
            // Left of the `+`: the eye toggling project visibility. Its dropdown is
            // the only place a hidden project still shows, so it lists every project —
            // hidden ones included — even when the sidebar list is empty.
            if !projects.is_empty() {
                let (rect, eye, hovered) =
                    crate::ui::clickable(ui, egui::vec2(PLUS_HIT, PLUS_HIT), true);
                let color = if hovered {
                    palette.text_secondary
                } else {
                    palette.text_muted
                };
                crate::ui::paint_icon(
                    ui.painter(),
                    rect.center(),
                    EYE_SIZE,
                    lucide_icons::Icon::Eye,
                    color,
                );
                let eye = eye.on_hover_text("Show or hide projects");
                eye.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Show or hide projects",
                    )
                });
                egui::Popup::menu(&eye)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .style(crate::theme::menu_style)
                    .show(|ui| {
                        for project in projects {
                            let mut visible = !project.hidden;
                            if ui.checkbox(&mut visible, project.name).changed() {
                                out.toggle_hidden = Some(project.root);
                            }
                        }
                    });
            }
        });
    });
}

/// Empty list: the only available action rendered as a real nav row
/// (design-system §4 "sidebar nav item"), not a muted label-looking link. The
/// shortcut hint stays visible (teaching empty state, like the central hint).
fn open_folder_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    open_folder: Option<&str>,
    out: &mut SidebarAction,
) {
    let width = ui.available_width();
    let (rect, response, hovered) = crate::ui::clickable(ui, egui::vec2(width, ROW_HEIGHT), true);
    if hovered {
        paint_row_highlight(ui, palette, rect, false);
    }
    let icon_left = rect.left() + ROW_PAD_X;
    let icon_galley = ui.painter().layout_no_wrap(
        lucide_icons::Icon::FolderPlus.unicode().to_string(),
        egui::FontId::proportional(ICON_SIZE),
        palette.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(icon_left, rect.center().y - icon_galley.size().y / 2.0),
        icon_galley,
        palette.text_secondary,
    );
    let label = ui.painter().layout_no_wrap(
        "Open Folder…".to_owned(),
        egui::FontId::proportional(NAME_SIZE),
        palette.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(
            icon_left + ICON_SIZE + ICON_GAP,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        palette.text_secondary,
    );
    if let Some(badge) = open_folder {
        let badge = ui.painter().layout_no_wrap(
            badge.to_owned(),
            egui::FontId::proportional(SHORTCUT_BADGE_SIZE),
            palette.text_muted,
        );
        ui.painter().galley(
            egui::pos2(
                rect.right() - ROW_PAD_X - badge.size().x,
                rect.center().y - badge.size().y / 2.0,
            ),
            badge,
            palette.text_muted,
        );
    }
    let info = match open_folder {
        Some(badge) => format!("Open Folder… · {badge}"),
        None => "Open Folder…".to_owned(),
    };
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, info.clone()));
    if response.clicked() {
        out.open = true;
    }
}

pub fn row_text_color(palette: &Palette, is_active: bool, missing: bool) -> egui::Color32 {
    if is_active {
        palette.text_primary
    } else if missing {
        palette.text_muted
    } else {
        palette.text_secondary
    }
}

pub fn repo_icon_color(palette: &Palette, is_active: bool, missing: bool) -> egui::Color32 {
    if is_active && !missing {
        palette.accent
    } else if !is_active && missing {
        palette.text_muted
    } else {
        palette.text_secondary
    }
}

fn hover_text(row: &RepoRow) -> String {
    let location = match row.branch {
        Some(branch) => format!("{branch}\n{}", row.path),
        None => row.path.to_owned(),
    };
    let base = if row.deleting {
        format!("{location}\nDeleting worktree…")
    } else if row.missing {
        format!("{location}\nFolder not found")
    } else if let Some((additions, deletions)) = row.stats {
        if additions > 0 || deletions > 0 {
            format!("{location}\n+{additions} −{deletions}")
        } else {
            format!("{location}\nUncommitted changes")
        }
    } else {
        location
    };
    match row.agent {
        AgentBadge::Working => format!("{base}\nAgent working…"),
        AgentBadge::Done => format!("{base}\nAgent finished"),
        AgentBadge::Idle | AgentBadge::None => base,
    }
}

/// Visible single-line label of the row: the branch, or the folder name when no
/// branch is readable (worktrees.md §3).
fn row_text<'a>(row: &RepoRow<'a>) -> &'a str {
    row.branch.unwrap_or(row.name)
}

/// Accessibility label of the row: the root reads as its branch; a linked
/// worktree reads as its folder name followed by its branch caption (the two
/// visible lines, worktrees.md §3). Suffixed with the uncommitted-changes marker
/// and the agent dot's state when they are visible (assertable in e2e UI).
pub fn row_label(row: &RepoRow) -> String {
    let base = match (!row.main).then_some(row.branch).flatten() {
        Some(branch) => format!("{} · {branch}", row.name),
        None => row_text(row).to_owned(),
    };
    let base = match row.stats {
        Some((additions, deletions)) if additions > 0 || deletions > 0 => {
            format!(
                "{base} · +{} −{}",
                abbrev_count(additions),
                abbrev_count(deletions)
            )
        }
        Some(_) => format!("{base} · uncommitted"),
        None => base,
    };
    match row.agent {
        AgentBadge::None => base,
        AgentBadge::Idle => format!("{base} · agent idle"),
        AgentBadge::Done => format!("{base} · agent done"),
        AgentBadge::Working => format!("{base} · agent working"),
    }
}

/// Compact line count for the row caption: thousands as `1.2k`, millions as `1.2M`
/// (one decimal, dropped when it would be `.0`, and dropped once the value reaches
/// two digits). Keeps the right-edge indicator narrow on a big diff.
fn abbrev_count(n: usize) -> String {
    fn scaled(n: usize, unit: f32, suffix: char) -> String {
        let v = n as f32 / unit;
        // One decimal below 10 (`1.2k`), whole otherwise (`16k`); a `.0` that survives
        // rounding (`1000 → 1.0`, `9999 → 10.0`) is dropped.
        let mag = if v >= 10.0 {
            format!("{}", v.round() as usize)
        } else {
            format!("{v:.1}")
        };
        let mag = mag.strip_suffix(".0").unwrap_or(&mag);
        format!("{mag}{suffix}")
    }
    if n >= 1_000_000 {
        scaled(n, 1_000_000.0, 'M')
    } else if n >= 1_000 {
        scaled(n, 1_000.0, 'k')
    } else {
        n.to_string()
    }
}

/// Lays out the tiny `+N −M` caption (abbreviated, green additions / red deletions),
/// each half kept only when non-zero. `None` when both are zero — the caller then
/// paints the dot fallback. The galley carries its own per-section colors.
fn stat_caption(
    ui: &egui::Ui,
    palette: &Palette,
    additions: usize,
    deletions: usize,
) -> Option<std::sync::Arc<egui::Galley>> {
    if additions == 0 && deletions == 0 {
        return None;
    }
    let font = egui::FontId::proportional(STAT_SIZE);
    let mut job = egui::text::LayoutJob::default();
    if additions > 0 {
        job.append(
            &format!("+{}", abbrev_count(additions)),
            0.0,
            egui::TextFormat::simple(font.clone(), palette.git_added),
        );
    }
    if deletions > 0 {
        let lead = if additions > 0 { STAT_GAP } else { 0.0 };
        job.append(
            &format!("−{}", abbrev_count(deletions)),
            lead,
            egui::TextFormat::simple(font, palette.git_deleted),
        );
    }
    Some(ui.painter().layout_job(job))
}

/// Paints the fixed-width green/red ratio bar with its right edge at `right` and its
/// top at `top` (mirrors the git panel's `ratio_bar`, scaled down for the sidebar).
fn paint_stat_bar(
    ui: &egui::Ui,
    palette: &Palette,
    right: f32,
    top: f32,
    additions: usize,
    deletions: usize,
) {
    let rect = egui::Rect::from_min_size(
        egui::pos2(right - STAT_BAR_W, top),
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

/// Agent activity indicator at `center` (specs/agents.md): a colored pinwheel
/// while working, a pulsing green dot when finished, a hollow muted ring when
/// idle. Drawn over the row icon slot, or at the project-header aggregate edge.
fn paint_agent_dot(ui: &mut egui::Ui, palette: &Palette, center: egui::Pos2, badge: AgentBadge) {
    match badge {
        AgentBadge::Working => {
            paint_pinwheel(
                ui,
                center,
                AGENT_SPINNER_SIZE,
                3,
                &palette.lane_colors,
                Some(palette.accent),
            );
        }
        AgentBadge::Done => {
            paint_done_dot(ui, center, AGENT_DOT_RADIUS, palette.git_added);
        }
        AgentBadge::Idle => {
            ui.painter().circle_stroke(
                center,
                AGENT_DOT_RADIUS,
                egui::Stroke::new(ICON_RING_WIDTH, palette.text_muted),
            );
        }
        AgentBadge::None => {}
    }
}

/// Full-bleed row highlight (workspace sidebar): the fill spans the whole panel
/// width — bleeding into the horizontal padding, square corners — and the active
/// row carries a primary bar down its left edge. The painter clip is widened by the
/// sidebar pad so the fill reaches the panel edges (kept tight vertically so a
/// scrolled-out row stays clipped).
fn paint_row_highlight(ui: &egui::Ui, palette: &Palette, rect: egui::Rect, active: bool) {
    let bleed = f32::from(super::SIDEBAR_PAD_X);
    let clip = ui.clip_rect();
    let wide = egui::Rect::from_min_max(
        egui::pos2(clip.left() - bleed, clip.top()),
        egui::pos2(clip.right() + bleed, clip.bottom()),
    );
    let full = egui::Rect::from_min_max(
        egui::pos2(rect.left() - bleed, rect.top()),
        egui::pos2(rect.right() + bleed, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(wide);
    painter.rect_filled(full, egui::CornerRadius::ZERO, palette.bg_surface);
    if active {
        painter.vline(
            full.left() + ROW_ACCENT_BAR / 2.0,
            full.top()..=full.bottom(),
            egui::Stroke::new(ROW_ACCENT_BAR, palette.accent),
        );
    }
}

fn repo_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: &RepoRow,
    is_active: bool,
    cmd_digits: bool,
    // Visible-order position for the `⌃⌘N` badge (None past the 9th slot); the row
    // index no longer maps to it once groups fold (worktrees.md §7).
    shortcut: Option<usize>,
    out: &mut SidebarAction,
) -> egui::Response {
    let width = ui.available_width();
    // The main worktree is a fixed slot (never reordered, worktrees.md §3): still
    // selectable, but not a drag source. A linked worktree drags to reorder; a
    // deleting row is inert.
    let draggable = !row.deleting && !row.main;
    let sense = if row.deleting {
        egui::Sense::hover()
    } else if draggable {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::click()
    };
    // A linked worktree shows two lines (folder name + branch caption, worktrees.md
    // §3); the root stays a single branch line. A worktree with no readable branch
    // falls back to the single name line.
    let subtitle = (!row.main).then_some(row.branch).flatten();
    let row_height = if subtitle.is_some() {
        ROW_HEIGHT_TWO_LINE
    } else {
        ROW_HEIGHT
    };
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, row_height), sense);
    if !row.deleting {
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    if draggable {
        response.dnd_set_drag_payload(DragRow(row.index));
    }
    let hovered = !row.deleting && response.hovered();
    let dragged = response.dragged();
    if dragged {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let text_color = if row.deleting {
        palette.text_muted
    } else {
        row_text_color(palette, is_active, row.missing)
    };
    let icon_color = repo_icon_color(palette, is_active, row.missing);

    if is_active || hovered || dragged {
        paint_row_highlight(ui, palette, rect, is_active);
    }

    if response.clicked() && !row.deleting {
        out.select = Some(row.index);
    }

    // Rows sit a touch further in than the project header (ROW_INDENT) so they read
    // as nested under it. A plain folder marks the main worktree; a git-folder glyph
    // marks a linked worktree (worktrees.md §3).
    let icon_left = rect.left() + ROW_PAD_X + ROW_INDENT;
    let icon_center = egui::pos2(icon_left + ICON_SIZE / 2.0, rect.center().y);
    if row.deleting {
        let spinner_rect =
            egui::Rect::from_center_size(icon_center, egui::vec2(ICON_SIZE, ICON_SIZE));
        ui.put(
            spinner_rect,
            Spinner::new().size(ICON_SIZE).color(icon_color),
        );
    } else if row.agent != AgentBadge::None {
        paint_agent_dot(ui, palette, icon_center, row.agent);
    } else {
        let icon = if row.main {
            lucide_icons::Icon::Folder
        } else {
            lucide_icons::Icon::FolderGit2
        };
        crate::ui::paint_icon(ui.painter(), icon_center, ICON_SIZE, icon, icon_color);
    }

    let label_left = icon_left + ICON_SIZE + ICON_GAP;
    // The bar/caption can be wider than the ⌃⌘N badge; reserve the widest of the
    // three (a per-row constant) so the label truncation — and the row's right edge —
    // holds still when Cmd toggles between the indicator and the shortcut badge.
    let caption = row
        .stats
        .filter(|_| !row.deleting)
        .and_then(|(additions, deletions)| stat_caption(ui, palette, additions, deletions));
    let badge_col = match &caption {
        Some(g) => g.size().x.max(STAT_BAR_W).max(BADGE_COL_W),
        None => BADGE_COL_W,
    };
    let label_avail = (rect.right() - ROW_PAD_X - badge_col - label_left).max(0.0);
    let truncated = |text: String, font: egui::FontId, color: egui::Color32| {
        let mut job = egui::text::LayoutJob::single_section(
            text,
            egui::text::TextFormat::simple(font, color),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(label_avail);
        ui.painter().layout_job(job)
    };
    if let Some(branch) = subtitle {
        // Linked worktree: folder name on top, branch caption beneath, the pair
        // vertically centered in the row (worktrees.md §3).
        let name = truncated(
            row.name.to_owned(),
            egui::FontId::proportional(NAME_SIZE),
            text_color,
        );
        let caption = truncated(
            branch.to_owned(),
            egui::FontId::monospace(BRANCH_SIZE),
            palette.text_muted,
        );
        let block = name.size().y + TWO_LINE_GAP + caption.size().y;
        let top = rect.center().y - block / 2.0;
        let name_h = name.size().y;
        ui.painter()
            .galley(egui::pos2(label_left, top), name, text_color);
        ui.painter().galley(
            egui::pos2(label_left, top + name_h + TWO_LINE_GAP),
            caption,
            palette.text_muted,
        );
    } else {
        let galley = truncated(
            row_text(row).to_owned(),
            egui::FontId::proportional(NAME_SIZE),
            text_color,
        );
        ui.painter().galley(
            egui::pos2(label_left, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
    }

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, is_active, row_label(row))
    });

    if let Some(shortcut) = shortcut.filter(|_| cmd_digits) {
        let mut badge = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(rect.right() - ROW_PAD_X - BADGE_COL_W, rect.top()),
                    egui::pos2(rect.right() - ROW_PAD_X, rect.bottom()),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        badge.label(
            egui::RichText::new(format!("⌃⌘{}", shortcut + 1))
                .size(SHORTCUT_BADGE_SIZE)
                .color(palette.text_muted),
        );
    } else if !cmd_digits && !row.deleting {
        if let (Some(galley), Some((additions, deletions))) = (caption, row.stats) {
            // Bar stacked over the tiny caption, the pair right-aligned and vertically
            // centered in the row.
            let right = rect.right() - ROW_PAD_X;
            let block_h = STAT_BAR_H + STAT_BAR_TEXT_GAP + galley.size().y;
            let top = rect.center().y - block_h / 2.0;
            paint_stat_bar(ui, palette, right, top, additions, deletions);
            ui.painter().galley(
                egui::pos2(
                    right - galley.size().x,
                    top + STAT_BAR_H + STAT_BAR_TEXT_GAP,
                ),
                galley,
                palette.text_primary,
            );
        } else if row.stats.is_some() {
            ui.painter().circle_filled(
                egui::pos2(
                    rect.right() - ROW_PAD_X - DIRTY_DOT_RADIUS - 2.0,
                    rect.center().y,
                ),
                DIRTY_DOT_RADIUS,
                palette.git_modified,
            );
        }
    }

    let response = response.on_hover_text(hover_text(row));
    if !row.deleting {
        egui::Popup::context_menu(&response)
            .style(crate::theme::menu_style)
            .show(|ui| {
                if ui.button("Reveal in Finder").clicked() {
                    out.reveal = Some(row.index);
                    ui.close();
                }
                if ui.button("Copy path").clicked() {
                    ui.ctx().copy_text(row.path.to_owned());
                    ui.close();
                }
                // Remove from sidebar lives on the project header (it drops the
                // whole group); a linked worktree only offers Delete (discovery
                // would bring back a mere hide, worktrees.md §6).
                if !row.main {
                    ui.separator();
                    if ui.button("Delete worktree from disk").clicked() {
                        out.delete_worktree = Some(row.index);
                        ui.close();
                    }
                }
            });
    }
    response
}

const CREATE_MODAL_WIDTH: f32 = 420.0;
const SOURCE_LIST_MAX_HEIGHT: f32 = 180.0;
const FILTER_HINT: &str = "Filter branches…";

pub fn create_worktree_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    prompt: &CreateWorktreePrompt<'_>,
    state: &mut CreateWorktreeState,
    out: &mut CreateWorktreeModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("create_worktree_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(CREATE_MODAL_WIDTH);
            ui.label(
                egui::RichText::new(format!("Create worktree for “{}”", prompt.root_label))
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Pick the source branch to check out in the new worktree.")
                    .color(palette.text_secondary),
            );
            ui.add_space(12.0);

            let selection = if prompt.loading {
                ui.horizontal(|ui| {
                    ui.add(Spinner::new().size(14.0).color(palette.text_secondary));
                    ui.label(
                        egui::RichText::new("Loading branches…").color(palette.text_secondary),
                    );
                });
                None
            } else {
                source_section(ui, palette, prompt, state, out)
            };

            if let Some(error) = prompt.error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(error).color(palette.git_deleted));
            }

            let mut destination_ok = false;
            if let Some(follow) = selection.and_then(|sel| follow_name(prompt, state, sel)) {
                destination_ok =
                    name_section(ui, palette, prompt.root, prompt.base, &follow, state);
            }

            ui.add_space(16.0);
            let enabled = destination_ok && !prompt.busy;
            if enabled && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out.create = true;
            }
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = egui::Button::new(
                        egui::RichText::new("Create worktree").color(egui::Color32::WHITE),
                    )
                    .fill(palette.primary_button_fill());
                    if ui.add_enabled(enabled, button).clicked() {
                        out.create = true;
                    }
                    if prompt.busy {
                        ui.add(Spinner::new().size(14.0).color(palette.text_secondary));
                    }
                });
            });
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Branch name the worktree folder follows for the current selection: the
/// source's local branch, or the typed query for the on-the-fly new branch.
fn follow_name(
    prompt: &CreateWorktreePrompt<'_>,
    state: &CreateWorktreeState,
    selection: CreateSelection,
) -> Option<String> {
    match selection {
        CreateSelection::Source(index) => prompt
            .sources
            .get(index)
            .map(|source| source.local_branch.clone()),
        CreateSelection::NewBranch => Some(state.query.trim().to_owned()),
    }
}

/// The trimmed query qualifies as an on-the-fly new branch (worktrees.md §6): a
/// valid branch path that no existing branch already uses (case-insensitive,
/// `taken` is pre-lowercased). Returns the trimmed name to display in the row.
fn new_branch_candidate<'q>(
    query: &'q str,
    root: &Path,
    base: Option<&Path>,
    taken: &HashSet<String>,
) -> Option<&'q str> {
    let name = query.trim();
    if name.is_empty() || path_for_branch(root, name, base).is_err() {
        return None;
    }
    (!taken.contains(&name.to_lowercase())).then_some(name)
}

/// Autocomplete filter that doubles as the new-branch name field: the merged
/// branch list (local and remote, the `origin/` prefix tells them apart) plus,
/// when the query is an eligible new-branch name, a pinned "Create branch …"
/// row below the list and outside the scroll area — last in keyboard order.
/// Returns the selection highlighted this frame; emits `out.select` whenever it
/// drifts from the app-side selection (click, ↑/↓, or the filter hid the choice).
fn source_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    prompt: &CreateWorktreePrompt<'_>,
    state: &mut CreateWorktreeState,
    out: &mut CreateWorktreeModalAction,
) -> Option<CreateSelection> {
    let response = ui.add(
        egui::TextEdit::singleline(&mut state.query)
            .hint_text(egui::RichText::new(FILTER_HINT).color(palette.text_muted))
            .desired_width(f32::INFINITY),
    );
    if !state.focused {
        state.focused = true;
        response.request_focus();
    }

    let query = state.query.trim().to_lowercase();
    let visible: Vec<usize> = prompt
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| query.is_empty() || source.name.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect();
    let candidate = (!prompt.base_branch.is_empty())
        .then(|| new_branch_candidate(&state.query, prompt.root, prompt.base, prompt.taken))
        .flatten();

    // Keyboard order: the visible sources, then the new-branch row last so Enter
    // on a match never creates a branch by accident (worktrees.md §6).
    let mut items: Vec<CreateSelection> = visible
        .iter()
        .map(|&index| CreateSelection::Source(index))
        .collect();
    if candidate.is_some() {
        items.push(CreateSelection::NewBranch);
    }

    let mut highlighted = prompt
        .selected
        .filter(|selection| items.contains(selection))
        .or_else(|| items.first().copied());
    let mut scroll_position = None;
    if let (Some(nav), Some(current)) = (arrow_nav_pressed(ui), highlighted) {
        let position = items.iter().position(|s| *s == current).unwrap_or(0);
        let next = match nav {
            ArrowNav::Up => position.saturating_sub(1),
            ArrowNav::Down => (position + 1).min(items.len() - 1),
        };
        highlighted = Some(items[next]);
        // The new-branch row is pinned outside the scroll area; only source rows scroll.
        scroll_position = matches!(items[next], CreateSelection::Source(_)).then_some(next);
    }
    if highlighted != prompt.selected {
        out.select = highlighted;
    }

    ui.add_space(6.0);
    let row_height =
        ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().button_padding.y;
    egui::Frame::new()
        .fill(palette.bg_surface)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            if prompt.sources.is_empty() {
                ui.label(
                    egui::RichText::new("No branch is available — type a name to create one.")
                        .color(palette.text_muted),
                );
                return;
            }
            if visible.is_empty() {
                ui.label(
                    egui::RichText::new(format!("No branch matches “{}”.", state.query.trim()))
                        .color(palette.text_muted),
                );
                return;
            }
            // Virtualized: only the rows in the viewport are laid out (a repo
            // can expose hundreds of remote branches).
            let output = egui::ScrollArea::vertical()
                .id_salt("create_worktree_sources")
                .max_height(SOURCE_LIST_MAX_HEIGHT)
                .auto_shrink([false, true])
                .show_rows(ui, row_height, visible.len(), |ui, range| {
                    for &index in &visible[range] {
                        let source = &prompt.sources[index];
                        let row = ui.add(
                            egui::Button::selectable(
                                highlighted == Some(CreateSelection::Source(index)),
                                source.name.as_str(),
                            )
                            .truncate(),
                        );
                        if row.clicked() {
                            out.select = Some(CreateSelection::Source(index));
                        }
                    }
                });
            if let Some(position) = scroll_position {
                scroll_row_into_view(ui, position, row_height, output);
            }
        });

    if let Some(name) = candidate {
        ui.add_space(6.0);
        let row = ui.add(
            egui::Button::selectable(
                highlighted == Some(CreateSelection::NewBranch),
                format!("Create branch “{name}” from {}", prompt.base_branch),
            )
            .truncate(),
        );
        if row.clicked() {
            out.select = Some(CreateSelection::NewBranch);
        }
    }

    highlighted
}

/// Worktree-name input pre-filled with the selected branch (it follows the
/// selection until the user types a custom name; clearing the field resumes
/// the follow), then the destination preview or the validation error. Returns
/// whether the destination is valid (gates the Create button).
fn name_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    root: &Path,
    base: Option<&Path>,
    follow: &str,
    state: &mut CreateWorktreeState,
) -> bool {
    if !state.name_edited && state.name != follow {
        state.name = follow.to_owned();
    }
    ui.add_space(10.0);
    ui.label(egui::RichText::new("Worktree name").color(palette.text_muted));
    let response = ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(f32::INFINITY));
    if response.changed() {
        state.name_edited = !state.name.trim().is_empty();
    }

    let name = state.effective_name(follow);
    match path_for_branch(root, name, base) {
        Ok(path) => {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Location").color(palette.text_muted));
            ui.label(
                egui::RichText::new(path.display().to_string())
                    .monospace()
                    .color(palette.text_secondary),
            );
            true
        }
        Err(_) => {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("“{name}” cannot be used as a worktree folder."))
                    .color(palette.git_deleted),
            );
            false
        }
    }
}

/// Keeps the keyboard selection visible: `show_rows` never lays out off-screen
/// rows, so `scroll_to_me` can't reach them — clamp the stored offset instead.
fn scroll_row_into_view(
    ui: &egui::Ui,
    position: usize,
    row_height: f32,
    output: egui::scroll_area::ScrollAreaOutput<()>,
) {
    let top = position as f32 * (row_height + ui.spacing().item_spacing.y);
    let bottom = top + row_height;
    let view = output.inner_rect.height();
    let mut state = output.state;
    let clamped = state.offset.y.clamp((bottom - view).max(0.0), top.max(0.0));
    if clamped != state.offset.y {
        state.offset.y = clamped;
        state.store(ui.ctx(), output.id);
        ui.ctx().request_repaint();
    }
}

pub fn delete_worktree_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    prompt: &DeletePrompt,
    out: &mut DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("delete_worktree_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            match prompt {
                DeletePrompt::Dirty { label, files } => {
                    ui.label(egui::RichText::new(format!("Delete worktree “{label}”?")).strong());
                    ui.add_space(4.0);
                    let plural = if *files > 1 { "s" } else { "" };
                    ui.label(
                        egui::RichText::new(format!(
                            "{files} file{plural} with uncommitted changes"
                        ))
                        .color(palette.text_secondary),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            out.dismiss = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(crate::ui::danger_button(palette, "Delete anyway"))
                                .clicked()
                            {
                                out.confirm = true;
                            }
                        });
                    });
                    if crate::ui::modal_confirm_pressed(ui) {
                        out.confirm = true;
                    }
                }
                DeletePrompt::Refused { label, reason } => {
                    ui.label(egui::RichText::new(format!("Cannot delete “{label}”")).strong());
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(reason).color(palette.text_secondary));
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            out.dismiss = true;
                        }
                    });
                    if crate::ui::modal_confirm_pressed(ui) {
                        out.dismiss = true;
                    }
                }
            }
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_branch_candidate_requires_a_valid_unused_name() {
        let root = Path::new("/tmp/main");
        let taken = HashSet::from(["feat/toto".to_owned(), "fix/a".to_owned()]);

        assert_eq!(new_branch_candidate("   ", root, None, &taken), None);
        assert_eq!(new_branch_candidate("../escape", root, None, &taken), None);
        assert_eq!(new_branch_candidate("feat/toto", root, None, &taken), None);
        assert_eq!(
            new_branch_candidate("FIX/A", root, None, &taken),
            None,
            "the collision is case-insensitive"
        );
        assert_eq!(
            new_branch_candidate("  feat/new  ", root, None, &taken),
            Some("feat/new"),
            "a fresh name is trimmed and offered"
        );
    }

    #[test]
    fn active_row_uses_primary_text() {
        let p = Palette::light();
        assert_eq!(row_text_color(&p, true, false), p.text_primary);
        assert_eq!(
            row_text_color(&p, true, true),
            p.text_primary,
            "active wins even when the path is missing"
        );
    }

    #[test]
    fn missing_row_is_greyed_when_inactive() {
        let p = Palette::light();
        assert_eq!(row_text_color(&p, false, true), p.text_muted);
    }

    #[test]
    fn present_inactive_row_uses_secondary_text() {
        let p = Palette::light();
        assert_eq!(row_text_color(&p, false, false), p.text_secondary);
    }

    #[test]
    fn icon_color_distinguishes_active_and_missing_from_a_plain_repo() {
        let p = Palette::light();
        assert_eq!(
            repo_icon_color(&p, true, false),
            p.accent,
            "an active repo icon takes the accent"
        );
        assert_eq!(
            repo_icon_color(&p, false, true),
            p.text_muted,
            "a missing repo icon is de-emphasised"
        );
        assert_eq!(repo_icon_color(&p, false, false), p.text_secondary);
    }

    fn plain_row(name: &str) -> RepoRow<'_> {
        RepoRow {
            index: 0,
            name,
            path: "/tmp/x",
            missing: false,
            main: false,
            branch: None,
            deleting: false,
            agent: AgentBadge::None,
            stats: None,
        }
    }

    #[test]
    fn dirty_row_announces_its_line_stats() {
        let dirty = RepoRow {
            stats: Some((46, 1)),
            ..plain_row("x")
        };
        assert_eq!(row_label(&dirty), "x · +46 −1");
        assert!(hover_text(&dirty).contains("+46 −1"));

        // Dirty with no countable lines (e.g. an empty new file): the diffstat would
        // be empty, so the row falls back to the bare "uncommitted" marker.
        let no_lines = RepoRow {
            stats: Some((0, 0)),
            ..plain_row("x")
        };
        assert_eq!(row_label(&no_lines), "x · uncommitted");
        assert!(hover_text(&no_lines).contains("Uncommitted changes"));

        // Big counts are abbreviated in the visible label; the hover keeps the exact
        // figures.
        let big = RepoRow {
            stats: Some((1234, 15)),
            ..plain_row("x")
        };
        assert_eq!(row_label(&big), "x · +1.2k −15");
        assert!(hover_text(&big).contains("+1234 −15"));

        assert_eq!(row_label(&plain_row("x")), "x", "a clean row stays bare");
    }

    #[test]
    fn abbrev_count_scales_thousands_and_millions() {
        assert_eq!(abbrev_count(0), "0");
        assert_eq!(abbrev_count(999), "999");
        assert_eq!(abbrev_count(1000), "1k");
        assert_eq!(abbrev_count(1234), "1.2k");
        assert_eq!(abbrev_count(9949), "9.9k");
        assert_eq!(abbrev_count(15500), "16k");
        assert_eq!(abbrev_count(1_000_000), "1M");
        assert_eq!(abbrev_count(2_500_000), "2.5M");
    }

    #[test]
    fn hover_text_leads_with_the_current_branch() {
        let on_branch = RepoRow {
            branch: Some("feature-x"),
            ..plain_row("x")
        };
        assert_eq!(hover_text(&on_branch), "feature-x\n/tmp/x");
    }

    #[test]
    fn hover_text_explains_the_missing_state() {
        let missing = RepoRow {
            missing: true,
            ..plain_row("x")
        };
        assert!(hover_text(&missing).contains("Folder not found"));
        let git = plain_row("z");
        assert_eq!(hover_text(&git), "/tmp/x");
    }

    #[test]
    fn hover_text_and_label_reflect_the_agent_badge() {
        let working = RepoRow {
            agent: AgentBadge::Working,
            ..plain_row("x")
        };
        assert!(hover_text(&working).contains("Agent working"));
        assert_eq!(row_label(&working), "x · agent working");

        let done = RepoRow {
            agent: AgentBadge::Done,
            ..plain_row("x")
        };
        assert!(hover_text(&done).contains("Agent finished"));
        assert_eq!(row_label(&done), "x · agent done");

        let idle = RepoRow {
            agent: AgentBadge::Idle,
            ..plain_row("x")
        };
        assert_eq!(hover_text(&idle), "/tmp/x", "idle adds no hover line");
        assert_eq!(row_label(&idle), "x · agent idle");

        assert_eq!(row_label(&plain_row("x")), "x");
    }
}
