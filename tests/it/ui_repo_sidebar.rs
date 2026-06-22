use std::collections::HashSet;
use std::path::PathBuf;

use egui::Theme;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::git::worktree::{WorktreeSource, WorktreeSourceKind};
use helm::keybindings::Keymap;
use helm::theme::palette;
use helm::ui::repo_sidebar::{
    create_worktree_modal, delete_worktree_modal, repo_sidebar, CreateSelection,
    CreateWorktreeModalAction, CreateWorktreePrompt, CreateWorktreeState, DeleteModalAction,
    DeletePrompt, ProjectHeader, ProjectVisibility, RepoRow, SidebarAction, SidebarItem,
};

/// Two standalone projects (alpha present, beta missing): each contributes a
/// non-selectable header and its main row. The main rows carry a branch so their
/// visible label differs from the header's project name (no `get_by_label` clash).
fn two_projects() -> (Vec<SidebarItem<'static>>, Vec<bool>) {
    let items = vec![
        SidebarItem::Header(ProjectHeader {
            root: 0,
            name: "alpha",
            path: "/tmp/alpha",
            collapsed: false,
            lane: 0,
            can_create_worktree: false,
            agent: AgentBadge::None,
        }),
        SidebarItem::Row(RepoRow {
            index: 0,
            name: "alpha",
            path: "/tmp/alpha",
            missing: false,
            main: true,
            branch: Some("main"),
            deleting: false,
            agent: AgentBadge::None,
            stats: None,
        }),
        SidebarItem::Header(ProjectHeader {
            root: 1,
            name: "beta",
            path: "/tmp/beta",
            collapsed: false,
            lane: 0,
            can_create_worktree: false,
            agent: AgentBadge::None,
        }),
        SidebarItem::Row(RepoRow {
            index: 1,
            name: "beta",
            path: "/tmp/beta",
            missing: true,
            main: true,
            branch: Some("dev"),
            deleting: false,
            agent: AgentBadge::None,
            stats: None,
        }),
    ];
    (items, vec![false, false])
}

/// One worktree group: the root `main` (its own working tree on `trunk`) plus a
/// linked worktree `feature-x`. The main row's branch differs from the project
/// name so the header label `main` stays unique.
fn grouped_items() -> (Vec<SidebarItem<'static>>, Vec<bool>) {
    let items = vec![
        SidebarItem::Header(ProjectHeader {
            root: 0,
            name: "main",
            path: "/tmp/main",
            collapsed: false,
            lane: 0,
            can_create_worktree: true,
            agent: AgentBadge::None,
        }),
        SidebarItem::Row(RepoRow {
            index: 0,
            name: "main",
            path: "/tmp/main",
            missing: false,
            main: true,
            branch: Some("trunk"),
            deleting: false,
            agent: AgentBadge::None,
            stats: None,
        }),
        SidebarItem::Row(RepoRow {
            index: 1,
            name: "feature-x",
            path: "/tmp/feature-x",
            missing: false,
            main: false,
            branch: None,
            deleting: false,
            agent: AgentBadge::None,
            stats: None,
        }),
    ];
    (items, vec![false, true])
}

fn grouped_harness() -> Harness<'static, SidebarAction> {
    Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            let (items, child_flags) = grouped_items();
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    )
}

fn modal_harness(prompt: DeletePrompt) -> Harness<'static, DeleteModalAction> {
    Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            delete_worktree_modal(ui, &palette, &prompt, state);
        },
        DeleteModalAction::default(),
    )
}

fn create_sources() -> Vec<WorktreeSource> {
    vec![
        WorktreeSource {
            name: "feat/toto".to_owned(),
            kind: WorktreeSourceKind::Local,
            local_branch: "feat/toto".to_owned(),
            path: PathBuf::from("/tmp/main.worktrees/feat/toto"),
        },
        WorktreeSource {
            name: "origin/fix/a".to_owned(),
            kind: WorktreeSourceKind::Remote,
            local_branch: "fix/a".to_owned(),
            path: PathBuf::from("/tmp/main.worktrees/fix/a"),
        },
    ]
}

fn create_taken() -> HashSet<String> {
    HashSet::from(["feat/toto".to_owned(), "fix/a".to_owned()])
}

