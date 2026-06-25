use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::git::status::RepoStatus;
use helm::keybindings::Keymap;
use helm::theme::Palette;
use helm::ui::file_list::FileMenuOutput;
use helm::ui::git_panel::GitPanelState;
use helm::ui::repo_sidebar::{repo_sidebar, ProjectHeader, RepoRow, SidebarAction, SidebarItem};
use helm::ui::{central_empty_state, root_layout};
use helm::workspace_launcher::WorkspaceOpener;

#[test]
fn renders_three_zones() {
    let palette = Palette::light();
    let status = RepoStatus::default();

    let mut harness = Harness::new_ui(move |ui| {
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
                branch: None,
                deleting: false,
                agent: AgentBadge::None,
                stats: None,
            }),
        ];
        let child_flags = [false];
        let mut intents = Vec::new();
        let mut show_workspace = true;
        let mut show_git = true;
        let mut sidebar = SidebarAction::default();
        let mut git_state = GitPanelState::default();
        let mut open_workspace = None;
        root_layout(
            ui,
            &palette,
            &items,
            &child_flags,
            &[],
            Some(0),
            "main",
            &status,
            false,
            None,
            &mut git_state,
            &mut intents,
            &mut show_workspace,
            &mut show_git,
            false,
            None,
            None,
            &mut None,
            None,
            &mut FileMenuOutput::default(),
            helm::ui::file_list::FileViewMode::default(),
            WorkspaceOpener::default(),
            &WorkspaceOpener::ALL,
            &mut open_workspace,
            &mut false,
            &mut false,
            helm::agent_watch::AgentBadge::None,
            false,
            &[],
            &mut sidebar,
            280.0,
            320.0,
            &Keymap::default(),
            false,
            true,
            200.0,
            |_ui| {},
            |ui| {
                ui.label("zone centrale");
            },
        );
    });
    harness.run();

    harness.get_by_label("PROJECTS");
    harness.get_by_label("Refresh");
    harness.get_by_label("zone centrale");
}

#[test]
fn workspace_launcher_shows_only_installed_openers_and_main_button_tracks_the_last_one() {
    use std::cell::Cell;
    use std::rc::Rc;

    let palette = Palette::light();
    let status = RepoStatus::default();
    let opened: Rc<Cell<Option<WorkspaceOpener>>> = Rc::new(Cell::new(None));
    let sink = opened.clone();

    let mut harness = Harness::new_ui(move |ui| {
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
                branch: None,
                deleting: false,
                agent: AgentBadge::None,
                stats: None,
            }),
        ];
        let child_flags = [false];
        let mut intents = Vec::new();
        let mut show_workspace = true;
        let mut show_git = true;
        let mut sidebar = SidebarAction::default();
        let mut git_state = GitPanelState::default();
        let mut open_workspace = None;
        root_layout(
            ui,
            &palette,
            &items,
            &child_flags,
            &[],
            Some(0),
            "main",
            &status,
            false,
            None,
            &mut git_state,
            &mut intents,
            &mut show_workspace,
            &mut show_git,
            false,
            None,
            None,
            &mut None,
            None,
            &mut FileMenuOutput::default(),
            helm::ui::file_list::FileViewMode::default(),
            // Last-used opener drives the main button; the menu lists only these
            // two (Cursor/Zed/Terminal/Ghostty are treated as not installed here).
            WorkspaceOpener::GitKraken,
            &[WorkspaceOpener::Finder, WorkspaceOpener::GitKraken],
            &mut open_workspace,
            &mut false,
            &mut false,
            helm::agent_watch::AgentBadge::None,
            false,
            &[],
            &mut sidebar,
            280.0,
            320.0,
            &Keymap::default(),
            false,
            true,
            200.0,
            |_ui| {},
            |ui| {
                ui.label("zone centrale");
            },
        );
        if let Some(opener) = open_workspace {
            sink.set(Some(opener));
        }
    });
    harness.run();

    // The main button reflects the last-used opener (dynamic, not hardcoded).
    harness.get_by_label("Open workspace in GitKraken");

    harness.get_by_label("Open workspace menu").click();
    harness.run();

    harness.get_by_label("Finder");
    harness.get_by_label("GitKraken");
    assert!(
        harness.query_by_label("Cursor").is_none(),
        "an uninstalled app must not appear in the menu"
    );
    assert!(harness.query_by_label("Zed").is_none());
    assert!(harness.query_by_label("Terminal").is_none());
    assert!(harness.query_by_label("Ghostty").is_none());

    harness.get_by_label("GitKraken").click();
    harness.run();
    assert_eq!(opened.get(), Some(WorkspaceOpener::GitKraken));
}

