use std::path::PathBuf;

use egui::Modifiers;
use egui_kittest::Harness;

use helm::app::{
    focus_zone, route_cycle_repo_keys, route_layout_keys, route_select_repo_keys, route_tab_keys,
    route_zoom_keys,
};
use helm::keybindings::{Action, Keymap, Shortcut};
use helm::terminal::emu::{FontZoom, DEFAULT_FONT_SIZE};
use helm::terminal::layout::{Layout, Orient};
use helm::workspace::{Repo, Workspace};

fn cmd(extra: Modifiers) -> Modifiers {
    Modifiers {
        command: true,
        mac_cmd: true,
        ..extra
    }
}

fn cmd_ctrl() -> Modifiers {
    cmd(Modifiers {
        ctrl: true,
        ..Default::default()
    })
}

fn harness() -> Harness<'static, Layout> {
    keymap_harness(Keymap::default())
}

fn keymap_harness(keymap: Keymap) -> Harness<'static, Layout> {
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    Harness::new_ui_state(
        move |ui, layout| {
            let mut close_tab = false;
            route_layout_keys(
                ui.ctx(),
                &keymap,
                layout,
                area,
                DEFAULT_FONT_SIZE,
                &mut close_tab,
            );
        },
        Layout::new(),
    )
}

fn zoom_harness() -> Harness<'static, FontZoom> {
    let keymap = Keymap::default();
    Harness::new_ui_state(
        move |ui, zoom| route_zoom_keys(ui.ctx(), &keymap, zoom),
        FontZoom::default(),
    )
}

#[test]
fn cmd_d_splits_vertical_and_focuses_new_pane() {
    let mut harness = harness();
    harness.run();
    assert_eq!(harness.state().pane_ids().len(), 1);
    let original = harness.state().focus();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();

    assert_eq!(harness.state().pane_ids().len(), 2);
    assert_ne!(
        harness.state().focus(),
        original,
        "Cmd+D focuses the newly created pane"
    );
}

#[test]
fn cmd_shift_d_splits_horizontal() {
    let mut harness = harness();
    harness.run();

    harness.key_press_modifiers(
        cmd(Modifiers {
            shift: true,
            ..Default::default()
        }),
        egui::Key::D,
    );
    harness.run();

    assert_eq!(harness.state().pane_ids().len(), 2);
    let rects = harness.state().rects(helm::terminal::layout::Rect {
        x: 0.0,
        y: 0.0,
        w: 800.0,
        h: 600.0,
    });
    let focus = harness.state().focus();
    let new_rect = rects.iter().find(|(id, _)| *id == focus).unwrap().1;
    assert!(
        new_rect.y > 0.0,
        "Cmd+Shift+D places the new pane at the bottom"
    );
}

#[test]
fn cmd_w_closes_focused_pane() {
    let mut harness = harness();
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();
    assert_eq!(harness.state().pane_ids().len(), 2);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::W);
    harness.run();
    assert_eq!(harness.state().pane_ids().len(), 1, "Cmd+W closes the pane");

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::W);
    harness.run();
    assert_eq!(
        harness.state().pane_ids().len(),
        1,
        "Cmd+W never destroys the last pane"
    );
}

#[test]
fn cmd_alt_arrow_moves_focus_geometrically() {
    let mut harness = harness();
    harness.run();
    let left = harness.state().focus();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();
    let right = harness.state().focus();
    assert_ne!(left, right);

    harness.key_press_modifiers(
        cmd(Modifiers {
            alt: true,
            ..Default::default()
        }),
        egui::Key::ArrowLeft,
    );
    harness.run();
    assert_eq!(
        harness.state().focus(),
        left,
        "Cmd+Alt+Left moves focus to the left pane"
    );
}

#[test]
fn cmd_ctrl_arrow_resizes_focused_split() {
    let mut harness = harness();
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();

    let before = split_ratio(harness.state());
    harness.key_press_modifiers(
        cmd(Modifiers {
            ctrl: true,
            ..Default::default()
        }),
        egui::Key::ArrowLeft,
    );
    harness.run();
    let after = split_ratio(harness.state());

    assert!(
        (after - (before - 0.05)).abs() < 1e-4,
        "Cmd+Ctrl+Left shifts the split ratio by -5% (before={before}, after={after})"
    );
}

