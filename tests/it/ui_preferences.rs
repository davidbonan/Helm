use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::agents::{Agent, CommitMessageSettings, Preset, ReviewSettings};
use helm::git::sync::PullDefault;
use helm::keybindings::{Action, Keymap, Shortcut};
use helm::pull_requests::runner::SourceStatus;
use helm::terminal::links::Editor;
use helm::theme::{Palette, ThemeMode};
use helm::ui::preferences::{
    preferences_page, setting_divider, setting_row, settings_card, AgentsView, KeyboardState,
    PhoneDeviceRow, PhoneView, PrSourcesView, PreferencesSection, ProjectView, UpdatesView,
    NO_PAIRED_DEVICE,
};
use helm::update::{UpdateState, Version};

/// Counters observed while driving `preferences_page`: final mode/section/themes/
/// Pull default + number of frames that reported a change, a Back or an updater
/// intent.
struct PageProbe {
    mode: Rc<RefCell<ThemeMode>>,
    section: Rc<RefCell<PreferencesSection>>,
    light_theme: Rc<RefCell<String>>,
    dark_theme: Rc<RefCell<String>>,
    pull: Rc<RefCell<PullDefault>>,
    commit_message: Rc<RefCell<CommitMessageSettings>>,
    review: Rc<RefCell<ReviewSettings>>,
    editor: Rc<RefCell<Editor>>,
    notify: Rc<RefCell<bool>>,
    agents: Rc<RefCell<Vec<Agent>>>,
    keymap: Rc<RefCell<Keymap>>,
    keyboard: Rc<RefCell<KeyboardState>>,
    theme_changes: Rc<RefCell<usize>>,
    pull_changes: Rc<RefCell<usize>>,
    editor_changes: Rc<RefCell<usize>>,
    notify_changes: Rc<RefCell<usize>>,
    agent_changes: Rc<RefCell<usize>>,
    keymap_changes: Rc<RefCell<usize>>,
    backs: Rc<RefCell<usize>>,
    update_checks: Rc<RefCell<usize>>,
    update_installs: Rc<RefCell<usize>>,
}

/// Updater snapshot of the harnesses that do not target the Updates section.
fn idle_updates() -> UpdatesView {
    UpdatesView {
        version: "0.1.0".to_owned(),
        state: UpdateState::Idle,
        bundled: true,
    }
}

/// PR-source snapshot for the harnesses that do not target the Pull Requests
/// section (both sources absent, nothing fetched yet).
fn idle_pr_sources() -> PrSourcesView {
    PrSourcesView {
        github: SourceStatus::Absent,
        bitbucket: SourceStatus::Absent,
        loaded: false,
    }
}

fn page_harness(start: ThemeMode) -> (Harness<'static>, PageProbe) {
    page_harness_with(start, PullDefault::default())
}

fn page_harness_with(start: ThemeMode, pull: PullDefault) -> (Harness<'static>, PageProbe) {
    page_harness_sized(start, pull, egui::vec2(800.0, 600.0))
}

fn page_harness_sized(
    start: ThemeMode,
    pull: PullDefault,
    size: egui::Vec2,
) -> (Harness<'static>, PageProbe) {
    page_harness_full(start, pull, size, idle_updates())
}

fn page_harness_full(
    start: ThemeMode,
    pull: PullDefault,
    size: egui::Vec2,
    updates: UpdatesView,
) -> (Harness<'static>, PageProbe) {
    let palette = Palette::light();
    let probe = PageProbe {
        mode: Rc::new(RefCell::new(start)),
        section: Rc::new(RefCell::new(PreferencesSection::default())),
        light_theme: Rc::new(RefCell::new("helm".to_owned())),
        dark_theme: Rc::new(RefCell::new("helm".to_owned())),
        pull: Rc::new(RefCell::new(pull)),
        commit_message: Rc::new(RefCell::new(CommitMessageSettings::default())),
        review: Rc::new(RefCell::new(ReviewSettings::default())),
        editor: Rc::new(RefCell::new(Editor::default())),
        notify: Rc::new(RefCell::new(true)),
        agents: Rc::new(RefCell::new(Agent::defaults())),
        keymap: Rc::new(RefCell::new(Keymap::default())),
        keyboard: Rc::new(RefCell::new(KeyboardState::default())),
        theme_changes: Rc::new(RefCell::new(0)),
        pull_changes: Rc::new(RefCell::new(0)),
        editor_changes: Rc::new(RefCell::new(0)),
        notify_changes: Rc::new(RefCell::new(0)),
        agent_changes: Rc::new(RefCell::new(0)),
        keymap_changes: Rc::new(RefCell::new(0)),
        backs: Rc::new(RefCell::new(0)),
        update_checks: Rc::new(RefCell::new(0)),
        update_installs: Rc::new(RefCell::new(0)),
    };
    let mode = probe.mode.clone();
    let section = probe.section.clone();
    let light_theme = probe.light_theme.clone();
    let dark_theme = probe.dark_theme.clone();
    let pull = probe.pull.clone();
    let commit_message = probe.commit_message.clone();
    let review = probe.review.clone();
    let editor = probe.editor.clone();
    let notify = probe.notify.clone();
    let agents = probe.agents.clone();
    let keymap = probe.keymap.clone();
    let keyboard = probe.keyboard.clone();
    let theme_changes = probe.theme_changes.clone();
    let pull_changes = probe.pull_changes.clone();
    let editor_changes = probe.editor_changes.clone();
    let notify_changes = probe.notify_changes.clone();
    let agent_changes = probe.agent_changes.clone();
    let keymap_changes = probe.keymap_changes.clone();
    let backs = probe.backs.clone();
    let update_checks = probe.update_checks.clone();
    let update_installs = probe.update_installs.clone();

    let cache = RefCell::new(egui_commonmark::CommonMarkCache::default());
    let mut harness = Harness::builder().with_size(size).build_ui(move |ui| {
        let mut bitbucket_email = String::new();
        let mut bitbucket_token = String::new();
        let pr_sources = idle_pr_sources();
        let action = preferences_page(
            ui,
            &palette,
            &mut section.borrow_mut(),
            &mut mode.borrow_mut(),
            &mut light_theme.borrow_mut(),
            &mut dark_theme.borrow_mut(),
            &mut pull.borrow_mut(),
            &mut editor.borrow_mut(),
            &mut bitbucket_email,
            &mut bitbucket_token,
            &pr_sources,
            &mut notify.borrow_mut(),
            AgentsView {
                agents: &mut agents.borrow_mut(),
                commit_message: &mut commit_message.borrow_mut(),
                review: &mut review.borrow_mut(),
            },
            PhoneView {
                start_at_launch: &mut false,
                devices: &[],
            },
            &mut keymap.borrow_mut(),
            &mut keyboard.borrow_mut(),
            &updates,
            &helm::cli::ShellCommand::Unbundled,
            &mut cache.borrow_mut(),
            None,
        );
        if action.theme_changed {
            *theme_changes.borrow_mut() += 1;
        }
        if action.pull_changed {
            *pull_changes.borrow_mut() += 1;
        }
        if action.editor_changed {
            *editor_changes.borrow_mut() += 1;
        }
        if action.agent_notify_changed {
            *notify_changes.borrow_mut() += 1;
        }
        if action.agents_changed {
            *agent_changes.borrow_mut() += 1;
        }
        if action.keymap_changed {
            *keymap_changes.borrow_mut() += 1;
        }
        if action.back {
            *backs.borrow_mut() += 1;
        }
        if action.check_updates {
            *update_checks.borrow_mut() += 1;
        }
        if action.install_update {
            *update_installs.borrow_mut() += 1;
        }
    });
    harness.run();
    (harness, probe)
}

