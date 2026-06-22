use egui::Theme;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::agent_watch::AgentBadge;
use helm::git::status::RepoStatus;
use helm::keybindings::Keymap;
use helm::theme::palette;
use helm::ui::file_list::FileMenuOutput;
use helm::ui::git_panel::GitPanelState;
use helm::ui::repo_sidebar::{ProjectHeader, RepoRow, SidebarAction, SidebarItem};
use helm::ui::root_layout;
use helm::workspace_launcher::WorkspaceOpener;

struct State {
    show_workspace: bool,
    show_git: bool,
    open_workspace: Option<WorkspaceOpener>,
    open_preferences: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            show_workspace: true,
            show_git: false,
            open_workspace: None,
            open_preferences: false,
        }
    }
}

fn harness() -> Harness<'static, State> {
    root_harness(None, false)
}

fn launcher_harness() -> Harness<'static, State> {
    root_harness(Some(0), false)
}

fn missing_harness() -> Harness<'static, State> {
    root_harness(Some(0), true)
}

fn root_harness(active_repo: Option<usize>, missing: bool) -> Harness<'static, State> {
    Harness::new_ui_state(
        move |ui, state| {
            ui.ctx()
                .global_style_mut(|style| style.animation_time = 0.0);
            let palette = palette(Theme::Light);
            let status = RepoStatus::default();
            let all = [
                SidebarItem::Header(ProjectHeader {
                    root: 0,
                    name: "helm",
                    path: "/tmp/helm",
                    collapsed: false,
                    lane: 0,
                    can_create_worktree: false,
                    agent: AgentBadge::None,
                }),
                SidebarItem::Row(RepoRow {
                    index: 0,
                    name: "helm",
                    path: "/tmp/helm",
                    missing,
                    main: true,
                    branch: None,
                    deleting: false,
                    agent: AgentBadge::None,
                    stats: None,
                }),
            ];
            let (items, child_flags): (&[SidebarItem], &[bool]) = if active_repo.is_some() {
                (&all[..], &[false][..])
            } else {
                (&[], &[])
            };
            let mut intents = Vec::new();
            let mut sidebar = SidebarAction::default();
            let mut git_state = GitPanelState::default();
            root_layout(
                ui,
                &palette,
                items,
                child_flags,
                &[],
                active_repo,
                "main",
                &status,
                false,
                None,
                &mut git_state,
                &mut intents,
                &mut state.show_workspace,
                &mut state.show_git,
                false,
                None,
                None,
                &mut None,
                None,
                &mut FileMenuOutput::default(),
                helm::ui::file_list::FileViewMode::default(),
                WorkspaceOpener::default(),
                &WorkspaceOpener::ALL,
                &mut state.open_workspace,
                &mut state.open_preferences,
                &mut false,
                helm::agent_watch::AgentBadge::None,
                false,
                &mut sidebar,
                280.0,
                320.0,
                &Keymap::default(),
                false,
                true,
                200.0,
                |_ui| {},
                |_ui| {},
            );
        },
        State::default(),
    )
}

#[test]
fn git_sidebar_hidden_by_default() {
    let mut harness = harness();
    harness.run();

    assert!(!harness.state().show_git);
    assert!(
        harness.query_by_label("Refresh").is_none(),
        "git panel is hidden by default"
    );
}

#[test]
fn toggle_icon_reveals_git_sidebar() {
    let mut harness = launcher_harness();
    harness.run();
    assert!(harness.query_by_label("Refresh").is_none());

    harness.get_by_label("Toggle git sidebar").click();
    harness.run();

    assert!(
        harness.state().show_git,
        "clicking the toggle opens the sidebar"
    );
    assert!(
        harness.query_by_label("Refresh").is_some(),
        "the git panel is revealed once toggled on"
    );
}

