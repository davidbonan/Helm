//! UI E2E for the cross-repo agents dashboard (specs/agents.md §5): drives
//! `agents_page` headless and checks the grouped rows, the empty state, the
//! row-body → select vs jump-icon → focus split, the List panel mirror, the
//! List/Columns view switch, and the column grid that mirrors a live terminal
//! per agent.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::theme::Palette;
use helm::ui::agents_view::{agents_page, AgentRow, AgentsViewMode, TermView};

#[derive(Default)]
struct Captured {
    select: Cell<Option<usize>>,
    jump: Cell<Option<usize>>,
    set_view: Cell<Option<AgentsViewMode>>,
    set_column_width: Cell<Option<f32>>,
    set_terminal_height: Cell<Option<f32>>,
    drawn: RefCell<Vec<usize>>,
}

struct Row {
    repo: &'static str,
    branch: Option<&'static str>,
    tab: &'static str,
    agent: &'static str,
    badge: AgentBadge,
    detail: &'static str,
    worktree_id: usize,
    stats: Option<(usize, usize)>,
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
        stats: None,
    }
}

fn harness(
    data: Vec<Row>,
    selected: Option<usize>,
    view: AgentsViewMode,
) -> (Harness<'static>, Rc<Captured>) {
    let palette = Palette::light();
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 1200.0))
        .build_ui(move |ui| {
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
                    stats: r.stats,
                })
                .collect();
            let action = agents_page(
                ui,
                &palette,
                &rows,
                selected,
                view,
                672.0,
                360.0,
                |idx, term_ui, view| match view {
                    TermView::Full => {
                        sink.drawn.borrow_mut().push(idx);
                        term_ui.label(format!("TERM-{idx}"));
                    }
                    TermView::Preview => {
                        term_ui.label(format!("PREV-{idx}"));
                    }
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
            if action.set_column_width.is_some() {
                sink.set_column_width.set(action.set_column_width);
            }
            if action.set_terminal_height.is_some() {
                sink.set_terminal_height.set(action.set_terminal_height);
            }
        });
    // A Working row paints a spinner that repaints forever, so `run()` would
    // exceed max_steps — step a fixed number of frames to settle the a11y tree.
    harness.step();
    harness.step();
    (harness, cap)
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
                stats: None,
            },
            Row {
                repo: "helm",
                branch: Some("feature/login"),
                tab: "Tab 1",
                agent: "codex",
                badge: AgentBadge::Done,
                detail: "",
                worktree_id: 1,
                stats: None,
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
            stats: None,
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
    let pos = egui::pos2(r.right() - 30.0, r.center().y);
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
    let mut drawn = cap.drawn.borrow().clone();
    drawn.sort_unstable();
    drawn.dedup();
    assert_eq!(drawn, vec![0]);
    harness.get_by_label("TERM-0");
}

#[test]
fn empty_state_when_no_agents() {
    let (harness, _) = harness(Vec::new(), None, AgentsViewMode::List);
    harness.get_by_label("No agents running");
}

#[test]
fn header_toggle_switches_to_columns() {
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        None,
        AgentsViewMode::List,
    );
    // The toggle shows in both modes; clicking the inactive segment emits the mode.
    harness.get_by_label("List");
    harness.get_by_label("Columns").click();
    harness.step();
    assert_eq!(cap.set_view.get(), Some(AgentsViewMode::Columns));
}

#[test]
fn header_toggle_switches_back_to_list() {
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        None,
        AgentsViewMode::Columns,
    );
    harness.get_by_label("List").click();
    harness.step();
    assert_eq!(cap.set_view.get(), Some(AgentsViewMode::List));
}

