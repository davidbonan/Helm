use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::git::graph::{Graph, GraphCommit, GraphRef, LaneCache, RefKind};
use helm::theme::Palette;
use helm::ui::graph_view::{
    close_chip_menu, delete_branch_modal, delete_stash_modal, delete_tag_modal, graph_view,
    BranchEditor, BranchEditorTarget, CreateBranchRequest, DeleteBranchTarget, GraphSearch,
    GraphViewState, RenameRequest, StashTarget, WipRow,
};
use helm::ui::repo_sidebar::DeleteModalAction;

fn paginated_graph() -> Graph {
    Graph {
        has_more: true,
        ..sample_graph()
    }
}

fn oid(byte: u8) -> git2::Oid {
    git2::Oid::from_bytes(&[byte; 20]).unwrap()
}

fn commit(byte: u8, summary: &str, parents: Vec<git2::Oid>, refs: Vec<GraphRef>) -> GraphCommit {
    GraphCommit {
        oid: oid(byte),
        short_id: format!("{byte:07x}"),
        summary: summary.to_string(),
        body: String::new(),
        author: "Ada".to_string(),
        time: 1_609_459_200,
        parents,
        refs,
        stash: false,
    }
}

fn graph_ref(name: &str, kind: RefKind, is_head: bool) -> GraphRef {
    GraphRef {
        name: name.to_string(),
        kind,
        is_head,
        also_remote: false,
        counterpart: None,
        worktree_available: false,
    }
}

fn sample_graph() -> Graph {
    // c (HEAD -> main) -> b -> a (v1.0)
    Graph {
        commits: vec![
            GraphCommit {
                // Message body ⇒ exercises the dim rendering following the summary (M10-6).
                body: "Approved-by: Ada".to_string(),
                ..commit(
                    3,
                    "Third commit",
                    vec![oid(2)],
                    vec![graph_ref("main", RefKind::Local, true)],
                )
            },
            commit(2, "Second commit", vec![oid(1)], vec![]),
            commit(
                1,
                "First commit",
                vec![],
                vec![graph_ref("v1.0", RefKind::Tag, false)],
            ),
        ],
        has_more: false,
    }
}

struct ViewState {
    graph: Option<Graph>,
    lanes: LaneCache,
    wip: Option<WipRow>,
    selected: Option<git2::Oid>,
    editor: BranchEditor,
    search: GraphSearch,
    scroll_to_head: bool,
    keyboard_nav: bool,
    can_pull_request: bool,
    clicked: Option<git2::Oid>,
    load_more: bool,
    wip_clicked: bool,
    checkout: Option<String>,
    create_branch: Option<String>,
    open_branch_editor: Option<CreateBranchRequest>,
    create_branch_at: Option<String>,
    open_tag_editor: Option<git2::Oid>,
    create_tag_at: Option<String>,
    create_worktree: Option<String>,
    rebase_onto: Option<String>,
    interactive_rebase_onto: Option<String>,
    ai_rebase_onto: Option<String>,
    merge: Option<String>,
    delete: Option<DeleteBranchTarget>,
    stash_apply: Option<git2::Oid>,
    stash_pop: Option<git2::Oid>,
    stash_drop: Option<StashTarget>,
    checkout_tag: Option<String>,
    push_tag: Option<String>,
    delete_tag: Option<String>,
    cherry_pick: Option<git2::Oid>,
    revert: Option<git2::Oid>,
    reset: Option<(git2::Oid, git2::ResetType)>,
    open_rename: Option<RenameRequest>,
    rename_branch: Option<(String, String)>,
    create_pull_request: Option<String>,
}

fn harness(graph: Graph, selected: Option<git2::Oid>) -> Harness<'static, ViewState> {
    harness_full(Some(graph), None, selected)
}

fn harness_full(
    graph: Option<Graph>,
    wip: Option<WipRow>,
    selected: Option<git2::Oid>,
) -> Harness<'static, ViewState> {
    Harness::builder().build_ui_state(
        |ui, state| {
            let palette = Palette::dark();
            let action = graph_view(
                ui,
                &palette,
                &GraphViewState {
                    graph: state.graph.as_ref(),
                    wip: state.wip,
                    selected: state.selected,
                    scroll_to_head: state.scroll_to_head,
                    keyboard_nav: state.keyboard_nav,
                    can_pull_request: state.can_pull_request,
                },
                &mut state.lanes,
                &mut state.editor,
                &mut state.search,
            );
            // Same contract as `HelmApp`: the selection (commit or WIP,
            // mutually exclusive) is adopted for the next frame.
            if let Some(oid) = action.selected {
                state.clicked = Some(oid);
                state.selected = Some(oid);
                if let Some(wip) = &mut state.wip {
                    wip.selected = false;
                }
            }
            if action.load_more {
                state.load_more = true;
            }
            if action.wip_selected {
                state.wip_clicked = true;
                state.selected = None;
                if let Some(wip) = &mut state.wip {
                    wip.selected = true;
                }
            }
            if let Some(branch) = action.checkout {
                state.checkout = Some(branch);
            }
            if let Some(branch) = action.create_branch {
                state.create_branch = Some(branch);
            }
            if let Some(request) = action.open_branch_editor {
                state.open_branch_editor = Some(request.clone());
                state.editor = BranchEditor::default();
                state.editor.open = true;
                state.editor.target = Some(BranchEditorTarget {
                    oid: request.oid,
                    source: request.source,
                });
            }
            if let Some(name) = action.create_branch_at {
                state.create_branch_at = Some(name);
            }
            if let Some(oid) = action.open_tag_editor {
                state.open_tag_editor = Some(oid);
                state.editor = BranchEditor::default();
                state.editor.open = true;
                state.editor.tag = true;
                state.editor.target = Some(BranchEditorTarget {
                    oid,
                    source: String::new(),
                });
            }
            if let Some(name) = action.create_tag_at {
                state.create_tag_at = Some(name);
            }
            if let Some(branch) = action.create_worktree {
                state.create_worktree = Some(branch);
            }
            if let Some(branch) = action.rebase_onto {
                state.rebase_onto = Some(branch);
            }
            if let Some(branch) = action.interactive_rebase_onto {
                state.interactive_rebase_onto = Some(branch);
            }
            if let Some(branch) = action.ai_rebase_onto {
                state.ai_rebase_onto = Some(branch);
            }
            if let Some(branch) = action.merge {
                state.merge = Some(branch);
            }
            if let Some(target) = action.delete {
                state.delete = Some(target);
            }
            if let Some(oid) = action.stash_apply {
                state.stash_apply = Some(oid);
            }
            if let Some(oid) = action.stash_pop {
                state.stash_pop = Some(oid);
            }
            if let Some(target) = action.stash_drop {
                state.stash_drop = Some(target);
            }
            if let Some(tag) = action.checkout_tag {
                state.checkout_tag = Some(tag);
            }
            if let Some(tag) = action.push_tag {
                state.push_tag = Some(tag);
            }
            if let Some(tag) = action.delete_tag {
                state.delete_tag = Some(tag);
            }
            if let Some(oid) = action.cherry_pick {
                state.cherry_pick = Some(oid);
            }
            if let Some(oid) = action.revert {
                state.revert = Some(oid);
            }
            if let Some(reset) = action.reset {
                state.reset = Some(reset);
            }
            if let Some(request) = action.open_rename_editor {
                state.open_rename = Some(request.clone());
                state.editor = BranchEditor::default();
                state.editor.open = true;
                state.editor.name = request.name.clone();
                state.editor.rename = Some(request.name);
                state.editor.target = Some(BranchEditorTarget {
                    oid: request.oid,
                    source: String::new(),
                });
            }
            if let Some(rename) = action.rename_branch {
                state.rename_branch = Some(rename);
            }
            if let Some(dest) = action.create_pull_request {
                state.create_pull_request = Some(dest);
            }
            // Same contract as `HelmApp`: the consumed request clears the one-shot.
            if action.scrolled_to_head {
                state.scroll_to_head = false;
            }
        },
        ViewState {
            graph,
            lanes: LaneCache::default(),
            wip,
            selected,
            editor: BranchEditor::default(),
            search: GraphSearch::default(),
            scroll_to_head: false,
            keyboard_nav: true,
            can_pull_request: false,
            clicked: None,
            load_more: false,
            wip_clicked: false,
            checkout: None,
            create_branch: None,
            open_branch_editor: None,
            create_branch_at: None,
            open_tag_editor: None,
            create_tag_at: None,
            create_worktree: None,
            rebase_onto: None,
            interactive_rebase_onto: None,
            ai_rebase_onto: None,
            merge: None,
            delete: None,
            stash_apply: None,
            stash_pop: None,
            stash_drop: None,
            checkout_tag: None,
            push_tag: None,
            delete_tag: None,
            cherry_pick: None,
            revert: None,
            reset: None,
            open_rename: None,
            rename_branch: None,
            create_pull_request: None,
        },
    )
}

