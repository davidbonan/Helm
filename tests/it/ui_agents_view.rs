//! UI E2E for the cross-repo agents dashboard (specs/agents.md §5): drives
//! `agents_page` headless and checks the List cockpit (grouped rows, empty state,
//! row-body → select vs jump-icon → focus, the panel mirror), the List/Terminals view
//! switch, and the Terminals **wall** — the header chip per running agent, the toggle
//! that puts one on the wall or takes it off, the cap of four, the mirrored terminal per
//! tile, and the split-tree gestures (seam resize, grip drop) it reports back.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::agents_wall::AgentWall;
use helm::terminal::layout::{Dir, PaneId, Rect};
use helm::theme::Palette;
use helm::ui::agents_view::{agents_page, AgentRow, AgentsViewMode, WallView};
use helm::ui::terminal_view::{DropZone, PaneDrop, ResizeDrag};

const WINDOW: egui::Vec2 = egui::vec2(1500.0, 1200.0);
/// Nominal wall area the test wall splits with — only its proportions matter, and they
/// match the window the harness renders in.
const AREA: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1500.0,
    h: 1140.0,
};
/// Center of the tile's drag grip, measured off its band: the tree paints it at the
/// tile's top-right corner, mirroring GRIP_W / GRIP_TOP / GRIP_H in `terminal_view`
/// (26 wide, inset 3, 14 tall).
fn grip_pos(band: egui::Rect) -> egui::Pos2 {
    egui::pos2(band.right() - 16.0, band.top() + 10.0)
}
/// The band's jump icon, clear of the grip: `GRIP_RESERVE` (32) + `CARD_JUMP_PAD` (14)
/// + half the 15px glyph in from the band's right edge.
const JUMP_INSET: f32 = 53.5;

#[derive(Default)]
struct Captured {
    select: Cell<Option<usize>>,
    jump: Cell<Option<usize>>,
    set_view: Cell<Option<AgentsViewMode>>,
    toggle: Cell<Option<usize>>,
    resize: Cell<Option<ResizeDrag>>,
    drop: Cell<Option<PaneDrop>>,
    drawn: RefCell<Vec<usize>>,
    /// Rect of each mirrored terminal drawn, by row — the tile geometry a test asserts.
    term_rects: RefCell<Vec<(usize, egui::Rect)>>,
}

impl Captured {
    fn drawn_rows(&self) -> Vec<usize> {
        let mut drawn = self.drawn.borrow().clone();
        drawn.sort_unstable();
        drawn.dedup();
        drawn
    }

    fn term_rect(&self, row: usize) -> egui::Rect {
        self.term_rects
            .borrow()
            .iter()
            .rev()
            .find(|(idx, _)| *idx == row)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("row {row} drew no terminal"))
    }
}

struct Row {
    repo: &'static str,
    branch: Option<&'static str>,
    tab: &'static str,
    agent: &'static str,
    badge: AgentBadge,
    detail: &'static str,
    worktree_id: usize,
}

fn row(repo: &'static str, agent: &'static str, tab: &'static str, badge: AgentBadge) -> Row {
    Row {
        repo,
        branch: Some("main"),
        tab,
        agent,
        badge,
        detail: "",
        worktree_id: 0,
    }
}

fn harness(
    data: Vec<Row>,
    selected: Option<usize>,
    view: AgentsViewMode,
) -> (Harness<'static>, Rc<Captured>) {
    wall_harness(data, selected, view, &[])
}

/// Same, with `shown` naming the rows the Terminals view mirrors — in the order they
/// were put on the wall, so the tree splits exactly as the app's would.
fn wall_harness(
    data: Vec<Row>,
    selected: Option<usize>,
    view: AgentsViewMode,
    shown: &[usize],
) -> (Harness<'static>, Rc<Captured>) {
    let palette = Palette::light();
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut wall: AgentWall<usize> = AgentWall::new();
    for row in shown {
        wall.show(*row, AREA);
    }
    let slots: Vec<(PaneId, usize)> = wall.slots().to_vec();
    let layout = wall.layout().cloned();
    let full = wall.full();
    let mut harness = Harness::builder().with_size(WINDOW).build_ui(move |ui| {
        let rows: Vec<AgentRow> = data
            .iter()
            .map(|r| AgentRow {
                repo: r.repo,
                branch: r.branch,
                tab: r.tab,
                agent: r.agent,
                badge: r.badge,
                detail: r.detail.to_owned(),
                worktree_id: r.worktree_id,
                lane: 0,
            })
            .collect();
        let wall = WallView {
            layout: layout.as_ref(),
            slots: &slots,
            full,
        };
        let action = agents_page(
            ui,
            &palette,
            &rows,
            selected,
            view,
            &wall,
            |idx, term_ui| {
                sink.drawn.borrow_mut().push(idx);
                sink.term_rects.borrow_mut().push((idx, term_ui.max_rect()));
                term_ui.label(format!("TERM-{idx}"));
                false
            },
        );
        // Latch: a Working row repaints (spinner), so later settle frames see no
        // click and would otherwise clear the action captured on the click frame.
        if action.select.is_some() {
            sink.select.set(action.select);
        }
        if action.jump.is_some() {
            sink.jump.set(action.jump);
        }
        if action.set_view.is_some() {
            sink.set_view.set(action.set_view);
        }
        if action.toggle.is_some() {
            sink.toggle.set(action.toggle);
        }
        if action.resize.is_some() {
            sink.resize.set(action.resize);
        }
        if action.drop.is_some() {
            sink.drop.set(action.drop);
        }
    });
    // A Working row paints a spinner that repaints forever, so `run()` would
    // exceed max_steps — step a fixed number of frames to settle the a11y tree.
    harness.step();
    harness.step();
    (harness, cap)
}

