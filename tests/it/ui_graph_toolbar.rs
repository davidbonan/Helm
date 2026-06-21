use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::git::sync::PullMode;
use helm::git::worker::SyncCommand;
use helm::theme::Palette;
use helm::ui::graph_toolbar::{
    force_push_modal, graph_toolbar, BusyAction, PullDefault, ToolbarState,
};
use helm::ui::graph_view::BranchEditor;
use helm::ui::repo_sidebar::DeleteModalAction;

fn ready() -> ToolbarState {
    ToolbarState {
        pull_default: PullDefault::Ff,
        busy: None,
        has_remote: true,
        has_upstream: true,
        detached: false,
        unborn: false,
        dirty: true,
        stash_count: 1,
        git_missing: false,
    }
}

/// Drives `graph_toolbar` with a frozen state and returns the intents emitted
/// frame by frame + the final state of the Branch editor (the button toggles
/// `open`, the field itself is rendered by `graph_view`). The closure receives
/// the harness (clicks + `run()`) and the shared editor.
#[allow(deprecated)]
fn drive(
    state: ToolbarState,
    actions: impl Fn(&mut Harness<'_, ()>, &Rc<RefCell<BranchEditor>>),
) -> (Vec<helm::ui::graph_toolbar::ToolbarAction>, BranchEditor) {
    let palette = Palette::light();
    let sink = Rc::new(RefCell::new(Vec::new()));
    let editor = Rc::new(RefCell::new(BranchEditor::default()));
    let sink_ui = sink.clone();
    let editor_ui = editor.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 400.0))
        .build(move |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_clip_rect(ctx.content_rect());
                let action = graph_toolbar(ui, &palette, &state, &mut editor_ui.borrow_mut());
                sink_ui.borrow_mut().push(action);
            });
        });
    // Fixed number of frames: the busy state shows a spinner that requests a
    // repaint on every frame, so `run()` (until stable) would not converge.
    harness.run_steps(2);
    actions(&mut harness, &editor);
    harness.run_steps(2);

    let out = sink.borrow().clone();
    let end = editor.borrow().clone();
    (out, end)
}

#[test]
fn pull_executes_the_default_operation() {
    let (actions, _) = drive(ready(), |h, _| {
        h.get_by_label("Pull").click();
    });
    assert_eq!(
        actions.iter().find_map(|a| a.sync.clone()),
        Some(SyncCommand::Pull(PullMode::Ff))
    );
    assert!(actions.iter().all(|a| a.set_default.is_none()));
}

#[test]
fn fetch_all_default_relabels_the_button_and_fetches() {
    let state = ToolbarState {
        pull_default: PullDefault::FetchAll,
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Fetch").click();
    });
    assert_eq!(
        actions.iter().find_map(|a| a.sync.clone()),
        Some(SyncCommand::FetchAll)
    );
}

#[test]
fn dropdown_selects_a_default_without_executing() {
    let (actions, _) = drive(ready(), |h, _| {
        h.get_by_label("Pull options").click();
        h.run();
        h.get_by_label(
            "Select a default pull/fetch operation to execute when clicking this button",
        );
        h.get_by_label("Fetch All");
        h.get_by_label("Pull (fast-forward only)");
        h.get_by_label("Pull (rebase)").click();
    });
    assert_eq!(
        actions.iter().find_map(|a| a.set_default),
        Some(PullDefault::Rebase)
    );
    assert!(
        actions.iter().all(|a| a.sync.is_none()),
        "the selection sets the default without executing it"
    );
}

#[test]
fn push_emits_the_push_intent() {
    let (actions, _) = drive(ready(), |h, _| {
        h.get_by_label("Push").click();
    });
    assert_eq!(
        actions.iter().find_map(|a| a.sync.clone()),
        Some(SyncCommand::Push)
    );
}

#[test]
fn push_chevron_force_entry_emits_the_force_push_intent_without_pushing() {
    let (actions, _) = drive(ready(), |h, _| {
        h.get_by_label("Push options").click();
        h.run();
        h.get_by_label("Push (force with lease)").click();
    });
    assert!(
        actions.iter().any(|a| a.force_push),
        "the one-shot entry emits the force-push intent"
    );
    assert!(
        actions.iter().all(|a| a.sync.is_none()),
        "force push goes through the modal — nothing runs from the menu"
    );
}