/// Primary double-click at `pos`: two press/release pairs pushed in the **same
/// frame** via `input_mut` (same timestamp ⇒ within egui's double-click
/// window). `Harness::event` won't do: kittest advances one frame per dequeued
/// event (0.25 s each), beyond the window's 0.3 s.
fn double_click_at(harness: &mut Harness<'_, ViewState>, pos: egui::Pos2) {
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
        }
    }
    harness.step();
}

/// Right-click at `pos`: secondary press/release in the same frame (same manual
/// hit-test as the chips, see `double_click_at`).
fn right_click_at(harness: &mut Harness<'_, ViewState>, pos: egui::Pos2) {
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    harness.step();
}

/// Single primary click at `pos` (press/release in the same frame).
fn click_at(harness: &mut Harness<'_, ViewState>, pos: egui::Pos2) {
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    harness.step();
}

/// Moves the pointer to `pos` and advances one frame (effective hover).
fn move_pointer_to(harness: &mut Harness<'_, ViewState>, pos: egui::Pos2) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    harness.step();
}

/// `n` parallel branches open simultaneously (tips first, then distinct roots)
/// ⇒ `n` lanes on screen.
fn wide_graph(lanes: u8) -> Graph {
    let tips = (0..lanes).map(|i| commit(100 + i, &format!("Tip {i}"), vec![oid(1 + i)], vec![]));
    let roots = (0..lanes).map(|i| commit(1 + i, &format!("Root {i}"), vec![], vec![]));
    Graph {
        commits: tips.chain(roots).collect(),
        has_more: false,
    }
}

/// Left edge of the message column: tracks `REFS_COL_WIDTH + graph zone`, the
/// observable for the graph column's width.
fn message_header_left(harness: &Harness<'_, ViewState>) -> f32 {
    harness.get_by_label("COMMIT MESSAGE").rect().left()
}

/// Horizontal primary drag: press at `from`, move by `dx`, release — one frame
/// per event (the handle only senses the drag, no threshold).
fn drag_horizontal(harness: &mut Harness<'_, ViewState>, from: egui::Pos2, dx: f32) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    let to = from + egui::vec2(dx, 0.0);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(to));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
}

/// Deep linear history: HEAD (`main`) at the very bottom, off-viewport until
/// the view scrolls.
fn deep_head_graph() -> Graph {
    let mut commits: Vec<GraphCommit> = (2u8..=50)
        .rev()
        .map(|byte| commit(byte, "Above head", vec![oid(byte - 1)], vec![]))
        .collect();
    commits.push(commit(
        1,
        "Deep head",
        vec![],
        vec![graph_ref("main", RefKind::Local, true)],
    ));
    Graph {
        commits,
        has_more: false,
    }
}

#[test]
fn opening_scrolls_to_the_head_row() {
    // git.md §9: on opening Graph mode, the view scrolls to the HEAD row
    // (centered) to locate the current branch.
    let mut harness = harness_full(Some(deep_head_graph()), None, None);
    harness.state_mut().scroll_to_head = true;
    harness.run();

    let screen = harness.ctx.content_rect();
    let head_row = harness.get_by_label("0000001 Deep head").rect();
    assert!(
        screen.contains_rect(head_row),
        "head row {head_row:?} should be scrolled into {screen:?}"
    );
    assert!(
        !harness.state().scroll_to_head,
        "the one-shot request is consumed once rows rendered"
    );
}

#[test]
fn without_a_scroll_request_the_view_stays_at_top() {
    let mut harness = harness_full(Some(deep_head_graph()), None, None);
    harness.run();

    // The view stays at the top: the first row is rendered, while the deep HEAD
    // row at the bottom of the history sits below the viewport — virtualized rows
    // off-screen are not allocated, so it is absent from the tree (had the view
    // auto-scrolled to it, the reverse would hold).
    assert!(
        harness.query_by_label("0000032 Above head").is_some(),
        "the top row stays rendered"
    );
    assert!(
        harness.query_by_label("0000001 Deep head").is_none(),
        "the deep head row stays below the viewport (off-screen, not rendered)"
    );
}

#[test]
fn scroll_request_survives_until_the_graph_arrives() {
    // Graph not yet received (spinner): the request is not consumed — the
    // scroll will fire when the rows arrive.
    let mut harness = harness_full(None, None, None);
    harness.state_mut().scroll_to_head = true;
    harness.step();

    assert!(harness.state().scroll_to_head);
}

#[test]
fn lanes_are_computed_once_across_frames() {
    // M10-8: the renderer memoizes `assign_lanes` — several frames of the same
    // graph ⇒ a single lane computation.
    let mut harness = harness(sample_graph(), None);
    harness.run();
    harness.run();

    assert_eq!(harness.state().lanes.computes(), 1);
}

#[test]
fn renders_column_headers() {
    // M10-5: header row BRANCH / TAG · GRAPH · COMMIT MESSAGE.
    let mut harness = harness(sample_graph(), None);
    harness.run();

    harness.get_by_label("BRANCH / TAG");
    harness.get_by_label("GRAPH");
    harness.get_by_label("COMMIT MESSAGE");
}

#[test]
fn placeholders_have_no_column_headers() {
    let mut harness = harness(Graph::default(), None);
    harness.run();

    assert!(harness.query_by_label("BRANCH / TAG").is_none());
}

#[test]
fn renders_a_row_per_commit() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    // Each row exposes "<short hash> <summary>" to the accessibility tree.
    harness.get_by_label("0000003 Third commit");
    harness.get_by_label("0000002 Second commit");
    harness.get_by_label("0000001 First commit");
}

#[test]
fn rows_are_contiguous_so_lane_lines_connect() {
    // Lanes are painted row by row (top → bottom of the rect): any vertical gap
    // between two rows would break the graph lines into dashes.
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let third = harness.get_by_label("0000003 Third commit").rect();
    let second = harness.get_by_label("0000002 Second commit").rect();
    let first = harness.get_by_label("0000001 First commit").rect();
    assert_eq!(third.bottom(), second.top());
    assert_eq!(second.bottom(), first.top());
}

#[test]
fn clicking_a_commit_emits_its_oid() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    harness.get_by_label("0000002 Second commit").click();
    harness.run();
    assert_eq!(harness.state().clicked, Some(oid(2)));
}

#[test]
fn stash_row_renders_and_clicking_it_selects_the_stash_commit() {
    // Stash row (inserted above its base commit by the domain): rendered like a
    // normal row (hash + message) and selectable — the stash commit's detail
    // opens like any other commit's.
    let mut graph = sample_graph();
    graph.commits.insert(
        1,
        GraphCommit {
            stash: true,
            ..commit(9, "On main: helm: stash", vec![oid(2)], vec![])
        },
    );
    let mut harness = harness(graph, None);
    harness.run();

    harness.get_by_label("0000009 On main: helm: stash").click();
    harness.run();
    assert_eq!(harness.state().clicked, Some(oid(9)));
}

#[test]
fn empty_git_repo_shows_no_commits_placeholder() {
    // Git repo but `HEAD` unborn (M9-8) ⇒ **No commits**, not the non-git message.
    let mut harness = harness(Graph::default(), None);
    harness.run();

    harness.get_by_label("No commits");
    assert_eq!(harness.state().clicked, None);
}

#[test]
fn pending_graph_shows_loader_not_no_commits() {
    // Graph not yet received (repo switch, large repo) ⇒ spinner, not the
    // **No commits** placeholder (reserved for the genuinely empty repo).
    let mut harness = harness_full(None, None, None);
    // `run()` waits for stability — but the spinner requests a repaint every
    // frame: we advance one explicit step (egui_kittest recommendation).
    harness.step();

    harness.get_by_label("Loading graph");
    assert!(harness.query_by_label("No commits").is_none());
}

#[test]
fn more_history_shows_load_more_and_emits_intent() {
    // Beyond the first page (M9-8) ⇒ **Load more** button; clicking emits the
    // pagination intent (no silent truncation).
    let mut harness = harness(paginated_graph(), None);
    harness.run();

    harness.get_by_label("Load more").click();
    harness.run();
    assert!(harness.state().load_more);
}