/// Press-drag-release from `from` to `to`, stepping the pointer so egui registers a real
/// drag (not a click) and a drop target sees the hover payload before the release.
fn drag(harness: &mut Harness<'static>, from: egui::Pos2, to: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let p = egui::pos2(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        harness.event(egui::Event::PointerMoved(p));
        harness.step();
        harness.step();
    }
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();
}

fn click_at(harness: &mut Harness<'static>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
}

#[test]
fn lists_every_agent_grouped_by_project() {
    let (harness, _) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Done),
            row("api", "aider", "Tab 1", AgentBadge::Idle),
        ],
        None,
        AgentsViewMode::List,
    );
    harness.get_by_label("Claude in helm — Tab 1");
    harness.get_by_label("Codex in helm — Tab 2");
    harness.get_by_label("Aider in api — Tab 1");
}

#[test]
fn worktrees_of_one_project_share_a_group() {
    // Same project name, different branches (a root and its worktree): they stay
    // in a single group — "across 1 project", not two.
    let (harness, _) = harness(
        vec![
            Row {
                repo: "helm",
                branch: Some("main"),
                tab: "Tab 1",
                agent: "claude",
                badge: AgentBadge::Working,
                detail: "",
                worktree_id: 0,
            },
            Row {
                repo: "helm",
                branch: Some("feature/login"),
                tab: "Tab 1",
                agent: "codex",
                badge: AgentBadge::Done,
                detail: "",
                worktree_id: 1,
            },
        ],
        None,
        AgentsViewMode::List,
    );
    harness.get_by_label("Claude in helm — Tab 1");
    harness.get_by_label("Codex in helm — Tab 1");
}

#[test]
fn long_branch_and_detail_keep_the_row_reachable() {
    // Selected + a wide window ⇒ the panel shows and the list runs at its narrow
    // 440px width: a very long branch chip and state caption used to collide with
    // each other and the jump icon. They now elide; the row stays rendered and
    // a11y-reachable (its label carries agent/repo/tab — elision is visual only).
    let (harness, _) = harness(
        vec![Row {
            repo: "helm",
            branch: Some("feature/a-very-long-branch-name-that-would-overflow-the-narrow-list"),
            tab: "Tab 1",
            agent: "claude",
            badge: AgentBadge::Done,
            detail: "Finished 12 minutes ago",
            worktree_id: 0,
        }],
        Some(0),
        AgentsViewMode::List,
    );
    harness.get_by_label("Claude in helm — Tab 1");
}

#[test]
fn clicking_a_row_body_selects_it() {
    let (mut harness, cap) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Done),
        ],
        None,
        AgentsViewMode::List,
    );
    // `Node::click()` clicks the row center — the body, well left of the jump icon.
    harness.get_by_label("Codex in helm — Tab 2").click();
    harness.step();
    assert_eq!(cap.select.get(), Some(1));
    assert_eq!(cap.jump.get(), None);
}

#[test]
fn clicking_the_jump_icon_focuses_the_workspace() {
    let (mut harness, cap) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Done),
        ],
        None,
        AgentsViewMode::List,
    );
    // The jump icon sits at the right edge: its center is CARD_PAD_X (16) + half the
    // 28px hit box in from the row's right — i.e. 30px. Click there, not the center.
    let r = harness.get_by_label("Codex in helm — Tab 2").rect();
    click_at(&mut harness, egui::pos2(r.right() - 30.0, r.center().y));
    assert_eq!(cap.jump.get(), Some(1));
    assert_eq!(cap.select.get(), None);
}

#[test]
fn selecting_an_agent_mirrors_its_terminal_in_the_panel() {
    let (harness, cap) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Done),
        ],
        Some(0),
        AgentsViewMode::List,
    );
    // List view mirrors only the selected agent (deduped across settle frames).
    assert_eq!(cap.drawn_rows(), vec![0]);
    harness.get_by_label("TERM-0");
}