#[test]
fn workspace_sidebar_can_be_hidden() {
    let palette = Palette::light();
    let status = RepoStatus::default();

    let mut harness = Harness::new_ui(move |ui| {
        ui.ctx()
            .global_style_mut(|style| style.animation_time = 0.0);
        let mut intents = Vec::new();
        let mut show_workspace = false;
        let mut show_git = false;
        let mut sidebar = SidebarAction::default();
        let mut git_state = GitPanelState::default();
        let mut open_workspace = None;
        root_layout(
            ui,
            &palette,
            &[],
            &[],
            &[],
            None,
            "main",
            &status,
            false,
            None,
            &mut git_state,
            &mut intents,
            &mut show_workspace,
            &mut show_git,
            false,
            None,
            None,
            &mut None,
            None,
            &mut FileMenuOutput::default(),
            helm::ui::file_list::FileViewMode::default(),
            WorkspaceOpener::default(),
            &WorkspaceOpener::ALL,
            &mut open_workspace,
            &mut false,
            &mut false,
            helm::agent_watch::AgentBadge::None,
            false,
            &[],
            &mut sidebar,
            280.0,
            320.0,
            &Keymap::default(),
            false,
            true,
            200.0,
            |_ui| {},
            |ui| {
                ui.label("zone centrale");
            },
        );
    });
    harness.run();

    assert!(harness.query_by_label("PROJECTS").is_none());
    harness.get_by_label("zone centrale");
}

#[test]
fn central_zone_invites_to_open_a_folder_when_no_repo() {
    let palette = Palette::light();
    let status = RepoStatus::default();

    let mut harness = Harness::new_ui(move |ui| {
        let mut intents = Vec::new();
        let mut show_workspace = true;
        let mut show_git = false;
        let mut sidebar = SidebarAction::default();
        let mut git_state = GitPanelState::default();
        let mut open_workspace = None;
        root_layout(
            ui,
            &palette,
            &[],
            &[],
            &[],
            None,
            "main",
            &status,
            false,
            None,
            &mut git_state,
            &mut intents,
            &mut show_workspace,
            &mut show_git,
            false,
            None,
            None,
            &mut None,
            None,
            &mut FileMenuOutput::default(),
            helm::ui::file_list::FileViewMode::default(),
            WorkspaceOpener::default(),
            &WorkspaceOpener::ALL,
            &mut open_workspace,
            &mut false,
            &mut false,
            helm::agent_watch::AgentBadge::None,
            false,
            &[],
            &mut sidebar,
            280.0,
            320.0,
            &Keymap::default(),
            false,
            true,
            200.0,
            |_ui| {},
            |ui| {
                central_empty_state(ui, &palette, &Keymap::default());
            },
        );
    });
    harness.run();

    harness.get_by_label("Open a project to get started");
    harness.get_by_label("Terminal splits and Git staging, per project");
    harness.get_by_label("Open Folder…");
    harness.get_by_label("⌘O");
}