#[test]
fn the_page_shows_back_nav_items_and_the_section_title() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Back to app");
    // "Appearance" appears twice: active nav item + content title.
    assert_eq!(harness.get_all_by_label("Appearance").count(), 2);
    assert_eq!(harness.get_all_by_label("Git").count(), 1);
    assert_eq!(harness.get_all_by_label("Updates").count(), 1);
}

#[test]
fn clicking_a_nav_item_switches_the_section() {
    let (mut harness, probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Git").click();
    harness.run();

    assert_eq!(*probe.section.borrow(), PreferencesSection::Git);
    assert_eq!(
        harness.get_all_by_label("Git").count(),
        2,
        "the Git section shows its title in addition to the nav item"
    );
    assert_eq!(harness.get_all_by_label("Appearance").count(), 1);
    assert!(
        harness.query_by_label("Auto").is_none(),
        "the Appearance controls leave the content when the Git section is active"
    );
}

#[test]
fn the_navigation_column_stays_left_of_the_content() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    let (_, _, back_x1, _) = bounds(&harness, "Back to app");
    let (_, _, git_x1, _) = bounds(&harness, "Git");
    let (auto_x0, _, _, _) = bounds(&harness, "Auto");

    assert!(
        back_x1 < auto_x0 && git_x1 < auto_x0,
        "the nav must stay left of the content: back x1={back_x1}, git x1={git_x1}, auto x0={auto_x0}"
    );
    assert!(
        auto_x0 >= 240.0,
        "the content must start after the fixed nav (~240pt), auto x0={auto_x0}"
    );
}

#[test]
fn clicking_back_signals_the_exit() {
    let (mut harness, probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Back to app").click();
    harness.run();
    assert_eq!(*probe.backs.borrow(), 1);
}

#[test]
fn toggling_completion_notifications_flips_the_pref_and_signals() {
    let (mut harness, probe) = agents_section();
    assert_eq!(*probe.section.borrow(), PreferencesSection::Agents);
    assert!(*probe.notify.borrow(), "notifications default on");

    harness
        .get_all_by_role(egui::accesskit::Role::CheckBox)
        .next()
        .unwrap()
        .click();
    harness.run();

    assert!(!*probe.notify.borrow(), "the toggle flips the pref off");
    assert_eq!(*probe.notify_changes.borrow(), 1);
}

/// Tall enough to lay the whole section out: kittest cannot click a control
/// scrolled out of the window.
fn agents_section() -> (Harness<'static>, PageProbe) {
    let (mut harness, probe) = page_harness_sized(
        ThemeMode::Auto,
        PullDefault::default(),
        egui::vec2(800.0, 2400.0),
    );
    harness.get_by_label("Agents").click();
    harness.run();
    (harness, probe)
}

fn agent_names(probe: &PageProbe) -> Vec<String> {
    probe
        .agents
        .borrow()
        .iter()
        .map(|agent| agent.name.clone())
        .collect()
}

#[test]
fn removing_an_agent_drops_its_row_and_reports_a_change() {
    let (mut harness, probe) = agents_section();
    click_button(&mut harness, "Codex, codex", "Button");
    harness.get_by_label("Remove agent Codex").click();
    harness.run();

    assert_eq!(agent_names(&probe), ["Claude Code", "opencode"]);
    assert_eq!(*probe.agent_changes.borrow(), 1);
}

fn click_button(harness: &mut Harness<'_>, label: &str, role: &str) {
    harness
        .get_all_by_label(label)
        .find(|n| format!("{:?}", n.accesskit_node().role()) == role)
        .unwrap_or_else(|| panic!("no {role} \"{label}\""))
        .click();
    harness.run();
}

fn text_input_count(harness: &Harness<'_>) -> usize {
    harness
        .query_all_by(|n| format!("{:?}", n.role()) == "TextInput")
        .count()
}

#[test]
fn agents_stay_folded_and_only_one_opens_at_a_time() {
    let (mut harness, _probe) = agents_section();
    assert_eq!(text_input_count(&harness), 0);

    click_button(&mut harness, "Codex, codex", "Button");
    let one_agent_open = text_input_count(&harness);
    assert_eq!(one_agent_open, 4);

    click_button(&mut harness, "opencode, opencode", "Button");
    assert_eq!(text_input_count(&harness), one_agent_open);
    harness.get_by_label("Remove agent opencode");
}

#[test]
fn an_empty_added_agent_takes_the_typed_name_in_its_focused_block() {
    let (mut harness, probe) = agents_section();
    click_button(&mut harness, "Add agent", "Button");
    click_button(&mut harness, "Empty agent", "Button");
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput" && n.is_focused())
        .type_text("Aider");
    harness.run();

    assert_eq!(
        agent_names(&probe),
        ["Claude Code", "Codex", "opencode", "Aider"]
    );
    assert!(*probe.agent_changes.borrow() >= 2);
}

#[test]
fn an_agent_added_from_a_preset_comes_with_its_three_commands() {
    let (mut harness, probe) = agents_section();
    probe.agents.borrow_mut().clear();
    harness.run();

    click_button(&mut harness, "Add agent", "Button");
    click_button(&mut harness, "Codex", "Button");

    assert_eq!(*probe.agents.borrow(), [Preset::Codex.agent()]);
    assert_eq!(*probe.agent_changes.borrow(), 1);
}

#[test]
fn an_agent_the_watcher_cannot_detect_is_flagged() {
    let (mut harness, probe) = agents_section();
    click_button(&mut harness, "Claude Code, claude", "Button");
    assert!(harness.query_by_label_contains("won't detect").is_none());

    probe.agents.borrow_mut()[1].command = "cursor-agent --fast".to_owned();
    harness.run();
    click_button(&mut harness, "Codex, cursor-agent --fast", "Button");

    harness.get_by_label("helm won't detect cursor-agent as an agent — the phone won't list it");
}

/// Picks `agent` in the nth agent dropdown still on the default choice: the
/// commit message card's first, the review card's second.
fn choose_agent(harness: &mut Harness<'_>, nth: usize, agent: &str) {
    harness
        .get_all_by_label("Claude Code")
        .filter(|n| format!("{:?}", n.accesskit_node().role()) == "Button")
        .nth(nth)
        .unwrap()
        .click();
    harness.run();
    click_button(harness, agent, "RadioButton");
}

#[test]
fn the_commit_message_takes_the_agent_picked_in_its_dropdown() {
    let (mut harness, probe) = agents_section();

    choose_agent(&mut harness, 0, "Codex");

    assert_eq!(probe.commit_message.borrow().agent, "Codex");
    assert_eq!(probe.review.borrow().agent, "Claude Code");
    assert_eq!(*probe.agent_changes.borrow(), 1);
}

#[test]
fn the_review_takes_the_agent_picked_in_its_dropdown() {
    let (mut harness, probe) = agents_section();

    choose_agent(&mut harness, 1, "opencode");

    assert_eq!(probe.review.borrow().agent, "opencode");
    assert_eq!(probe.commit_message.borrow().agent, "Claude Code");
}

#[test]
fn a_renamed_agent_keeps_the_actions_that_chose_it() {
    let (mut harness, probe) = agents_section();
    click_button(&mut harness, "Claude Code, claude", "Button");
    harness
        .get_all_by(|n| format!("{:?}", n.role()) == "TextInput")
        .next()
        .unwrap()
        .click();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput" && n.is_focused())
        .type_text("!");
    harness.run();

    let renamed = probe.agents.borrow()[0].name.clone();
    assert_ne!(renamed, "Claude Code");
    assert_eq!(probe.commit_message.borrow().agent, renamed);
    assert_eq!(probe.review.borrow().agent, renamed);
}

#[test]
fn an_agent_without_the_needed_command_is_not_a_choice() {
    let (mut harness, probe) = agents_section();
    assert!(harness.query_by_label("Choose an agent").is_none());

    probe.agents.borrow_mut()[0].headless_command.clear();
    harness.run();

    harness.get_by_label("Choose an agent");
}

#[test]
fn a_menu_opened_without_a_choice_lists_the_agents_once_they_can() {
    let (mut harness, probe) = agents_section();
    let agents = std::mem::take(&mut *probe.agents.borrow_mut());
    harness.run();
    click_button(&mut harness, "Choose an agent", "Button");
    harness.key_press(egui::Key::Escape);
    harness.run();

    *probe.agents.borrow_mut() = agents;
    harness.run();
    click_button(&mut harness, "Claude Code", "Button");

    let choice = harness
        .get_all_by_label("opencode")
        .find(|n| format!("{:?}", n.accesskit_node().role()) == "RadioButton")
        .unwrap();
    let bounds = choice.accesskit_node().raw_bounds().unwrap();
    assert!(bounds.height() < 30.0, "the choice wraps: {bounds:?}");
}

#[test]
fn an_edited_prompt_offers_to_restore_its_default() {
    let (mut harness, probe) = agents_section();
    assert!(harness.query_by_label_contains("Restore default").is_none());

    probe.commit_message.borrow_mut().prompt = "Short.".to_owned();
    harness.run();
    click_button(&mut harness, "Commit message prompt, Edited", "Button");
    click_button(&mut harness, "Restore default", "Button");

    assert_eq!(
        probe.commit_message.borrow().prompt,
        CommitMessageSettings::default().prompt
    );
    assert_eq!(*probe.agent_changes.borrow(), 1);
}

#[test]
fn the_three_theme_modes_render() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Auto");
    harness.get_by_label("Light");
    harness.get_by_label("Dark");
}

