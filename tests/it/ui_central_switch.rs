use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::keybindings::Keymap;
use helm::theme::Palette;
use helm::ui::central_switch;

#[derive(Default)]
struct SwitchState {
    graph_active: bool,
    requested: Option<bool>,
}

fn harness(graph_active: bool) -> Harness<'static, SwitchState> {
    Harness::builder().build_ui_state(
        |ui, state| {
            let palette = Palette::dark();
            let keymap = Keymap::default();
            // kittest replays the click over several frames (press/release); keep
            // only the frame where the switch emits, otherwise the release frame
            // with no event would overwrite the request back to `None`.
            if let Some(request) =
                central_switch(ui, &palette, state.graph_active, &keymap, None, None, true)
            {
                state.requested = Some(request);
            }
        },
        SwitchState {
            graph_active,
            requested: None,
        },
    )
}

#[test]
fn both_segments_render() {
    let mut harness = harness(false);
    harness.run();

    harness.get_by_label("Terminal");
    harness.get_by_label("Git");
}

#[test]
fn renders_the_project_and_worktree_reminder() {
    // The reminder is a painter overlay (no a11y node): assert the switch still
    // renders both segments alongside it and the reminder path doesn't panic.
    let mut harness = Harness::builder().build_ui_state(
        |ui, _state: &mut SwitchState| {
            let palette = Palette::dark();
            let keymap = Keymap::default();
            central_switch(
                ui,
                &palette,
                false,
                &keymap,
                Some("helm-studio"),
                Some("feature-x"),
                true,
            );
        },
        SwitchState::default(),
    );
    harness.run();

    harness.get_by_label("Terminal");
    harness.get_by_label("Git");
}

#[test]
fn clicking_git_requests_graph_mode() {
    let mut harness = harness(false);
    harness.run();

    harness.get_by_label("Git").click();
    harness.run();
    assert_eq!(harness.state().requested, Some(true));
}

#[test]
fn clicking_terminal_requests_terminal_mode() {
    let mut harness = harness(true);
    harness.run();

    harness.get_by_label("Terminal").click();
    harness.run();
    assert_eq!(harness.state().requested, Some(false));
}

#[test]
fn holding_cmd_reveals_the_badge_in_the_target_segment_without_moving_anything() {
    let mut harness = harness(false);
    harness.run();
    assert!(harness.query_by_label("⇧⌘G").is_none());
    let terminal_rest = harness.get_by_label("Terminal").rect();
    let graph_rest = harness.get_by_label("Git").rect();

    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    // The badge appears in the target segment's internal reserve (Git
    // when the terminal is active), without shifting the segments.
    let badge = harness.get_by_label("⇧⌘G").rect();
    assert!(
        graph_rest.contains_rect(badge),
        "badge {badge:?} outside the Git segment {graph_rest:?}"
    );
    assert_eq!(terminal_rest, harness.get_by_label("Terminal").rect());
    assert_eq!(graph_rest, harness.get_by_label("Git").rect());
}

#[test]
fn the_badge_targets_the_terminal_segment_when_graph_is_active() {
    let mut harness = harness(true);
    harness.run();
    let terminal = harness.get_by_label("Terminal").rect();

    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    let badge = harness.get_by_label("⇧⌘G").rect();
    assert!(
        terminal.contains_rect(badge),
        "badge {badge:?} outside the Terminal segment {terminal:?}"
    );
}

#[test]
fn the_badge_does_not_shift_the_segment_ids() {
    // Graph active: the badge (new_child conditional on Cmd) paints in the
    // first segment. Without a stable per-segment id, the auto-id of the next
    // segment would shift when Cmd is toggled — same rect, different id — and
    // egui would paint a red "widget changed id between passes" frame.
    let mut harness = harness(true);
    harness.run();
    let rest = harness.get_by_label("Git").accesskit_node().id();

    harness.input_mut().modifiers.command = true;
    harness.input_mut().modifiers.mac_cmd = true;
    harness.run();

    let held = harness.get_by_label("Git").accesskit_node().id();
    assert_eq!(
        rest, held,
        "the Git segment id must not depend on the badge display"
    );
}

#[test]
fn clicking_the_active_segment_requests_nothing() {
    let mut harness = harness(false);
    harness.run();

    harness.get_by_label("Terminal").click();
    harness.run();
    assert_eq!(
        harness.state().requested,
        None,
        "clicking the already-active segment is a no-op"
    );
}