fn split_ratio(layout: &Layout) -> f32 {
    match layout.root() {
        helm::terminal::layout::Node::Split { ratio, .. } => *ratio,
        _ => panic!("expected a split"),
    }
}

#[test]
fn rebound_split_fires_on_the_new_combo_and_the_default_goes_dead() {
    let mut keymap = Keymap::default();
    keymap.set(Action::SplitRight, Some(Shortcut::cmd_shift(egui::Key::X)));
    let mut harness = keymap_harness(keymap);
    harness.run();
    assert_eq!(harness.state().pane_ids().len(), 1);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();
    assert_eq!(
        harness.state().pane_ids().len(),
        1,
        "the default Cmd+D must go dead once Split right is rebound"
    );

    harness.key_press_modifiers(
        cmd(Modifiers {
            shift: true,
            ..Default::default()
        }),
        egui::Key::X,
    );
    harness.run();
    assert_eq!(
        harness.state().pane_ids().len(),
        2,
        "the rebound Cmd+Shift+X must split"
    );
}

#[test]
fn unbound_split_leaves_its_default_combo_inert() {
    let mut keymap = Keymap::default();
    keymap.set(Action::SplitRight, None);
    let mut harness = keymap_harness(keymap);
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();
    assert_eq!(
        harness.state().pane_ids().len(),
        1,
        "an unbound action must leave its default combo inert"
    );
}

#[test]
fn cmd_equals_and_minus_change_the_global_font_size() {
    let mut harness = zoom_harness();
    harness.run();
    assert_eq!(harness.state().point_size(), DEFAULT_FONT_SIZE);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Equals);
    harness.run();
    let zoomed_in = harness.state().point_size();
    assert!(
        zoomed_in > DEFAULT_FONT_SIZE,
        "Cmd+= grows the global font size (was {DEFAULT_FONT_SIZE}, now {zoomed_in})"
    );

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Minus);
    harness.run();
    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Minus);
    harness.run();
    let zoomed_out = harness.state().point_size();
    assert!(
        zoomed_out < DEFAULT_FONT_SIZE,
        "Cmd+- shrinks the global font size (now {zoomed_out})"
    );
}

fn workspace_harness(workspace: Workspace) -> Harness<'static, Workspace> {
    Harness::new_ui_state(
        move |ui, workspace| route_select_repo_keys(ui.ctx(), workspace),
        workspace,
    )
}

#[test]
fn cmd_ctrl_digits_switch_the_active_repo_and_restore_its_tree() {
    let mut workspace = Workspace::new();
    workspace.add(Repo::new(PathBuf::from("/tmp/repo-a")));
    workspace.add(Repo::new(PathBuf::from("/tmp/repo-b")));
    workspace.set_active(0);
    workspace
        .active_layout_mut()
        .unwrap()
        .split(Orient::Vertical);
    let a_root = workspace.active_layout().unwrap().root().clone();

    let mut harness = workspace_harness(workspace);
    harness.run();
    assert_eq!(harness.state().active(), Some(0));

    harness.key_press_modifiers(cmd_ctrl(), egui::Key::Num2);
    harness.run();
    assert_eq!(harness.state().active(), Some(1));
    assert_eq!(
        harness.state().active_layout().unwrap().pane_ids().len(),
        1,
        "repo B keeps its pristine single-pane tree"
    );

    harness.key_press_modifiers(cmd_ctrl(), egui::Key::Num1);
    harness.run();
    assert_eq!(harness.state().active(), Some(0));
    assert_eq!(
        harness.state().active_layout().unwrap().root(),
        &a_root,
        "switching back to repo A restores its split tree identically"
    );
}

fn ctrl(extra: Modifiers) -> Modifiers {
    Modifiers {
        ctrl: true,
        ..extra
    }
}

fn cycle_keys_harness(workspace: Workspace) -> Harness<'static, Workspace> {
    let keymap = Keymap::default();
    Harness::new_ui_state(
        move |ui, workspace| route_cycle_repo_keys(ui.ctx(), &keymap, workspace),
        workspace,
    )
}

