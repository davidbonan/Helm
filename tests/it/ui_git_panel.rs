use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::status::{ChangeKind, FileEntry, OpSummary, RepoStatus};
use helm::keybindings::Keymap;
use helm::theme::Palette;
use helm::ui::file_list::{FileMenuOutput, FileViewMode};
use helm::ui::git_panel::{abort_op_modal, git_panel, GitFileSelection, GitIntent, GitPanelState};
use helm::ui::repo_sidebar::DeleteModalAction;

fn file(path: &str, kind: ChangeKind) -> FileEntry {
    file_stats(path, kind, 0, 0)
}

fn file_stats(path: &str, kind: ChangeKind, additions: usize, deletions: usize) -> FileEntry {
    FileEntry {
        path: path.into(),
        kind,
        additions,
        deletions,
    }
}

fn sample_status() -> RepoStatus {
    RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    }
}

#[allow(deprecated)]
fn git_panel_harness(mut app: impl FnMut(&mut egui::Ui) + 'static) -> Harness<'static, ()> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 800.0))
        .build(move |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_clip_rect(ctx.content_rect());
                app(ui);
            });
        })
}

/// Drives `git_panel` with a shared frozen state and returns the emitted intents.
/// The closure receives the harness to chain click + `run()` (multi-frame flow).
fn drive(
    status: RepoStatus,
    subject: &str,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> Vec<GitIntent> {
    drive_with_state(
        status,
        GitPanelState {
            subject: subject.to_owned(),
            ..Default::default()
        },
        actions,
    )
}

fn drive_with_state(
    status: RepoStatus,
    initial_state: GitPanelState,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> Vec<GitIntent> {
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let state = Rc::new(RefCell::new(initial_state));
    let state_in_ui = state.clone();

    let mut harness = git_panel_harness(move |ui| {
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state_in_ui.borrow_mut(),
            &Keymap::default(),
            &mut sink.borrow_mut(),
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();
    actions(&mut harness);
    harness.run();

    let out = intents.borrow().clone();
    out
}

#[test]
fn renders_header_summary_and_section_headers() {
    let palette = Palette::light();
    let status = sample_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "feature/x",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    harness.get_by_label("Git");
    harness.get_by_label("feature/x");
    harness.get_by_label("1 file changed");
    harness.get_by_label_contains("Unstaged (1)");
    harness.get_by_label_contains("Staged (0)");
}

#[test]
fn summary_band_shows_totals_and_per_file_stats() {
    let palette = Palette::light();
    let status = RepoStatus {
        unstaged: vec![file_stats("src/lib.rs", ChangeKind::Modified, 2, 1)],
        staged: vec![file_stats("src/ui.rs", ChangeKind::Modified, 19, 3)],
    };
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    harness.get_by_label("2 files changed");
    harness.get_by_label("+21");
    harness.get_by_label("−4");
    harness.get_by_label("+2");
    harness.get_by_label("−1");
    harness.get_by_label("+19");
    harness.get_by_label("−3");
}

#[test]
fn row_stats_give_way_to_actions_on_hover() {
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file_stats("a.txt", ChangeKind::Added, 7, 0)],
    };
    let intents = drive(status, "", |h| {
        assert_eq!(
            h.get_all_by_label("+7").count(),
            2,
            "the +7 stat shows in the summary band and on the row"
        );
        h.get_by_label("a.txt").hover();
        h.run();
        assert_eq!(
            h.get_all_by_label("+7").count(),
            1,
            "the row stat hides while its actions show; the summary stays"
        );
        h.get_by_label("Unstage");
    });
    assert!(intents.is_empty(), "hovering a row emits no git intent");
}

#[test]
fn commit_inputs_show_labels_and_soft_limit_counters() {
    let palette = Palette::light();
    let status = sample_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    harness.get_by_label("Commit message");
    harness.get_by_label("Description (optional)");
    harness.get_by_label("0 / 72");
    harness.get_by_label("0 / 1000");
}

#[test]
fn clicking_stage_all_emits_intent() {
    let intents = drive(sample_status(), "", |h| h.get_by_label("Stage All").click());
    assert!(intents.contains(&GitIntent::StageAll));
}

#[test]
fn clicking_unstage_all_emits_intent() {
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file("a.txt", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| h.get_by_label("Unstage All").click());
    assert!(intents.contains(&GitIntent::UnstageAll));
}

#[test]
fn per_file_actions_are_hidden_until_row_hover() {
    let intents = drive(sample_status(), "", |h| {
        assert!(
            h.query_by_label("Stage").is_none(),
            "file action is hidden before hovering the row"
        );
        assert!(
            h.query_by_label("Discard").is_none(),
            "discard action is hidden before hovering the row"
        );
        h.get_by_label("src/main.rs").hover();
        h.run();
        h.get_by_label("Stage");
        h.get_by_label("Discard");
    });
    assert!(intents.is_empty(), "hovering actions emits no git intent");
}

#[test]
fn clicking_per_file_stage_emits_path_intent() {
    let intents = drive(sample_status(), "", |h| {
        h.get_by_label("src/main.rs").hover();
        h.run();
        h.get_by_label("Stage").click();
    });
    assert!(intents.contains(&GitIntent::Stage("src/main.rs".into())));
}

#[test]
fn clicking_an_unstaged_file_path_opens_its_diff() {
    let intents = drive(sample_status(), "", |h| {
        h.get_by_label("src/main.rs").click()
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "src/main.rs".into(),
            staged: false,
        }),
        "clicking an unstaged file path opens its diff from the Unstaged section, got {intents:?}"
    );
}

#[test]
fn arrow_down_after_clicking_an_unstaged_file_opens_the_next_file() {
    let status = RepoStatus {
        unstaged: vec![
            file("a.rs", ChangeKind::Modified),
            file("b.rs", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("a.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "b.rs".into(),
            staged: false,
        }),
        "ArrowDown after selecting an unstaged file opens the next unstaged file, got {intents:?}"
    );
}

#[test]
fn arrow_up_after_clicking_a_staged_file_opens_the_previous_file() {
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![
            file("a.txt", ChangeKind::Added),
            file("b.txt", ChangeKind::Added),
        ],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("b.txt").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "a.txt".into(),
            staged: true,
        }),
        "ArrowUp after selecting a staged file opens the previous staged file, got {intents:?}"
    );
}