#[test]
fn fully_loaded_graph_has_no_load_more() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    assert!(
        harness.query_by_label("Load more").is_none(),
        "no Load more once the whole history is loaded"
    );
}

#[test]
fn dirty_tree_shows_wip_row_with_counter() {
    // M10-7: dirty working tree ⇒ head row `// WIP · N file(s)`.
    let wip = Some(WipRow {
        files: 2,
        selected: false,
    });
    let mut harness = harness_full(Some(sample_graph()), wip, None);
    harness.run();

    harness.get_by_label("// WIP · 2 files");
}

#[test]
fn clean_tree_has_no_wip_row() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    assert!(harness.query_by_label_contains("// WIP").is_none());
}

#[test]
fn clicking_wip_row_emits_intent() {
    let wip = Some(WipRow {
        files: 1,
        selected: false,
    });
    let mut harness = harness_full(Some(sample_graph()), wip, None);
    harness.run();

    harness.get_by_label("// WIP · 1 file").click();
    harness.run();
    assert!(harness.state().wip_clicked);
    assert_eq!(harness.state().clicked, None);
}

#[test]
fn selected_wip_row_is_marked_in_accessibility_tree() {
    let wip = Some(WipRow {
        files: 1,
        selected: true,
    });
    let mut harness = harness_full(Some(sample_graph()), wip, None);
    harness.run();

    let node = harness.get_by_label("// WIP · 1 file");
    assert_eq!(
        format!("{:?}", node.accesskit_node().toggled()),
        "Some(True)",
        "the selected WIP row reports toggled=True"
    );
}

#[test]
fn selected_commit_is_marked_in_accessibility_tree() {
    let mut harness = harness(sample_graph(), Some(oid(3)));
    harness.run();

    let node = harness.get_by_label("0000003 Third commit");
    // egui maps `WidgetInfo::selected` to the accesskit `Toggled` flag; compare by
    // its Debug form to stay independent of the accesskit enum import path/version.
    assert_eq!(
        format!("{:?}", node.accesskit_node().toggled()),
        "Some(True)",
        "the selected commit row reports toggled=True"
    );
    let other = harness.get_by_label("0000002 Second commit");
    assert_eq!(
        format!("{:?}", other.accesskit_node().toggled()),
        "Some(False)",
        "unselected rows report toggled=False"
    );
}

#[test]
fn arrow_down_selects_the_next_commit() {
    let mut harness = harness(sample_graph(), Some(oid(3)));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(harness.state().clicked, Some(oid(2)));
}

#[test]
fn arrow_up_selects_the_previous_commit() {
    let mut harness = harness(sample_graph(), Some(oid(2)));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    harness.run();

    assert_eq!(harness.state().clicked, Some(oid(3)));
}

#[test]
fn arrows_do_not_wrap_at_the_graph_edges() {
    // No wrapping (paginated history): ↓ on the last commit and ↑ on the first
    // (clean tree) do not move the selection.
    let mut bottom = harness(sample_graph(), Some(oid(1)));
    bottom.run();
    bottom.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    bottom.run();
    assert_eq!(bottom.state().clicked, None);

    let mut top = harness(sample_graph(), Some(oid(3)));
    top.run();
    top.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    top.run();
    assert_eq!(top.state().clicked, None);
    assert!(!top.state().wip_clicked);
}

#[test]
fn arrow_up_from_the_first_commit_selects_the_wip_row_when_dirty() {
    let wip = Some(WipRow {
        files: 1,
        selected: false,
    });
    let mut harness = harness_full(Some(sample_graph()), wip, Some(oid(3)));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    harness.run();

    assert!(harness.state().wip_clicked);
    assert_eq!(harness.state().clicked, None);
}

#[test]
fn arrow_down_from_the_wip_row_selects_the_first_commit() {
    let wip = Some(WipRow {
        files: 1,
        selected: true,
    });
    let mut harness = harness_full(Some(sample_graph()), wip, None);
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(harness.state().clicked, Some(oid(3)));
}

#[test]
fn arrows_without_selection_take_the_first_row() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(harness.state().clicked, Some(oid(3)));
}

#[test]
fn modified_arrows_do_not_move_the_selection() {
    // ⌘↓ and the like stay reserved (panel focus, etc.): only bare ↑/↓ navigates.
    let mut harness = harness(sample_graph(), Some(oid(3)));
    harness.run();

    let cmd = egui::Modifiers {
        command: true,
        mac_cmd: true,
        ..Default::default()
    };
    harness.key_press_modifiers(cmd, egui::Key::ArrowDown);
    harness.run();

    assert_eq!(harness.state().clicked, None);
}

#[test]
fn disabled_keyboard_nav_ignores_arrows() {
    // The caller disables nav when a commit diff is open or the status sidebar
    // already holds the arrows.
    let mut harness = harness(sample_graph(), Some(oid(3)));
    harness.state_mut().keyboard_nav = false;
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(harness.state().clicked, None);
}

#[test]
fn arrow_navigation_scrolls_the_new_selection_into_view() {
    // Selection on the HEAD row at the very bottom (off-viewport after
    // rendering at the top): ↑ selects the row above and scrolls it into view.
    let mut harness = harness_full(Some(deep_head_graph()), None, Some(oid(1)));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    harness.run();

    assert_eq!(harness.state().clicked, Some(oid(2)));
    let screen = harness.ctx.content_rect();
    let row = harness.get_by_label("0000002 Above head").rect();
    assert!(
        screen.contains_rect(row),
        "row {row:?} scrolled into viewport {screen:?}"
    );
}

#[test]
fn double_click_on_local_branch_chip_requests_checkout() {
    let graph = Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(
                1,
                "First commit",
                vec![],
                vec![graph_ref("feat/x", RefKind::Local, false)],
            ),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    double_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("feat/x"));
}

#[test]
fn double_click_shortly_after_a_click_still_requests_checkout() {
    // A click (selecting the row) < 0.6 s before the double-click at the same
    // spot requalifies the double's 2nd click as a **triple** in egui's counter
    // (`last_last_click_time`): `button_double_clicked` alone would lose the
    // intent.
    let graph = Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(
                1,
                "First commit",
                vec![],
                vec![graph_ref("feat/x", RefKind::Local, false)],
            ),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    let pos = egui::pos2(row.left() + 20.0, row.center().y);
    // Single click (selection), then 2 frames (0.5 s simulated) before the double.
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    harness.step();
    harness.step();
    double_click_at(&mut harness, pos);
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("feat/x"));
}

#[test]
fn double_click_on_remote_chip_requests_checkout() {
    // The DWIM (local homonym, tracked creation) lives on the domain side: the
    // UI emits the remote name as-is.
    let graph = Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(
                1,
                "First commit",
                vec![],
                vec![graph_ref("origin/feat", RefKind::Remote, false)],
            ),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    double_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("origin/feat"));
}

#[test]
fn double_click_on_head_or_tag_chip_is_ignored() {
    // Sample: commit 3 = checked-out branch (main), commit 1 = tag v1.0 —
    // neither of these double-clicks should produce a checkout intent.
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let head_row = harness.get_by_label("0000003 Third commit").rect();
    double_click_at(
        &mut harness,
        egui::pos2(head_row.left() + 20.0, head_row.center().y),
    );
    harness.run();
    let tag_row = harness.get_by_label("0000001 First commit").rect();
    double_click_at(
        &mut harness,
        egui::pos2(tag_row.left() + 20.0, tag_row.center().y),
    );
    harness.run();

    assert_eq!(harness.state().checkout, None);
}

/// 2-commit graph: HEAD (`main`) at the top, a local branch `feat/x` (or the
/// passed ref) on the commit below.
fn two_branch_graph(gref: GraphRef) -> Graph {
    Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(1, "First commit", vec![], vec![gref]),
        ],
        has_more: false,
    }
}