#[test]
fn the_appearance_section_shows_the_theme_row_with_its_description() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Theme");
    harness.get_by_label("Use light, dark, or match your system");
}

#[test]
fn the_theme_control_sits_right_of_its_label() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    let (_, _, label_x1, _) = bounds(&harness, "Theme");
    let (auto_x0, _, _, _) = bounds(&harness, "Auto");
    assert!(
        auto_x0 > label_x1,
        "the segmented control must be in the right slot: label x1={label_x1}, auto x0={auto_x0}"
    );
}

#[test]
fn the_right_aligned_controls_stay_inside_the_window() {
    // Default kittest window: 800×600 — narrower than the content's max width
    // (640 + nav 240). The content must clamp to the real space, otherwise the
    // rows' right slot goes off-screen and becomes unclickable.
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    let (_, _, dark_x1, _) = bounds(&harness, "Dark");
    assert!(
        dark_x1 <= 800.0,
        "the right-slot control must stay inside the window: dark x1={dark_x1}"
    );
}

#[test]
fn the_content_column_is_centered_in_the_content_area() {
    // Wide window: the column (max 640) floats at the center of the content area
    // instead of staying anchored left. The "Theme" label carries the card's left
    // inset, the "Dark" segment its right inset — the two insets being equal, the
    // measured margins must be too.
    let (harness, _probe) = page_harness_sized(
        ThemeMode::Auto,
        PullDefault::default(),
        egui::vec2(1400.0, 800.0),
    );
    let (theme_x0, _, _, _) = bounds(&harness, "Theme");
    let (_, _, dark_x1, _) = bounds(&harness, "Dark");
    let left_gap = theme_x0 - 240.0;
    let right_gap = 1400.0 - dark_x1;
    assert!(
        (left_gap - right_gap).abs() <= 1.0,
        "column not centered: left margin={left_gap}, right margin={right_gap}"
    );
    assert!(
        left_gap > 32.0,
        "wide window: the column must pull away from the nav edge (margin={left_gap})"
    );
}

#[test]
fn the_theme_segments_keep_their_visual_order_in_the_slot() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    let (auto_x0, _, auto_x1, _) = bounds(&harness, "Auto");
    let (light_x0, _, light_x1, _) = bounds(&harness, "Light");
    let (dark_x0, _, _, _) = bounds(&harness, "Dark");
    assert!(
        auto_x1 <= light_x0 && light_x1 <= dark_x0,
        "expected visual order Auto|Light|Dark: auto=[{auto_x0},{auto_x1}], light=[{light_x0},{light_x1}], dark x0={dark_x0}"
    );
}