#[test]
fn columns_expand_only_the_selected_card() {
    // Two projects (→ two columns), the first split across two worktrees. Cards
    // collapse to a status header by default; only the selected agent's card
    // expands to a mirrored live terminal — the others stay collapsed.
    let (harness, cap) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            Row {
                worktree_id: 1,
                ..row("helm", "codex", "Tab 1", AgentBadge::Done)
            },
            row("api", "aider", "Tab 1", AgentBadge::Idle),
        ],
        Some(1),
        AgentsViewMode::Columns,
    );
    let mut drawn = cap.drawn.borrow().clone();
    drawn.sort_unstable();
    drawn.dedup();
    assert_eq!(
        drawn,
        vec![1],
        "only the selected card mirrors a full terminal"
    );
    harness.get_by_label("TERM-1");
    // Every other agent stays reachable as a collapsed status header over a
    // read-only progress preview of its last lines.
    harness.get_by_label("Claude in helm — Tab 1");
    harness.get_by_label("Aider in api — Tab 1");
    harness.get_by_label("PREV-0");
    harness.get_by_label("PREV-2");
}

#[test]
fn clicking_a_collapsed_column_card_selects_it() {
    // Clicking another card's body (not its jump icon) selects it, so the app
    // expands that card's terminal on the next frame.
    let (mut harness, cap) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Working),
            Row {
                worktree_id: 1,
                ..row("helm", "codex", "Tab 1", AgentBadge::Done)
            },
        ],
        Some(0),
        AgentsViewMode::Columns,
    );
    // `Node::click()` clicks the header center — the body, well left of the jump icon.
    harness.get_by_label("Codex in helm — Tab 1").click();
    harness.step();
    assert_eq!(cap.select.get(), Some(1));
    assert_eq!(cap.jump.get(), None);
}

#[test]
fn horizontal_gesture_over_a_terminal_scrolls_the_columns() {
    // Three projects ⇒ three 672px columns overflow the 1500px window, so the
    // grid scrolls horizontally. The hovered terminal owns only the vertical wheel
    // axis (scrollback); a horizontal gesture must still reach the columns'
    // horizontal ScrollArea instead of being swallowed on both axes.
    let (mut harness, _) = harness(
        vec![
            row("helm", "claude", "Tab 1", AgentBadge::Idle),
            row("api", "codex", "Tab 1", AgentBadge::Idle),
            row("web", "aider", "Tab 1", AgentBadge::Idle),
        ],
        Some(0),
        AgentsViewMode::Columns,
    );
    // `TERM-0`'s a11y rect is just its label at the strip's top-left; aim well
    // inside the 360px-tall strip so the pointer is unambiguously over the terminal.
    let label = harness.get_by_label("TERM-0").rect();
    let over_terminal = egui::pos2(label.left() + 250.0, label.top() + 150.0);
    // Probe a collapsed card in the second column (only column 0 has a terminal).
    let before = harness
        .get_by_label("Codex in api — Tab 1")
        .rect()
        .center()
        .x;
    // A continuous horizontal gesture with the pointer pinned over a terminal:
    // egui smooths each wheel notch over several frames, so feed one per frame.
    for _ in 0..20 {
        harness.event(egui::Event::PointerMoved(over_terminal));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(-40.0, 0.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        harness.step();
    }
    let after = harness
        .get_by_label("Codex in api — Tab 1")
        .rect()
        .center()
        .x;
    assert!(
        (before - after).abs() > 50.0,
        "horizontal gesture over a terminal must scroll the columns: \
         column 2 x {before} -> {after}"
    );
}

#[test]
fn vertical_gesture_scrolls_the_whole_wall() {
    // Many collapsed cards in one project overflow the 1200px window height. The
    // wall is a single 2D scroll plane (no per-column scrollbars), so a vertical
    // gesture over any non-terminal area moves the whole wall — every visible card
    // shifts up together.
    let data: Vec<Row> = (0..16)
        .map(|i| {
            let tab: &'static str = Box::leak(format!("Tab {}", i + 1).into_boxed_str());
            row("helm", "claude", tab, AgentBadge::Idle)
        })
        .collect();
    let (mut harness, _) = harness(data, None, AgentsViewMode::Columns);
    // Pin the pointer over the worktree band (top of the column, not a terminal) and
    // measure a card mid-column that stays visible across the scroll.
    let over = harness.get_by_label("main").rect().center();
    let before = harness
        .get_by_label("Claude in helm — Tab 10")
        .rect()
        .center()
        .y;
    for _ in 0..20 {
        harness.event(egui::Event::PointerMoved(over));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -40.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        harness.step();
    }
    let after = harness
        .get_by_label("Claude in helm — Tab 10")
        .rect()
        .center()
        .y;
    assert!(
        (before - after) > 50.0,
        "vertical gesture must scroll the wall up: card y {before} -> {after}"
    );
}

#[test]
fn dragging_a_column_gap_resizes_the_columns() {
    // One project ⇒ one column; its resize handle sits in the trailing gap just
    // past the column's right edge. Anchor on the worktree header rect (it spans
    // the column's inner width) so the grab point survives layout offsets.
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        None,
        AgentsViewMode::Columns,
    );
    let header = harness.get_by_label("main").rect();
    let start = egui::pos2(header.right() + 10.0, header.center().y);
    let end = start + egui::vec2(60.0, 0.0);
    harness.event(egui::Event::PointerMoved(start));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.event(egui::Event::PointerMoved(end));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    let width = cap.set_column_width.get().expect("drag emits a new width");
    assert!(
        width > 672.0,
        "dragging right widens the column, got {width}"
    );
}