#[test]
fn arrow_down_crosses_from_unstaged_to_staged_files() {
    let status = RepoStatus {
        unstaged: vec![file("unstaged.rs", ChangeKind::Modified)],
        staged: vec![file("staged.rs", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("unstaged.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "staged.rs".into(),
            staged: true,
        }),
        "ArrowDown follows the unstaged → staged order, got {intents:?}"
    );
}

#[test]
fn arrow_up_crosses_from_staged_back_to_unstaged_files() {
    let status = RepoStatus {
        unstaged: vec![
            file("a-unstaged.rs", ChangeKind::Modified),
            file("b-unstaged.rs", ChangeKind::Modified),
        ],
        staged: vec![file("staged.rs", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("staged.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "b-unstaged.rs".into(),
            staged: false,
        }),
        "ArrowUp follows the reverse staged → unstaged order, got {intents:?}"
    );
}

#[test]
fn arrow_down_wraps_from_the_last_file_to_the_first_file() {
    let status = RepoStatus {
        unstaged: vec![file("first-unstaged.rs", ChangeKind::Modified)],
        staged: vec![file("last-staged.rs", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("last-staged.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "first-unstaged.rs".into(),
            staged: false,
        }),
        "ArrowDown wraps from the last file to the first file, got {intents:?}"
    );
}

#[test]
fn repeated_arrow_down_cycles_through_unstaged_then_staged_then_wraps() {
    let status = RepoStatus {
        unstaged: vec![file("unstaged.rs", ChangeKind::Modified)],
        staged: vec![file("staged.rs", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("unstaged.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "staged.rs".into(),
            staged: true,
        }),
        "first ArrowDown opens the staged file, got {intents:?}"
    );
    assert!(
        intents
            .iter()
            .filter(|intent| matches!(intent, GitIntent::OpenDiff { path, staged: false } if path == "unstaged.rs"))
            .count()
            >= 2,
        "second ArrowDown wraps back to the first unstaged file, got {intents:?}"
    );
}

#[test]
fn arrow_up_wraps_from_the_first_file_to_the_last_file() {
    let status = RepoStatus {
        unstaged: vec![file("first-unstaged.rs", ChangeKind::Modified)],
        staged: vec![file("last-staged.rs", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("first-unstaged.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "last-staged.rs".into(),
            staged: true,
        }),
        "ArrowUp wraps from the first file to the last file, got {intents:?}"
    );
}

#[test]
fn arrow_keys_use_the_active_file_selection_without_row_focus() {
    let status = RepoStatus {
        unstaged: vec![file("unstaged.rs", ChangeKind::Modified)],
        staged: vec![file("staged.rs", ChangeKind::Added)],
    };
    let intents = drive_with_state(
        status,
        GitPanelState {
            selected_file: Some(GitFileSelection {
                path: "staged.rs".into(),
                staged: true,
            }),
            file_nav_active: true,
            ..Default::default()
        },
        |h| h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown),
    );
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "unstaged.rs".into(),
            staged: false,
        }),
        "Arrow navigation must not require focus on rows or section titles, got {intents:?}"
    );
}

#[test]
fn arrow_keys_do_not_open_files_before_a_sidebar_file_is_selected() {
    let intents = drive(sample_status(), "", |h| {
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    });
    assert!(
        !intents
            .iter()
            .any(|intent| matches!(intent, GitIntent::OpenDiff { .. })),
        "plain arrows are ignored until a git file row has focus, got {intents:?}"
    );
}

#[test]
fn clicking_a_staged_file_path_opens_its_diff_from_the_staged_section() {
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file("a.txt", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| h.get_by_label("a.txt").click());
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "a.txt".into(),
            staged: true,
        }),
        "clicking a staged file path opens its diff from the Staged section, got {intents:?}"
    );
}