#[test]
fn clicking_dark_switches_the_mode_and_reports_a_change() {
    let (mut harness, probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Dark").click();
    harness.run();
    assert_eq!(*probe.mode.borrow(), ThemeMode::Dark);
    assert_eq!(
        *probe.theme_changes.borrow(),
        1,
        "selecting a new mode reports exactly one change"
    );
}

#[test]
fn clicking_light_switches_the_mode() {
    let (mut harness, probe) = page_harness(ThemeMode::Dark);
    harness.get_by_label("Light").click();
    harness.run();
    assert_eq!(*probe.mode.borrow(), ThemeMode::Light);
    assert_eq!(*probe.theme_changes.borrow(), 1);
}

#[test]
fn clicking_the_already_selected_mode_reports_no_change() {
    let (mut harness, probe) = page_harness(ThemeMode::Light);
    harness.get_by_label("Light").click();
    harness.run();
    assert_eq!(*probe.mode.borrow(), ThemeMode::Light);
    assert_eq!(
        *probe.theme_changes.borrow(),
        0,
        "re-selecting the active mode must not persist or repaint"
    );
}

// ---- Light/dark theme choice (Appearance) ----

#[test]
fn the_appearance_section_shows_the_light_and_dark_theme_rows() {
    let (harness, _probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Light theme");
    harness.get_by_label("Colors used when the appearance is light");
    harness.get_by_label("Dark theme");
    harness.get_by_label("Colors used when the appearance is dark");
    assert_eq!(
        harness.get_all_by_label("Helm").count(),
        2,
        "both dropdowns show the current family (Helm by default)"
    );
}

#[test]
fn opening_the_dark_dropdown_lists_the_dark_presets() {
    let (mut harness, _probe) = page_harness(ThemeMode::Auto);
    // Two "Helm" buttons: the Dark theme row's is the lower one.
    let button = harness
        .get_all_by_label("Helm")
        .max_by(|a, b| {
            let ay = a.accesskit_node().bounding_box().map_or(0.0, |b| b.y0);
            let by = b.accesskit_node().bounding_box().map_or(0.0, |b| b.y0);
            ay.total_cmp(&by)
        })
        .expect("dropdown Dark theme");
    button.click();
    harness.run();

    harness.get_by_label("GitHub Dark");
    harness.get_by_label("Catppuccin Mocha");
    harness.get_by_label("One Dark");
    harness.get_by_label("Tokyo Night");
}

#[test]
fn selecting_a_dark_preset_updates_the_family_and_reports_one_change() {
    let (mut harness, probe) = page_harness(ThemeMode::Auto);
    let button = harness
        .get_all_by_label("Helm")
        .max_by(|a, b| {
            let ay = a.accesskit_node().bounding_box().map_or(0.0, |b| b.y0);
            let by = b.accesskit_node().bounding_box().map_or(0.0, |b| b.y0);
            ay.total_cmp(&by)
        })
        .expect("dropdown Dark theme");
    button.click();
    harness.run();
    harness.get_by_label("GitHub Dark").click();
    harness.run();

    assert_eq!(*probe.dark_theme.borrow(), "github");
    assert_eq!(
        *probe.light_theme.borrow(),
        "helm",
        "the dark choice does not touch the light family"
    );
    assert_eq!(*probe.theme_changes.borrow(), 1);
    harness.get_by_label("GitHub Dark");
}

// ---- Git section (M14-4) ----

/// Page opened on the Git section, with the given starting Pull default.
fn git_section(start: PullDefault) -> (Harness<'static>, PageProbe) {
    let (mut harness, probe) = page_harness_with(ThemeMode::Auto, start);
    harness.get_by_label("Git").click();
    harness.run();
    (harness, probe)
}

/// Radio node (option of the open menu) carrying `label` — the dropdown button
/// carries the same label as the current option, the role tells them apart.
fn radio_status(harness: &Harness<'_>, label: &str) -> String {
    let node = harness
        .get_all_by_label(label)
        .find(|n| format!("{:?}", n.accesskit_node().role()) == "RadioButton")
        .unwrap_or_else(|| panic!("radio option \"{label}\" missing from the menu"));
    format!("{:?}", node.accesskit_node().toggled())
}

#[test]
fn the_git_section_shows_the_pull_row_with_the_current_default() {
    let (harness, _probe) = git_section(PullDefault::default());
    harness.get_by_label("Default pull behavior");
    harness.get_by_label("Operation run by the Pull button in the graph toolbar");
    harness.get_by_label("Pull (fast-forward if possible)");
}

#[test]
fn the_pull_dropdown_sits_right_of_its_label_inside_the_window() {
    let (harness, _probe) = git_section(PullDefault::default());
    let (_, _, label_x1, _) = bounds(&harness, "Default pull behavior");
    let (button_x0, _, button_x1, _) = bounds(&harness, "Pull (fast-forward if possible)");
    assert!(
        button_x0 > label_x1,
        "the dropdown must be in the right slot: label x1={label_x1}, button x0={button_x0}"
    );
    assert!(
        button_x1 <= 800.0,
        "the dropdown must stay inside the window: button x1={button_x1}"
    );
}

#[test]
fn opening_the_dropdown_lists_the_four_options_with_the_current_checked() {
    let (mut harness, _probe) = git_section(PullDefault::Rebase);
    harness.get_by_label("Pull (rebase)").click();
    harness.run();

    harness.get_by_label("Fetch All");
    harness.get_by_label("Pull (fast-forward if possible)");
    harness.get_by_label("Pull (fast-forward only)");
    assert_eq!(
        radio_status(&harness, "Pull (rebase)"),
        "Some(True)",
        "the current option is checked"
    );
    assert_eq!(
        radio_status(&harness, "Fetch All"),
        "Some(False)",
        "the other options are unchecked"
    );
}

#[test]
fn selecting_an_option_updates_the_default_and_reports_one_change() {
    let (mut harness, probe) = git_section(PullDefault::default());
    harness
        .get_by_label("Pull (fast-forward if possible)")
        .click();
    harness.run();
    harness.get_by_label("Pull (rebase)").click();
    harness.run();

    assert_eq!(*probe.pull.borrow(), PullDefault::Rebase);
    assert_eq!(
        *probe.pull_changes.borrow(),
        1,
        "the selection reports exactly one change (to persist)"
    );
    harness.get_by_label("Pull (rebase)");
    assert!(
        harness
            .query_by_label("Pull (fast-forward if possible)")
            .is_none(),
        "the dropdown button follows the new default"
    );
}

#[test]
fn re_selecting_the_current_default_reports_no_change() {
    let (mut harness, probe) = git_section(PullDefault::Rebase);
    harness.get_by_label("Pull (rebase)").click();
    harness.run();
    harness
        .get_all_by_label("Pull (rebase)")
        .find(|n| format!("{:?}", n.accesskit_node().role()) == "RadioButton")
        .expect("radio option missing")
        .click();
    harness.run();

    assert_eq!(*probe.pull.borrow(), PullDefault::Rebase);
    assert_eq!(
        *probe.pull_changes.borrow(),
        0,
        "re-selecting the current default must not rewrite the prefs"
    );
}

// ---- Terminal section (M30-4) ----

#[test]
fn the_terminal_section_shows_the_editor_dropdown() {
    let (mut harness, _probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Terminal").click();
    harness.run();

    harness.get_by_label("Editor");
    harness.get_by_label("IDE opened by a Cmd+click on a file link in the terminal");
    harness.get_by_label("VS Code");
}

#[test]
fn picking_an_editor_updates_the_choice_and_reports_a_change() {
    let (mut harness, probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Terminal").click();
    harness.run();
    // The dropdown button carries the current IDE; opening it lists the three.
    harness.get_by_label("VS Code").click();
    harness.run();
    harness.get_by_label("Cursor");
    harness.get_by_label("Zed").click();
    harness.run();

    assert_eq!(*probe.editor.borrow(), Editor::Zed);
    assert!(
        *probe.editor_changes.borrow() >= 1,
        "the selection reports at least one change (to persist)"
    );
}

// ---- Updates section (M16-7) ----

/// Page opened on the Updates section, with the given updater snapshot.
fn updates_section(updates: UpdatesView) -> (Harness<'static>, PageProbe) {
    let (mut harness, probe) = page_harness_full(
        ThemeMode::Auto,
        PullDefault::default(),
        egui::vec2(800.0, 600.0),
        updates,
    );
    harness.get_by_label("Updates").click();
    // `run_steps`, not `run`: the busy states render a spinner, which repaints
    // forever and trips `run`'s settle check.
    harness.run_steps(2);
    (harness, probe)
}

fn available_state() -> UpdateState {
    UpdateState::Available {
        version: Version::parse("0.2.0").expect("semver"),
        asset_url: "https://example.invalid/helm-macos.zip".to_owned(),
    }
}

#[test]
fn the_updates_section_shows_the_version_and_check_rows() {
    let (harness, _probe) = updates_section(UpdatesView {
        version: "9.9.9".to_owned(),
        ..idle_updates()
    });
    assert_eq!(
        harness.get_all_by_label("Updates").count(),
        2,
        "active nav item + content title"
    );
    harness.get_by_label("Version");
    harness.get_by_label("v9.9.9");
    harness.get_by_label("Check for updates");
    harness.get_by_label("Check now");
}

#[test]
fn clicking_check_now_raises_the_check_intent() {
    let (mut harness, probe) = updates_section(idle_updates());
    harness.get_by_label("Check now").click();
    harness.run();
    assert_eq!(*probe.update_checks.borrow(), 1);
    assert_eq!(
        *probe.update_installs.borrow(),
        0,
        "the check button never raises the install intent"
    );
}

#[test]
fn the_checking_state_shows_progress_and_disables_the_button() {
    let (mut harness, probe) = updates_section(UpdatesView {
        state: UpdateState::Checking,
        ..idle_updates()
    });
    harness.get_by_label("Checking…");
    harness.get_by_label("Check now").click();
    harness.run_steps(2);
    assert_eq!(
        *probe.update_checks.borrow(),
        0,
        "the button is disabled while a check runs"
    );
}

#[test]
fn the_up_to_date_state_renders_inline() {
    let (harness, _probe) = updates_section(UpdatesView {
        state: UpdateState::UpToDate,
        ..idle_updates()
    });
    harness.get_by_label("Up to date");
}

#[test]
fn the_available_state_offers_install_and_relaunch() {
    let (mut harness, probe) = updates_section(UpdatesView {
        state: available_state(),
        ..idle_updates()
    });
    harness.get_by_label("New version v0.2.0");
    harness.get_by_label("Install & Relaunch").click();
    harness.run();
    assert_eq!(*probe.update_installs.borrow(), 1);
    assert_eq!(
        *probe.update_checks.borrow(),
        0,
        "installing does not re-raise the check intent"
    );
}

#[test]
fn the_install_progress_disables_the_check_button() {
    let (mut harness, probe) = updates_section(UpdatesView {
        state: UpdateState::Downloading,
        ..idle_updates()
    });
    harness.get_by_label("Downloading…");
    harness.get_by_label("Check now").click();
    harness.run_steps(2);
    assert_eq!(*probe.update_checks.borrow(), 0);
}

#[test]
fn the_installing_state_renders_its_progress() {
    let (harness, _probe) = updates_section(UpdatesView {
        state: UpdateState::Installing,
        ..idle_updates()
    });
    harness.get_by_label("Installing…");
}

#[test]
fn the_error_state_renders_the_message_inline() {
    let (harness, _probe) = updates_section(UpdatesView {
        state: UpdateState::Error("Update check failed — network unreachable".to_owned()),
        ..idle_updates()
    });
    harness.get_by_label("Update check failed — network unreachable");
    harness.get_by_label("Check now");
}

#[test]
fn the_error_state_allows_retrying_the_check() {
    let (mut harness, probe) = updates_section(UpdatesView {
        state: UpdateState::Error("Update check failed — network unreachable".to_owned()),
        ..idle_updates()
    });
    harness.get_by_label("Check now").click();
    harness.run();
    assert_eq!(
        *probe.update_checks.borrow(),
        1,
        "an inline error leaves the check button enabled (update.md §8)"
    );
}

#[test]
fn dev_mode_shows_the_disabled_note_without_controls() {
    let (harness, _probe) = updates_section(UpdatesView {
        bundled: false,
        ..idle_updates()
    });
    harness.get_by_label("Running outside an app bundle — updates disabled");
    assert!(
        harness.query_by_label("Check now").is_none(),
        "no check button outside a bundle"
    );
}

// ---- Project section (M21) ----

struct ProjectProbe {
    project_changes: Rc<RefCell<usize>>,
    picks: Rc<RefCell<usize>>,
    selections: Rc<RefCell<Vec<usize>>>,
}

/// Drives `preferences_page` with the Project section active. `present` toggles
/// the project view vs. the "no repository" placeholder; `projects`/`selected`
/// drive the section's project picker.
fn project_harness(
    present: bool,
    projects: &[&str],
    selected: usize,
) -> (Harness<'static>, ProjectProbe) {
    let palette = Palette::light();
    let probe = ProjectProbe {
        project_changes: Rc::new(RefCell::new(0)),
        picks: Rc::new(RefCell::new(0)),
        selections: Rc::new(RefCell::new(Vec::new())),
    };
    let base = Rc::new(RefCell::new(String::new()));
    let post = Rc::new(RefCell::new(String::new()));
    let run = Rc::new(RefCell::new(String::new()));
    let port = Rc::new(RefCell::new(String::new()));
    let project_changes = probe.project_changes.clone();
    let picks = probe.picks.clone();
    let selections = probe.selections.clone();
    let projects: Vec<String> = projects.iter().map(|s| s.to_string()).collect();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let mut base = base.borrow_mut();
            let mut post = post.borrow_mut();
            let mut run = run.borrow_mut();
            let mut port = port.borrow_mut();
            let mut notify = true;
            let project = present.then_some(ProjectView {
                projects: &projects,
                selected,
                worktree_base: &mut base,
                post_create: &mut post,
                run_command: &mut run,
                base_port: &mut port,
                base_hint: "/Users/dev/helm-studio.worktrees",
            });
            let mut release_notes_cache = egui_commonmark::CommonMarkCache::default();
            let action = preferences_page(
                ui,
                &palette,
                &mut PreferencesSection::Project,
                &mut ThemeMode::Auto,
                &mut "helm".to_owned(),
                &mut "helm".to_owned(),
                &mut PullDefault::default(),
                &mut Editor::default(),
                &mut String::new(),
                &mut String::new(),
                &idle_pr_sources(),
                &mut notify,
                AgentsView {
                    agents: &mut Agent::defaults(),
                    commit_message: &mut CommitMessageSettings::default(),
                    review: &mut ReviewSettings::default(),
                },
                PhoneView {
                    start_at_launch: &mut false,
                    devices: &[],
                },
                &mut Keymap::default(),
                &mut KeyboardState::default(),
                &idle_updates(),
                &helm::cli::ShellCommand::Unbundled,
                &mut release_notes_cache,
                project,
            );
            if action.project_changed {
                *project_changes.borrow_mut() += 1;
            }
            if action.pick_worktree_base {
                *picks.borrow_mut() += 1;
            }
            if let Some(index) = action.project_selected {
                selections.borrow_mut().push(index);
            }
        });
    harness.run();
    (harness, probe)
}

#[test]
fn the_project_section_titles_with_the_project_and_shows_both_rows() {
    let (harness, _probe) = project_harness(true, &["helm-studio"], 0);
    harness.get_by_label("helm-studio");
    harness.get_by_label("Worktrees base");
    harness.get_by_label("Post-create script");
    harness.get_by_label("Choose…");
}

#[test]
fn the_project_picker_switches_the_configured_project() {
    let (mut harness, probe) = project_harness(true, &["helm-studio", "other-app"], 0);
    harness.get_by_label("helm-studio").click();
    harness.run();
    harness.get_by_label("other-app").click();
    harness.run();
    assert_eq!(
        *probe.selections.borrow(),
        vec![1],
        "picking another project raises its index"
    );
}

#[test]
fn clicking_choose_raises_the_picker_intent() {
    let (mut harness, probe) = project_harness(true, &["helm-studio"], 0);
    harness.get_by_label("Choose…").click();
    harness.run();
    assert_eq!(*probe.picks.borrow(), 1);
    assert_eq!(
        *probe.project_changes.borrow(),
        0,
        "asking for the picker is not itself a field edit"
    );
}

#[test]
fn without_a_repository_the_section_invites_opening_one() {
    let (harness, _probe) = project_harness(false, &[], 0);
    harness.get_by_label("Open a repository to configure it.");
    assert!(
        harness.query_by_label("Worktrees base").is_none(),
        "no fields render until a repository is open"
    );
}

// ---- Pull Requests section (M-PR / PR8) ----

struct PrProbe {
    email: Rc<RefCell<String>>,
    token: Rc<RefCell<String>>,
    email_changes: Rc<RefCell<usize>>,
    token_saves: Rc<RefCell<usize>>,
}

/// Drives `preferences_page` with the Pull Requests section active and the given
/// (already-loaded) source statuses. Real email/token buffers so edits and the
/// "Save" intent can be observed.
fn pr_harness(github: SourceStatus, bitbucket: SourceStatus) -> (Harness<'static>, PrProbe) {
    let palette = Palette::light();
    let probe = PrProbe {
        email: Rc::new(RefCell::new(String::new())),
        token: Rc::new(RefCell::new(String::new())),
        email_changes: Rc::new(RefCell::new(0)),
        token_saves: Rc::new(RefCell::new(0)),
    };
    let email = probe.email.clone();
    let token = probe.token.clone();
    let email_changes = probe.email_changes.clone();
    let token_saves = probe.token_saves.clone();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let pr_sources = PrSourcesView {
                github: github.clone(),
                bitbucket: bitbucket.clone(),
                loaded: true,
            };
            let mut release_notes_cache = egui_commonmark::CommonMarkCache::default();
            let action = preferences_page(
                ui,
                &palette,
                &mut PreferencesSection::PullRequests,
                &mut ThemeMode::Auto,
                &mut "helm".to_owned(),
                &mut "helm".to_owned(),
                &mut PullDefault::default(),
                &mut Editor::default(),
                &mut email.borrow_mut(),
                &mut token.borrow_mut(),
                &pr_sources,
                &mut true,
                AgentsView {
                    agents: &mut Agent::defaults(),
                    commit_message: &mut CommitMessageSettings::default(),
                    review: &mut ReviewSettings::default(),
                },
                PhoneView {
                    start_at_launch: &mut false,
                    devices: &[],
                },
                &mut Keymap::default(),
                &mut KeyboardState::default(),
                &idle_updates(),
                &helm::cli::ShellCommand::Unbundled,
                &mut release_notes_cache,
                None,
            );
            if action.bitbucket_email_changed {
                *email_changes.borrow_mut() += 1;
            }
            if action.save_bitbucket_token {
                *token_saves.borrow_mut() += 1;
            }
        });
    harness.run();
    (harness, probe)
}

