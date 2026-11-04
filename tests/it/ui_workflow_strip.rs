use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::workflow_strip::{
    state_label, workflow_strip, StripAction, StripView, CONTINUE_LABEL, DISMISS_LABEL,
    NEXT_STEP_LABEL, STOP_LABEL,
};
use helm::workflow::run::RunState;

struct StripState {
    view: StripView,
    clicked: Option<StripAction>,
}

fn harness(state: RunState) -> Harness<'static, StripState> {
    let view = StripView {
        name: "Ticket".to_owned(),
        step: 2,
        steps: 3,
        title: "Review".to_owned(),
        state,
        gate_message: (state == RunState::Gate).then(|| "Check the diff".to_owned()),
    };
    Harness::builder()
        .with_size(egui::vec2(900.0, 60.0))
        .build_ui_state(
            |ui, strip: &mut StripState| {
                if let Some(action) = workflow_strip(ui, &Palette::dark(), &strip.view) {
                    strip.clicked = Some(action);
                }
            },
            StripState {
                view,
                clicked: None,
            },
        )
}

#[test]
fn each_state_offers_only_its_actions() {
    let all = [CONTINUE_LABEL, NEXT_STEP_LABEL, STOP_LABEL, DISMISS_LABEL];
    let cases: [(RunState, &[&str]); 6] = [
        (RunState::Running, &[STOP_LABEL]),
        (RunState::AwaitingReply, &[NEXT_STEP_LABEL, STOP_LABEL]),
        (RunState::NeedsYou, &[NEXT_STEP_LABEL, STOP_LABEL]),
        (RunState::Gate, &[CONTINUE_LABEL, STOP_LABEL]),
        (RunState::Done, &[DISMISS_LABEL]),
        (RunState::Stopped, &[DISMISS_LABEL]),
    ];
    for (state, offered) in cases {
        let mut harness = harness(state);
        harness.step();
        harness.get_by_label_contains(state_label(state));
        for label in all {
            let shown = harness.query_by_label(label).is_some();
            assert_eq!(shown, offered.contains(&label), "{state:?} / {label}");
        }
    }
}

#[test]
fn the_strip_names_the_step_and_shows_the_gate_message() {
    let mut harness = harness(RunState::Gate);
    harness.run();
    harness.get_by_label("Ticket, step 2/3 — Review, Paused: Check the diff");
}

#[test]
fn a_click_reports_its_action() {
    let mut harness = harness(RunState::Gate);
    harness.run();
    harness.get_by_label(CONTINUE_LABEL).click();
    harness.run();
    assert_eq!(harness.state().clicked, Some(StripAction::Continue));
}
