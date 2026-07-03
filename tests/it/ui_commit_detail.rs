use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use helm::git::commit_detail::{CommitDetail, CommitFile, CommitMeta};
use helm::git::status::ChangeKind;
use helm::theme::Palette;
use helm::ui::commit_detail::commit_detail_panel;
use helm::ui::file_list::{FileMenuOutput, FileViewMode};

fn oid(byte: u8) -> git2::Oid {
    git2::Oid::from_bytes(&[byte; 20]).unwrap()
}

fn sample_detail() -> CommitDetail {
    CommitDetail {
        meta: CommitMeta {
            oid: oid(7),
            short_id: "0000007".to_string(),
            author: "Ada".to_string(),
            email: "ada@example.com".to_string(),
            // 2021-01-01T13:30:00Z at UTC+1 ⇒ wall-clock time 14:30.
            time: 1_609_507_800,
            offset_minutes: 60,
            committer: "Ada".to_string(),
            summary: "Add lib and tests".to_string(),
            body: "Approved-by: Florian".to_string(),
            parents: vec![oid(6)],
        },
        files: vec![
            CommitFile {
                path: "src/lib.rs".to_string(),
                kind: ChangeKind::Modified,
                additions: 62,
                deletions: 5,
            },
            CommitFile {
                path: "tests/new.rs".to_string(),
                kind: ChangeKind::Added,
                additions: 64,
                deletions: 7,
            },
        ],
    }
}

struct State {
    detail: Option<CommitDetail>,
    open_file: Option<(git2::Oid, String)>,
    view: FileViewMode,
    can_amend: bool,
    amended: Option<String>,
}

fn harness(detail: Option<CommitDetail>) -> Harness<'static, State> {
    harness_opts(detail, false)
}

fn harness_opts(detail: Option<CommitDetail>, can_amend: bool) -> Harness<'static, State> {
    // Small frame dt so two queued clicks land inside egui's 0.3s double-click
    // window (each event steps its own frame; the default 0.25s dt spreads them).
    Harness::builder().with_step_dt(0.05).build_ui_state(
        |ui, state| {
            let palette = Palette::dark();
            // Same contract as `HelmApp`: the emitted intent becomes the commit
            // diff opened on the next frame (highlighted row, active arrows). The
            // toggle target is applied to the shared mode, like `SetFileView`.
            let open = state.open_file.clone();
            let mut set_view = None;
            let mut amend = None;
            commit_detail_panel(
                ui,
                &palette,
                state.detail.as_ref(),
                open.as_ref(),
                &mut state.open_file,
                None,
                &mut FileMenuOutput::default(),
                state.view,
                &mut set_view,
                state.can_amend,
                &mut amend,
            );
            if let Some(view) = set_view {
                state.view = view;
            }
            if let Some(message) = amend {
                state.amended = Some(message);
            }
        },
        State {
            detail,
            open_file: None,
            view: FileViewMode::Flat,
            can_amend,
            amended: None,
        },
    )
}

#[test]
fn detail_in_flight_shows_a_spinner() {
    // The panel is only rendered with a commit selected: no detail yet means
    // the worker's reply is in flight — a spinner, never a placeholder.
    let mut harness = harness(None);
    // `run()` waits for stability — but the spinner requests a repaint every
    // frame: we advance one explicit step (egui_kittest recommendation).
    harness.step();

    // The panel header still renders while the detail loads.
    harness.get_by_label("Commit");
    harness.get_by_label("Loading commit");
}

#[test]
fn detail_renders_meta_and_file_rows() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    // Header + author block: initials avatar, name, "authored" line at the
    // author's wall-clock time, hash chip.
    harness.get_by_label("Commit");
    harness.get_by_label("0000007");
    harness.get_by_label("AD");
    harness.get_by_label("Ada");
    harness.get_by_label("authored");
    harness.get_by_label("01/01/2021 @ 14:30");
    // Two-level message: subject + body.
    harness.get_by_label("Add lib and tests");
    harness.get_by_label("Approved-by: Florian");
    // One row per changed file, labeled by its path.
    harness.get_by_label("src/lib.rs");
    harness.get_by_label("tests/new.rs");
    // Read-only: no staging controls leak into the commit detail.
    assert!(harness.query_by_label("Stage").is_none());
    assert!(harness.query_by_label("Unstage").is_none());
}