/// Focuses then types into the section's sole field of the given accesskit role
/// (email = "TextInput", masked token = "PasswordInput") — `type_text` only sends
/// the text event, so focus must land first.
fn type_into_field(harness: &mut Harness<'_>, role: &str, text: &str) {
    harness
        .get_all_by(|n| format!("{:?}", n.role()) == role)
        .next()
        .unwrap_or_else(|| panic!("{role} field missing"))
        .focus();
    harness.run();
    harness
        .get_all_by(|n| format!("{:?}", n.role()) == role)
        .next()
        .unwrap_or_else(|| panic!("{role} field missing"))
        .type_text(text);
    harness.run();
}

#[test]
fn the_pull_requests_nav_opens_the_section_with_both_sources() {
    let (mut harness, probe) = page_harness(ThemeMode::Auto);
    harness.get_by_label("Pull Requests").click();
    harness.run();
    assert_eq!(*probe.section.borrow(), PreferencesSection::PullRequests);
    harness.get_by_label("GitHub");
    harness.get_by_label("Bitbucket");
    harness.get_by_label("Email");
    harness.get_by_label("API token");
    harness.get_by_label("Save");
}

#[test]
fn each_source_surfaces_its_status() {
    let (harness, _probe) = pr_harness(
        SourceStatus::Unavailable("Install gh and run `gh auth login`".to_owned()),
        SourceStatus::Ok,
    );
    harness.get_by_label("Install gh and run `gh auth login`");
    harness.get_by_label("Connected");
}