#[test]
fn push_chevron_force_entry_is_disabled_without_an_upstream() {
    let state = ToolbarState {
        has_upstream: false,
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Push options").click();
        h.run();
        h.get_by_label("Push (force with lease)").click();
    });
    assert!(
        actions.iter().all(|a| !a.force_push),
        "no upstream to overwrite ⇒ the entry is greyed out"
    );
}

#[test]
fn force_push_modal_confirms_with_the_red_button_and_cancel_dismisses() {
    let mut confirm = Harness::new_ui_state(
        |ui, state| {
            let palette = Palette::dark();
            force_push_modal(ui, &palette, "feat/x", "origin", state);
        },
        DeleteModalAction::default(),
    );
    confirm.run();
    confirm.get_by_label("Force-push “feat/x” to origin?");
    confirm.get_by_label("Force push").click();
    confirm.run();
    assert!(confirm.state().confirm);

    let mut cancel = Harness::new_ui_state(
        |ui, state| {
            let palette = Palette::dark();
            force_push_modal(ui, &palette, "feat/x", "origin", state);
        },
        DeleteModalAction::default(),
    );
    cancel.run();
    cancel.get_by_label("Cancel").click();
    cancel.run();
    assert!(cancel.state().dismiss);
    assert!(!cancel.state().confirm);
}

#[test]
fn no_remote_disables_pull_and_push() {
    let state = ToolbarState {
        has_remote: false,
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Pull").click();
        h.run();
        h.get_by_label("Push").click();
    });
    assert!(actions.iter().all(|a| a.sync.is_none()));
}

#[test]
fn detached_head_disables_pull_and_push() {
    let state = ToolbarState {
        detached: true,
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Pull").click();
        h.run();
        h.get_by_label("Push").click();
    });
    assert!(actions.iter().all(|a| a.sync.is_none()));
}

#[test]
fn a_running_pull_ignores_clicks_on_both_network_buttons() {
    let state = ToolbarState {
        busy: Some(BusyAction::Pull),
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Pull").click();
        h.step();
        h.get_by_label("Push").click();
    });
    assert!(actions.iter().all(|a| a.sync.is_none()));
}

#[test]
fn a_running_stash_grays_the_whole_toolbar() {
    let state = ToolbarState {
        busy: Some(BusyAction::Stash),
        ..ready()
    };
    let (actions, editor) = drive(state, |h, _| {
        h.get_by_label("Stash").click();
        h.step();
        h.get_by_label("Pull").click();
        h.step();
        h.get_by_label("Push").click();
        h.step();
        h.get_by_label("Branch").click();
        h.step();
        h.get_by_label("Pop").click();
    });
    assert!(actions
        .iter()
        .all(|a| !a.stash && !a.pop && a.sync.is_none()));
    assert!(!editor.open, "Branch grayed out: the editor does not open");
}

/// X ranges of the five buttons (a11y) for a frozen toolbar state.
#[allow(deprecated)]
fn button_x_ranges(state: ToolbarState) -> Vec<(f64, f64)> {
    let palette = Palette::light();
    let editor = Rc::new(RefCell::new(BranchEditor::default()));
    let editor_ui = editor.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 400.0))
        .build(move |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_clip_rect(ctx.content_rect());
                graph_toolbar(ui, &palette, &state, &mut editor_ui.borrow_mut());
            });
        });
    harness.run_steps(2);
    ["Pull", "Push", "Branch", "Stash", "Pop"]
        .iter()
        .map(|label| {
            let bounds = harness
                .get_by_label(label)
                .accesskit_node()
                .bounding_box()
                .expect("button without bounding box");
            (bounds.x0, bounds.x1)
        })
        .collect()
}

#[test]
fn busy_spinner_keeps_the_buttons_in_place() {
    // Regression: the spinner placed via `ui.put` pushed the row's cursor back
    // — Pop painted over Stash during a stash.
    let idle = button_x_ranges(ready());
    let busy = button_x_ranges(ToolbarState {
        busy: Some(BusyAction::Stash),
        ..ready()
    });
    assert_eq!(idle, busy, "the spinner must not shift the row");
    for pair in busy.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "overlapping buttons: {pair:?}");
    }
}