#[test]
fn clicking_per_file_unstage_emits_path_intent() {
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file("a.txt", ChangeKind::Added)],
    };
    let intents = drive(status, "", |h| {
        assert!(
            h.query_by_label("Unstage").is_none(),
            "file action is hidden before hovering the row"
        );
        h.get_by_label("a.txt").hover();
        h.run();
        h.get_by_label("Unstage").click();
    });
    assert!(intents.contains(&GitIntent::Unstage("a.txt".into())));
}

#[test]
fn commit_button_is_disabled_without_message_or_staged() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let intents = drive(status, "a message", |h| h.get_by_label("Commit").click());
    assert!(
        !intents.iter().any(|i| matches!(i, GitIntent::Commit(_))),
        "a message but nothing staged keeps commit disabled"
    );
}

#[test]
fn commit_button_emits_message_when_staged_and_non_empty() {
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file("a.txt", ChangeKind::Added)],
    };
    let intents = drive(status, "land it", |h| {
        h.get_by_label("Commit 1 file").click_accesskit()
    });
    assert!(intents.contains(&GitIntent::Commit("land it".into())));
}

#[test]
fn commit_button_ignores_clicks_while_a_commit_is_in_flight() {
    // No `drive_with_state`: the spinner repaints continuously, so `run()` would
    // exceed its frame budget — drive it via `step()` (cf. the AI button test).
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file("a.txt", ChangeKind::Added)],
    };
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState {
            subject: "land it".to_owned(),
            commit_busy: true,
            ..Default::default()
        };
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink.borrow_mut(),
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.step();
    harness.get_by_label("Commit 1 file").click_accesskit();
    harness.step();
    harness.step();

    let intents = intents.borrow().clone();
    assert!(
        !intents.iter().any(|i| matches!(i, GitIntent::Commit(_))),
        "a second click while the commit is being written must not enqueue another commit"
    );
}

#[test]
fn discard_all_requires_confirmation_then_emits_intent() {
    let intents = drive(sample_status(), "", |h| {
        h.get_by_label("Discard all").click();
        h.run();
        h.get_by_label("Discard").click();
    });
    assert!(
        intents.contains(&GitIntent::DiscardAll),
        "confirming the modal emits DiscardAll"
    );
}

#[test]
fn pressing_enter_confirms_the_discard_modal() {
    let intents = drive(sample_status(), "", |h| {
        h.get_by_label("Discard all").click();
        h.run();
        h.key_press(egui::Key::Enter);
    });
    assert!(
        intents.contains(&GitIntent::DiscardAll),
        "Enter confirms the discard modal like the red button"
    );
}