fn create_modal_harness(
    selected: Option<CreateSelection>,
    query: &str,
) -> Harness<'static, (CreateWorktreeState, CreateWorktreeModalAction)> {
    let state = CreateWorktreeState {
        query: query.to_owned(),
        ..Default::default()
    };
    Harness::new_ui_state(
        move |ui, (state, action)| {
            let palette = palette(Theme::Light);
            let sources = create_sources();
            let taken = create_taken();
            create_worktree_modal(
                ui,
                &palette,
                &CreateWorktreePrompt {
                    root_label: "main",
                    root: std::path::Path::new("/tmp/main"),
                    base: None,
                    sources: &sources,
                    selected,
                    base_branch: "main",
                    taken: &taken,
                    error: None,
                    loading: false,
                    busy: false,
                },
                state,
                action,
            );
        },
        (state, CreateWorktreeModalAction::default()),
    )
}

fn harness(active: Option<usize>) -> Harness<'static, SidebarAction> {
    harness_with_agents(active, false)
}

fn harness_with_agents(
    active: Option<usize>,
    agents_active: bool,
) -> Harness<'static, SidebarAction> {
    Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            let (items, child_flags) = two_projects();
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                active,
                AgentBadge::None,
                agents_active,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    )
}

#[test]
fn rows_show_names_and_hide_shortcut_digits_until_cmd_is_held() {
    let mut harness = harness(Some(0));
    harness.run();

    harness.get_by_label("alpha");
    harness.get_by_label("beta");
    assert!(
        harness.query_by_label("⌃⌘1").is_none(),
        "no shortcut badge is shown until Cmd is held"
    );
    assert!(
        harness.query_by_label("⌘O").is_none(),
        "the open-folder badge stays hidden until Cmd is held"
    );
    assert!(harness.query_by_label("Open Folder… · ⌘O").is_none());
}

#[test]
fn holding_cmd_reveals_the_shortcut_badges() {
    let mut harness = harness(Some(0));
    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    harness.get_by_label("⌃⌘1");
    harness.get_by_label("⌃⌘2");
    harness.get_by_label("⌘O");
}

#[test]
fn clicking_a_row_selects_that_repo() {
    let mut harness = harness(Some(0));
    harness.run();

    // The main row shows its branch; clicking it selects the project's entry.
    harness.get_by_label("dev").click();
    harness.run();

    assert_eq!(
        harness.state().select,
        Some(1),
        "clicking the second row selects repo index 1"
    );
}

#[test]
fn agents_dashboard_clears_the_active_repo_highlight() {
    // Active repo stays index 0, but the Agents dashboard owns the central area:
    // only the Agents entry may read as selected, not the repo row underneath it.
    let mut harness = harness_with_agents(Some(0), true);
    harness.run();

    assert_eq!(
        format!(
            "{:?}",
            harness.get_by_label("Agents").accesskit_node().toggled()
        ),
        "Some(True)",
        "the Agents entry is the selected row while the dashboard is open"
    );
    assert_eq!(
        format!(
            "{:?}",
            harness.get_by_label("main").accesskit_node().toggled()
        ),
        "Some(False)",
        "the active repo row must not stay highlighted under the Agents dashboard"
    );
}

/// Press on `from`, drag through the threshold onto `to`, release. Each leg is a
/// separate frame so egui registers the drag start, the hover and the drop in turn.
fn drag_row(harness: &mut Harness<'static, SidebarAction>, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.input_mut().events.push(button(from, true));
    harness.run_steps(2);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from + egui::vec2(0.0, 10.0)));
    harness.run_steps(2);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(to));
    harness.run_steps(2);
    harness.input_mut().events.push(button(to, false));
    harness.run_steps(2);
}

#[test]
fn dropping_a_project_below_another_emits_a_reorder() {
    let mut harness = harness(Some(0));
    harness.run();

    let from = harness.get_by_label("alpha").rect();
    let onto = harness.get_by_label("beta").rect();
    // Lower half of beta: alpha lands after it.
    let target = egui::pos2(onto.center().x, onto.center().y + onto.height() * 0.3);
    drag_row(&mut harness, from.center(), target);

    let reorder = harness
        .state()
        .reorder
        .expect("dropping a row onto another emits a reorder");
    assert_eq!(
        (reorder.from, reorder.anchor, reorder.after),
        (0, 1, true),
        "alpha (0) is dropped after beta (1)"
    );
}