#[test]
fn files_changed_band_shows_count_totals_and_ratio_bar() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("Files changed");
    // Counter chip + totals summed over the files (62+64 / 5+7).
    harness.get_by_label("2");
    harness.get_by_label("+126");
    harness.get_by_label("−12");
    harness.get_by_label("diff ratio");
}

#[test]
fn file_rows_show_per_file_line_stats() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("+62");
    harness.get_by_label("−5");
    harness.get_by_label("+64");
    harness.get_by_label("−7");
}

#[test]
fn clicking_a_file_emits_the_commit_oid_and_path() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("tests/new.rs").click();
    harness.run();

    assert_eq!(
        harness.state().open_file,
        Some((oid(7), "tests/new.rs".to_string()))
    );
}

#[test]
fn arrow_down_after_opening_a_file_opens_the_next_file() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("src/lib.rs").click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(
        harness.state().open_file,
        Some((oid(7), "tests/new.rs".to_string()))
    );
}

#[test]
fn arrow_up_after_opening_a_file_opens_the_previous_file() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("tests/new.rs").click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    harness.run();

    assert_eq!(
        harness.state().open_file,
        Some((oid(7), "src/lib.rs".to_string()))
    );
}

#[test]
fn arrows_wrap_around_the_commit_files() {
    // Same traversal as the status sidebar (keybindings §3): wraps at start/end.
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("tests/new.rs").click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();
    assert_eq!(
        harness.state().open_file,
        Some((oid(7), "src/lib.rs".to_string()))
    );

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    harness.run();
    assert_eq!(
        harness.state().open_file,
        Some((oid(7), "tests/new.rs".to_string()))
    );
}

#[test]
fn arrows_do_nothing_until_a_file_diff_is_open() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(harness.state().open_file, None);
}

#[test]
fn arrows_ignore_a_diff_open_on_another_commit() {
    // The open diff carries its own oid (file click on A then selecting B): the
    // arrows only navigate within the list actually displayed.
    let mut harness = harness(Some(sample_detail()));
    harness.state_mut().open_file = Some((oid(9), "src/lib.rs".to_string()));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.run();

    assert_eq!(
        harness.state().open_file,
        Some((oid(9), "src/lib.rs".to_string()))
    );
}

#[test]
fn the_open_file_row_is_marked_selected_in_the_accessibility_tree() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();

    harness.get_by_label("src/lib.rs").click();
    harness.run();

    let node = harness.get_by_label("src/lib.rs");
    assert_eq!(
        format!("{:?}", node.accesskit_node().toggled()),
        "Some(True)",
        "the open file row reports toggled=True"
    );
    let other = harness.get_by_label("tests/new.rs");
    assert_eq!(
        format!("{:?}", other.accesskit_node().toggled()),
        "Some(False)",
        "other rows report toggled=False"
    );
}

fn copied_text(harness: &Harness<State>) -> Option<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
}

#[test]
fn file_row_context_menu_copies_relative_and_reveals_absolute() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let menu = Rc::new(RefCell::new(FileMenuOutput::default()));
    let menu_in_ui = menu.clone();
    let mut harness = Harness::builder().build_ui_state(
        move |ui, state: &mut State| {
            let palette = Palette::dark();
            let open = state.open_file.clone();
            commit_detail_panel(
                ui,
                &palette,
                state.detail.as_ref(),
                open.as_ref(),
                &mut state.open_file,
                Some(std::path::Path::new("/repo")),
                &mut menu_in_ui.borrow_mut(),
                state.view,
                &mut None,
                false,
                &mut None,
            );
        },
        State {
            detail: Some(sample_detail()),
            open_file: None,
            view: FileViewMode::Flat,
            can_amend: false,
            amended: None,
        },
    );
    harness.run();

    harness.get_by_label("src/lib.rs").click_secondary();
    harness.run();
    harness.get_by_label("Copy relative path").click();
    harness.step();
    assert_eq!(copied_text(&harness).as_deref(), Some("src/lib.rs"));

    harness.get_by_label("src/lib.rs").click_secondary();
    harness.run();
    harness.get_by_label("Reveal in Finder").click();
    harness.run();
    assert_eq!(
        menu.borrow().reveal.as_deref(),
        Some(std::path::Path::new("/repo/src/lib.rs"))
    );
}