#[test]
fn a_mutation_outside_the_toolbar_ignores_every_click() {
    let state = ToolbarState {
        busy: Some(BusyAction::Other),
        ..ready()
    };
    let (actions, editor) = drive(state, |h, _| {
        h.get_by_label("Pull").click();
        h.step();
        h.get_by_label("Push").click();
        h.step();
        h.get_by_label("Branch").click();
        h.step();
        h.get_by_label("Stash").click();
        h.step();
        h.get_by_label("Pop").click();
    });
    assert!(actions
        .iter()
        .all(|a| !a.stash && !a.pop && a.sync.is_none()));
    assert!(!editor.open);
}

#[test]
fn an_ai_rebase_shows_a_timed_chip_whose_cancel_emits_the_intent() {
    let state = ToolbarState {
        busy: Some(BusyAction::AiRebase {
            seconds: 75,
            cancelling: false,
        }),
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("AI rebase · 1:15");
        h.get_by_label("Pull").click();
        h.step();
        h.get_by_label("Cancel").click();
    });
    assert!(actions.iter().any(|a| a.cancel_ai_rebase));
    assert!(
        actions.iter().all(|a| a.sync.is_none()),
        "the rest of the toolbar stays inert during the run"
    );
}

#[test]
fn a_cancelling_ai_rebase_turns_the_button_inert() {
    let state = ToolbarState {
        busy: Some(BusyAction::AiRebase {
            seconds: 130,
            cancelling: true,
        }),
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("AI rebase · 2:10");
        h.get_by_label("Cancelling…").click();
    });
    assert!(
        actions.iter().all(|a| !a.cancel_ai_rebase),
        "a second cancel has nothing to do"
    );
}

#[test]
fn stash_and_pop_emit_their_intents() {
    let (actions, _) = drive(ready(), |h, _| {
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Pop").click();
    });
    assert!(actions.iter().any(|a| a.stash));
    assert!(actions.iter().any(|a| a.pop));
}

#[test]
fn clean_tree_and_empty_stash_disable_stash_and_pop() {
    let state = ToolbarState {
        dirty: false,
        stash_count: 0,
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Pop").click();
    });
    assert!(actions.iter().all(|a| !a.stash && !a.pop));
}

#[test]
fn unborn_repo_only_keeps_fetch_all_runnable() {
    let state = ToolbarState {
        unborn: true,
        pull_default: PullDefault::FetchAll,
        ..ready()
    };
    let (actions, editor) = drive(state, |h, _| {
        h.get_by_label("Branch").click();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Pop").click();
        h.run();
        h.get_by_label("Fetch").click();
    });
    assert_eq!(
        actions.iter().find_map(|a| a.sync.clone()),
        Some(SyncCommand::FetchAll)
    );
    assert!(actions.iter().all(|a| !a.stash && !a.pop));
    assert!(!editor.open, "Branch grayed out: the editor does not open");
}

#[test]
fn missing_git_blocks_network_but_not_local_actions() {
    let state = ToolbarState {
        git_missing: true,
        ..ready()
    };
    let (actions, _) = drive(state, |h, _| {
        h.get_by_label("Pull").click();
        h.run();
        h.get_by_label("Push").click();
        h.run();
        h.get_by_label("Stash").click();
    });
    assert!(actions.iter().all(|a| a.sync.is_none()));
    assert!(actions.iter().any(|a| a.stash));
}

// The input field itself (Enter, inline error, Esc) is rendered by
// `graph_view` on the HEAD row: see `ui_graph_view.rs`.
#[test]
fn branch_button_toggles_the_inline_editor() {
    let (_, editor) = drive(ready(), |h, _| {
        h.get_by_label("Branch").click();
    });
    assert!(
        editor.open,
        "1st click: opens the editor (rendered by graph_view)"
    );

    let (_, editor) = drive(ready(), |h, editor| {
        h.get_by_label("Branch").click();
        h.run();
        assert!(editor.borrow().open);
        h.get_by_label("Branch").click();
    });
    assert!(!editor.open, "2nd click: closes it");
}