#[test]
fn dropping_a_row_onto_itself_emits_nothing() {
    let mut harness = harness(Some(0));
    harness.run();

    let alpha = harness.get_by_label("alpha").rect();
    drag_row(&mut harness, alpha.center(), alpha.center());

    assert!(
        harness.state().reorder.is_none(),
        "a no-op drop (a row onto its own slot) must not reorder"
    );
}

#[test]
fn header_plus_button_requests_the_open_dialog() {
    let mut harness = harness(Some(0));
    harness.run();

    harness.get_by_label("Add a project").click();
    harness.run();

    assert!(
        harness.state().open,
        "the header + button asks to add a repo"
    );
}

#[test]
fn root_plus_button_requests_create_worktree() {
    let mut harness = grouped_harness();
    harness.run();

    // The `+` create-worktree button reveals on header hover; the header label
    // ("main") is shared with its row, so hover the first match.
    harness.get_all_by_label("main").next().unwrap().hover();
    harness.run();
    harness.get_by_label("Create worktree").click();
    harness.run();

    assert_eq!(harness.state().create_worktree, Some(0));
    assert_eq!(harness.state().select, None);
}

#[test]
fn header_click_requests_collapse_toggle_without_selecting() {
    let mut harness = grouped_harness();
    harness.run();

    // The whole header band is the collapse affordance; it carries the project
    // name and is never selectable.
    harness.get_by_label("main").click();
    harness.run();

    assert_eq!(harness.state().toggle_collapse, Some(0));
    assert_eq!(
        harness.state().select,
        None,
        "folding a group must not select its root"
    );
}

#[test]
fn a_linked_worktree_row_stacks_its_folder_name_over_its_branch() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            // A worktree whose folder name differs from its branch, so both lines
            // are distinguishable in the accessibility tree (worktrees.md §3).
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "superset",
                    path: "/tmp/superset",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: true,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "superset",
                    path: "/tmp/superset",
                    missing: false,
                    main: true,
                    branch: Some("main"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 1,
                    name: "ui-blocking-threads",
                    path: "/tmp/ui-blocking-threads",
                    missing: false,
                    main: false,
                    branch: Some("feat/threads"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let child_flags = [false, true];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.run();

    // The root is a single line (its branch); the linked worktree carries both its
    // folder name and its branch.
    harness.get_by_label("main");
    harness.get_by_label_contains("ui-blocking-threads");
    harness.get_by_label_contains("feat/threads");
}

#[test]
fn a_collapsed_group_hides_its_rows_and_renumbers_shortcuts() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            // `main` is folded: only its header shows, both its main row and the
            // `feature-x` worktree drop out — so `solo` is the only numbered row.
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "main",
                    path: "/tmp/main",
                    collapsed: true,
                    lane: 0,
                    can_create_worktree: true,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Header(ProjectHeader {
                    root: 2,
                    name: "solo",
                    path: "/tmp/solo",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: false,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 2,
                    name: "solo",
                    path: "/tmp/solo",
                    missing: false,
                    main: true,
                    branch: Some("release"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            // child_flags mirrors every entry, the folded `feature-x` (index 1)
            // included.
            let child_flags = [false, true, false];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    assert!(
        harness.query_by_label("feature-x").is_none(),
        "a collapsed group hides its worktree children"
    );
    harness.get_by_label("main");
    harness.get_by_label("⌃⌘1");
    assert!(
        harness.query_by_label("⌃⌘2").is_none(),
        "the folded group frees its slots: 'solo' is the only numbered row (⌃⌘1)"
    );
}

#[test]
fn create_worktree_modal_lists_sources_and_emits_selection_then_create() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "");
    harness.run();

    harness.get_by_label("feat/toto");
    harness.get_by_label("origin/fix/a").click();
    harness.run();
    assert_eq!(harness.state().1.select, Some(CreateSelection::Source(1)));

    harness.get_by_label("Create worktree").click();
    harness.run();
    assert!(harness.state().1.create);
}

#[test]
fn create_worktree_modal_filter_narrows_the_list_and_reselects() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "fix");
    harness.run();

    assert!(
        harness.query_by_label("feat/toto").is_none(),
        "a branch outside the filter leaves the list"
    );
    harness.get_by_label("origin/fix/a");
    assert_eq!(
        harness.state().1.select,
        Some(CreateSelection::Source(1)),
        "the filter hid the selection: the first visible branch takes over"
    );
}