#[test]
fn editing_the_bitbucket_email_reports_a_change() {
    let (mut harness, probe) = pr_harness(SourceStatus::Ok, SourceStatus::Absent);
    type_into_field(&mut harness, "TextInput", "me@corp.com");
    assert_eq!(*probe.email.borrow(), "me@corp.com");
    assert!(
        *probe.email_changes.borrow() >= 1,
        "every keystroke flags the email as changed for the app to persist"
    );
}

#[test]
fn saving_the_bitbucket_token_signals_without_a_field_edit() {
    let (mut harness, probe) = pr_harness(
        SourceStatus::Ok,
        SourceStatus::Unavailable("Set a Bitbucket email and token in Preferences".to_owned()),
    );
    type_into_field(&mut harness, "TextInput", "me@corp.com");
    type_into_field(&mut harness, "PasswordInput", "secret-token");
    harness.get_by_label("Save").click();
    harness.run();
    assert_eq!(*probe.token.borrow(), "secret-token");
    assert_eq!(
        *probe.token_saves.borrow(),
        1,
        "clicking Save raises exactly one store-token intent"
    );
}

// ---- Keyboard section (M24) ----

/// Drives `preferences_page` with the Keyboard section active. Tall enough for
/// the three group cards to render without scrolling.
fn keyboard_harness() -> (Harness<'static>, PageProbe) {
    let (mut harness, probe) = page_harness_sized(
        ThemeMode::Auto,
        PullDefault::default(),
        egui::vec2(900.0, 1900.0),
    );
    harness.get_by_label("Keyboard").click();
    harness.run();
    (harness, probe)
}

fn center_of(harness: &Harness<'_>, label: &str) -> egui::Pos2 {
    let (x0, y0, x1, y1) = bounds(harness, label);
    egui::pos2(((x0 + x1) / 2.0) as f32, ((y0 + y1) / 2.0) as f32)
}

fn cmd(shift: bool) -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        mac_cmd: true,
        shift,
        ..Default::default()
    }
}

#[test]
fn the_keyboard_section_lists_groups_rows_and_default_keycaps() {
    let (harness, _probe) = keyboard_harness();
    harness.get_by_label("Restore defaults");
    harness.get_by_label("Global");
    assert_eq!(
        harness.get_all_by_label("Terminal").count(),
        2,
        "nav item + the Terminal group label"
    );
    assert_eq!(
        harness.get_all_by_label("Git").count(),
        2,
        "nav item + the Git group label"
    );
    harness.get_by_label("Split right");
    harness.get_by_label("Commit the staged changes");
    harness.get_by_label("⌘D");
    harness.get_by_label("⇧⌘D");
    harness.get_by_label("⌘↩");
    harness.get_by_label("⌥⌘←");
    harness.get_by_label("⌃⌘→");
}