#[test]
fn clicking_a_column_card_jump_icon_focuses_the_workspace() {
    // Each column card carries the same external-link affordance as a list row: the
    // jump icon sits 30px in from the card's right edge (CARD_PAD_X 16 + half the
    // 28px hit box). Clicking it focuses the pane (emits `jump`, not `select`).
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        None,
        AgentsViewMode::Columns,
    );
    let r = harness.get_by_label("Claude in helm — Tab 1").rect();
    let pos = egui::pos2(r.right() - 30.0, r.center().y);
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
    assert_eq!(cap.jump.get(), Some(0));
    assert_eq!(cap.select.get(), None);
}

#[test]
fn dragging_a_card_bottom_resizes_the_terminal_height() {
    // The height handle sits just below the 360px-tall terminal strip; anchor on the
    // mirrored terminal's rect (its top is the strip top) so the grab survives layout.
    let (mut harness, cap) = harness(
        vec![row("helm", "claude", "Tab 1", AgentBadge::Working)],
        Some(0),
        AgentsViewMode::Columns,
    );
    let strip = harness.get_by_label("TERM-0").rect();
    let start = egui::pos2(strip.center().x, strip.top() + 365.0);
    let end = start + egui::vec2(0.0, 80.0);
    harness.event(egui::Event::PointerMoved(start));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.event(egui::Event::PointerMoved(end));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    let height = cap
        .set_terminal_height
        .get()
        .expect("drag emits a new height");
    assert!(
        height > 360.0,
        "dragging down grows the terminal, got {height}"
    );
}

#[test]
fn dirty_worktree_header_exposes_uncommitted_stats() {
    // A dirty worktree's column header carries the uncommitted ratio bar; its a11y
    // label spells the stats. A clean worktree stays a bare branch label.
    let (harness, _) = harness(
        vec![
            Row {
                stats: Some((46, 3)),
                ..row("helm", "claude", "Tab 1", AgentBadge::Working)
            },
            Row {
                branch: Some("clean"),
                worktree_id: 1,
                stats: None,
                ..row("helm", "codex", "Tab 1", AgentBadge::Idle)
            },
        ],
        None,
        AgentsViewMode::Columns,
    );
    assert!(
        harness
            .query_by_label_contains("main · +46 −3 uncommitted")
            .is_some(),
        "dirty worktree header spells its stats"
    );
    harness.get_by_label("clean");
}