#[test]
fn right_click_on_branch_chip_opens_menu_and_checkout_emits_intent() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Checkout").click();
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("feat/x"));
    assert!(
        harness.query_by_label("Checkout").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn right_click_on_available_branch_chip_can_create_worktree() {
    let mut gref = graph_ref("feat/x", RefKind::Local, false);
    gref.worktree_available = true;
    let mut harness = harness(two_branch_graph(gref), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Create worktree").click();
    harness.run();

    assert_eq!(harness.state().create_worktree.as_deref(), Some("feat/x"));
}

#[test]
fn right_click_on_branch_chip_can_rebase_onto_it() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Rebase onto feat/x").click();
    harness.run();

    assert_eq!(harness.state().rebase_onto.as_deref(), Some("feat/x"));
    assert!(
        harness.query_by_label("Rebase onto feat/x").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn right_click_on_branch_chip_can_open_an_interactive_rebase() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness
        .get_by_label("Interactive rebase onto feat/x")
        .click();
    harness.run();

    assert_eq!(
        harness.state().interactive_rebase_onto.as_deref(),
        Some("feat/x")
    );
    // The plain rebase intent stays untouched: nothing runs on the click.
    assert_eq!(harness.state().rebase_onto, None);
}

#[test]
fn right_click_on_branch_chip_can_open_an_ai_rebase() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("AI rebase onto feat/x").click();
    harness.run();

    assert_eq!(harness.state().ai_rebase_onto.as_deref(), Some("feat/x"));
    // The other rebase intents stay untouched: nothing runs on the click.
    assert_eq!(harness.state().rebase_onto, None);
    assert_eq!(harness.state().interactive_rebase_onto, None);
}

#[test]
fn right_click_on_branch_chip_can_merge_it_into_the_current_branch() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    // The entry names both sides: clicked branch → current branch.
    harness.get_by_label("Merge feat/x into main").click();
    harness.run();

    assert_eq!(harness.state().merge.as_deref(), Some("feat/x"));
    // The rebase intents stay untouched: nothing else runs on the click.
    assert_eq!(harness.state().rebase_onto, None);
    assert!(
        harness.query_by_label("Merge feat/x into main").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn right_click_on_branch_chip_can_open_a_pull_request_when_a_forge_is_configured() {
    // `origin` resolves to a known forge (can_pull_request): the clicked branch
    // is the PR destination, the current branch (main) the source.
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.state_mut().can_pull_request = true;
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness
        .get_by_label("Create pull request into feat/x")
        .click();
    harness.run();

    assert_eq!(
        harness.state().create_pull_request.as_deref(),
        Some("feat/x")
    );
    assert_eq!(harness.state().merge, None);
    assert!(
        harness
            .query_by_label("Create pull request into feat/x")
            .is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn pull_request_entry_is_hidden_without_a_recognized_forge() {
    // No forge (can_pull_request stays false): no Create pull request entry,
    // though the clicked branch would otherwise be eligible (Rebase shows).
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Rebase onto feat/x");
    assert!(
        harness
            .query_by_label_contains("Create pull request")
            .is_none(),
        "no forge ⇒ no Create pull request"
    );
}

#[test]
fn remote_chip_pull_request_targets_the_branch_without_its_remote_prefix() {
    // The destination handed to the forge is the branch name on the remote: the
    // chip's `origin/` prefix is stripped (the label still names the chip).
    let mut harness = harness(
        two_branch_graph(graph_ref("origin/feat", RefKind::Remote, false)),
        None,
    );
    harness.state_mut().can_pull_request = true;
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness
        .get_by_label("Create pull request into origin/feat")
        .click();
    harness.run();

    assert_eq!(harness.state().create_pull_request.as_deref(), Some("feat"));
}

#[test]
fn the_head_chip_offers_no_pull_request_even_with_a_forge() {
    // A PR into the current branch makes no sense: the head chip never offers it.
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().can_pull_request = true;
    harness.run();

    let head_row = harness.get_by_label("0000003 Third commit").rect();
    right_click_at(
        &mut harness,
        egui::pos2(head_row.left() + 20.0, head_row.center().y),
    );
    harness.run();

    assert!(
        harness
            .query_by_label_contains("Create pull request")
            .is_none(),
        "a branch never opens a PR into itself"
    );
}

#[test]
fn right_click_on_branch_chip_can_create_a_branch() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Create branch").click();
    harness.run();

    // The entry opens the inline editor on the targeted ref's row, carrying the
    // fully-qualified source. The field opens empty (no pre-fill).
    assert_eq!(
        harness.state().open_branch_editor,
        Some(CreateBranchRequest {
            oid: oid(1),
            source: "refs/heads/feat/x".into(),
        })
    );
    assert!(
        harness.query_by_label("Create branch").is_none(),
        "the menu closes once the entry is activated"
    );
    assert!(harness.state().editor.open);
    assert_eq!(harness.state().editor.name, "");

    // A typed name + Enter creates the branch at the source, without checkout.
    harness.run();
    harness.state_mut().editor.name = "release".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().create_branch_at.as_deref(), Some("release"));
    assert_eq!(harness.state().create_branch, None);
}

#[test]
fn create_tag_entry_opens_the_tag_editor_and_enter_emits_the_create_intent() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    // The ref-less "Second commit" row: its commit menu offers Create tag.
    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Create tag").click();
    harness.run();

    // The entry opens the inline editor (tag mode) on that commit's row, empty.
    assert_eq!(harness.state().open_tag_editor, Some(oid(2)));
    assert!(harness.state().editor.open);
    assert!(harness.state().editor.tag);
    assert_eq!(harness.state().editor.name, "");
    assert!(
        harness.query_by_label("Create tag").is_none(),
        "the menu closes once the entry is activated"
    );

    // A typed name + Enter tags the commit — never a branch.
    harness.run();
    harness.state_mut().editor.name = "v2.0".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().create_tag_at.as_deref(), Some("v2.0"));
    assert_eq!(harness.state().create_branch_at, None);
    assert_eq!(harness.state().create_branch, None);
}

#[test]
fn the_tag_editor_refuses_a_name_git_rejects_for_a_tag() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();
    harness.get_by_label("Create tag").click();
    harness.run();

    // `git tag` refuses a leading dash even though `refs/tags/-rc1` passes
    // check-ref-format: the editor validates with the tag rules, and names its
    // error after what the field creates.
    harness.state_mut().editor.name = "-rc1".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().create_tag_at, None);
    harness.get_by_label("Invalid tag name");

    harness.state_mut().editor.name = "v1.0".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().create_tag_at.as_deref(), Some("v1.0"));
}

#[test]
fn row_menu_cherry_pick_emits_the_target_commit() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    // The ref-less "Second commit" row (single parent); HEAD is on `main`, so the
    // commit menu offers Cherry-pick / Revert.
    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Cherry-pick").click();
    harness.run();

    assert_eq!(harness.state().cherry_pick, Some(oid(2)));
    assert_eq!(harness.state().revert, None);
}

#[test]
fn row_menu_revert_emits_the_target_commit() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Revert").click();
    harness.run();

    assert_eq!(harness.state().revert, Some(oid(2)));
    assert_eq!(harness.state().cherry_pick, None);
}