#[test]
fn central_empty_state_texts_yield_when_squeezed() {
    let palette = Palette::light();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(140.0, 600.0))
        .build_ui(move |ui| {
            central_empty_state(ui, &palette, &Keymap::default());
        });
    harness.run();

    assert!(
        harness
            .query_by_label("Open a project to get started")
            .is_none(),
        "below the title's intrinsic width the texts disappear instead of wrapping"
    );
    harness.get_by_label("Open Folder…");
    harness.get_by_label("⌘O");
}

#[test]
fn central_open_button_reports_clicks() {
    use std::cell::Cell;
    use std::rc::Rc;

    let palette = Palette::light();
    let clicked = Rc::new(Cell::new(false));
    let sink = clicked.clone();

    let mut harness = Harness::new_ui(move |ui| {
        if central_empty_state(ui, &palette, &Keymap::default()) {
            sink.set(true);
        }
    });
    harness.run();

    harness.get_by_label("Open Folder…").click();
    harness.run();

    assert!(
        clicked.get(),
        "clicking the central empty-state button signals open-folder"
    );
}

#[test]
fn sidebar_widths_round_trip_through_the_layout_state() {
    let palette = Palette::light();
    let status = RepoStatus::default();
    let left = 232.0;
    let right = 520.0;

    let mut harness = Harness::new_ui(move |ui| {
        let mut intents = Vec::new();
        let mut show_workspace = true;
        let mut show_git = true;
        let mut sidebar = SidebarAction::default();
        let mut git_state = GitPanelState::default();
        let mut open_workspace = None;
        root_layout(
            ui,
            &palette,
            &[],
            &[],
            &[],
            None,
            "main",
            &status,
            false,
            None,
            &mut git_state,
            &mut intents,
            &mut show_workspace,
            &mut show_git,
            false,
            None,
            None,
            &mut None,
            None,
            &mut FileMenuOutput::default(),
            helm::ui::file_list::FileViewMode::default(),
            WorkspaceOpener::default(),
            &WorkspaceOpener::ALL,
            &mut open_workspace,
            &mut false,
            &mut false,
            helm::agent_watch::AgentBadge::None,
            false,
            &[],
            &mut sidebar,
            left,
            right,
            &Keymap::default(),
            false,
            true,
            200.0,
            |_ui| {},
            |ui| {
                ui.label("zone centrale");
            },
        );
    });
    harness.run();

    assert_eq!(
        helm::ui::left_sidebar_width(&harness.ctx),
        Some(left),
        "the left sidebar honors the width passed in (loaded from prefs at boot)"
    );
    assert_eq!(
        helm::ui::right_sidebar_width(&harness.ctx),
        Some(right),
        "the right sidebar honors the width passed in (loaded from prefs at boot)"
    );
}

#[test]
fn repo_sidebar_empty_shows_prompt() {
    let palette = Palette::light();

    let mut harness = Harness::new_ui(move |ui| {
        repo_sidebar(
            ui,
            &palette,
            &[],
            &[],
            &[],
            None,
            AgentBadge::None,
            false,
            &[],
            &Keymap::default(),
            &mut SidebarAction::default(),
        );
    });
    harness.run();

    harness.get_by_label("PROJECTS");
    harness.get_by_label("Open Folder… · ⌘O");
}

#[test]
fn repo_sidebar_lists_repo_names() {
    let palette = Palette::light();

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
            missing: false,
            main: true,
            branch: Some("dev"),
            deleting: false,
            agent: AgentBadge::None,
            stats: None,
        }),
    ];
    let child_flags = [false, false];
    let mut harness = Harness::new_ui(move |ui| {
        repo_sidebar(
            ui,
            &palette,
            &items,
            &child_flags,
            &[],
            None,
            AgentBadge::None,
            false,
            &[],
            &Keymap::default(),
            &mut SidebarAction::default(),
        );
    });
    harness.run();

    harness.get_by_label("alpha");
    harness.get_by_label("beta");
    assert!(harness.query_by_label("Open Folder… · ⌘O").is_none());
}