#[test]
fn files_header_toggle_switches_between_flat_and_tree() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();
    assert_eq!(harness.state().view, FileViewMode::Flat);
    // Flat view has no directory rows: files render at their full path.
    assert!(harness.query_by_label("src").is_none());

    harness.get_by_label("Tree view").click();
    harness.run();
    assert_eq!(harness.state().view, FileViewMode::Tree);

    harness.get_by_label("Flat view").click();
    harness.run();
    assert_eq!(harness.state().view, FileViewMode::Flat);
}

#[test]
fn tree_view_groups_commit_files_under_directory_rows() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();
    harness.get_by_label("Tree view").click();
    harness.run();

    // `src/lib.rs` and `tests/new.rs` fold under one row per directory.
    harness.get_by_label("src");
    harness.get_by_label("tests");
    assert!(harness.query_by_label("src/lib.rs").is_some());
}

#[test]
fn collapsing_a_commit_directory_hides_its_files() {
    let mut harness = harness(Some(sample_detail()));
    harness.run();
    harness.get_by_label("Tree view").click();
    harness.run();
    assert!(harness.query_by_label("src/lib.rs").is_some());

    harness.get_by_label("src").click();
    harness.run();
    assert!(
        harness.query_by_label("src/lib.rs").is_none(),
        "collapsing the directory removes its file row"
    );
    assert!(
        harness.query_by_label("tests/new.rs").is_some(),
        "sibling directories stay expanded"
    );
}

/// Two clicks in a single frame register as a double click (egui counts them by
/// time-since-last-click, which is 0 within one input batch).
fn double_click(harness: &mut Harness<State>, label: &str) {
    harness.get_by_label(label).click();
    harness.get_by_label(label).click();
    harness.run();
}

#[test]
fn double_click_opens_the_message_editor_prefilled() {
    let mut harness = harness_opts(Some(sample_detail()), true);
    harness.run();
    // Read-only until the double click: no editor controls.
    assert!(harness.query_by_label("Amend").is_none());

    double_click(&mut harness, "Add lib and tests");

    harness.get_by_label("Amend");
    harness.get_by_label("Cancel");
    // Subject field prefilled from the commit.
    let subject = harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .value();
    assert_eq!(subject.as_deref(), Some("Add lib and tests"));
}

#[test]
fn amend_emits_the_edited_message() {
    let mut harness = harness_opts(Some(sample_detail()), true);
    harness.run();
    double_click(&mut harness, "Add lib and tests");

    harness.get_by_label("Amend").click();
    harness.run();

    assert_eq!(
        harness.state().amended.as_deref(),
        Some("Add lib and tests\n\nApproved-by: Florian"),
        "Amend composes subject + description into the reworded message"
    );
}

#[test]
fn cancel_closes_the_editor_without_emitting() {
    let mut harness = harness_opts(Some(sample_detail()), true);
    harness.run();
    double_click(&mut harness, "Add lib and tests");

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.query_by_label("Amend").is_none(), "editor closed");
    harness.get_by_label("Add lib and tests");
    assert!(harness.state().amended.is_none(), "Cancel emits nothing");
}

#[test]
fn a_non_head_commit_stays_read_only() {
    let mut harness = harness_opts(Some(sample_detail()), false);
    harness.run();

    double_click(&mut harness, "Add lib and tests");

    assert!(
        harness.query_by_label("Amend").is_none(),
        "only HEAD's message can be amended"
    );
    assert!(harness.state().amended.is_none());
}
