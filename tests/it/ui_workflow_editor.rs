use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::workflow_editor::{
    workflows_section, ADD_GATE_LABEL, ADD_INPUT_LABEL, ADD_PROMPT_LABEL, CONFIRM_DELETE_LABEL,
    DELETE_LABEL, GATE_LABEL, NEW_WORKFLOW_LABEL, READY_LABEL,
};
use helm::workflow::definition::{Input, InputKind, Step, StepKind, Workflow};

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

fn ticket_input() -> Input {
    Input {
        key: "ticket".to_owned(),
        label: "Ticket number".to_owned(),
        default: String::new(),
        kind: InputKind::Text,
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

fn click(harness: &mut Harness<'static, EditorState>, label: &str) {
    harness.get_all_by_label(label).next().unwrap().click();
    harness.run();
}

fn titles(harness: &Harness<'static, EditorState>) -> Vec<String> {
    harness.state().workflows[0]
        .steps
        .iter()
        .map(|step| step.title.clone())
        .collect()
}

#[test]
fn a_new_workflow_opens_with_one_blank_prompt_and_says_what_is_missing() {
    let mut harness = harness(Vec::new());
    harness.run();
    click(&mut harness, NEW_WORKFLOW_LABEL);

    let state = harness.state();
    assert!(state.changed);
    assert_eq!(state.workflows.len(), 1);
    assert_eq!(state.workflows[0].steps.len(), 1);
    harness.get_by_label("Step 1: the prompt is empty");
}

#[test]
fn a_folded_card_says_whether_it_launches() {
    let mut broken = review_cycle();
    broken.name = "Broken".to_owned();
    broken.steps[0] = prompt("Dev", "");
    let mut harness = harness(vec![review_cycle(), broken]);
    harness.run();
    harness.get_by_label(READY_LABEL);
    harness.get_by_label("1 issue");
}

#[test]
fn inputs_prompts_and_gates_are_added_and_steps_reordered() {
    let mut harness = opened(vec![review_cycle()], "Review cycle");
    click(&mut harness, ADD_INPUT_LABEL);
    click(&mut harness, ADD_GATE_LABEL);
    click(&mut harness, ADD_PROMPT_LABEL);
    let workflow = &harness.state().workflows[0];
    assert_eq!(workflow.inputs.len(), 1);
    assert_eq!(workflow.inputs[0].kind, InputKind::Text);
    assert!(matches!(workflow.steps[2].kind, StepKind::Gate { .. }));
    assert_eq!(titles(&harness), ["Dev", "Review", "", ""]);

    click(&mut harness, "Move step 1 down");
    assert_eq!(titles(&harness), ["Review", "Dev", "", ""]);
}

#[test]
fn a_placeholder_chip_writes_the_input_into_the_prompt() {
    let mut workflow = review_cycle();
    workflow.inputs.push(ticket_input());
    let mut harness = opened(vec![workflow], "Review cycle");
    click(&mut harness, "{ticket}");
    assert_eq!(
        harness.state().workflows[0].steps[0].kind,
        StepKind::Prompt {
            text: "Implement it{ticket}".to_owned()
        }
    );
}

#[test]
fn turning_a_prompt_into_a_gate_keeps_its_text() {
    let mut harness = opened(vec![review_cycle()], "Review cycle");
    click(&mut harness, GATE_LABEL);
    assert_eq!(
        harness.state().workflows[0].steps[0].kind,
        StepKind::Gate {
            message: "Implement it".to_owned()
        }
    );
}

#[test]
fn delete_takes_a_second_click() {
    let mut harness = opened(vec![review_cycle()], "Review cycle");
    click(&mut harness, DELETE_LABEL);
    assert_eq!(harness.state().workflows.len(), 1, "one click only arms it");
    click(&mut harness, CONFIRM_DELETE_LABEL);
    assert!(harness.state().workflows.is_empty());
}

#[test]
fn a_turnless_command_shows_its_warning_and_an_unknown_placeholder_its_error() {
    let mut workflow = review_cycle();
    workflow.steps[0] = prompt("Compact", "/compact");
    workflow.steps[1] = prompt("Review", "Review {ticket}");
    let harness = opened(vec![workflow], "Review cycle");
    harness.get_by_label("Step 1: /compact ends no turn, so the step would never finish");
    harness.get_by_label("Step 2: {ticket} is neither an input nor a built-in");
}
