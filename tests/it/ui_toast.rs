//! UI E2E — toast overlay (`ui::toast`, git.md §10): message rendering, dismiss
//! via the cross, success gone after expiry. Pure logic (dedup, TTL, cap) is
//! covered by the module's unit tests.

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::toast::{toast_overlay, ToastAction, Toasts};

/// Drives `toast_overlay` over an empty CentralPanel from a seeded state;
/// returns the shared stack for state assertions.
#[allow(deprecated)]
fn harness(toasts: Toasts) -> (Harness<'static, ()>, Rc<RefCell<Toasts>>) {
    let palette = Palette::light();
    let shared = Rc::new(RefCell::new(toasts));
    let shared_ui = shared.clone();
    let harness = Harness::builder()
        .with_size(egui::vec2(900.0, 400.0))
        .build(move |ctx| {
            egui::CentralPanel::default().show(ctx, |_ui| {});
            toast_overlay(ctx, &palette, &mut shared_ui.borrow_mut());
        });
    (harness, shared)
}

#[test]
fn error_and_success_toasts_render_their_messages() {
    let mut toasts = Toasts::default();
    toasts.error(
        "Stash pop failed — conflicts while applying — the stash was kept",
        0.0,
    );
    toasts.success("Pulled — branch updated", 0.0);
    let (mut harness, shared) = harness(toasts);
    harness.run_steps(2);

    harness.get_by_label("Stash pop failed — conflicts while applying — the stash was kept");
    harness.get_by_label("Pulled — branch updated");
    assert_eq!(shared.borrow().items().len(), 2);
}

#[test]
fn cross_dismisses_only_the_targeted_toast() {
    let mut toasts = Toasts::default();
    toasts.error("Push rejected — not fast-forward, never forced", 0.0);
    toasts.error("Authentication failed", 0.0);
    let (mut harness, shared) = harness(toasts);
    harness.run_steps(2);

    harness.get_by_label("Dismiss notification 0").click();
    harness.run_steps(2);

    let remaining: Vec<String> = shared
        .borrow()
        .items()
        .iter()
        .map(|t| t.message.clone())
        .collect();
    assert_eq!(remaining, ["Authentication failed"]);
}

#[test]
fn expired_toast_is_not_rendered() {
    let mut toasts = Toasts::default();
    // The success is born well before the harness clock (which starts at ~0 and
    // advances per frame): the first render's tick expires it. The error is
    // recent, so it stays within its TTL.
    toasts.success("Pushed", -3600.0);
    toasts.error("Authentication failed", 0.0);
    let (mut harness, shared) = harness(toasts);
    harness.run_steps(2);

    harness.get_by_label("Authentication failed");
    assert_eq!(
        shared
            .borrow()
            .items()
            .iter()
            .map(|t| t.message.clone())
            .collect::<Vec<_>>(),
        ["Authentication failed"],
        "the expired success disappears, the recent error remains"
    );
}

#[test]
fn empty_stack_renders_nothing() {
    let (mut harness, _) = harness(Toasts::default());
    harness.run_steps(2);
    assert!(harness.query_by_label("Dismiss notification 0").is_none());
}

// ---- Action toasts (M16-7) ----

/// Same harness, plus a counter of frames where `toast_overlay` reported an
/// action click.
#[allow(deprecated)]
fn action_harness(
    toasts: Toasts,
) -> (
    Harness<'static, ()>,
    Rc<RefCell<Toasts>>,
    Rc<RefCell<usize>>,
) {
    let palette = Palette::light();
    let shared = Rc::new(RefCell::new(toasts));
    let shared_ui = shared.clone();
    let clicks = Rc::new(RefCell::new(0usize));
    let clicks_ui = clicks.clone();
    let harness = Harness::builder()
        .with_size(egui::vec2(900.0, 400.0))
        .build(move |ctx| {
            egui::CentralPanel::default().show(ctx, |_ui| {});
            if toast_overlay(ctx, &palette, &mut shared_ui.borrow_mut()).is_some() {
                *clicks_ui.borrow_mut() += 1;
            }
        });
    (harness, shared, clicks)
}

#[test]
fn the_action_button_signals_and_dismisses_its_toast() {
    let mut toasts = Toasts::default();
    toasts.info_with_action("Update available v0.2.0", ToastAction::InstallUpdate, 0.0);
    let (mut harness, shared, clicks) = action_harness(toasts);
    harness.run_steps(2);

    harness.get_by_label("Update available v0.2.0");
    harness.get_by_label("Install").click();
    harness.run_steps(2);

    assert_eq!(*clicks.borrow(), 1, "the overlay reports the action click");
    assert!(
        shared.borrow().is_empty(),
        "the acted-on toast is dismissed"
    );
}

#[test]
fn an_action_toast_persists_past_the_success_ttl() {
    let mut toasts = Toasts::default();
    // Born well before the harness clock: a success would already be expired.
    toasts.info_with_action(
        "Update available v0.2.0",
        ToastAction::InstallUpdate,
        -3600.0,
    );
    let (mut harness, shared, _clicks) = action_harness(toasts);
    harness.run_steps(2);

    harness.get_by_label("Update available v0.2.0");
    harness.get_by_label("Install");
    assert_eq!(shared.borrow().items().len(), 1);
}

#[test]
fn the_cross_dismisses_an_action_toast_without_signaling() {
    let mut toasts = Toasts::default();
    toasts.info_with_action("Update available v0.2.0", ToastAction::InstallUpdate, 0.0);
    let (mut harness, shared, clicks) = action_harness(toasts);
    harness.run_steps(2);

    harness.get_by_label("Dismiss notification 0").click();
    harness.run_steps(2);

    assert_eq!(*clicks.borrow(), 0, "the cross is not the action");
    assert!(shared.borrow().is_empty());
}