#[test]
fn a_disarmed_panel_drops_the_armed_discard_modal() {
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let state = Rc::new(RefCell::new(GitPanelState::default()));
    let state_in_ui = state.clone();
    let status = sample_status();

    let mut harness = git_panel_harness(move |ui| {
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state_in_ui.borrow_mut(),
            &Keymap::default(),
            &mut sink.borrow_mut(),
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();
    harness.get_by_label("Discard all").click();
    harness.run();
    assert!(harness.query_by_label("Discard changes?").is_some());

    // The repo switch disarms the panel before the next frame renders.
    state.borrow_mut().disarm_on_repo_switch();
    harness.run();
    assert!(
        harness.query_by_label("Discard changes?").is_none(),
        "the confirmation must not re-render over the new repo"
    );

    harness.key_press(egui::Key::Enter);
    harness.run();
    let intents = intents.borrow().clone();
    assert!(
        !intents.iter().any(|i| matches!(i, GitIntent::DiscardAll)),
        "a confirmation armed before the switch must not discard the new repo"
    );
}

#[test]
fn cancelling_discard_emits_no_intent() {
    let intents = drive(sample_status(), "", |h| {
        h.get_by_label("Discard all").click();
        h.run();
        h.get_by_label("Cancel").click();
    });
    assert!(
        !intents.iter().any(|i| matches!(i, GitIntent::DiscardAll)),
        "cancelling the modal discards nothing"
    );
}

#[test]
fn a_partially_staged_file_appears_in_both_sections() {
    let palette = Palette::light();
    let status = RepoStatus {
        unstaged: vec![file("partial.rs", ChangeKind::Modified)],
        staged: vec![file("partial.rs", ChangeKind::Modified)],
    };
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    let rows = harness.get_all_by_label("partial.rs").count();
    assert_eq!(
        rows, 2,
        "a partially staged file shows once in Unstaged and once in Staged"
    );
}

fn merge_op() -> OpSummary {
    OpSummary {
        verb: "Merging",
        source: Some("theirs".to_owned()),
        target: Some("main".to_owned()),
    }
}

fn conflict_status() -> RepoStatus {
    RepoStatus {
        unstaged: vec![file("src/conflict.rs", ChangeKind::Conflicted)],
        staged: vec![],
    }
}

#[test]
fn an_in_progress_merge_shows_the_conflict_header_and_subline() {
    let palette = Palette::light();
    let status = conflict_status();
    let op = merge_op();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            true,
            Some(&op),
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    harness.get_by_label("Merge conflicts detected");
    harness.get_by_label_contains("theirs");
    harness.get_by_label_contains("main");
    harness.get_by_label_contains("Conflicted Files (1)");
    harness.get_by_label_contains("Resolved Files (0)");
}

#[test]
fn the_abort_button_emits_the_abort_intent() {
    let intents = drive_op(conflict_status(), |h| {
        h.get_by_label("Abort Merge").click();
    });
    // The intent only opens the confirmation modal — the caller runs the abort.
    assert!(intents.contains(&GitIntent::AbortOp));
}

/// Drives `git_panel` with a merge in progress (the [`merge_op`] summary) and
/// returns the intents.
fn drive_op(
    status: RepoStatus,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> Vec<GitIntent> {
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let state = Rc::new(RefCell::new(GitPanelState::default()));
    let state_in_ui = state.clone();
    let op = merge_op();

    let mut harness = git_panel_harness(move |ui| {
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            true,
            Some(&op),
            &mut state_in_ui.borrow_mut(),
            &Keymap::default(),
            &mut sink.borrow_mut(),
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();
    actions(&mut harness);
    harness.run();

    let out = intents.borrow().clone();
    out
}

#[test]
fn continue_is_disabled_while_a_conflict_remains() {
    let intents = drive_op(conflict_status(), |h| {
        h.get_by_label("Continue Merge").click();
    });
    assert!(
        !intents.contains(&GitIntent::ContinueOp),
        "Continue is gated until every conflict is resolved"
    );
}

#[test]
fn continue_runs_once_no_conflict_remains() {
    // Op still in progress but no conflict stage left (every resolution applied).
    let status = RepoStatus {
        unstaged: vec![],
        staged: vec![file("src/conflict.rs", ChangeKind::Modified)],
    };
    let intents = drive_op(status, |h| {
        h.get_by_label("Continue Merge").click();
    });
    assert!(intents.contains(&GitIntent::ContinueOp));
}

#[test]
fn mark_all_resolved_stages_every_conflicted_file() {
    let status = RepoStatus {
        unstaged: vec![
            file("src/a.rs", ChangeKind::Conflicted),
            file("src/b.rs", ChangeKind::Conflicted),
        ],
        staged: vec![],
    };
    let intents = drive_op(status, |h| {
        h.get_by_label("Mark All Resolved").click();
    });
    assert!(intents.contains(&GitIntent::Stage("src/a.rs".to_owned())));
    assert!(intents.contains(&GitIntent::Stage("src/b.rs".to_owned())));
}

#[test]
fn the_resolved_group_lists_the_staged_files() {
    let status = RepoStatus {
        unstaged: vec![file("src/conflict.rs", ChangeKind::Conflicted)],
        staged: vec![file("src/done.rs", ChangeKind::Modified)],
    };
    let op = merge_op();
    let palette = Palette::light();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            true,
            Some(&op),
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    harness.get_by_label_contains("Resolved Files (1)");
    harness.get_by_label("src/done.rs");
}

#[test]
fn clicking_a_conflicted_file_opens_the_editor_on_that_file() {
    let intents = drive_op(conflict_status(), |h| {
        h.get_by_label("src/conflict.rs").click();
    });
    assert!(intents.contains(&GitIntent::OpenConflictEditor {
        focus: Some("src/conflict.rs".to_owned()),
    }));
}

fn abort_modal_harness() -> Harness<'static, DeleteModalAction> {
    Harness::new_ui_state(
        |ui, state| {
            let palette = Palette::light();
            abort_op_modal(ui, &palette, state);
        },
        DeleteModalAction::default(),
    )
}

#[test]
fn the_abort_modal_confirms_with_the_red_button() {
    let mut harness = abort_modal_harness();
    harness.run();

    harness.get_by_label("Abort the merge/rebase in progress?");
    harness.get_by_label("Abort").click();
    harness.run();

    assert!(harness.state().confirm);
}

#[test]
fn the_abort_modal_confirms_with_enter() {
    let mut harness = abort_modal_harness();
    harness.run();

    harness.key_press(egui::Key::Enter);
    harness.run();

    assert!(harness.state().confirm);
}

#[test]
fn the_abort_modal_cancel_dismisses_without_confirming() {
    let mut harness = abort_modal_harness();
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().dismiss);
    assert!(!harness.state().confirm);
}

#[test]
fn a_clean_repository_state_shows_no_conflict_panel() {
    let palette = Palette::light();
    let status = sample_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    // The normal status layout is shown, not the conflict panel.
    harness.get_by_label("Git");
    assert!(
        harness
            .query_by_label_contains("conflicts detected")
            .is_none(),
        "no conflict panel when Repository::state() is Clean"
    );
}

// ---- AI button on the commit card ----

fn staged_only_status() -> RepoStatus {
    RepoStatus {
        unstaged: vec![],
        staged: vec![file("src/main.rs", ChangeKind::Modified)],
    }
}

#[test]
fn the_ai_button_emits_the_generate_intent_when_changes_are_staged() {
    let intents = drive(staged_only_status(), "", |harness| {
        harness.get_by_label("Generate commit message").click();
    });
    assert_eq!(intents, vec![GitIntent::GenerateMessage]);
}

#[test]
fn the_ai_button_is_inert_without_staged_changes() {
    // sample_status only has unstaged changes: since the prompt only analyzes
    // staged ones, the button stays disabled.
    let intents = drive(sample_status(), "", |harness| {
        harness.get_by_label("Generate commit message").click();
    });
    assert!(
        intents.is_empty(),
        "nothing staged to describe — the button must not emit: {intents:?}"
    );
}

#[test]
fn the_ai_button_ignores_clicks_while_a_generation_is_in_flight() {
    // No `drive_with_state`: the spinner repaints continuously, so `run()` would
    // exceed its frame budget — drive it via `step()`.
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let status = staged_only_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState {
            ai_busy: true,
            ..Default::default()
        };
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink.borrow_mut(),
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.step();
    harness.get_by_label("Generate commit message").click();
    harness.step();
    harness.step();

    let intents = intents.borrow().clone();
    assert!(
        intents.is_empty(),
        "busy: the click must be ignored, got {intents:?}"
    );
}

#[test]
fn pending_status_shows_loader_not_clean_state() {
    // First snapshot not yet received (spawn, repo switch) ⇒ spinner: the
    // default status would otherwise read as a clean tree.
    let palette = Palette::light();
    let status = RepoStatus::default();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState {
            status_loading: true,
            ..Default::default()
        };
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    // `run()` waits for stability — but the spinner requests a repaint every
    // frame: we advance one explicit step (egui_kittest recommendation).
    harness.step();

    harness.get_by_label("Loading status");
    assert!(harness.query_by_label("Nothing to commit").is_none());
    assert!(harness.query_by_label("0 files changed").is_none());
}

#[test]
fn mutation_replaces_refresh_with_a_spinner() {
    // Mutation awaiting its worker reply (slow stage-all / discard / checkout)
    // ⇒ the Refresh slot spins instead of staying inert.
    let palette = Palette::light();
    let status = sample_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState {
            mutation_busy: true,
            ..Default::default()
        };
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.step();

    harness.get_by_label("Working");
    assert!(harness.query_by_label("Refresh").is_none());
}

#[test]
fn hovering_an_enabled_pill_shows_the_pointer_cursor() {
    let palette = Palette::light();
    let status = sample_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    let pos = harness.get_by_label("Stage All").rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
}

#[test]
fn hovering_the_disabled_commit_button_keeps_the_default_cursor() {
    let palette = Palette::light();
    // 1 unstaged file, nothing staged + empty subject ⇒ "Commit" disabled.
    let status = sample_status();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();

    let pos = harness.get_by_label("Commit").rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();

    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::Default
    );
}

fn copied_text(harness: &Harness) -> Option<String> {
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

/// Drives `git_panel` over a single unstaged file, right-clicks its row, then
/// runs `actions` on the open menu; returns the menu output it produced.
fn drive_file_menu(
    repo_root: Option<std::path::PathBuf>,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> FileMenuOutput {
    let palette = Palette::light();
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let menu = Rc::new(RefCell::new(FileMenuOutput::default()));
    let menu_in_ui = menu.clone();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            repo_root.as_deref(),
            &mut menu_in_ui.borrow_mut(),
            FileViewMode::Flat,
        );
    });
    harness.run();
    harness.get_by_label("src/main.rs").click_secondary();
    harness.run();
    actions(&mut harness);
    harness.run();
    menu.take()
}

#[test]
fn file_row_menu_copies_relative_path() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let palette = Palette::light();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            Some(std::path::Path::new("/repo")),
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();
    harness.get_by_label("src/main.rs").click_secondary();
    harness.run();
    // No duplication with the row's hover actions.
    assert!(harness.query_by_label("Unstage").is_none());
    harness.get_by_label("Copy relative path").click();
    // A single step keeps the frame's copy command (run() settles past it).
    harness.step();

    assert_eq!(copied_text(&harness).as_deref(), Some("src/main.rs"));
}