#[test]
fn git_sidebar_without_repo_shows_a_dedicated_state() {
    let mut harness = harness();
    harness.run();

    harness.get_by_label("Toggle git sidebar").click();
    harness.run();

    harness.get_by_label("No repository open");
    assert!(
        harness.query_by_label("Refresh").is_none(),
        "no git panel without a repository"
    );
}

#[test]
fn toggle_icon_hides_workspace_sidebar() {
    let mut harness = harness();
    harness.run();
    assert!(harness.state().show_workspace);
    assert!(harness.query_by_label("PROJECTS").is_some());

    harness.get_by_label("Toggle workspace sidebar").click();
    harness.run();

    assert!(
        !harness.state().show_workspace,
        "clicking the toggle hides the workspace sidebar"
    );
    assert!(
        harness.query_by_label("PROJECTS").is_none(),
        "the workspace sidebar is hidden once toggled off"
    );
    assert!(
        harness.query_by_label("Toggle workspace sidebar").is_some(),
        "the toggle remains reachable while the sidebar is hidden"
    );
}

#[test]
fn preferences_button_toggles_the_window() {
    let mut harness = harness();
    harness.run();
    assert!(!harness.state().open_preferences);

    harness.get_by_label("Open preferences").click();
    harness.run();

    assert!(
        harness.state().open_preferences,
        "clicking the gear opens preferences"
    );
}

#[test]
fn shortcut_badges_appear_only_while_cmd_is_held() {
    let mut harness = harness();
    harness.run();
    assert!(harness.query_by_label("⌘B").is_none());
    assert!(harness.query_by_label("⌘G").is_none());
    assert!(harness.query_by_label("⌘,").is_none());
    let workspace_rest = harness.get_by_label("Toggle workspace sidebar").rect();
    let git_rest = harness.get_by_label("Toggle git sidebar").rect();
    let prefs_rest = harness.get_by_label("Open preferences").rect();
    assert!(
        workspace_rest.right() <= 280.0,
        "workspace toggle lives inside the left workspace sidebar"
    );
    assert!(
        git_rest.left() > workspace_rest.right(),
        "git toggle stays in the right-side actions"
    );

    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    harness.get_by_label("⌘B");
    harness.get_by_label("⌘G");
    harness.get_by_label("⌘,");

    // The badges are painted as an overlay: revealing the shortcuts must not
    // shift the icons when Cmd toggles the display.
    assert_eq!(
        workspace_rest,
        harness.get_by_label("Toggle workspace sidebar").rect()
    );
    assert_eq!(git_rest, harness.get_by_label("Toggle git sidebar").rect());
    assert_eq!(prefs_rest, harness.get_by_label("Open preferences").rect());
}

#[test]
fn default_workspace_launcher_emits_the_default_opener() {
    let mut harness = launcher_harness();
    harness.run();

    assert!(
        harness.query_by_label("Zed").is_none(),
        "the default launcher control is icon-only until the dropdown is opened"
    );
    harness.get_by_label("Open workspace in Zed").click();
    harness.run();

    assert_eq!(harness.state().open_workspace, Some(WorkspaceOpener::Zed));
}

#[test]
fn workspace_launcher_menu_emits_the_selected_opener() {
    let mut harness = launcher_harness();
    harness.run();

    harness.get_by_label("Open workspace menu").click();
    harness.run();
    harness.get_by_label("Ghostty").click();
    harness.run();

    assert_eq!(
        harness.state().open_workspace,
        Some(WorkspaceOpener::Ghostty)
    );
}

#[test]
fn workspace_launcher_is_hidden_without_any_repo() {
    let mut harness = harness();
    harness.run();

    assert!(
        harness.query_by_label("Open workspace in Zed").is_none(),
        "no launcher on the empty home screen"
    );
    assert!(harness.query_by_label("Open workspace menu").is_none());
}

#[test]
fn workspace_launcher_is_disabled_when_the_folder_is_missing() {
    let mut harness = missing_harness();
    harness.run();

    harness.get_by_label("Open workspace in Zed").click();
    harness.run();

    assert_eq!(harness.state().open_workspace, None);
}
