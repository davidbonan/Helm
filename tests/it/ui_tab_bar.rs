use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::keybindings::{Action, Keymap, Shortcut};
use helm::theme::Palette;
use helm::ui::tab_bar::{tab_bar, TabBarAction, TabRename};

#[derive(Default)]
struct TabBarState {
    action: TabBarAction,
    rename: Option<TabRename>,
}

fn harness(count: usize, active: usize) -> Harness<'static, TabBarState> {
    keymap_harness(count, active, Keymap::default())
}

fn keymap_harness(count: usize, active: usize, keymap: Keymap) -> Harness<'static, TabBarState> {
    let titles: Vec<String> = (1..=count).map(|i| format!("Tab {i}")).collect();
    // kittest replays each queued event in its own frame: with the default
    // step_dt (0.25s), two clicks fall outside egui's double-click window
    // (0.3s). A short step keeps the clicks close together.
    Harness::builder().with_step_dt(0.02).build_ui_state(
        move |ui, state| {
            let palette = Palette::dark();
            tab_bar(
                ui,
                &palette,
                &titles,
                active,
                &mut state.rename,
                &keymap,
                &mut state.action,
            );
        },
        TabBarState::default(),
    )
}

#[test]
fn two_tabs_and_the_plus_button_render() {
    let mut harness = harness(2, 0);
    harness.run();

    harness.get_by_label("Tab 1");
    harness.get_by_label("Tab 2");
    harness.get_by_label("New tab");
}

