use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::ai::AiProvider;
use helm::git::ai_rebase::{AiRebaseOutcome, AiRebaseReport};
use helm::git::rebase::RebaseCommit;
use helm::theme::Palette;
use helm::ui::ai_rebase_modal::{
    ai_rebase_modal, ai_rebase_report_modal, AiRebasePage, START_LABEL,
};

fn commit(n: u8, summary: &str) -> RebaseCommit {
    RebaseCommit {
        oid: git2::Oid::from_str(&format!("{n:040x}")).unwrap(),
        short_id: format!("{n:07x}"),
        summary: summary.into(),
        message: summary.into(),
        author: "Test".into(),
    }
}

/// Page already filled by the worker reply — `summaries` **oldest first**
/// (the wire order; the modal displays newest first).
fn loaded_page(summaries: &[&str]) -> AiRebasePage {
    let mut page = AiRebasePage::loading("feature", "main");
    page.adopt(
        summaries
            .iter()
            .enumerate()
            .map(|(index, summary)| commit(index as u8 + 1, summary))
            .collect(),
    );
    page
}

struct ModalState {
    page: AiRebasePage,
    busy: bool,
    start: bool,
    dismiss: bool,
}

fn harness(page: AiRebasePage, busy: bool) -> Harness<'static, ModalState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, state| {
                let palette = Palette::dark();
                let action = ai_rebase_modal(
                    ui,
                    &palette,
                    &mut state.page,
                    AiProvider::Claude,
                    state.busy,
                );
                state.start |= action.start;
                state.dismiss |= action.dismiss;
            },
            ModalState {
                page,
                busy,
                start: false,
                dismiss: false,
            },
        )
}

#[test]
fn the_loading_modal_shows_the_title_the_contract_and_a_loader() {
    let mut harness = harness(AiRebasePage::loading("feature", "main"), false);
    // `step`, not `run`: the spinner requests a repaint every frame.
    harness.step();

    harness.get_by_label("AI rebase — feature onto main");
    harness.get_by_label(
        "claude runs the rebase in the repository and resolves conflicts itself — \
         it never pushes.",
    );
    harness.get_by_label("Loading commits");

    harness.get_by_label(START_LABEL).click();
    harness.step();
    assert!(
        !harness.state().start,
        "nothing can start while the recap is loading"
    );
}

#[test]
fn a_failed_recap_shows_the_error_and_cannot_start() {
    let mut page = AiRebasePage::loading("feature", "main");
    page.fail("revspec 'ghost' not found");
    let mut harness = harness(page, false);
    harness.run();

    harness.get_by_label("revspec 'ghost' not found");

    harness.get_by_label(START_LABEL).click();
    harness.run();
    assert!(!harness.state().start, "a failed recap cannot start");
}

#[test]
fn an_empty_recap_says_the_branch_is_already_contained() {
    let mut harness = harness(loaded_page(&[]), false);
    harness.run();

    harness.get_by_label("No commits to replay — feature is already contained in main");

    harness.get_by_label(START_LABEL).click();
    harness.run();
    assert!(!harness.state().start, "an empty recap cannot start");
}

#[test]
fn the_recap_lists_the_commits_newest_first_with_their_count() {
    let mut harness = harness(loaded_page(&["oldest commit", "newest commit"]), false);
    harness.run();

    harness.get_by_label("2 commits to replay");
    let newest = harness.get_by_label("newest commit").rect();
    let oldest = harness.get_by_label("oldest commit").rect();
    assert!(
        newest.top() < oldest.top(),
        "newest on top like the graph: {newest:?} vs {oldest:?}"
    );
    harness.get_by_label("0000001");
    harness.get_by_label("0000002");
}

#[test]
fn typed_instructions_land_in_the_page_state() {
    let mut harness = harness(loaded_page(&["c1"]), false);
    harness.run();

    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .type_text("Squash everything into a single commit");
    harness.run();

    assert_eq!(
        harness.state().page.instructions,
        "Squash everything into a single commit"
    );
}

#[test]
fn start_emits_on_a_loaded_recap() {
    let mut harness = harness(loaded_page(&["c1", "c2"]), false);
    harness.run();

    harness.get_by_label(START_LABEL).click();
    harness.run();

    assert!(harness.state().start);
    assert!(!harness.state().dismiss);
}

#[test]
fn a_busy_runner_greys_start_out() {
    let mut harness = harness(loaded_page(&["c1"]), true);
    harness.run();

    harness.get_by_label(START_LABEL).click();
    harness.run();

    assert!(
        !harness.state().start,
        "Start stays inert while another git command runs"
    );
}

#[test]
fn cancel_dismisses_without_starting() {
    let mut harness = harness(loaded_page(&["c1"]), false);
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().dismiss);
    assert!(!harness.state().start);
}

#[test]
fn expected_oids_rebuild_the_oldest_first_order() {
    let page = loaded_page(&["c1", "c2"]);
    let expected = page.expected();
    assert_eq!(expected.len(), 2);
    assert_eq!(
        expected[0],
        git2::Oid::from_str(&format!("{:040x}", 1)).unwrap()
    );
    assert_eq!(
        expected[1],
        git2::Oid::from_str(&format!("{:040x}", 2)).unwrap()
    );
}

// ---- Report modal ----

fn report_harness(report: AiRebaseReport) -> Harness<'static, (AiRebaseReport, bool)> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, state| {
                let palette = Palette::dark();
                state.1 |= ai_rebase_report_modal(ui, &palette, &state.0);
            },
            (report, false),
        )
}

#[test]
fn the_report_modal_shows_the_verified_outcome_and_the_summary() {
    let mut harness = report_harness(AiRebaseReport {
        summary: "Rebased 3 commits onto main.\nOne conflict in a.txt: kept both hunks.".into(),
        outcome: AiRebaseOutcome::Completed,
    });
    harness.run();

    harness.get_by_label("AI rebase — Rebase completed");
    harness.get_by_label("Rebased 3 commits onto main.\nOne conflict in a.txt: kept both hunks.");
}

#[test]
fn a_left_in_progress_report_points_at_the_banner() {
    let mut harness = report_harness(AiRebaseReport {
        summary: "Could not finish.".into(),
        outcome: AiRebaseOutcome::LeftInProgress,
    });
    harness.run();

    harness.get_by_label("AI rebase — Rebase left in progress");
    harness.get_by_label(
        "Conflicts are unresolved — resolve them in the terminal or Abort \
         from the sidebar banner.",
    );
}

#[test]
fn copy_report_puts_the_summary_on_the_clipboard_without_closing() {
    let mut harness = report_harness(AiRebaseReport {
        summary: "One conflict in a.txt: kept both hunks.".into(),
        outcome: AiRebaseOutcome::Completed,
    });
    harness.run();

    harness.get_by_label("Copy report").click();
    harness.step();

    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        });
    assert_eq!(
        copied.as_deref(),
        Some("One conflict in a.txt: kept both hunks.")
    );
    assert!(!harness.state().1, "Copy keeps the report open");
}

#[test]
fn close_dismisses_the_report() {
    let mut harness = report_harness(AiRebaseReport {
        summary: "Nothing to do.".into(),
        outcome: AiRebaseOutcome::Unchanged,
    });
    harness.run();

    harness.get_by_label("AI rebase — Branch unchanged");
    harness.get_by_label("Close").click();
    harness.run();

    assert!(harness.state().1, "Close dismisses the report modal");
}
