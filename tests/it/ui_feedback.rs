use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::feedback::FeedbackKind;
use helm::theme::Palette;
use helm::ui::feedback_modal::{feedback_modal, FeedbackPage, SEND_LABEL};

struct ModalState {
    page: FeedbackPage,
    submit: bool,
    dismiss: bool,
}

fn harness(page: FeedbackPage) -> Harness<'static, ModalState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, state| {
                let palette = Palette::dark();
                let action = feedback_modal(ui, &palette, &mut state.page);
                state.submit |= action.submit;
                state.dismiss |= action.dismiss;
            },
            ModalState {
                page,
                submit: false,
                dismiss: false,
            },
        )
}

#[test]
fn the_modal_shows_its_title_and_the_default_kind() {
    let mut harness = harness(FeedbackPage::default());
    harness.run();

    harness.get_by_label("Send feedback");
    // The closed combo exposes its selection as the a11y value, not a label.
    harness.get_by_value("Bug");
}

#[test]
fn send_is_inert_until_the_description_is_non_empty() {
    let mut harness = harness(FeedbackPage::default());
    harness.run();

    harness.get_by_label(SEND_LABEL).click();
    harness.run();
    assert!(
        !harness.state().submit,
        "an empty description cannot be sent"
    );

    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .type_text("The terminal split focus is lost on resize");
    harness.run();

    harness.get_by_label(SEND_LABEL).click();
    harness.run();
    assert!(harness.state().submit, "a filled description sends");
    assert!(!harness.state().dismiss);
}

#[test]
fn whitespace_only_description_stays_inert() {
    let page = FeedbackPage {
        description: "   \n  ".into(),
        ..Default::default()
    };
    let mut harness = harness(page);
    harness.run();

    harness.get_by_label(SEND_LABEL).click();
    harness.run();
    assert!(!harness.state().submit, "blank description is not sendable");
}

#[test]
fn choosing_suggestion_from_the_combo_updates_the_page() {
    let mut harness = harness(FeedbackPage::default());
    harness.run();

    harness.get_by_value("Bug").click();
    harness.run();
    harness.get_by_label("Suggestion").click();
    harness.run();

    assert_eq!(harness.state().page.kind, FeedbackKind::Suggestion);
}

#[test]
fn cancel_dismisses_without_sending() {
    let page = FeedbackPage {
        description: "anything".into(),
        ..Default::default()
    };
    let mut harness = harness(page);
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().dismiss);
    assert!(!harness.state().submit);
}