#[test]
fn ctrl_tab_cycles_the_active_repo_forward_and_back_wrapping() {
    let mut workspace = Workspace::new();
    for name in ["a", "b", "c"] {
        workspace.add(Repo::new(PathBuf::from(format!("/tmp/repo-{name}"))));
    }
    workspace.set_active(0);

    let mut harness = cycle_keys_harness(workspace);
    harness.run();
    assert_eq!(harness.state().active(), Some(0));

    harness.key_press_modifiers(ctrl(Modifiers::default()), egui::Key::Tab);
    harness.run();
    assert_eq!(
        harness.state().active(),
        Some(1),
        "Ctrl+Tab advances to the next repo"
    );

    harness.key_press_modifiers(ctrl(Modifiers::default()), egui::Key::Tab);
    harness.run();
    assert_eq!(harness.state().active(), Some(2));

    harness.key_press_modifiers(ctrl(Modifiers::default()), egui::Key::Tab);
    harness.run();
    assert_eq!(
        harness.state().active(),
        Some(0),
        "Ctrl+Tab wraps from the last repo back to the first"
    );

    harness.key_press_modifiers(
        ctrl(Modifiers {
            shift: true,
            ..Default::default()
        }),
        egui::Key::Tab,
    );
    harness.run();
    assert_eq!(
        harness.state().active(),
        Some(2),
        "Ctrl+Shift+Tab goes backward, wrapping to the last repo"
    );
}

#[test]
fn cmd_one_without_ctrl_does_not_select_a_repo() {
    let mut workspace = Workspace::new();
    workspace.add(Repo::new(PathBuf::from("/tmp/repo-a")));
    workspace.add(Repo::new(PathBuf::from("/tmp/repo-b")));
    workspace.set_active(1);

    let mut harness = workspace_harness(workspace);
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Num1);
    harness.run();
    assert_eq!(
        harness.state().active(),
        Some(1),
        "Cmd+1 selects a tab, not a repo — the repo selector is Cmd+Ctrl+1"
    );
}

#[test]
fn cmd_ctrl_digit_selects_by_physical_key_on_azerty() {
    // AZERTY-FR: the physical Num4 key emits `'` (logical Quote, shift=false). The
    // repo selector must still land on repo 4 from the physical slot — egui only
    // falls back to the physical key for symbols it has no `Key` for, and `'` has
    // a dedicated `Key::Quote`, so `key` stays Quote here.
    let mut workspace = Workspace::new();
    for name in ["a", "b", "c", "d"] {
        workspace.add(Repo::new(PathBuf::from(format!("/tmp/repo-{name}"))));
    }
    workspace.set_active(0);

    let mut harness = workspace_harness(workspace);
    harness.run();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Quote,
        physical_key: Some(egui::Key::Num4),
        pressed: true,
        repeat: false,
        modifiers: cmd_ctrl(),
    });
    harness.run();

    assert_eq!(
        harness.state().active(),
        Some(3),
        "⌃⌘4 on AZERTY (logical Quote / physical Num4) must select repo 4"
    );
}

/// Harness that routes terminal shortcuts exactly like `HelmApp::ui`: through
/// the zone gate (`focus_zone(...).terminal_shortcuts_active()`). `diff_open`
/// simulates the diff view open in the central zone.
fn zone_gated_layout_harness(diff_open: bool, terminal_focused: bool) -> Harness<'static, Layout> {
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let keymap = Keymap::default();
    Harness::new_ui_state(
        move |ui, layout| {
            if focus_zone(diff_open, terminal_focused).terminal_shortcuts_active() {
                let mut close_tab = false;
                route_layout_keys(
                    ui.ctx(),
                    &keymap,
                    layout,
                    area,
                    DEFAULT_FONT_SIZE,
                    &mut close_tab,
                );
            }
        },
        Layout::new(),
    )
}

#[test]
fn diff_overlay_open_blocks_cmd_d_split() {
    let mut harness = zone_gated_layout_harness(true, true);
    harness.run();
    assert_eq!(harness.state().pane_ids().len(), 1);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();

    assert_eq!(
        harness.state().pane_ids().len(),
        1,
        "Cmd+D must not split while the diff overlay owns the central zone (keybindings §4)"
    );
}

#[test]
fn terminal_focus_lets_cmd_d_split() {
    let mut harness = zone_gated_layout_harness(false, true);
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::D);
    harness.run();

    assert_eq!(
        harness.state().pane_ids().len(),
        2,
        "Cmd+D splits when a terminal pane holds the focus"
    );
}

#[test]
fn cmd_zero_resets_the_global_font_size() {
    let mut harness = zoom_harness();
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Equals);
    harness.run();
    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Equals);
    harness.run();
    assert_ne!(harness.state().point_size(), DEFAULT_FONT_SIZE);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Num0);
    harness.run();
    assert_eq!(
        harness.state().point_size(),
        DEFAULT_FONT_SIZE,
        "Cmd+0 resets the global font size"
    );
}

