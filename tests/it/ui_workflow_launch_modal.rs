use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme::Palette;
use helm::ui::workflow_launch_modal::{workflow_launch_modal, LaunchForm, LAUNCH_LABEL};
use helm::workflow::definition::{Input, InputKind, Step, StepKind, Workflow};

struct ModalState {
    form: LaunchForm,
    launch: bool,
    dismiss: bool,
}

fn ticket_flow() -> Workflow {
    let input = |key: &str, label: &str, kind| Input {
        key: key.to_owned(),
        label: label.to_owned(),
        default: String::new(),
        kind,
    };
    Workflow {
        name: "Ticket flow".to_owned(),
        inputs: vec![
            input("ticket", "Ticket number", InputKind::Text),
            input("context", "Context", InputKind::LongText),
            input(
                "depth",
                "Review depth",
                InputKind::Select {
                    options: vec!["light".to_owned(), "deep".to_owned()],
                },
            ),
        ],
        steps: vec![Step {
            title: "Dev".to_owned(),
            kind: StepKind::Prompt {
                text: "Fix {ticket}".to_owned(),
            },
        }],
    }
}

fn harness() -> Harness<'static, ModalState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, state| {
                let action = workflow_launch_modal(ui, &Palette::dark(), &mut state.form);
                state.launch |= action.launch;
                state.dismiss |= action.dismiss;
            },
            ModalState {
                form: LaunchForm::new(ticket_flow()),
                launch: false,
                dismiss: false,
            },
        )
}

fn type_into(harness: &mut Harness<'static, ModalState>, role: &str, text: &str) {
    harness
        .get_by(|n| format!("{:?}", n.role()) == role)
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == role)
        .type_text(text);
    harness.run();
}

#[test]
fn one_field_per_input_under_the_workflow_name() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Ticket flow");
    harness.get_by_label("Ticket number");
    harness.get_by_label("Context");
    harness.get_by_label("Review depth");
    harness.get_by_label("light");
}

#[test]
fn launch_waits_for_every_input_then_enter_launches() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label(LAUNCH_LABEL).click();
    harness.run();
    assert!(!harness.state().launch, "empty inputs cannot launch");

    type_into(&mut harness, "MultilineTextInput", "Legacy parity matters");
    type_into(&mut harness, "TextInput", "BAPS-12");
    harness.key_press(egui::Key::Enter);
    harness.run();

    assert!(harness.state().launch);
    assert_eq!(harness.state().form.values["ticket"], "BAPS-12");
}

#[test]
fn cancel_dismisses_without_launching() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.state().dismiss && !harness.state().launch);
}