#[test]
fn clicking_a_keycap_arms_the_recorder() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();

    harness.get_by_label("Press shortcut…");
    assert!(
        harness.query_by_label("⌘D").is_none(),
        "the armed row's keycap is replaced by the recorder"
    );
    assert_eq!(probe.keyboard.borrow().recording, Some(Action::SplitRight));
}

#[test]
fn recording_a_valid_combo_rebinds_and_signals_once() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();

    harness.key_press_modifiers(cmd(true), egui::Key::X);
    harness.run();

    harness.get_by_label("⇧⌘X");
    assert!(harness.query_by_label("Press shortcut…").is_none());
    assert_eq!(
        probe.keymap.borrow().shortcut_for(Action::SplitRight),
        Some(Shortcut::cmd_shift(egui::Key::X))
    );
    assert_eq!(*probe.keymap_changes.borrow(), 1);
}

#[test]
fn a_conflicting_combo_is_refused_naming_the_holder() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();

    harness.key_press_modifiers(cmd(false), egui::Key::W);
    harness.run();

    harness.get_by_label("Already used by Close pane");
    harness.get_by_label("Press shortcut…");
    assert_eq!(
        probe.keymap.borrow().shortcut_for(Action::SplitRight),
        Some(Shortcut::cmd(egui::Key::D)),
        "a refused combo leaves the binding untouched"
    );
    assert_eq!(*probe.keymap_changes.borrow(), 0);
}

#[test]
fn reserved_and_modifierless_combos_are_refused() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();

    harness.key_press_modifiers(cmd(false), egui::Key::Num1);
    harness.run();
    harness.get_by_label("Reserved shortcut");

    harness.key_press(egui::Key::X);
    harness.run();
    harness.get_by_label("Add Cmd, Ctrl or Alt");
    harness.get_by_label("Press shortcut…");
    assert_eq!(*probe.keymap_changes.borrow(), 0);
}

#[test]
fn escape_cancels_the_recording_without_rebinding() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(harness.query_by_label("Press shortcut…").is_none());
    harness.get_by_label("⌘D");
    assert!(probe.keyboard.borrow().recording.is_none());
    assert_eq!(*probe.keymap_changes.borrow(), 0);
}

#[test]
fn backspace_unbinds_the_armed_action() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();

    harness.key_press(egui::Key::Backspace);
    harness.run();

    harness.get_by_label("unbound");
    assert_eq!(probe.keymap.borrow().shortcut_for(Action::SplitRight), None);
    assert_eq!(*probe.keymap_changes.borrow(), 1);
}

#[test]
fn clicking_away_cancels_the_recording() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("⌘D").click();
    harness.run();
    harness.get_by_label("Press shortcut…");

    let away = center_of(&harness, "Global");
    harness.event(egui::Event::PointerMoved(away));
    harness.event(egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();

    assert!(harness.query_by_label("Press shortcut…").is_none());
    harness.get_by_label("⌘D");
    assert_eq!(*probe.keymap_changes.borrow(), 0);
}

#[test]
fn hovering_a_deviating_row_reveals_reset_which_restores_the_default() {
    let (mut harness, probe) = keyboard_harness();
    probe
        .keymap
        .borrow_mut()
        .set(Action::SplitRight, Some(Shortcut::cmd_shift(egui::Key::X)));
    harness.run();

    harness.event(egui::Event::PointerMoved(center_of(&harness, "⇧⌘X")));
    harness.run();

    harness.get_by_label("Unbind Split right");
    harness.get_by_label("Reset Split right").click();
    harness.run();

    harness.get_by_label("⌘D");
    assert!(!probe.keymap.borrow().deviates(Action::SplitRight));
    assert_eq!(*probe.keymap_changes.borrow(), 1);
}

#[test]
fn the_unbind_affordance_clears_the_binding() {
    let (mut harness, probe) = keyboard_harness();
    probe
        .keymap
        .borrow_mut()
        .set(Action::SplitRight, Some(Shortcut::cmd_shift(egui::Key::X)));
    harness.run();

    harness.event(egui::Event::PointerMoved(center_of(&harness, "⇧⌘X")));
    harness.run();
    harness.get_by_label("Unbind Split right").click();
    harness.run();

    harness.get_by_label("unbound");
    assert_eq!(probe.keymap.borrow().shortcut_for(Action::SplitRight), None);
    assert_eq!(*probe.keymap_changes.borrow(), 1);
}

#[test]
fn restore_defaults_is_inert_without_deviation_and_resets_everything() {
    let (mut harness, probe) = keyboard_harness();
    harness.get_by_label("Restore defaults").click();
    harness.run();
    assert_eq!(
        *probe.keymap_changes.borrow(),
        0,
        "without a deviation the button is disabled"
    );

    probe
        .keymap
        .borrow_mut()
        .set(Action::SplitRight, Some(Shortcut::cmd_shift(egui::Key::X)));
    probe.keymap.borrow_mut().set(Action::Commit, None);
    harness.run();

    harness.get_by_label("Restore defaults").click();
    harness.run();

    harness.get_by_label("⌘D");
    harness.get_by_label("⌘↩");
    assert!(harness.query_by_label("unbound").is_none());
    assert_eq!(*probe.keymap_changes.borrow(), 1);
}

// ---- Preferences page components (M14-1) ----

/// Two-row card (label + description + button in the slot) separated by a rule —
/// the structure every section of the page reuses.
fn two_row_card() -> Harness<'static> {
    let palette = Palette::light();
    let mut harness = Harness::new_ui(move |ui| {
        settings_card(ui, &palette, |ui| {
            setting_row(
                ui,
                &palette,
                "Theme",
                Some("Use light, dark, or match your system"),
                |ui| {
                    let _ = ui.button("Control A");
                },
            );
            setting_divider(ui, &palette);
            setting_row(ui, &palette, "Default pull behavior", None, |ui| {
                let _ = ui.button("Control B");
            });
        });
    });
    harness.run();
    harness
}

/// a11y bounding box `(x0, y0, x1, y1)` of the node carrying `label`.
fn bounds(harness: &Harness<'_>, label: &str) -> (f64, f64, f64, f64) {
    let b = harness
        .get_by_label(label)
        .accesskit_node()
        .bounding_box()
        .unwrap_or_else(|| panic!("{label} without bounding box"));
    (b.x0, b.y0, b.x1, b.y1)
}

#[test]
fn a_settings_card_renders_labels_descriptions_and_controls() {
    let harness = two_row_card();
    harness.get_by_label("Theme");
    harness.get_by_label("Use light, dark, or match your system");
    harness.get_by_label("Default pull behavior");
    harness.get_by_label("Control A");
    harness.get_by_label("Control B");
}