/// Press on `from`, drag past the horizontal threshold onto `to`, release. Each leg
/// is a separate frame so egui registers the drag start, the hover and the drop.
fn drag_tab(harness: &mut Harness<'static, TabBarState>, from: egui::Pos2, to: egui::Pos2) {
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
        .push(egui::Event::PointerMoved(from + egui::vec2(10.0, 0.0)));
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
fn dropping_a_tab_after_another_emits_a_reorder() {
    let mut harness = harness(3, 0);
    harness.run();

    let from = harness.get_by_label("Tab 1").rect();
    let onto = harness.get_by_label("Tab 3").rect();
    // Right half of Tab 3: Tab 1 lands after it.
    let target = egui::pos2(onto.center().x + onto.width() * 0.3, onto.center().y);
    drag_tab(&mut harness, from.center(), target);

    assert_eq!(
        harness.state().action.reorder,
        Some((0, 2, true)),
        "Tab 1 (0) is dropped after Tab 3 (2)"
    );
}

#[test]
fn dropping_a_tab_onto_itself_emits_nothing() {
    let mut harness = harness(3, 0);
    harness.run();

    let tab1 = harness.get_by_label("Tab 1").rect();
    drag_tab(&mut harness, tab1.center(), tab1.center());

    assert!(
        harness.state().action.reorder.is_none(),
        "a no-op drop (a tab onto its own slot) must not reorder"
    );
}

#[test]
fn clicking_a_tab_emits_select() {
    let mut harness = harness(3, 0);
    harness.run();

    harness.get_by_label("Tab 3").click();
    harness.run();
    assert_eq!(harness.state().action.select, Some(2));
    assert_eq!(harness.state().action.close, None);
    assert!(!harness.state().action.new);
}

#[test]
fn clicking_the_plus_button_requests_a_new_tab() {
    let mut harness = harness(2, 1);
    harness.run();

    harness.get_by_label("New tab").click();
    harness.run();
    assert!(harness.state().action.new);
    assert_eq!(harness.state().action.select, None);
}

#[test]
fn clicking_a_tab_close_emits_close_not_select() {
    let mut harness = harness(2, 0);
    harness.run();

    harness.get_by_label("Close Tab 2").click();
    harness.run();
    assert_eq!(harness.state().action.close, Some(1));
    assert_eq!(
        harness.state().action.select,
        None,
        "the close hit takes priority over selecting the tab"
    );
}

#[test]
fn holding_cmd_reveals_tab_badges_without_shifting_the_chips() {
    let mut harness = harness(3, 0);
    harness.run();
    assert!(harness.query_by_label("⌘1").is_none());
    assert!(harness.query_by_label("⌘T").is_none());
    let chip1 = harness.get_by_label("Tab 1").rect();
    let chip3 = harness.get_by_label("Tab 3").rect();

    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    harness.get_by_label("⌘1");
    harness.get_by_label("⌘3");
    harness.get_by_label("⌘T");
    assert_eq!(
        chip1,
        harness.get_by_label("Tab 1").rect(),
        "revealing the badges must not move the chips"
    );
    assert_eq!(chip3, harness.get_by_label("Tab 3").rect());
}

#[test]
fn tab_badges_hide_when_ctrl_joins_the_chord() {
    let mut harness = harness(2, 0);
    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.input_mut().modifiers.ctrl = true;
    harness.run();

    assert!(
        harness.query_by_label("⌘1").is_none(),
        "Cmd+Ctrl selects a repo (⌃⌘N), so the tab badges hide"
    );
}

#[test]
fn new_tab_badge_shows_the_rebound_shortcut() {
    let mut keymap = Keymap::default();
    keymap.set(Action::NewTab, Some(Shortcut::cmd_alt(egui::Key::N)));
    let mut harness = keymap_harness(2, 0, keymap);
    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    harness.get_by_label("⌥⌘N");
    assert!(
        harness.query_by_label("⌘T").is_none(),
        "the default ⌘T badge is replaced by the override"
    );
}

#[test]
fn new_tab_badge_hides_when_the_action_is_unbound() {
    let mut keymap = Keymap::default();
    keymap.set(Action::NewTab, None);
    let mut harness = keymap_harness(2, 0, keymap);
    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    assert!(
        harness.query_by_label("⌘T").is_none(),
        "no badge for an unbound action"
    );
    harness.get_by_label("⌘1");
}

#[test]
fn cmd_swaps_the_close_affordance_for_the_badge() {
    let mut harness = harness(2, 0);
    harness.run();
    harness.get_by_label("Close Tab 2");

    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    assert!(
        harness.query_by_label("Close Tab 2").is_none(),
        "while Cmd is held the close hit is inert (the badge takes its place)"
    );
}

/// Two clicks in the same frame: under egui's `max_double_click_delay` threshold.
fn double_click(harness: &mut Harness<'_, TabBarState>, label: &str) {
    harness.get_by_label(label).click();
    harness.get_by_label(label).click();
    harness.run();
}

fn rename_editor_open(harness: &Harness<'_, TabBarState>) -> bool {
    harness.state().rename.is_some()
}

#[test]
fn double_clicking_a_tab_opens_the_rename_editor() {
    let mut harness = harness(2, 0);
    harness.run();
    assert!(!rename_editor_open(&harness));

    double_click(&mut harness, "Tab 2");

    assert!(rename_editor_open(&harness));
    assert!(
        harness.query_by_label("Tab 2").is_none(),
        "the chip label is replaced by the text editor"
    );
    harness.get_by_label("Tab 1");
}

#[test]
fn right_click_rename_opens_the_editor() {
    let mut harness = harness(2, 0);
    harness.run();

    harness.get_by_label("Tab 1").click_secondary();
    harness.run();
    harness.get_by_label("Rename").click();
    harness.run();

    assert!(rename_editor_open(&harness));
    assert!(harness.query_by_label("Tab 1").is_none());
}

#[test]
fn typing_and_enter_commits_the_rename() {
    let mut harness = harness(2, 0);
    harness.run();
    double_click(&mut harness, "Tab 1");
    harness.run();

    // The editor is focused with the whole title selected: typing replaces it.
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("build".to_owned()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    assert_eq!(
        harness.state().action.rename,
        Some((0, "build".to_owned())),
        "Enter commits the typed name for the edited tab"
    );
    assert!(!rename_editor_open(&harness));
}

#[test]
fn escape_cancels_the_rename() {
    let mut harness = harness(2, 0);
    harness.run();
    double_click(&mut harness, "Tab 1");
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert_eq!(harness.state().action.rename, None);
    assert!(!rename_editor_open(&harness));
    harness.run();
    harness.get_by_label("Tab 1");
}

#[test]
fn clicking_away_commits_the_rename() {
    let mut harness = harness(2, 0);
    harness.run();
    double_click(&mut harness, "Tab 1");
    harness.run();

    harness.get_by_label("Tab 2").click();
    harness.run();

    assert_eq!(
        harness.state().action.rename,
        Some((0, "Tab 1".to_owned())),
        "losing focus commits the buffer as-is"
    );
    assert!(!rename_editor_open(&harness));
}

#[test]
fn hovering_a_tab_shows_the_pointer_cursor() {
    let mut harness = harness(2, 0);
    harness.run();

    let pos = harness.get_by_label("Tab 2").rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
}