#[test]
fn file_row_menu_copies_absolute_path() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let palette = Palette::light();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            Some(std::path::Path::new("/repo")),
            &mut FileMenuOutput::default(),
            FileViewMode::Flat,
        );
    });
    harness.run();
    harness.get_by_label("src/main.rs").click_secondary();
    harness.run();
    harness.get_by_label("Copy path").click();
    harness.step();

    assert_eq!(copied_text(&harness).as_deref(), Some("/repo/src/main.rs"));
}

#[test]
fn file_row_menu_emits_reveal_with_absolute_path() {
    let out = drive_file_menu(Some("/repo".into()), |harness| {
        harness.get_by_label("Reveal in Finder").click();
    });
    assert_eq!(
        out.reveal.as_deref(),
        Some(std::path::Path::new("/repo/src/main.rs"))
    );
    assert!(out.open_in_editor.is_none());
}

#[test]
fn file_row_menu_emits_open_in_editor_with_absolute_path() {
    let out = drive_file_menu(Some("/repo".into()), |harness| {
        harness.get_by_label("Open in editor").click();
    });
    assert_eq!(
        out.open_in_editor.as_deref(),
        Some(std::path::Path::new("/repo/src/main.rs"))
    );
    assert!(out.reveal.is_none());
}

#[test]
fn file_row_menu_hides_reveal_and_open_without_a_repo_root() {
    let _ = drive_file_menu(None, |harness| {
        assert!(harness.query_by_label("Reveal in Finder").is_none());
        assert!(harness.query_by_label("Open in editor").is_none());
        // Relative copy still works without a root.
        harness.get_by_label("Copy relative path").click();
    });
}