#[test]
fn create_worktree_modal_arrow_down_moves_the_selection() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "");
    harness.run();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();

    assert_eq!(harness.state().1.select, Some(CreateSelection::Source(1)));
}

#[test]
fn create_worktree_modal_prefills_the_name_with_the_selected_branch() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(1)), "");
    harness.run();

    assert_eq!(
        harness.state().0.name,
        "fix/a",
        "the name follows the selection (remote source -> local branch)"
    );
    assert!(
        harness
            .query_by_label_contains("/tmp/main.worktrees/fix/a")
            .is_some(),
        "Location previews the default destination"
    );
}

#[test]
fn create_worktree_modal_custom_name_drives_the_location() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "");
    harness.state_mut().0.name = "team/dave".to_owned();
    harness.state_mut().0.name_edited = true;
    harness.run();

    assert!(
        harness
            .query_by_label_contains("/tmp/main.worktrees/team/dave")
            .is_some(),
        "a custom name with a slash nests the destination folder"
    );
}

#[test]
fn create_worktree_modal_invalid_name_disables_create() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "");
    harness.state_mut().0.name = "../escape".to_owned();
    harness.state_mut().0.name_edited = true;
    harness.run();

    harness.get_by_label_contains("cannot be used as a worktree folder");
    harness.get_by_label("Create worktree").click();
    harness.run();
    assert!(
        !harness.state().1.create,
        "an invalid name must keep the Create button disabled"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert!(!harness.state().1.create, "Enter must be inert too");
}

#[test]
fn create_worktree_modal_enter_creates_from_the_selection() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "");
    harness.run();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();

    assert!(harness.state().1.create);
}

#[test]
fn create_worktree_modal_offers_the_new_branch_row_only_for_an_unused_name() {
    let mut harness = create_modal_harness(None, "brand-new");
    harness.run();
    harness.get_by_label_contains("Create branch “brand-new” from main");

    let mut harness = create_modal_harness(None, "feat/toto");
    harness.run();
    assert!(
        harness.query_by_label_contains("Create branch").is_none(),
        "an existing branch name never offers a new branch"
    );

    let mut harness = create_modal_harness(None, "");
    harness.run();
    assert!(
        harness.query_by_label_contains("Create branch").is_none(),
        "a blank query offers nothing to create"
    );
}

#[test]
fn create_worktree_modal_enter_on_a_match_never_selects_the_new_branch() {
    let mut harness = create_modal_harness(None, "feat");
    harness.run();
    harness.get_by_label_contains("Create branch “feat” from main");
    assert_eq!(
        harness.state().1.select,
        Some(CreateSelection::Source(0)),
        "the highlight stays on the first match, not the pinned row"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert!(harness.state().1.create);
    assert_eq!(
        harness.state().1.select,
        Some(CreateSelection::Source(0)),
        "Enter on a highlighted match must not switch to the new branch"
    );
}

#[test]
fn create_worktree_modal_arrow_down_reaches_the_pinned_new_branch_row() {
    let mut harness = create_modal_harness(Some(CreateSelection::Source(0)), "feat");
    harness.run();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();

    assert_eq!(harness.state().1.select, Some(CreateSelection::NewBranch));
}

#[test]
fn create_worktree_modal_with_no_match_selects_and_creates_the_new_branch() {
    let mut harness = create_modal_harness(None, "brand-new");
    harness.run();

    assert_eq!(
        harness.state().1.select,
        Some(CreateSelection::NewBranch),
        "zero matches hand the selection to the pinned row"
    );
    assert_eq!(
        harness.state().0.name,
        "brand-new",
        "the worktree name follows the typed branch name"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert!(harness.state().1.create);
}

#[test]
fn header_context_menu_offers_reveal_copy_and_remove() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "solo",
                    path: "/tmp/solo",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: false,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "solo",
                    path: "/tmp/solo",
                    missing: false,
                    main: true,
                    branch: Some("main"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let child_flags = [false];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.run();
    // Reveal / Copy / Remove all live on the band's right-click menu, hidden until opened.
    assert!(harness.query_by_label("Reveal in Finder").is_none());
    assert!(harness.query_by_label("Remove from sidebar").is_none());

    harness.get_by_label("solo").click_secondary();
    harness.run();
    harness.get_by_label("Reveal in Finder");
    harness.get_by_label("Copy path");
    harness.get_by_label("Remove from sidebar").click();
    harness.run();
    assert_eq!(
        harness.state().remove,
        Some(0),
        "Remove from sidebar drops the project from the sidebar"
    );
}

#[test]
fn child_row_menu_offers_delete_worktree_never_remove() {
    let mut harness = grouped_harness();
    harness.run();

    harness.get_by_label("feature-x").click_secondary();
    harness.run();

    assert_eq!(
        harness.query_all_by_label("Remove from sidebar").count(),
        0,
        "the child row menu offers no Remove (discovery would bring a hidden child back); \
         Remove lives on the header menu"
    );
    harness.get_by_label("Delete worktree from disk").click();
    harness.run();

    assert_eq!(harness.state().delete_worktree, Some(1));
    assert_eq!(harness.state().remove, None);
}

#[test]
fn a_deleting_row_is_inert_clicks_and_menu_ignored() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "main",
                    path: "/tmp/main",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: true,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "main",
                    path: "/tmp/main",
                    missing: false,
                    main: true,
                    branch: Some("trunk"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 1,
                    name: "feature-x",
                    path: "/tmp/feature-x",
                    missing: false,
                    main: false,
                    branch: None,
                    deleting: true,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let child_flags = [false, true];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    // The spinner repaints continuously: `run` (waiting for quiescence) never
    // converges — advance with explicit steps.
    harness.run_steps(2);

    harness.get_by_label("feature-x").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().select,
        None,
        "clicking a row whose worktree is being deleted is inert"
    );

    harness.get_by_label("feature-x").click_secondary();
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label("Delete worktree from disk")
            .is_none(),
        "no context menu while the delete runs"
    );
}