#[test]
fn empty_state_when_no_agents() {
    let (list, _) = harness(Vec::new(), None, AgentsViewMode::List);
    list.get_by_label("No agents running");
    // The wall says the same thing when nothing runs at all — the header would be empty
    // too, so there is nothing to pick.
    let (wall, _) = harness(Vec::new(), None, AgentsViewMode::Terminals);
    wall.get_by_label("No agents running");
}

#[test]
fn header_toggle_switches_to_the_wall() {
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        None,
        AgentsViewMode::List,
    );
    // The toggle shows in both modes; clicking the inactive segment emits the mode.
    harness.get_by_label("List");
    harness.get_by_label("Terminals").click();
    harness.step();
    assert_eq!(cap.set_view.get(), Some(AgentsViewMode::Terminals));
}

#[test]
fn header_toggle_switches_back_to_list() {
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        None,
        AgentsViewMode::Terminals,
    );
    harness.get_by_label("List").click();
    harness.step();
    assert_eq!(cap.set_view.get(), Some(AgentsViewMode::List));
}

#[test]
fn the_header_carries_a_chip_per_running_agent() {
    // Every agent is listed whether or not it is on the wall: the chip names it, the
    // worktree it runs in and its tab, and carries its live state indicator.
    let (harness, _) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            Row {
                branch: Some("feature"),
                worktree_id: 1,
                ..row("helm", "codex", "Tab 2", AgentBadge::Done)
            },
            Row {
                branch: None,
                worktree_id: 2,
                ..row("api", "aider", "Tab 1", AgentBadge::Idle)
            },
        ],
        None,
        AgentsViewMode::Terminals,
        &[0],
    );
    harness.get_by_label("Claude · helm · main · Tab 1");
    harness.get_by_label("Codex · helm · feature · Tab 2");
    // A worktree with no branch (detached) still gets a chip.
    harness.get_by_label("Aider · api · Tab 1");
}

#[test]
fn clicking_a_chip_puts_that_agent_on_the_wall() {
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0],
    );
    harness.get_by_label("Codex · helm · main · Tab 2").click();
    harness.step();
    assert_eq!(cap.toggle.get(), Some(1));
    assert_eq!(
        cap.select.get(),
        None,
        "the chip toggles, it does not select"
    );
}

#[test]
fn clicking_the_chip_of_a_shown_agent_takes_it_off_the_wall() {
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1],
    );
    harness.get_by_label("Claude · helm · main · Tab 1").click();
    harness.step();
    assert_eq!(cap.toggle.get(), Some(0));
}

#[test]
fn the_wall_mirrors_a_live_terminal_per_shown_agent() {
    let (harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
            row("api", "aider", "Tab 1", AgentBadge::Done),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 2],
    );
    assert_eq!(cap.drawn_rows(), vec![0, 2], "only the shown agents mirror");
    harness.get_by_label("TERM-0");
    harness.get_by_label("TERM-2");
    assert!(
        harness.query_by_label("TERM-1").is_none(),
        "a hidden agent draws no terminal"
    );
    // Each tile is topped by its own status band.
    harness.get_by_label("Claude in helm · main — Tab 1");
    harness.get_by_label("Aider in api · main — Tab 1");
}

#[test]
fn two_tiles_split_the_wall_side_by_side() {
    // The wall is laid out by the terminal's split tree: two tiles on a wide wall are
    // two equal columns that together span it.
    let (_harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Idle),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1],
    );
    let left = cap.term_rect(0);
    let right = cap.term_rect(1);
    assert!(
        (left.width() - right.width()).abs() < 2.0,
        "equal halves, got {} vs {}",
        left.width(),
        right.width()
    );
    assert!(
        left.right() <= right.left() + 2.0,
        "the second tile sits beside the first, got {left:?} then {right:?}"
    );
    assert!(
        right.right() > WINDOW.x - 20.0,
        "the pair spans the wall, right edge {}",
        right.right()
    );
}

#[test]
fn a_lone_tile_fills_the_wall() {
    let (_harness, cap) = wall_harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        Some(0),
        AgentsViewMode::Terminals,
        &[0],
    );
    let term = cap.term_rect(0);
    assert!(
        term.width() > WINDOW.x - 20.0 && term.height() > 900.0,
        "a single terminal fills the wall, got {term:?}"
    );
}