#[test]
fn row_menu_hides_cherry_pick_and_revert_on_a_merge_commit() {
    // A ref-less merge of b and a (so the right-click lands on the row, not a
    // chip): an ambiguous mainline, no replay offered. HEAD stays on `main`.
    let graph = Graph {
        commits: vec![
            commit(
                5,
                "Tip commit",
                vec![oid(4)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(4, "Merge commit", vec![oid(2), oid(1)], vec![]),
            commit(2, "Second commit", vec![oid(1)], vec![]),
            commit(1, "First commit", vec![], vec![]),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000004 Merge commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Copy commit SHA");
    assert!(harness.query_by_label("Cherry-pick").is_none());
    assert!(harness.query_by_label("Revert").is_none());
}

#[test]
fn row_menu_hides_cherry_pick_and_revert_when_head_is_detached() {
    // The synthetic "HEAD" ref ⇒ no checked-out branch to replay onto.
    let graph = Graph {
        commits: vec![
            commit(
                3,
                "Third commit",
                vec![oid(2)],
                vec![graph_ref("HEAD", RefKind::Local, true)],
            ),
            commit(2, "Second commit", vec![oid(1)], vec![]),
            commit(1, "First commit", vec![], vec![]),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Copy commit SHA");
    assert!(harness.query_by_label("Cherry-pick").is_none());
    assert!(harness.query_by_label("Revert").is_none());
}

#[test]
fn row_menu_reset_submenu_names_the_branch_and_emits_the_chosen_mode() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    // The ref-less "Second commit" row; HEAD is on `main`, so the commit menu
    // nests "Reset main to here" with the three git flavors.
    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Reset main to here ⏵").click();
    harness.run();

    harness.get_by_label("Mixed").click();
    harness.run();

    assert_eq!(
        harness.state().reset,
        Some((oid(2), git2::ResetType::Mixed))
    );
}

#[test]
fn row_menu_reset_hard_emits_the_hard_mode() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Reset main to here ⏵").click();
    harness.run();

    harness.get_by_label("Hard").click();
    harness.run();

    assert_eq!(harness.state().reset, Some((oid(2), git2::ResetType::Hard)));
}

#[test]
fn row_menu_hides_reset_when_head_is_detached() {
    // The synthetic "HEAD" ref ⇒ no checked-out branch to reset.
    let graph = Graph {
        commits: vec![
            commit(
                3,
                "Third commit",
                vec![oid(2)],
                vec![graph_ref("HEAD", RefKind::Local, true)],
            ),
            commit(2, "Second commit", vec![oid(1)], vec![]),
            commit(1, "First commit", vec![], vec![]),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Copy commit SHA");
    assert!(harness.query_by_label("Reset main to here ⏵").is_none());
}

#[test]
fn branch_chip_rename_pre_fills_the_editor_and_enter_emits_the_rename() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    // HEAD is on the lone `main` chip ⇒ its menu offers a flat "Rename".
    let row = harness.get_by_label("0000003 Third commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Rename").click();
    harness.run();

    // The entry opens the inline editor on the branch's row, pre-filled with the
    // current name (carried with the row's oid). Nothing is sent yet.
    assert_eq!(
        harness.state().open_rename,
        Some(RenameRequest {
            oid: oid(3),
            name: "main".into(),
        })
    );
    assert!(harness.state().editor.open);
    assert_eq!(harness.state().editor.name, "main");
    assert_eq!(harness.state().rename_branch, None);
    assert!(
        harness.query_by_label("Rename").is_none(),
        "the menu closes once the entry is activated"
    );

    // Editing the pre-filled name + Enter emits the rename from old to new.
    harness.run();
    harness.state_mut().editor.name = "trunk".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().rename_branch,
        Some(("main".into(), "trunk".into()))
    );
}

#[test]
fn right_click_on_tag_chip_offers_the_tag_actions_but_no_branch_ones() {
    // The tag sits on the bottom commit of the sample graph (a → v1.0).
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    // A tag carries Checkout (detached), Create branch and the tag-only actions —
    // but none of the branch ones (copy *branch* name, rebase, merge).
    harness.get_by_label("Checkout");
    harness.get_by_label("Create branch");
    harness.get_by_label("Copy tag name");
    harness.get_by_label("Push tag");
    harness.get_by_label("Delete tag");
    assert!(harness.query_by_label("Copy branch name").is_none());
    assert!(harness.query_by_label("Rebase onto v1.0").is_none());

    harness.get_by_label("Create branch").click();
    harness.run();

    assert_eq!(
        harness.state().open_branch_editor,
        Some(CreateBranchRequest {
            oid: oid(1),
            source: "refs/tags/v1.0".into(),
        })
    );
}

#[test]
fn tag_chip_checkout_emits_the_detached_checkout_intent() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Checkout").click();
    harness.run();

    // A tag detaches HEAD: the dedicated tag intent fires, never the branch one.
    assert_eq!(harness.state().checkout_tag.as_deref(), Some("v1.0"));
    assert_eq!(harness.state().checkout, None);
}

#[test]
fn tag_chip_copy_tag_name_copies_to_the_clipboard() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Copy tag name").click();
    harness.step();

    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == "v1.0"));
    assert!(copied, "the tag name goes to the clipboard");
}

#[test]
fn tag_chip_push_tag_emits_the_push_intent() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Push tag").click();
    harness.run();

    assert_eq!(harness.state().push_tag.as_deref(), Some("v1.0"));
}

#[test]
fn tag_chip_delete_tag_emits_the_target_for_the_modal() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete tag").click();
    harness.run();

    assert_eq!(harness.state().delete_tag.as_deref(), Some("v1.0"));
    assert!(
        harness.query_by_label("Delete tag").is_none(),
        "the menu closes once the entry is activated"
    );
}

/// The menu state lives in egui memory, outside any session: a repo switch closes
/// it from the app, otherwise its entries keep naming the previous repo's refs.
#[test]
fn close_chip_menu_dismisses_the_open_menu() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();
    assert!(harness.query_by_label("Checkout").is_some());

    close_chip_menu(&harness.ctx);
    harness.run();

    assert!(harness.query_by_label("Checkout").is_none());
}

#[test]
fn the_chip_menu_opens_below_the_chip_not_over_it() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    // The chip (CHIP_HEIGHT = 22) is centered on the row: the menu anchors
    // below its bottom edge — never overlapping the label.
    let chip_bottom = row.center().y + 11.0;
    let menu_entry = harness.get_by_label("Checkout").rect();
    assert!(
        menu_entry.top() > chip_bottom,
        "Checkout entry {menu_entry:?} above the chip bottom {chip_bottom}"
    );
}

#[test]
fn right_click_menu_on_remote_chip_emits_the_remote_name() {
    let mut harness = harness(
        two_branch_graph(graph_ref("origin/feat", RefKind::Remote, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Checkout").click();
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("origin/feat"));
}

#[test]
fn copy_branch_name_copies_to_the_clipboard() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Copy branch name").click();
    harness.step();

    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == "feat/x"));
    assert!(copied, "the branch name goes to the clipboard");
    assert_eq!(harness.state().checkout, None);
}

#[test]
fn right_click_on_the_head_chip_offers_copy_but_no_checkout() {
    // The current branch is not checked out (already on it): the menu only
    // offers copying the name.
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let head_row = harness.get_by_label("0000003 Third commit").rect();
    right_click_at(
        &mut harness,
        egui::pos2(head_row.left() + 20.0, head_row.center().y),
    );
    harness.run();

    harness.get_by_label("Copy branch name");
    assert!(harness.query_by_label("Checkout").is_none());
    assert!(
        harness.query_by_label("Rebase onto main").is_none(),
        "a branch never rebases onto itself"
    );
    assert!(
        harness
            .query_by_label("Interactive rebase onto main")
            .is_none(),
        "a branch never rebases onto itself"
    );
    assert!(
        harness.query_by_label("AI rebase onto main").is_none(),
        "a branch never rebases onto itself"
    );
    assert!(
        harness.query_by_label("Merge main into main").is_none(),
        "a branch never merges into itself"
    );
}

#[test]
fn delete_branch_entry_emits_the_local_intent() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    // Local without a remote homonym: a single Delete entry, no combined one.
    assert!(harness.query_by_label_contains(" and ").is_none());
    harness.get_by_label("Delete feat/x").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Local("feat/x".into()))
    );
    assert!(
        harness.query_by_label("Delete feat/x").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn delete_on_remote_entry_emits_the_chip_name() {
    let mut harness = harness(
        two_branch_graph(graph_ref("origin/feat", RefKind::Remote, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    // Remote chip without a local homonym: no local deletion.
    assert!(harness.query_by_label("Delete feat").is_none());
    harness.get_by_label("Delete origin/feat").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Remote("origin/feat".into()))
    );
}

#[test]
fn merged_local_chip_offers_the_three_named_deletions() {
    // Local chip merged with its remote homonym: three named entries — local,
    // remote (full name), and the combined one.
    let gref = GraphRef {
        also_remote: true,
        counterpart: Some("origin/feat/x".to_string()),
        ..graph_ref("feat/x", RefKind::Local, false)
    };
    let mut harness = harness(two_branch_graph(gref), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete feat/x");
    harness.get_by_label("Delete feat/x and origin/feat/x");
    harness.get_by_label("Delete origin/feat/x").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Remote("origin/feat/x".into()))
    );
}

#[test]
fn combined_entry_emits_both_targets() {
    let gref = GraphRef {
        counterpart: Some("origin/feat".to_string()),
        ..graph_ref("feat", RefKind::Local, false)
    };
    let mut harness = harness(two_branch_graph(gref), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete feat and origin/feat").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Both {
            local: "feat".into(),
            remote: "origin/feat".into(),
        })
    );
}

#[test]
fn remote_chip_with_a_local_homonym_offers_both_deletions() {
    // Branch present on both sides but diverged (separate chips): the remote
    // chip also offers the **local** deletion, named with the local name.
    let gref = GraphRef {
        counterpart: Some("feat".to_string()),
        ..graph_ref("origin/feat", RefKind::Remote, false)
    };
    let mut harness = harness(two_branch_graph(gref), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete origin/feat");
    harness.get_by_label("Delete feat and origin/feat");
    harness.get_by_label("Delete feat").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Local("feat".into()))
    );
}

