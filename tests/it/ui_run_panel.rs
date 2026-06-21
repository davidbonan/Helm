use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::run_panel::{run_panel, RunPanelAction, RunStatus};

/// Drives `run_panel` with a frozen status/command and records the action of the
/// last frame. The closure chains a click then `run()` so the intent is observed
/// on the frame after the press.
fn drive(
    status: RunStatus,
    command: &'static str,
    collapsed: bool,
    editing: bool,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> RunPanelAction {
    let last = Rc::new(RefCell::new(RunPanelAction::default()));
    let buffer = Rc::new(RefCell::new(String::new()));
    let sink = last.clone();
    let palette = Palette::light();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 320.0))
        .build_ui(move |ui| {
            let mut guard = buffer.borrow_mut();
            let edit = editing.then_some(&mut *guard);
            let action = run_panel(
                ui,
                &palette,
                &status,
                command,
                None,
                collapsed,
                edit,
                None,
                None,
                |ui| {
                    ui.label("RUN_BODY");
                },
            );
            // `harness.run()` settles over several frames, only one of which
            // carries the click — keep the last non-empty action, not the
            // default of the final idle frame.
            if action.any() {
                *sink.borrow_mut() = action;
            }
        });

    harness.run();
    actions(&mut harness);
    last.take()
}

#[test]
fn stopped_shows_command_and_paints_the_body() {
    drive(RunStatus::Stopped, "cargo run", false, false, |harness| {
        harness.get_by_label("cargo run");
        harness.get_by_label("RUN_BODY");
    });
}

#[test]
fn empty_command_prompts_to_set_one() {
    drive(RunStatus::Stopped, "", false, false, |harness| {
        harness.get_by_label("Set a run command");
    });
}

#[test]
fn collapsed_hides_the_body() {
    drive(RunStatus::Running, "cargo run", true, false, |harness| {
        assert!(
            harness.query_by_label("RUN_BODY").is_none(),
            "a folded strip must not paint its terminal body"
        );
        harness.get_by_label("cargo run");
    });
}

#[test]
fn running_offers_stop_and_relaunch() {
    let action = drive(RunStatus::Running, "cargo run", false, false, |harness| {
        harness.get_by_label("Relaunch");
        harness.get_by_label("Stop").click();
        harness.run();
    });
    assert!(action.stop);
    assert!(!action.run);
}

#[test]
fn relaunch_button_emits_relaunch() {
    let action = drive(RunStatus::Running, "cargo run", false, false, |harness| {
        harness.get_by_label("Relaunch").click();
        harness.run();
    });
    assert!(action.relaunch);
}

#[test]
fn pencil_begins_the_inline_edit() {
    let action = drive(RunStatus::Stopped, "cargo run", false, false, |harness| {
        harness.get_by_label("Edit run command").click();
        harness.run();
    });
    assert!(action.begin_edit);
}

#[test]
fn check_commits_the_inline_edit() {
    let action = drive(RunStatus::Stopped, "cargo run", false, true, |harness| {
        harness.get_by_label("Save command").click();
        harness.run();
    });
    assert!(action.commit_edit);
}

#[test]
fn enter_commits_the_inline_edit() {
    let action = drive(RunStatus::Stopped, "cargo run", false, true, |harness| {
        harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
        harness.run();
    });
    assert!(action.commit_edit);
    assert!(
        !action.cancel_edit,
        "Enter commits, it must not also cancel"
    );
}

#[test]
fn chevron_toggles_the_panel() {
    let action = drive(RunStatus::Stopped, "cargo run", false, false, |harness| {
        harness.get_by_label("Toggle run panel").click();
        harness.run();
    });
    assert!(action.toggle_collapsed);
}

/// Drives `run_panel` focused on the port chip: `port` is the resolved value shown
/// as a chip, `port_editing` opens the inline numeric editor instead.
fn drive_port(
    port: Option<u16>,
    port_editing: bool,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> RunPanelAction {
    let last = Rc::new(RefCell::new(RunPanelAction::default()));
    let buffer = Rc::new(RefCell::new(String::new()));
    let sink = last.clone();
    let palette = Palette::light();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 320.0))
        .build_ui(move |ui| {
            let mut guard = buffer.borrow_mut();
            let port_edit = port_editing.then_some(&mut *guard);
            let action = run_panel(
                ui,
                &palette,
                &RunStatus::Stopped,
                "vite --port $PORT",
                port,
                false,
                None,
                port_edit,
                None,
                |ui| {
                    ui.label("RUN_BODY");
                },
            );
            if action.any() {
                *sink.borrow_mut() = action;
            }
        });

    harness.run();
    actions(&mut harness);
    last.take()
}

#[test]
fn port_chip_shows_the_resolved_value() {
    drive_port(Some(3001), false, |harness| {
        harness.get_by_label(":3001");
    });
}

#[test]
fn no_port_hides_the_chip() {
    drive_port(None, false, |harness| {
        assert!(
            harness.query_by_label(":3000").is_none(),
            "a command without $PORT must not show a port chip"
        );
    });
}

#[test]
fn clicking_the_chip_begins_the_port_edit() {
    let action = drive_port(Some(3001), false, |harness| {
        harness.get_by_label(":3001").click();
        harness.run();
    });
    assert!(action.begin_port_edit);
}

#[test]
fn check_commits_the_port_edit() {
    let action = drive_port(Some(3001), true, |harness| {
        harness.get_by_label("Save port").click();
        harness.run();
    });
    assert!(action.commit_port_edit);
}

#[test]
fn enter_commits_the_port_edit() {
    let action = drive_port(Some(3001), true, |harness| {
        harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::Enter);
        harness.run();
    });
    assert!(action.commit_port_edit);
}

/// Renders `run_panel` with the held-Cmd `shortcut` badge so the assertions can
/// query the resulting tree (keybindings §5).
fn drive_shortcut(
    status: RunStatus,
    shortcut: Option<&'static str>,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) {
    let palette = Palette::light();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 320.0))
        .build_ui(move |ui| {
            run_panel(
                ui,
                &palette,
                &status,
                "cargo run",
                None,
                false,
                None,
                None,
                shortcut,
                |ui| {
                    ui.label("RUN_BODY");
                },
            );
        });
    harness.run();
    actions(&mut harness);
}

#[test]
fn cmd_held_shows_the_run_shortcut_badge() {
    drive_shortcut(RunStatus::Stopped, Some("⌘R"), |harness| {
        harness.get_by_label("⌘R");
    });
}

#[test]
fn cmd_held_shows_the_badge_while_running() {
    drive_shortcut(RunStatus::Running, Some("⌘R"), |harness| {
        harness.get_by_label("⌘R");
    });
}

#[test]
fn no_shortcut_hides_the_badge() {
    drive_shortcut(RunStatus::Stopped, None, |harness| {
        assert!(
            harness.query_by_label("⌘R").is_none(),
            "no held Cmd ⇒ no run shortcut badge"
        );
    });
}