#[test]
fn file_menu_stash_confirms_then_emits_stash_files() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("src/main.rs").click_secondary();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        // The modal confirm, not the menu entry (the menu has closed).
        h.get_by_label("Stash").click();
    });
    assert!(intents.contains(&GitIntent::StashFiles(vec!["src/main.rs".to_owned()])));
}

#[test]
fn file_menu_stash_targets_only_the_right_clicked_file() {
    let status = RepoStatus {
        unstaged: vec![
            file("a.txt", ChangeKind::Modified),
            file("b.txt", ChangeKind::Modified),
            file("c.txt", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("b.txt").click_secondary();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Stash").click();
    });
    assert!(
        intents.contains(&GitIntent::StashFiles(vec!["b.txt".to_owned()])),
        "right-click stash with no prior selection targets only that file, got {intents:?}"
    );
}

#[test]
fn file_menu_stash_after_clicking_another_file_targets_only_the_right_clicked() {
    let status = RepoStatus {
        unstaged: vec![
            file("a.txt", ChangeKind::Modified),
            file("b.txt", ChangeKind::Modified),
            file("c.txt", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let intents = drive(status, "", |h| {
        // Open a.txt's diff (plain click), then stash b.txt via its menu.
        h.get_by_label("a.txt").click();
        h.run();
        h.get_by_label("b.txt").click_secondary();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Stash").click();
    });
    assert!(
        intents.contains(&GitIntent::StashFiles(vec!["b.txt".to_owned()])),
        "got {intents:?}"
    );
}

#[test]
fn file_menu_stash_on_a_staged_file_targets_only_that_file() {
    let status = RepoStatus {
        unstaged: vec![file("a.txt", ChangeKind::Modified)],
        staged: vec![
            file("b.txt", ChangeKind::Modified),
            file("c.txt", ChangeKind::Modified),
        ],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("b.txt").click_secondary();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Stash").click();
    });
    assert!(
        intents.contains(&GitIntent::StashFiles(vec!["b.txt".to_owned()])),
        "got {intents:?}"
    );
}

#[test]
fn file_menu_stash_cancel_emits_nothing() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let intents = drive(status, "", |h| {
        h.get_by_label("src/main.rs").click_secondary();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Cancel").click();
    });
    assert!(intents.is_empty());
}

#[test]
fn multi_select_menu_stashes_the_whole_selection_in_one_intent() {
    let status = RepoStatus {
        unstaged: vec![
            file("src/main.rs", ChangeKind::Modified),
            file("src/lib.rs", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let state = GitPanelState {
        marked_files: vec![
            GitFileSelection {
                path: "src/main.rs".into(),
                staged: false,
            },
            GitFileSelection {
                path: "src/lib.rs".into(),
                staged: false,
            },
        ],
        ..Default::default()
    };
    let intents = drive_with_state(status, state, |h| {
        h.get_by_label("src/main.rs").click_secondary();
        h.run();
        h.get_by_label("Stash").click();
        h.run();
        h.get_by_label("Stash").click();
    });
    assert!(intents.contains(&GitIntent::StashFiles(vec![
        "src/lib.rs".to_owned(),
        "src/main.rs".to_owned(),
    ])));
}

#[test]
fn multi_select_menu_stages_every_marked_file() {
    let status = RepoStatus {
        unstaged: vec![
            file("src/main.rs", ChangeKind::Modified),
            file("src/lib.rs", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let state = GitPanelState {
        marked_files: vec![
            GitFileSelection {
                path: "src/main.rs".into(),
                staged: false,
            },
            GitFileSelection {
                path: "src/lib.rs".into(),
                staged: false,
            },
        ],
        ..Default::default()
    };
    let intents = drive_with_state(status, state, |h| {
        h.get_by_label("src/main.rs").click_secondary();
        h.run();
        // Move off the rows so their hover "Stage" pill stops shadowing the menu
        // entry of the same label.
        h.event(egui::Event::PointerMoved(egui::pos2(2.0, 2.0)));
        h.run();
        h.get_by_label("Stage").click();
    });
    assert!(intents.contains(&GitIntent::Stage("src/main.rs".to_owned())));
    assert!(intents.contains(&GitIntent::Stage("src/lib.rs".to_owned())));
}

#[test]
fn cmd_click_selects_without_opening_a_diff() {
    let status = RepoStatus {
        unstaged: vec![
            file("src/main.rs", ChangeKind::Modified),
            file("src/lib.rs", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let intents = drive_with_state(status, GitPanelState::default(), |h| {
        h.get_by_label("src/lib.rs")
            .click_modifiers(egui::Modifiers::COMMAND);
    });
    assert!(
        !intents
            .iter()
            .any(|intent| matches!(intent, GitIntent::OpenDiff { .. })),
        "Cmd+click toggles the selection, it never opens the diff"
    );
}

/// Like [`drive`] but renders the file lists in `view` and keeps the panel state
/// across frames, so a directory collapse or a tree-order arrow nav settles.
fn drive_view(
    status: RepoStatus,
    view: FileViewMode,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> Vec<GitIntent> {
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let state = Rc::new(RefCell::new(GitPanelState::default()));
    let state_in_ui = state.clone();

    let mut harness = git_panel_harness(move |ui| {
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state_in_ui.borrow_mut(),
            &Keymap::default(),
            &mut sink.borrow_mut(),
            None,
            &mut FileMenuOutput::default(),
            view,
        );
    });
    harness.run();
    actions(&mut harness);
    harness.run();

    let out = intents.borrow().clone();
    out
}

#[test]
fn header_view_toggle_requests_tree_then_flat() {
    let to_tree = drive_view(sample_status(), FileViewMode::Flat, |h| {
        h.get_by_label("Tree view").click();
    });
    assert!(
        to_tree.contains(&GitIntent::SetFileView(FileViewMode::Tree)),
        "the header toggle in Flat mode requests Tree, got {to_tree:?}"
    );
    let to_flat = drive_view(sample_status(), FileViewMode::Tree, |h| {
        h.get_by_label("Flat view").click();
    });
    assert!(
        to_flat.contains(&GitIntent::SetFileView(FileViewMode::Flat)),
        "the header toggle in Tree mode requests Flat, got {to_flat:?}"
    );
}

#[test]
fn tree_view_groups_files_under_a_directory_row() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let palette = Palette::light();
    let mut harness = git_panel_harness(move |ui| {
        let mut state = GitPanelState::default();
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state,
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Tree,
        );
    });
    harness.run();
    // The directory grouping row exists only in the tree layout; the leaf keeps
    // the full path for selection / accessibility.
    harness.get_by_label("src");
    harness.get_by_label("src/main.rs");
}

#[test]
fn collapsing_a_tree_directory_hides_its_files() {
    let status = RepoStatus {
        unstaged: vec![file("src/main.rs", ChangeKind::Modified)],
        staged: vec![],
    };
    let palette = Palette::light();
    let state = Rc::new(RefCell::new(GitPanelState::default()));
    let state_in_ui = state.clone();
    let mut harness = git_panel_harness(move |ui| {
        let mut sink = Vec::new();
        git_panel(
            ui,
            &palette,
            "main",
            &status,
            false,
            None,
            &mut state_in_ui.borrow_mut(),
            &Keymap::default(),
            &mut sink,
            None,
            &mut FileMenuOutput::default(),
            FileViewMode::Tree,
        );
    });
    harness.run();
    harness.get_by_label("src/main.rs");
    harness.get_by_label("src").click();
    harness.run();
    assert!(
        harness.query_by_label("src/main.rs").is_none(),
        "collapsing the directory row hides its files"
    );
    // The directory row itself stays, ready to expand again.
    harness.get_by_label("src");
}

#[test]
fn arrow_navigation_follows_the_tree_order() {
    // Tree order floats the `src` directory above the root files, so the row
    // after `a.rs` is `b.rs` — not `src/x.rs`, which the flat order would pick.
    let status = RepoStatus {
        unstaged: vec![
            file("a.rs", ChangeKind::Modified),
            file("src/x.rs", ChangeKind::Modified),
            file("b.rs", ChangeKind::Modified),
        ],
        staged: vec![],
    };
    let intents = drive_view(status, FileViewMode::Tree, |h| {
        h.get_by_label("a.rs").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    });
    assert!(
        intents.contains(&GitIntent::OpenDiff {
            path: "b.rs".into(),
            staged: false,
        }),
        "tree-order nav after a.rs opens b.rs, got {intents:?}"
    );
    assert!(
        !intents.contains(&GitIntent::OpenDiff {
            path: "src/x.rs".into(),
            staged: false,
        }),
        "tree-order nav skips src/x.rs (the flat successor), got {intents:?}"
    );
}