#[test]
fn diverged_local_chip_offers_the_remote_deletion_too() {
    // Local paired with a remote homonym on a different commit (no
    // `also_remote` merge): the remote entry is offered anyway, full name.
    let gref = GraphRef {
        counterpart: Some("origin/feat".to_string()),
        ..graph_ref("feat", RefKind::Local, false)
    };
    let mut harness = harness(two_branch_graph(gref), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete feat");
    harness.get_by_label("Delete origin/feat").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Remote("origin/feat".into()))
    );
}

#[test]
fn the_head_chip_offers_no_local_deletion() {
    // The current branch cannot be deleted (git refuses): no Delete entry —
    // and without a remote homonym, no remote entry either.
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let head_row = harness.get_by_label("0000003 Third commit").rect();
    right_click_at(
        &mut harness,
        egui::pos2(head_row.left() + 20.0, head_row.center().y),
    );
    harness.run();

    harness.get_by_label("Copy branch name");
    assert!(harness.query_by_label_contains("Delete").is_none());
}

fn delete_modal_harness(target: DeleteBranchTarget) -> Harness<'static, DeleteModalAction> {
    Harness::new_ui_state(
        move |ui, state| {
            let palette = Palette::dark();
            delete_branch_modal(ui, &palette, &target, state);
        },
        DeleteModalAction::default(),
    )
}

#[test]
fn local_delete_modal_confirms_with_the_red_button() {
    let mut harness = delete_modal_harness(DeleteBranchTarget::Local("feat/x".into()));
    harness.run();

    harness.get_by_label("Delete branch “feat/x”?");
    harness.get_by_label("Delete").click();
    harness.run();

    assert!(harness.state().confirm);
}

#[test]
fn remote_delete_modal_cancel_dismisses_without_confirming() {
    let mut harness = delete_modal_harness(DeleteBranchTarget::Remote("origin/feat".into()));
    harness.run();

    harness.get_by_label("Delete “origin/feat” on the remote?");
    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().dismiss);
    assert!(!harness.state().confirm);
}

#[test]
fn both_delete_modal_names_the_two_branches() {
    let mut harness = delete_modal_harness(DeleteBranchTarget::Both {
        local: "feat".into(),
        remote: "origin/feat".into(),
    });
    harness.run();

    harness.get_by_label("Delete “feat” and “origin/feat”?");
    harness.get_by_label("Delete").click();
    harness.run();

    assert!(harness.state().confirm);
}

#[test]
fn delete_tag_modal_offers_the_origin_option_and_confirms() {
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut (bool, DeleteModalAction)| {
            let palette = Palette::dark();
            delete_tag_modal(ui, &palette, "v1.0", true, &mut state.0, &mut state.1);
        },
        (false, DeleteModalAction::default()),
    );
    harness.run();

    harness.get_by_label("Delete tag “v1.0”?");
    harness.get_by_label("Also delete on origin").click();
    harness.run();
    assert!(
        harness.state().0,
        "ticking the box selects the remote deletion"
    );

    harness.get_by_label("Delete").click();
    harness.run();
    assert!(harness.state().1.confirm);
}

#[test]
fn delete_tag_modal_without_a_remote_hides_the_origin_option() {
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut (bool, DeleteModalAction)| {
            let palette = Palette::dark();
            delete_tag_modal(ui, &palette, "v1.0", false, &mut state.0, &mut state.1);
        },
        (false, DeleteModalAction::default()),
    );
    harness.run();

    harness.get_by_label("Delete tag “v1.0”?");
    assert!(
        harness.query_by_label("Also delete on origin").is_none(),
        "no remote ⇒ no origin option"
    );
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.state().1.dismiss);
    assert!(!harness.state().1.confirm);
}

/// Graph with a stash row (no chip) above its base commit.
fn stash_graph() -> Graph {
    Graph {
        commits: vec![
            GraphCommit {
                stash: true,
                ..commit(9, "WIP on main: stashed work", vec![oid(1)], vec![])
            },
            commit(1, "First commit", vec![], vec![]),
        ],
        has_more: false,
    }
}

#[test]
fn right_click_on_a_stash_row_offers_pop_and_pop_emits_the_intent() {
    let mut harness = harness(stash_graph(), None);
    harness.run();

    let row = harness
        .get_by_label("0000009 WIP on main: stashed work")
        .rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete stash");
    assert!(
        harness.query_by_label("Copy commit SHA").is_none(),
        "a stash row never carries the commit actions"
    );
    harness.get_by_label("Pop stash").click();
    harness.run();

    assert_eq!(harness.state().stash_pop, Some(oid(9)));
    assert!(
        harness.query_by_label("Pop stash").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn apply_stash_entry_emits_the_apply_intent_without_dropping() {
    let mut harness = harness(stash_graph(), None);
    harness.run();

    let row = harness
        .get_by_label("0000009 WIP on main: stashed work")
        .rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Apply stash").click();
    harness.run();

    assert_eq!(harness.state().stash_apply, Some(oid(9)));
    assert!(
        harness.state().stash_pop.is_none(),
        "apply is the no-drop twin of pop — it never pops"
    );
    assert!(
        harness.query_by_label("Apply stash").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn delete_stash_entry_emits_the_target_for_the_modal() {
    let mut harness = harness(stash_graph(), None);
    harness.run();

    let row = harness
        .get_by_label("0000009 WIP on main: stashed work")
        .rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    harness.get_by_label("Delete stash").click();
    harness.run();

    assert_eq!(
        harness.state().stash_drop,
        Some(StashTarget {
            oid: oid(9),
            summary: "WIP on main: stashed work".into(),
        })
    );
    assert!(harness.state().stash_pop.is_none(), "delete never pops");
}

#[test]
fn right_click_on_a_ref_less_row_opens_the_commit_menu_and_copies_the_sha() {
    // The base commit (not a stash, no branch) now opens a commit-actions menu:
    // Copy commit SHA, and never the stash entries.
    let mut harness = harness(stash_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    assert!(harness.query_by_label("Pop stash").is_none());
    assert!(harness.query_by_label("Delete stash").is_none());

    harness.get_by_label("Copy commit SHA").click();
    harness.step();

    let copied =
        harness.output().platform_output.commands.iter().any(
            |c| matches!(c, egui::OutputCommand::CopyText(text) if *text == oid(1).to_string()),
        );
    assert!(copied, "the full commit hash goes to the clipboard");
}

#[test]
fn right_click_on_the_wip_row_opens_no_commit_menu() {
    let wip = Some(WipRow {
        files: 1,
        selected: false,
    });
    let mut harness = harness_full(Some(sample_graph()), wip, None);
    harness.run();

    let row = harness.get_by_label("// WIP · 1 file").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();

    assert!(harness.query_by_label("Copy commit SHA").is_none());
}

#[test]
fn delete_stash_modal_confirms_with_the_red_button_and_cancel_dismisses() {
    let target = StashTarget {
        oid: oid(9),
        summary: "WIP on main: stashed work".into(),
    };
    let confirm_target = target.clone();
    let mut harness = Harness::new_ui_state(
        move |ui, state: &mut DeleteModalAction| {
            let palette = Palette::dark();
            delete_stash_modal(ui, &palette, &confirm_target, state);
        },
        DeleteModalAction::default(),
    );
    harness.run();

    harness.get_by_label("Delete stash “WIP on main: stashed work”?");
    harness.get_by_label("Delete").click();
    harness.run();
    assert!(harness.state().confirm);

    let mut harness = Harness::new_ui_state(
        move |ui, state: &mut DeleteModalAction| {
            let palette = Palette::dark();
            delete_stash_modal(ui, &palette, &target, state);
        },
        DeleteModalAction::default(),
    );
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.state().dismiss);
    assert!(!harness.state().confirm);
}

/// Graph whose last commit carries 2 refs: a single visible chip + `+1`, the
/// second only appears on expansion (hovering the refs zone).
fn multi_ref_graph() -> Graph {
    Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(
                1,
                "First commit",
                vec![],
                vec![
                    graph_ref("feat/x", RefKind::Local, false),
                    graph_ref("origin/feat", RefKind::Remote, false),
                ],
            ),
        ],
        has_more: false,
    }
}

#[test]
fn the_expanded_chips_stay_open_while_the_pointer_travels_to_them() {
    // Expanded chips stack **below** the row: reaching the 2nd chip takes the
    // pointer out of the refs zone — the overlay must stay expanded (hovering
    // it sustains the expansion), otherwise the `+N` is unreachable.
    let mut harness = harness(multi_ref_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    // 2nd expanded chip: one chip height (22) + gap (4) below the first.
    let second_chip = egui::pos2(row.left() + 20.0, row.center().y + 26.0);
    move_pointer_to(&mut harness, second_chip);
    right_click_at(&mut harness, second_chip);
    harness.run();

    harness.get_by_label("Checkout").click();
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("origin/feat"));
}

#[test]
fn right_click_menu_on_an_expanded_chip_keeps_the_chip_visible() {
    // Menu opened from the expanded overlay: the row stays expanded down to the
    // targeted chip (the label used to disappear, collapsed by the hover
    // freeze). Hit-test proof: a second right-click at the same spot lands back
    // on the chip and re-arms the menu — collapsed, it would hit empty space
    // and the click outside the menu would close it.
    let mut harness = harness(multi_ref_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    let second_chip = egui::pos2(row.left() + 20.0, row.center().y + 26.0);
    move_pointer_to(&mut harness, second_chip);
    right_click_at(&mut harness, second_chip);
    harness.run();
    harness.get_by_label("Checkout");

    right_click_at(&mut harness, second_chip);
    harness.run();

    harness.get_by_label("Checkout");
    assert_eq!(harness.state().checkout, None);
}

#[test]
fn the_expanded_chips_collapse_once_the_pointer_leaves() {
    let mut harness = harness(multi_ref_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    let second_chip = egui::pos2(row.left() + 20.0, row.center().y + 26.0);
    move_pointer_to(&mut harness, second_chip);
    // Pointer gone far from both the refs zone and the overlay: collapse — a
    // right-click at the 2nd chip's old position must no longer hit anything.
    let outside = harness.ctx.content_rect().right_top() + egui::vec2(-4.0, 4.0);
    move_pointer_to(&mut harness, outside);
    right_click_at(&mut harness, second_chip);
    harness.run();

    assert!(
        harness.query_by_label("Copy branch name").is_none(),
        "collapsed: no more chip under the row, the right-click opens nothing"
    );
}

#[test]
fn right_click_on_a_row_opens_the_menu_of_its_branch() {
    // Right-click in the message column, far from the chips: same menu as the
    // chip, flat — the row carries a single branch.
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, row.center());
    harness.run();

    harness.get_by_label("Delete feat/x");
    harness.get_by_label("Checkout").click();
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("feat/x"));
}

#[test]
fn right_click_on_a_row_without_branches_opens_no_menu() {
    let mut harness = harness(sample_graph(), None);
    harness.run();

    // No ref at all, then a tag-only row: no branch action either way.
    let bare_row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, bare_row.center());
    harness.run();
    assert!(harness.query_by_label("Copy branch name").is_none());

    let tag_row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, tag_row.center());
    harness.run();
    assert!(harness.query_by_label("Copy branch name").is_none());
}

