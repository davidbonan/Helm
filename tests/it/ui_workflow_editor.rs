use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::workflow_editor::{
    workflows_section, ADD_INPUT_LABEL, ADD_STEP_LABEL, CONFIRM_DELETE_LABEL, DELETE_LABEL,
    NEW_WORKFLOW_LABEL,
};
use helm::workflow::definition::{InputKind, Step, StepKind, Workflow};

struct EditorState {
    workflows: Vec<Workflow>,
    changed: bool,
}

fn prompt(title: &str, text: &str) -> Step {
    Step {
        title: title.to_owned(),
        kind: StepKind::Prompt {
            text: text.to_owned(),
        },
    }
}

fn review_cycle() -> Workflow {
    Workflow {
        name: "Review cycle".to_owned(),
        inputs: Vec::new(),
        steps: vec![prompt("Dev", "Implement it"), prompt("Review", "Review it")],
    }
}

fn harness(workflows: Vec<Workflow>) -> Harness<'static, EditorState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 1400.0))
        .build_ui_state(
            |ui, state: &mut EditorState| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    state.changed |= workflows_section(ui, &Palette::dark(), &mut state.workflows);
                });
            },
            EditorState {
                workflows,
                changed: false,
            },
        )
}

fn opened(workflows: Vec<Workflow>, title: &str) -> Harness<'static, EditorState> {
    let mut harness = harness(workflows);
    harness.run();
    harness.get_by_label(title).click();
    harness.run();
    harness
}

fn titles(harness: &Harness<'static, EditorState>) -> Vec<String> {
    harness.state().workflows[0]
        .steps
        .iter()
        .map(|step| step.title.clone())
        .collect()
}

#[test]
fn a_new_workflow_starts_with_one_blank_prompt_and_says_what_is_missing() {
    let mut harness = harness(Vec::new());
    harness.run();
    harness
        .get_by_label(&format!("+ {NEW_WORKFLOW_LABEL}"))
        .click();
    harness.run();

    let state = harness.state();
    assert!(state.changed);
    assert_eq!(state.workflows.len(), 1);
    assert_eq!(state.workflows[0].steps.len(), 1);
    harness.get_by_label("Step 1: the prompt is empty");
}

#[test]
fn inputs_and_steps_are_added_and_reordered() {
    let mut harness = opened(vec![review_cycle()], "Review cycle");
    harness
        .get_by_label(&format!("+ {ADD_INPUT_LABEL}"))
        .click();
    harness.run();
    harness.get_by_label(&format!("+ {ADD_STEP_LABEL}")).click();
    harness.run();
    assert_eq!(harness.state().workflows[0].inputs.len(), 1);
    assert_eq!(harness.state().workflows[0].inputs[0].kind, InputKind::Text);
    assert_eq!(titles(&harness), ["Dev", "Review", ""]);

    // Steps' controls follow the inputs' in the tree: the second "↓" is step 1's.
    harness.get_all_by_label("↓").nth(1).unwrap().click();
    harness.run();
    assert_eq!(titles(&harness), ["Review", "Dev", ""]);
}

#[test]
fn delete_takes_a_second_click() {
    let mut harness = opened(vec![review_cycle()], "Review cycle");
    harness.get_by_label(DELETE_LABEL).click();
    harness.run();
    assert_eq!(harness.state().workflows.len(), 1, "one click only arms it");
    harness.get_by_label(CONFIRM_DELETE_LABEL).click();
    harness.run();
    assert!(harness.state().workflows.is_empty());
}

#[test]
fn a_turnless_command_shows_its_warning_and_an_unknown_placeholder_its_error() {
    let mut workflow = review_cycle();
    workflow.steps[0] = prompt("Compact", "/compact");
    workflow.steps[1] = prompt("Review", "Review {ticket}");
    let mut harness = harness(vec![workflow]);
    harness.run();
    harness.get_by_label("Step 1: /compact ends no turn, so the step would never finish");
    harness.get_by_label("Step 2: {ticket} is neither an input nor a built-in");
}