fn tab_keys_harness(workspace: Workspace) -> Harness<'static, Workspace> {
    let keymap = Keymap::default();
    Harness::new_ui_state(
        move |ui, workspace| route_tab_keys(ui.ctx(), &keymap, workspace),
        workspace,
    )
}

fn one_repo_workspace() -> Workspace {
    let mut workspace = Workspace::new();
    workspace.add(Repo::new(PathBuf::from("/tmp/repo-a")));
    workspace.set_active(0);
    workspace
}

#[test]
fn cmd_t_opens_a_new_tab_and_activates_it() {
    let mut harness = tab_keys_harness(one_repo_workspace());
    harness.run();
    assert_eq!(harness.state().tab_count(), Some(1));

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::T);
    harness.run();

    assert_eq!(harness.state().tab_count(), Some(2));
    assert_eq!(
        harness.state().active_tab(),
        Some(1),
        "Cmd+T focuses the freshly opened tab"
    );
}

#[test]
fn cmd_three_selects_the_third_tab() {
    let mut workspace = one_repo_workspace();
    workspace.add_tab();
    workspace.add_tab();
    workspace.set_active_tab(0);

    let mut harness = tab_keys_harness(workspace);
    harness.run();
    assert_eq!(harness.state().active_tab(), Some(0));

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Num3);
    harness.run();

    assert_eq!(
        harness.state().active_tab(),
        Some(2),
        "Cmd+3 selects tab 3 of the active repo"
    );
}

#[test]
fn cmd_digit_for_a_missing_tab_is_a_no_op() {
    let mut harness = tab_keys_harness(one_repo_workspace());
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Num3);
    harness.run();

    assert_eq!(
        harness.state().active_tab(),
        Some(0),
        "selecting a tab that does not exist leaves the active tab unchanged"
    );
}

#[test]
fn tab_keys_are_a_no_op_without_an_active_repo() {
    let mut harness = tab_keys_harness(Workspace::new());
    harness.run();

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::T);
    harness.run();
    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::Num2);
    harness.run();

    assert_eq!(
        harness.state().tab_count(),
        None,
        "with no active repo, Cmd+T / Cmd+N do nothing (terminal.md §10)"
    );
}

/// Harness that routes `Cmd+W` exactly like `HelmApp::ui` and captures the
/// tab-close signal (`Cmd+W` on the **last pane** of a tab, terminal.md §11).
struct CloseProbe {
    layout: Layout,
    tab_closed: bool,
}

fn close_probe_harness(layout: Layout) -> Harness<'static, CloseProbe> {
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let keymap = Keymap::default();
    Harness::new_ui_state(
        move |ui, probe| {
            let mut close_tab = false;
            route_layout_keys(
                ui.ctx(),
                &keymap,
                &mut probe.layout,
                area,
                DEFAULT_FONT_SIZE,
                &mut close_tab,
            );
            if close_tab {
                probe.tab_closed = true;
            }
        },
        CloseProbe {
            layout,
            tab_closed: false,
        },
    )
}

#[test]
fn cmd_w_on_the_last_pane_requests_a_tab_close() {
    let mut harness = close_probe_harness(Layout::new());
    harness.run();
    assert_eq!(harness.state().layout.pane_ids().len(), 1);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::W);
    harness.run();

    assert!(
        harness.state().tab_closed,
        "Cmd+W on the sole pane of a tab requests the tab to close, not a fresh leaf"
    );
    assert_eq!(
        harness.state().layout.pane_ids().len(),
        1,
        "the layout tree is left untouched; the tab close is handled by the app"
    );
}

#[test]
fn cmd_w_with_a_sibling_closes_the_pane_not_the_tab() {
    let mut layout = Layout::new();
    layout.split(Orient::Vertical);
    let mut harness = close_probe_harness(layout);
    harness.run();
    assert_eq!(harness.state().layout.pane_ids().len(), 2);

    harness.key_press_modifiers(cmd(Modifiers::default()), egui::Key::W);
    harness.run();

    assert_eq!(
        harness.state().layout.pane_ids().len(),
        1,
        "with a sibling pane, Cmd+W closes the focused pane"
    );
    assert!(
        !harness.state().tab_closed,
        "closing one of several panes never closes the tab"
    );
}