#[test]
fn row_menu_with_several_branches_nests_the_actions() {
    // Several branches on the row: the branch actions fold into Checkout /
    // Copy branch name / Delete submenus — never a flat per-branch entry at
    // the top level.
    let mut harness = harness(multi_ref_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, row.center());
    harness.run();

    assert!(harness.query_by_label("Delete feat/x").is_none());
    harness.get_by_label("Checkout ⏵");
    harness.get_by_label("Copy branch name ⏵");
    harness.get_by_label("Delete ⏵").click();
    harness.run();

    harness.get_by_label("Delete origin/feat");
    harness.get_by_label("Delete feat/x").click();
    harness.run();

    assert_eq!(
        harness.state().delete,
        Some(DeleteBranchTarget::Local("feat/x".into()))
    );
    assert!(
        harness.query_by_label("Delete ⏵").is_none(),
        "the menu closes once the entry is activated"
    );
}

#[test]
fn row_menu_width_is_comfortable_and_long_branch_entries_stay_single_line() {
    let long =
        "feature/super-long-branch-name-used-to-verify-context-menu-truncation-without-wrapping";
    let graph = Graph {
        commits: vec![commit(
            1,
            "First commit",
            vec![],
            vec![
                graph_ref("feat/x", RefKind::Local, false),
                graph_ref(long, RefKind::Local, false),
            ],
        )],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, row.center());
    harness.run();

    let checkout = harness.get_by_label("Checkout ⏵").rect();
    assert!(checkout.width() >= 220.0);
    harness.get_by_label("Checkout ⏵").click();
    harness.run();

    let branch = harness.get_by_label(long).rect();
    assert!(branch.width() >= 220.0);
    assert!(branch.width() <= 361.0);
    assert!(branch.height() <= checkout.height() + 1.0);
}

#[test]
fn row_menu_checkout_submenu_skips_the_current_branch() {
    // HEAD shares its row with another branch: the Checkout submenu lists only
    // the other one (the current branch is not checkoutable).
    let graph = Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![
                    graph_ref("main", RefKind::Local, true),
                    graph_ref("feat/y", RefKind::Local, false),
                ],
            ),
            commit(1, "First commit", vec![], vec![]),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000002 Second commit").rect();
    right_click_at(&mut harness, row.center());
    harness.run();

    harness.get_by_label("Checkout ⏵").click();
    harness.run();

    assert!(
        harness.query_by_label("main").is_none(),
        "HEAD is not checkoutable"
    );
    harness.get_by_label("feat/y").click();
    harness.run();

    assert_eq!(harness.state().checkout.as_deref(), Some("feat/y"));
}