#[test]
fn main_row_menu_offers_reveal_copy_only() {
    let mut harness = grouped_harness();
    harness.run();

    // The main worktree shows its branch (`trunk`); its row context menu cannot
    // delete the worktree, and Remove lives on the project header.
    harness.get_by_label("trunk").click_secondary();
    harness.run();

    harness.get_by_label("Reveal in Finder");
    harness.get_by_label("Copy path");
    assert!(
        harness
            .query_by_label("Delete worktree from disk")
            .is_none(),
        "the main worktree cannot be deleted from the sidebar"
    );
    assert_eq!(
        harness.query_all_by_label("Remove from sidebar").count(),
        0,
        "the main row menu offers no Remove; that lives on the project header menu"
    );
}

#[test]
fn dirty_modal_announces_the_count_and_confirms_force_delete() {
    let mut harness = modal_harness(DeletePrompt::Dirty {
        label: "feature-x".to_owned(),
        files: 2,
    });
    harness.run();

    harness.get_by_label("2 files with uncommitted changes");
    harness.get_by_label("Delete anyway").click();
    harness.run();

    assert!(harness.state().confirm);
    assert!(!harness.state().dismiss);
}

#[test]
fn dirty_modal_cancel_dismisses_without_confirming() {
    let mut harness = modal_harness(DeletePrompt::Dirty {
        label: "feature-x".to_owned(),
        files: 1,
    });
    harness.run();

    harness.get_by_label("1 file with uncommitted changes");
    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().dismiss);
    assert!(!harness.state().confirm);
}

#[test]
fn refused_modal_shows_the_lock_reason_and_only_closes() {
    let mut harness = modal_harness(DeletePrompt::Refused {
        label: "feature-x".to_owned(),
        reason: "in use by CI".to_owned(),
    });
    harness.run();

    harness.get_by_label("in use by CI");
    assert!(harness.query_by_label("Delete anyway").is_none());
    harness.get_by_label("Close").click();
    harness.run();

    assert!(harness.state().dismiss);
}

#[test]
fn bare_root_row_is_not_selectable_but_its_children_are() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            // A bare root owns only a header (no working tree of its own); its
            // checkout is a linked worktree row.
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "proj.git",
                    path: "/tmp/proj.git",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: true,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 1,
                    name: "checkout",
                    path: "/tmp/checkout",
                    missing: false,
                    main: false,
                    branch: None,
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let child_flags = [false, true];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(1),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.run();

    harness.get_by_label("proj.git").click();
    harness.run();
    assert_eq!(
        harness.state().select,
        None,
        "a bare root has only a header — clicking it never selects"
    );

    harness.get_by_label("checkout").click();
    harness.run();
    assert_eq!(harness.state().select, Some(1));
}