#[test]
fn the_control_sits_right_of_its_label_inside_the_row() {
    let harness = two_row_card();
    let (_, label_y0, label_x1, label_y1) = bounds(&harness, "Theme");
    let (control_x0, control_y0, _, control_y1) = bounds(&harness, "Control A");
    assert!(
        control_x0 > label_x1,
        "the control must be right of the label: label x1={label_x1}, control x0={control_x0}"
    );
    assert!(
        control_y0 >= label_y0 - 56.0 && control_y1 <= label_y1 + 56.0,
        "the control must stay within its row height"
    );
}

#[test]
fn the_label_block_is_vertically_centered_in_its_row() {
    // The right slot already centers its control in the row height: the
    // label+description block must share that center (no top anchoring).
    let harness = two_row_card();
    let (_, label_y0, _, _) = bounds(&harness, "Theme");
    let (_, _, _, desc_y1) = bounds(&harness, "Use light, dark, or match your system");
    let (_, control_y0, _, control_y1) = bounds(&harness, "Control A");
    let block_center = (label_y0 + desc_y1) / 2.0;
    let control_center = (control_y0 + control_y1) / 2.0;
    assert!(
        (block_center - control_center).abs() <= 1.5,
        "label+description block not centered: block center={block_center}, control center={control_center}"
    );
}

#[test]
fn rows_stack_vertically_without_overlapping() {
    let harness = two_row_card();
    let (_, _, _, first_y1) = bounds(&harness, "Theme");
    let (_, second_y0, _, _) = bounds(&harness, "Default pull behavior");
    assert!(
        second_y0 > first_y1,
        "the second row must be below the first: y1={first_y1} vs y0={second_y0}"
    );
}

#[test]
fn the_control_slot_stays_interactive() {
    let palette = Palette::light();
    let clicked = Rc::new(RefCell::new(false));
    let clicked_sink = clicked.clone();
    let mut harness = Harness::new_ui(move |ui| {
        settings_card(ui, &palette, |ui| {
            setting_row(ui, &palette, "Theme", Some("description"), |ui| {
                if ui.button("Control").clicked() {
                    *clicked_sink.borrow_mut() = true;
                }
            });
        });
    });
    harness.run();
    harness.get_by_label("Control").click();
    harness.run();
    assert!(
        *clicked.borrow(),
        "a click on the control must reach the slot's widget"
    );
}

fn updates_section_harness(bundled: bool) -> Harness<'static> {
    let palette = Palette::light();
    let updates = UpdatesView {
        version: "0.8.4".to_owned(),
        state: UpdateState::Idle,
        bundled,
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let mut release_notes_cache = egui_commonmark::CommonMarkCache::default();
            let _ = preferences_page(
                ui,
                &palette,
                &mut PreferencesSection::Updates,
                &mut ThemeMode::Auto,
                &mut "helm".to_owned(),
                &mut "helm".to_owned(),
                &mut PullDefault::default(),
                &mut Editor::default(),
                &mut String::new(),
                &mut String::new(),
                &idle_pr_sources(),
                &mut true,
                AgentsView {
                    agents: &mut Agent::defaults(),
                    commit_message: &mut CommitMessageSettings::default(),
                    review: &mut ReviewSettings::default(),
                },
                PhoneView {
                    start_at_launch: &mut false,
                    devices: &[],
                },
                &mut Keymap::default(),
                &mut KeyboardState::default(),
                &updates,
                &helm::cli::ShellCommand::Unbundled,
                &mut release_notes_cache,
                None,
            );
        });
    harness.run();
    harness
}

#[test]
fn the_updates_section_renders_the_bundled_release_notes() {
    let harness = updates_section_harness(true);
    assert!(
        harness
            .query_by_label_contains(&crate::ui_release_notes::oldest_notes_fragment())
            .is_some(),
        "Preferences › Updates must render the bundled release notes"
    );
}

#[test]
fn release_notes_stay_browsable_outside_a_bundle() {
    let harness = updates_section_harness(false);
    assert!(
        harness
            .query_by_label_contains(&crate::ui_release_notes::oldest_notes_fragment())
            .is_some(),
        "the notes block is independent of the updater (readable in dev runs)"
    );
}

/// What the Phone section reported, frame after frame.
#[derive(Default)]
struct PhoneProbe {
    start_at_launch: bool,
    at_launch_changes: usize,
    revoked: Vec<usize>,
    revoked_all: usize,
}

fn phone_harness(devices: &[&str]) -> (Harness<'static>, Rc<RefCell<PhoneProbe>>) {
    let palette = Palette::dark();
    let probe = Rc::new(RefCell::new(PhoneProbe::default()));
    let observed = probe.clone();
    let devices: Vec<PhoneDeviceRow> = devices
        .iter()
        .map(|name| PhoneDeviceRow {
            name: (*name).to_owned(),
            detail: "Paired 2026-10-01 · Last seen just now".to_owned(),
        })
        .collect();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let mut probe = observed.borrow_mut();
            let mut start_at_launch = probe.start_at_launch;
            let action = preferences_page(
                ui,
                &palette,
                &mut PreferencesSection::Phone,
                &mut ThemeMode::Auto,
                &mut "helm".to_owned(),
                &mut "helm".to_owned(),
                &mut PullDefault::default(),
                &mut Editor::default(),
                &mut String::new(),
                &mut String::new(),
                &idle_pr_sources(),
                &mut true,
                AgentsView {
                    agents: &mut Agent::defaults(),
                    commit_message: &mut CommitMessageSettings::default(),
                    review: &mut ReviewSettings::default(),
                },
                PhoneView {
                    start_at_launch: &mut start_at_launch,
                    devices: &devices,
                },
                &mut Keymap::default(),
                &mut KeyboardState::default(),
                &idle_updates(),
                &helm::cli::ShellCommand::Unbundled,
                &mut egui_commonmark::CommonMarkCache::default(),
                None,
            );
            probe.start_at_launch = start_at_launch;
            if action.phone_at_launch_changed {
                probe.at_launch_changes += 1;
            }
            probe.revoked.extend(action.revoke_device);
            if action.revoke_all_devices {
                probe.revoked_all += 1;
            }
        });
    harness.run();
    (harness, probe)
}

#[test]
fn start_at_launch_is_off_by_default_and_its_toggle_signals() {
    let (mut harness, probe) = phone_harness(&[]);
    assert!(!probe.borrow().start_at_launch);

    harness.get_by_role(egui::accesskit::Role::CheckBox).click();
    harness.run();

    assert!(probe.borrow().start_at_launch);
    assert_eq!(probe.borrow().at_launch_changes, 1);
}

#[test]
fn each_paired_device_has_its_own_revoke_and_all_go_at_once() {
    let (mut harness, probe) = phone_harness(&["iPhone", "iPad"]);
    harness.get_by_label("iPad");

    harness.get_all_by_label("Revoke").nth(1).unwrap().click();
    harness.run();
    harness.get_by_label("Revoke all").click();
    harness.run();

    assert_eq!(probe.borrow().revoked, vec![1], "the iPad's row");
    assert_eq!(probe.borrow().revoked_all, 1);
}

#[test]
fn no_paired_device_says_how_to_pair_and_offers_no_revoke_all() {
    let (harness, _probe) = phone_harness(&[]);

    harness.get_by_label(NO_PAIRED_DEVICE);
    assert!(harness.query_by_label("Revoke all").is_none());
}
