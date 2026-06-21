use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::rebase::{RebaseChoice, RebaseCommit};
use helm::theme::Palette;
use helm::ui::rebase_view::{rebase_view, RebasePage};

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
/// (the wire order; the page displays newest first).
fn loaded_page(summaries: &[&str]) -> RebasePage {
    let mut page = RebasePage::loading("feature", "main");
    page.adopt(
        summaries
            .iter()
            .enumerate()
            .map(|(index, summary)| commit(index as u8 + 1, summary))
            .collect(),
    );
    page
}

struct PageState {
    page: RebasePage,
    busy: bool,
    start: bool,
    cancel: bool,
}

fn harness(page: RebasePage, busy: bool) -> Harness<'static, PageState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, state| {
                let palette = Palette::dark();
                let action = rebase_view(ui, &palette, &mut state.page, state.busy);
                state.start |= action.start;
                state.cancel |= action.cancel;
            },
            PageState {
                page,
                busy,
                start: false,
                cancel: false,
            },
        )
}

#[test]
fn the_loading_page_shows_the_title_and_a_loader() {
    let mut harness = harness(RebasePage::loading("feature", "main"), false);
    // `step`, not `run`: the spinner requests a repaint every frame.
    harness.step();

    harness.get_by_label("Interactive rebase — feature onto main");
    harness.get_by_label("Loading commits");
    assert!(
        harness.query_by_label("Start rebase").is_none(),
        "nothing can start while the plan is loading"
    );
}

#[test]
fn a_failed_plan_shows_the_error_and_close_cancels() {
    let mut page = RebasePage::loading("feature", "main");
    page.fail("revspec 'ghost' not found");
    let mut harness = harness(page, false);
    harness.run();

    harness.get_by_label("revspec 'ghost' not found");
    assert!(
        harness.query_by_label("Start rebase").is_none(),
        "a failed plan cannot start"
    );

    harness.get_by_label("Close").click();
    harness.run();

    assert!(harness.state().cancel);
}

#[test]
fn an_empty_plan_says_the_branch_is_already_contained() {
    let mut harness = harness(loaded_page(&[]), false);
    harness.run();

    harness.get_by_label("No commits to replay — feature is already contained in main");

    harness.get_by_label("Close").click();
    harness.run();

    assert!(harness.state().cancel);
}

#[test]
fn entries_render_newest_first_with_sha_and_author() {
    let mut harness = harness(loaded_page(&["oldest commit", "newest commit"]), false);
    harness.run();

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
fn a_meld_with_nothing_below_disables_start_with_an_inline_error() {
    let mut page = loaded_page(&["only commit"]);
    page.entries[0].choice = RebaseChoice::Squash;
    let mut harness = harness(page, false);
    harness.run();

    harness.get_by_label(
        "the oldest kept commit cannot squash or fixup — there is no commit below to meld into",
    );

    harness.get_by_label("Start rebase").click();
    harness.run();

    assert!(
        !harness.state().start,
        "Start is disabled on an invalid plan"
    );
}

#[test]
fn reword_opens_the_editor_prefilled_with_the_original_message() {
    let mut page = loaded_page(&["c1", "c2"]);
    page.entries[0].choice = RebaseChoice::Reword;
    // adopt() prefilled the buffer with the commit message; a multi-line body
    // keeps it distinct from the summary label rendered above the editor.
    page.entries[0].message = "c2\n\nbody".into();
    let mut harness = harness(page, false);
    harness.run();

    // The multiline editor exposes its content as the a11y value.
    harness.get_by_value("c2\n\nbody");
    assert!(
        harness
            .query_by_label("a reworded commit needs a non-empty message")
            .is_none(),
        "a prefilled reword is valid"
    );
}

#[test]
fn a_blank_reword_disables_start_with_an_inline_error() {
    let mut page = loaded_page(&["c1", "c2"]);
    page.entries[0].choice = RebaseChoice::Reword;
    page.entries[0].message = String::new();
    let mut harness = harness(page, false);
    harness.run();

    harness.get_by_label("a reworded commit needs a non-empty message");

    harness.get_by_label("Start rebase").click();
    harness.run();

    assert!(
        !harness.state().start,
        "Start is disabled on a blank reword"
    );
}

#[test]
fn dropping_every_commit_states_the_reset_consequence_but_can_start() {
    let mut page = loaded_page(&["c1", "c2"]);
    for entry in &mut page.entries {
        entry.choice = RebaseChoice::Drop;
    }
    let mut harness = harness(page, false);
    harness.run();

    harness.get_by_label("All commits dropped — feature will be reset to main");

    harness.get_by_label("Start rebase").click();
    harness.run();

    assert!(
        harness.state().start,
        "an all-drop plan is explicit, not an error"
    );
}

#[test]
fn start_emits_on_a_valid_plan() {
    let mut harness = harness(loaded_page(&["c1", "c2"]), false);
    harness.run();

    harness.get_by_label("Start rebase").click();
    harness.run();

    assert!(harness.state().start);
    assert!(!harness.state().cancel);
}

#[test]
fn a_busy_runner_greys_start_out() {
    let mut harness = harness(loaded_page(&["c1", "c2"]), true);
    harness.run();

    harness.get_by_label("Start rebase").click();
    harness.run();

    assert!(
        !harness.state().start,
        "Start stays inert while another git command runs"
    );
}

#[test]
fn cancel_closes_the_page() {
    let mut harness = harness(loaded_page(&["c1"]), false);
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().cancel);
    assert!(!harness.state().start);
}

#[test]
fn escape_closes_the_page() {
    let mut harness = harness(loaded_page(&["c1"]), false);
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(harness.state().cancel, "Esc closes the page");
}

#[test]
fn choosing_squash_from_the_combo_updates_the_plan_live() {
    // Single entry: its combo is the only "Pick" on screen.
    let mut harness = harness(loaded_page(&["only commit"]), false);
    harness.run();

    // The closed combo exposes its selection as the a11y value, not a label.
    harness.get_by_value("Pick").click();
    harness.run();
    harness.get_by_label("Squash").click();
    harness.run();

    assert_eq!(harness.state().page.entries[0].choice, RebaseChoice::Squash);
    harness.get_by_label(
        "the oldest kept commit cannot squash or fixup — there is no commit below to meld into",
    );
}