/// Two-ref row directly **above** a row carrying its own branch chip: the
/// expanded overlay's 2nd chip covers the row below and its inline chip.
fn overlay_over_chip_graph() -> Graph {
    Graph {
        commits: vec![
            commit(
                3,
                "Top commit",
                vec![oid(2)],
                vec![
                    graph_ref("feat/x", RefKind::Local, false),
                    graph_ref("origin/feat", RefKind::Remote, false),
                ],
            ),
            commit(
                2,
                "Below commit",
                vec![oid(1)],
                vec![graph_ref("victim", RefKind::Local, false)],
            ),
            commit(
                1,
                "First commit",
                vec![],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
        ],
        has_more: false,
    }
}

#[test]
fn right_click_on_an_expanded_chip_over_another_row_targets_that_chip() {
    // The expanded 2nd chip covers the row below and its inline chip: the
    // right-click belongs to the overlay — it used to open the covered chip's
    // menu instead (manual hit-test, the last row processed stole the claim).
    let mut harness = harness(overlay_over_chip_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000003 Top commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    let second_chip = egui::pos2(row.left() + 20.0, row.center().y + 26.0);
    move_pointer_to(&mut harness, second_chip);
    right_click_at(&mut harness, second_chip);
    harness.run();

    harness.get_by_label("Checkout").click();
    harness.run();

    assert_eq!(
        harness.state().checkout.as_deref(),
        Some("origin/feat"),
        "the expanded chip wins over the covered row's chip"
    );
}

#[test]
fn clicking_an_expanded_chip_over_another_row_does_not_select_that_row() {
    // Same occlusion as the right-click twin: the covered row's `clicked()` used
    // to fire under the overlay, selecting a commit the user never pointed at.
    let mut harness = harness(overlay_over_chip_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000003 Top commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    let second_chip = egui::pos2(row.left() + 20.0, row.center().y + 26.0);
    move_pointer_to(&mut harness, second_chip);
    double_click_at(&mut harness, second_chip);
    harness.run();

    assert_eq!(
        harness.state().checkout.as_deref(),
        Some("origin/feat"),
        "the click landed on the expanded chip"
    );
    assert_eq!(
        harness.state().clicked,
        None,
        "the row covered by the overlay is not selected"
    );
}

#[test]
fn clicking_outside_closes_the_chip_menu() {
    let mut harness = harness(
        two_branch_graph(graph_ref("feat/x", RefKind::Local, false)),
        None,
    );
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    right_click_at(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));
    harness.run();
    harness.get_by_label("Checkout");

    // Primary click far from the menu (top-right corner of the view): closes without intent.
    let outside = harness.ctx.content_rect().right_top() + egui::vec2(-4.0, 4.0);
    click_at(&mut harness, outside);
    harness.run();

    assert!(harness.query_by_label("Checkout").is_none());
    assert_eq!(harness.state().checkout, None);
}

#[test]
fn wide_history_caps_the_graph_column_by_default() {
    // Graph column capped by default (~16 lanes): below the cap it follows the
    // natural width; beyond it, 20 lanes widen no more than 16 — the message
    // column stays readable.
    let header_left = |lanes: u8| {
        let mut harness = harness(wide_graph(lanes), None);
        harness.run();
        message_header_left(&harness)
    };

    assert!(header_left(3) < header_left(16));
    assert_eq!(header_left(16), header_left(20));
}

#[test]
fn dragging_the_column_boundary_resizes_the_graph_zone() {
    let mut harness = harness(wide_graph(20), None);
    harness.run();

    // The handle straddles the graph ⇄ message boundary, just before the COMMIT
    // MESSAGE header (TEXT_GAP = 10).
    let before = message_header_left(&harness);
    let y = harness.ctx.content_rect().center().y;
    drag_horizontal(&mut harness, egui::pos2(before - 10.0, y), 60.0);
    harness.run();
    let widened = message_header_left(&harness);
    assert!(
        (widened - (before + 60.0)).abs() < 0.5,
        "widened by 60: {before} -> {widened}"
    );

    drag_horizontal(&mut harness, egui::pos2(widened - 10.0, y), -90.0);
    harness.run();
    let narrowed = message_header_left(&harness);
    assert!(
        (narrowed - (widened - 90.0)).abs() < 0.5,
        "narrowed by 90: {widened} -> {narrowed}"
    );
}

/// HEAD on the **2nd** row: checks that the Branch editor anchors on the HEAD
/// row, not the first.
fn head_on_second_row_graph() -> Graph {
    Graph {
        commits: vec![
            commit(
                3,
                "Above head",
                vec![oid(2)],
                vec![graph_ref("feature", RefKind::Local, false)],
            ),
            commit(
                2,
                "Head commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(1, "First commit", vec![], vec![]),
        ],
        has_more: false,
    }
}

/// Branch editor field: the view's only TextInput.
fn editor_field_rect(harness: &Harness<'_, ViewState>) -> egui::Rect {
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .rect()
}

#[test]
fn branch_editor_field_sits_on_the_head_row_in_the_refs_column() {
    // git.md §10: the field sits in the BRANCH / TAG column, on the HEAD row —
    // exactly where the new branch's chip will appear.
    let mut harness = harness(head_on_second_row_graph(), None);
    harness.state_mut().editor.open = true;
    harness.run();

    let field = editor_field_rect(&harness);
    let head_row = harness.get_by_label("0000002 Head commit").rect();
    assert!(
        (field.center().y - head_row.center().y).abs() <= 1.0,
        "field {field:?} centered on the HEAD row {head_row:?}"
    );
    assert!(
        field.left() >= head_row.left() && field.right() <= head_row.left() + 245.0,
        "field {field:?} in the BRANCH / TAG column (245px from {})",
        head_row.left()
    );
}

#[test]
fn branch_editor_enter_emits_create_branch_and_waits() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().editor.open = true;
    harness.run();
    harness.state_mut().editor.name = "feature/x".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();

    assert_eq!(harness.state().create_branch.as_deref(), Some("feature/x"));
    let editor = &harness.state().editor;
    assert!(editor.open, "stays open while waiting for the worker");
    assert!(editor.pending);
    assert_eq!(editor.error, None);
}

#[test]
fn branch_editor_rejects_an_invalid_name_inline() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().editor.open = true;
    harness.run();
    harness.state_mut().editor.name = "with space".into();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();

    harness.get_by_label("Invalid branch name");
    assert_eq!(harness.state().create_branch, None);
    let editor = &harness.state().editor;
    assert!(editor.open);
    assert!(!editor.pending);
    assert_eq!(editor.error.as_deref(), Some("Invalid branch name"));
}

#[test]
fn branch_editor_escape_cancels() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().editor.open = true;
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Escape);
    harness.run();

    assert!(!harness.state().editor.open);
    assert_eq!(harness.state().create_branch, None);
}

#[test]
fn branch_editor_click_elsewhere_cancels() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().editor.open = true;
    harness.run();
    let away = harness.get_by_label("0000001 First commit").rect().center();
    click_at(&mut harness, away);
    harness.run();

    assert!(!harness.state().editor.open);
    assert_eq!(harness.state().create_branch, None);
}

#[test]
fn branch_editor_replaces_the_head_row_chips() {
    // Painted chips + manual hit-test (not widgets): during editing they are
    // removed, otherwise a right-click in the field would open the context menu
    // of the covered chip.
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().editor.open = true;
    harness.run();
    let field = editor_field_rect(&harness);
    right_click_at(&mut harness, field.center());
    harness.run();

    assert!(harness.query_by_label("Copy branch name").is_none());
    assert!(
        harness.state().editor.open,
        "click in the field: stays open"
    );
}

#[test]
fn branch_editor_without_a_head_row_in_the_page_closes() {
    // HEAD beyond the loaded page (git.md §9): no anchor for the field, the
    // editor closes instead of staying open and invisible.
    let graph = Graph {
        commits: vec![commit(2, "Tip without head", vec![oid(1)], vec![])],
        has_more: true,
    };
    let mut harness = harness(graph, None);
    harness.state_mut().editor.open = true;
    harness.step();

    assert!(!harness.state().editor.open);
}

#[test]
fn hovering_a_commit_row_keeps_the_default_cursor() {
    // Graph rows are clickable (selection) but do not show the pointer — only
    // the chips display it (revised decision).
    let mut harness = harness(wide_graph(2), None);
    harness.run();

    let pos = harness.get_by_label_contains("Tip 0").rect().center();
    move_pointer_to(&mut harness, pos);

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::Default
    );
}

#[test]
fn hovering_a_branch_chip_shows_the_pointer_cursor() {
    let graph = Graph {
        commits: vec![
            commit(
                2,
                "Second commit",
                vec![oid(1)],
                vec![graph_ref("main", RefKind::Local, true)],
            ),
            commit(
                1,
                "First commit",
                vec![],
                vec![graph_ref("feat/x", RefKind::Local, false)],
            ),
        ],
        has_more: false,
    };
    let mut harness = harness(graph, None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
}

#[test]
fn hovering_a_tag_chip_shows_the_pointer_cursor() {
    // A tag has no left-click action, but it is interactive (right-click menu:
    // Checkout / Push / Delete tag), so it shows the pointer like any other chip.
    let mut harness = harness(sample_graph(), None);
    harness.run();

    let row = harness.get_by_label("0000001 First commit").rect();
    move_pointer_to(&mut harness, egui::pos2(row.left() + 20.0, row.center().y));

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
}

#[test]
fn hovering_the_column_boundary_keeps_the_resize_cursor() {
    let mut harness = harness(wide_graph(12), None);
    harness.run();

    // Same geometry as the drag: the handle straddles the boundary, just before
    // the COMMIT MESSAGE header (TEXT_GAP = 10). The handle must keep its Resize
    // cursor despite the pointer resting on a clickable row.
    let x = message_header_left(&harness) - 10.0;
    let y = harness.ctx.content_rect().center().y;
    move_pointer_to(&mut harness, egui::pos2(x, y));

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::ResizeHorizontal
    );
}

#[test]
fn cmd_f_opens_the_search_box() {
    let mut harness = harness(sample_graph(), None);
    harness.run();
    assert!(harness.query_by_label("Close search").is_none());

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run();

    assert!(harness.state().search.open);
    // The box rendered: its close button is present.
    harness.get_by_label("Close search");
}

#[test]
fn search_query_counts_the_matches() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().search.open = true;
    // Every summary in `sample_graph` contains "commit" (First/Second/Third).
    harness.state_mut().search.query = "commit".into();
    harness.run();

    harness.get_by_label("1/3");
}

#[test]
fn search_no_result_shows_zero_counter() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().search.open = true;
    harness.state_mut().search.query = "zzz-nothing".into();
    harness.run();

    harness.get_by_label("0/0");
}

#[test]
fn enter_cycles_to_the_next_match() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().search.open = true;
    harness.state_mut().search.query = "commit".into();
    harness.run();
    harness.get_by_label("1/3");

    // Enter (field focused) walks forward through the matches.
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
    harness.run();
    harness.get_by_label("2/3");
}

#[test]
fn escape_closes_the_search_box() {
    let mut harness = harness(sample_graph(), None);
    harness.state_mut().search.open = true;
    harness.state_mut().search.query = "commit".into();
    harness.run();
    harness.get_by_label("Close search");

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Escape);
    harness.run();

    assert!(!harness.state().search.open);
    assert!(harness.query_by_label("Close search").is_none());
}