#[test]
fn agent_badges_expose_their_state_through_the_row_label() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            // Each project is a header plus its main row; the row's label is the
            // branch, kept distinct from the project name so it is unambiguous.
            let project = |root: usize, name, path, branch, agent| {
                [
                    SidebarItem::Header(ProjectHeader {
                        root,
                        name,
                        path,
                        collapsed: false,
                        lane: 0,
                        can_create_worktree: false,
                        agent: AgentBadge::None,
                    }),
                    SidebarItem::Row(RepoRow {
                        index: root,
                        name,
                        path,
                        missing: false,
                        main: true,
                        branch: Some(branch),
                        deleting: false,
                        agent,
                        stats: None,
                    }),
                ]
            };
            let mut items = Vec::new();
            items.extend(project(
                0,
                "p-alpha",
                "/tmp/alpha",
                "alpha",
                AgentBadge::Working,
            ));
            items.extend(project(1, "p-beta", "/tmp/beta", "beta", AgentBadge::Done));
            items.extend(project(
                2,
                "p-gamma",
                "/tmp/gamma",
                "gamma",
                AgentBadge::Idle,
            ));
            items.extend(project(
                3,
                "p-delta",
                "/tmp/delta",
                "delta",
                AgentBadge::None,
            ));
            let child_flags = [false, false, false, false];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &[],
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    // No `run()`: the Working spinner requests a repaint on every frame.
    harness.run_steps(2);

    harness.get_by_label("alpha · agent working");
    harness.get_by_label("beta · agent done");
    harness.get_by_label("gamma · agent idle");
    harness.get_by_label("delta");
    assert!(
        harness.query_by_label("delta · agent idle").is_none(),
        "a row without an agent keeps its plain label"
    );
}

#[test]
fn empty_sidebar_open_prompt_is_clickable() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            repo_sidebar(
                ui,
                &palette,
                &[],
                &[],
                &[],
                None,
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.run();

    harness.get_by_label("Open Folder… · ⌘O").click();
    harness.run();

    assert!(
        harness.state().open,
        "clicking the empty-state prompt asks to add a repo"
    );
}

#[test]
fn eye_dropdown_lists_every_project_and_toggles_hidden() {
    // Only `alpha` is in the item list; `beta` is hidden, so the dropdown is the
    // sole place it still appears. Unchecking it must emit the toggle for root 1.
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "alpha",
                    path: "/tmp/alpha",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: false,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "alpha",
                    path: "/tmp/alpha",
                    missing: false,
                    main: true,
                    branch: Some("main"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let child_flags = [false];
            let projects = [
                ProjectVisibility {
                    root: 0,
                    name: "alpha",
                    hidden: false,
                },
                ProjectVisibility {
                    root: 1,
                    name: "beta",
                    hidden: true,
                },
            ];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &projects,
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.run();
    // The checkboxes live behind the eye, hidden until it is clicked.
    assert!(harness.query_by_label("beta").is_none());

    harness.get_by_label("Show or hide projects").click();
    harness.run();

    harness.get_by_label("beta").click();
    harness.run();
    assert_eq!(
        harness.state().toggle_hidden,
        Some(1),
        "toggling a dropdown checkbox flips that project's visibility"
    );
}

#[test]
fn header_context_menu_offers_hide_project() {
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            let palette = palette(Theme::Light);
            let items = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "solo",
                    path: "/tmp/solo",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: false,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "solo",
                    path: "/tmp/solo",
                    missing: false,
                    main: true,
                    branch: Some("main"),
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let child_flags = [false];
            let projects = [ProjectVisibility {
                root: 0,
                name: "solo",
                hidden: false,
            }];
            repo_sidebar(
                ui,
                &palette,
                &items,
                &child_flags,
                &projects,
                Some(0),
                AgentBadge::None,
                false,
                &Keymap::default(),
                state,
            );
        },
        SidebarAction::default(),
    );
    harness.run();
    assert!(harness.query_by_label("Hide project").is_none());

    harness.get_by_label("solo").click_secondary();
    harness.run();
    harness.get_by_label("Hide project").click();
    harness.run();
    assert_eq!(
        harness.state().toggle_hidden,
        Some(0),
        "the header right-click menu hides the whole project"
    );
}