#[test]
fn a_tile_pane_sits_flush_under_its_band() {
    // The band is a strip on the tile, not a card around it: the pane starts straight
    // under it and spans the same width, so nothing frames the terminal.
    let (harness, cap) = wall_harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        Some(0),
        AgentsViewMode::Terminals,
        &[0],
    );
    let band = harness.get_by_label("Claude in helm · main — Tab 1").rect();
    let term = cap.term_rect(0);
    assert!(
        (term.top() - band.bottom()).abs() < 1.0,
        "the pane must sit flush under the band, got a {}px gap",
        term.top() - band.bottom()
    );
    assert!(
        (term.width() - band.width()).abs() < 1.0,
        "the pane must be as wide as the band, got {} vs {}",
        term.width(),
        band.width()
    );
}

#[test]
fn a_full_wall_leaves_the_remaining_chips_out_of_reach() {
    // Four terminals is the cap: the fifth agent's chip reads disabled and clicking it
    // emits nothing — hiding one is the way to make room.
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Idle),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
            row("helm", "aider", "Tab 3", AgentBadge::Idle),
            row("helm", "amp", "Tab 4", AgentBadge::Idle),
            row("helm", "gemini", "Tab 5", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1, 2, 3],
    );
    assert_eq!(cap.drawn_rows(), vec![0, 1, 2, 3]);
    harness.get_by_label("Gemini · helm · main · Tab 5").click();
    harness.step();
    assert_eq!(cap.toggle.get(), None, "a blocked chip emits nothing");
    // A shown agent's chip still comes off, which is what frees the slot.
    harness.get_by_label("Amp · helm · main · Tab 4").click();
    harness.step();
    assert_eq!(cap.toggle.get(), Some(3));
}

#[test]
fn an_empty_wall_points_back_at_the_header() {
    let (harness, cap) = wall_harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        Some(0),
        AgentsViewMode::Terminals,
        &[],
    );
    harness.get_by_label("No terminal on the wall");
    // The chip is still there to pick from, and nothing mirrors.
    harness.get_by_label("Claude · helm · main · Tab 1");
    assert!(cap.drawn_rows().is_empty());
}

#[test]
fn clicking_a_tile_band_selects_it() {
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1],
    );
    // `Node::click()` clicks the band center — well left of its jump icon.
    harness.get_by_label("Codex in helm · main — Tab 2").click();
    harness.step();
    assert_eq!(cap.select.get(), Some(1));
    assert_eq!(cap.jump.get(), None);
}

#[test]
fn clicking_a_tile_jump_icon_focuses_the_workspace() {
    let (mut harness, cap) = wall_harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        Some(0),
        AgentsViewMode::Terminals,
        &[0],
    );
    let band = harness.get_by_label("Claude in helm · main — Tab 1").rect();
    click_at(
        &mut harness,
        egui::pos2(band.right() - JUMP_INSET, band.center().y),
    );
    assert_eq!(cap.jump.get(), Some(0));
    assert_eq!(cap.select.get(), None);
}

#[test]
fn dragging_the_seam_between_two_tiles_resizes_them() {
    // The wall's seams are the split tree's own (terminal.md §5): dragging the one
    // between two tiles reports the ratio change for the app to apply.
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Idle),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1],
    );
    let left = cap.term_rect(0);
    let seam = egui::pos2(left.right() + 1.0, left.center().y);
    drag(&mut harness, seam, seam + egui::vec2(90.0, 0.0));
    let drag = cap.resize.get().expect("the seam drag reports a resize");
    assert!(
        drag.delta > 0.0,
        "dragging right widens the first tile, got delta {}",
        drag.delta
    );
    assert_ne!(drag.first, drag.second, "the seam names both its tiles");
}

#[test]
fn dropping_a_tile_grip_on_another_tile_rearranges_the_wall() {
    // Same drag-and-drop reorg as a workspace tab: the grip the tree reveals at a tile's
    // top-right corner drags onto another tile — its center swaps the two, an edge
    // re-splits. The grip sits in the band's corner, so measure it off the band.
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Idle),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1],
    );
    let band = harness.get_by_label("Claude in helm · main — Tab 1").rect();
    drag(&mut harness, grip_pos(band), cap.term_rect(1).center());
    let drop = cap.drop.get().expect("the grip drag reports a drop");
    assert_eq!(drop.zone, DropZone::Swap, "the target's center swaps");
    assert_ne!(drop.src, drop.target);

    // And an edge drop re-splits the target on that side instead.
    let (mut harness, cap) = wall_harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Idle),
            row("helm", "codex", "Tab 2", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Terminals,
        &[0, 1],
    );
    let band = harness.get_by_label("Claude in helm · main — Tab 1").rect();
    let target = cap.term_rect(1);
    drag(
        &mut harness,
        grip_pos(band),
        egui::pos2(target.center().x, target.bottom() - 20.0),
    );
    let drop = cap.drop.get().expect("the grip drag reports a drop");
    assert_eq!(
        drop.zone,
        DropZone::Side(Dir::Down),
        "the bottom edge stacks"
    );
}
