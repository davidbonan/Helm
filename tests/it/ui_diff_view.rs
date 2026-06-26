use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::diff::{DiffLine, FileDiff, Hunk, ImageBlob, LineOrigin};
use helm::review::{FileComments, ForgeThreads, LineComment, ReviewIntent, ReviewPool};
use helm::theme::Palette;
use helm::ui::diff_view::{content_x_offset, diff_view, DiffReview, DiffSurface, DiffViewState};
use helm::ui::git_panel::GitIntent;

fn line(origin: LineOrigin, content: &str) -> DiffLine {
    DiffLine {
        origin,
        content: content.to_owned(),
        old_lineno: None,
        new_lineno: None,
    }
}

fn sample_diff() -> FileDiff {
    FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1,2 +1,3 @@".into(),
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 3,
            lines: vec![
                line(LineOrigin::Context, "fn main() {\n"),
                line(LineOrigin::Deletion, "    old();\n"),
                line(LineOrigin::Addition, "    new();\n"),
            ],
        }],
        source_lines: Vec::new(),
        image: None,
    }
}

fn two_hunk_diff() -> FileDiff {
    let mut diff = sample_diff();
    diff.hunks.push(Hunk {
        header: "@@ -20,2 +20,3 @@".into(),
        old_start: 20,
        old_lines: 2,
        new_start: 20,
        new_lines: 3,
        lines: vec![
            line(LineOrigin::Context, "tail() {\n"),
            line(LineOrigin::Deletion, "    old_tail();\n"),
            line(LineOrigin::Addition, "    new_tail();\n"),
        ],
    });
    diff
}

fn adjacent_changed_lines_diff() -> FileDiff {
    FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1,4 +1,4 @@".into(),
            old_start: 1,
            old_lines: 4,
            new_start: 1,
            new_lines: 4,
            lines: vec![
                line(LineOrigin::Deletion, "    delete_a();\n"),
                line(LineOrigin::Deletion, "    delete_b();\n"),
                line(LineOrigin::Addition, "    add_a();\n"),
                line(LineOrigin::Addition, "    add_b();\n"),
            ],
        }],
        source_lines: Vec::new(),
        image: None,
    }
}

/// Single context-line hunk in the middle of a 9-line file: room to extend on
/// both sides (5 above, 2 below before the end of the file).
fn extendable_diff() -> FileDiff {
    FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -7,1 +7,1 @@".into(),
            old_start: 7,
            old_lines: 1,
            new_start: 7,
            new_lines: 1,
            lines: vec![DiffLine {
                origin: LineOrigin::Context,
                content: "mid()\n".to_owned(),
                old_lineno: Some(7),
                new_lineno: Some(7),
            }],
        }],
        source_lines: (1..=9).map(|n| format!("src-line-{n}")).collect(),
        image: None,
    }
}

/// Drives `diff_view` with shared state, returns (emitted intents, close request).
fn drive(
    diff: FileDiff,
    staged: bool,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> (Vec<GitIntent>, bool) {
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let closed = Rc::new(RefCell::new(false));
    let closed_sink = closed.clone();
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();

    let mut harness = Harness::new_ui(move |ui| {
        let did_close = diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged },
            &mut state_in_ui.borrow_mut(),
            &mut sink.borrow_mut(),
            None,
        );
        *closed_sink.borrow_mut() |= did_close;
    });
    harness.run();
    actions(&mut harness);
    harness.run();

    let out = intents.borrow().clone();
    let c = *closed.borrow();
    (out, c)
}

#[test]
fn renders_the_path_and_diff_lines() {
    let palette = Palette::light();
    let diff = sample_diff();
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    harness.get_by_label_contains("src/main.rs");
    harness.get_by_label_contains("fn main()");
    harness.get_by_label_contains("old()");
    harness.get_by_label_contains("new()");
}

#[test]
fn a_line_longer_than_the_preview_extends_the_row_past_the_viewport() {
    // The row must be allocated at the line's full width — not clamped to the
    // viewport — so egui exposes a horizontal scrollbar instead of silently
    // clipping the overflow with no way to reach it.
    let palette = Palette::light();
    let long = format!("scroll_marker_{}", "x".repeat(200));
    let diff = FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1 +1 @@".into(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![line(LineOrigin::Addition, &format!("{long}\n"))],
        }],
        source_lines: Vec::new(),
        image: None,
    };
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    let char_w = harness.ctx.fonts_mut(|fonts| {
        fonts
            .glyph_width(&egui::FontId::monospace(12.0), ' ')
            .max(1.0)
    });
    // Same gutter (3 digits) as `copy_diff`, so the content offset matches.
    let content_left = content_x_offset(&copy_diff(""), char_w);
    let row = harness.get_by_label_contains("scroll_marker_").rect();
    assert!(
        row.width() > content_left + 200.0 * char_w,
        "a 200-char line must allocate a row wide enough to hold it (> {} px), got {} px",
        content_left + 200.0 * char_w,
        row.width(),
    );
}

#[test]
fn adjacent_changed_lines_are_contiguous_so_backgrounds_connect() {
    let palette = Palette::light();
    let diff = adjacent_changed_lines_diff();
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    let delete_a = harness.get_by_label_contains("delete_a()").rect();
    let delete_b = harness.get_by_label_contains("delete_b()").rect();
    let add_a = harness.get_by_label_contains("add_a()").rect();
    let add_b = harness.get_by_label_contains("add_b()").rect();
    assert_eq!(delete_a.bottom(), delete_b.top());
    assert_eq!(add_a.bottom(), add_b.top());
}

#[test]
fn unstaged_diff_shows_stage_hunk_button_that_emits_intent() {
    let (intents, _) = drive(sample_diff(), false, |h| {
        h.get_by_label("Stage hunk").click()
    });
    assert!(
        intents.contains(&GitIntent::StageHunk(0)),
        "the Stage hunk button emits StageHunk for hunk 0, got {intents:?}"
    );
}

#[test]
fn staged_diff_shows_unstage_hunk_button_that_emits_intent() {
    let (intents, _) = drive(sample_diff(), true, |h| {
        h.get_by_label("Unstage hunk").click()
    });
    assert!(
        intents.contains(&GitIntent::UnstageHunk(0)),
        "the Unstage hunk button emits UnstageHunk for hunk 0, got {intents:?}"
    );
}

#[test]
fn unstaged_diff_shows_discard_hunk_button_that_emits_intent() {
    let (intents, _) = drive(sample_diff(), false, |h| {
        h.get_by_label("Discard hunk").click()
    });
    assert!(
        intents.contains(&GitIntent::DiscardHunk(0)),
        "the Discard hunk button emits DiscardHunk for hunk 0, got {intents:?}"
    );
}

#[test]
fn staged_diff_has_no_discard_hunk_button() {
    let (_, _) = drive(sample_diff(), true, |h| {
        assert!(
            h.query_by_label("Discard hunk").is_none(),
            "Discard reverts the working tree, so it is never offered on the staged side"
        );
    });
}

#[test]
fn clicking_last_stage_hunk_button_emits_last_hunk_intent() {
    let (intents, _) = drive(two_hunk_diff(), false, |h| {
        h.get_all_by_label("Stage hunk").last().unwrap().click()
    });
    assert!(
        intents.contains(&GitIntent::StageHunk(1)),
        "clicking the last hunk button emits StageHunk(1), got {intents:?}"
    );
}

#[test]
fn selecting_a_line_then_staging_emits_stage_lines_for_that_line() {
    // Line index 2 of the hunk is the `new();` addition. Once selected, the
    // button switches from "Stage hunk" to "Stage lines" and emits StageLines.
    let (intents, _) = drive(sample_diff(), false, |h| {
        h.get_by_label_contains("new()").click();
        h.run();
        h.get_by_label("Stage lines").click();
    });
    assert!(
        intents.contains(&GitIntent::StageLines {
            hunk: 0,
            lines: vec![2],
        }),
        "selecting the addition line drives a partial stage of that line, got {intents:?}"
    );
}

#[test]
fn selecting_a_line_then_unstaging_emits_unstage_lines_for_that_line() {
    let (intents, _) = drive(sample_diff(), true, |h| {
        h.get_by_label_contains("new()").click();
        h.run();
        h.get_by_label("Unstage lines").click();
    });
    assert!(
        intents.contains(&GitIntent::UnstageLines {
            hunk: 0,
            lines: vec![2],
        }),
        "selecting an already-staged line drives a partial unstage of that line, got {intents:?}"
    );
}

#[test]
fn clicking_stage_line_action_emits_stage_lines_for_that_line() {
    let (intents, _) = drive(sample_diff(), false, |h| {
        h.get_all_by_label("Stage line").last().unwrap().click()
    });
    assert!(
        intents.contains(&GitIntent::StageLines {
            hunk: 0,
            lines: vec![2],
        }),
        "clicking the + line action stages that line, got {intents:?}"
    );
}

#[test]
fn clicking_unstage_line_action_emits_unstage_lines_for_that_line() {
    let (intents, _) = drive(sample_diff(), true, |h| {
        h.get_all_by_label("Unstage line").last().unwrap().click()
    });
    assert!(
        intents.contains(&GitIntent::UnstageLines {
            hunk: 0,
            lines: vec![2],
        }),
        "clicking the - line action unstages that line, got {intents:?}"
    );
}

#[test]
fn close_button_requests_close() {
    let (_, closed) = drive(sample_diff(), false, |h| h.get_by_label("Close").click());
    assert!(closed, "clicking Close requests the overlay to close");
}

/// Drives `diff_view` read-only (full-screen commit diff, M9-7).
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

fn text_cell(harness: &mut Harness<'_, ()>, label: &str, col: usize) -> egui::Pos2 {
    let row = harness.get_by_label_contains(label).rect();
    let char_w = harness.ctx.fonts_mut(|fonts| {
        fonts
            .glyph_width(&egui::FontId::monospace(12.0), ' ')
            .max(1.0)
    });
    // Same layout as the copy-test diffs (3-digit gutter).
    let content_left = row.left() + content_x_offset(&copy_diff(""), char_w);
    egui::pos2(content_left + (col as f32 + 0.5) * char_w, row.center().y)
}

fn copy_diff(content: &str) -> FileDiff {
    FileDiff {
        path: "notes.txt".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1 +1 @@".into(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![line(LineOrigin::Addition, content)],
        }],
        source_lines: Vec::new(),
        image: None,
    }
}

fn click_text_at(harness: &mut Harness<'_, ()>, pos: egui::Pos2, clicks: usize) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    for _ in 0..clicks {
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
        }
    }
    harness.step();
}

fn drive_copy_diff(
    content: &'static str,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> Option<String> {
    let palette = Palette::light();
    let diff = copy_diff(content);
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state_in_ui.borrow_mut(),
            &mut sink,
            None,
        );
    });
    harness.run();
    actions(&mut harness);
    harness.event(egui::Event::Copy);
    harness.step();
    copied_text(&harness)
}

fn drive_read_only(
    diff: FileDiff,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> (Vec<GitIntent>, bool) {
    let palette = Palette::light();
    let intents = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let closed = Rc::new(RefCell::new(false));
    let closed_sink = closed.clone();
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();

    let mut harness = Harness::new_ui(move |ui| {
        let did_close = diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::Commit,
            &mut state_in_ui.borrow_mut(),
            &mut sink.borrow_mut(),
            None,
        );
        *closed_sink.borrow_mut() |= did_close;
    });
    harness.run();
    actions(&mut harness);
    harness.run();

    let out = intents.borrow().clone();
    let c = *closed.borrow();
    (out, c)
}

#[test]
fn read_only_diff_renders_lines_but_no_staging_controls() {
    // Clicking an addition line emits no intent and no staging control is
    // rendered: this is history (git.md §9), not the current index.
    let (intents, _) = drive_read_only(two_hunk_diff(), |h| {
        h.get_by_label_contains("new()").click();
        h.run();
        assert!(h.query_by_label("Stage hunk").is_none());
        assert!(h.query_by_label("Stage line").is_none());
        assert!(h.query_by_label("Unstage hunk").is_none());
        assert!(h.query_by_label("Discard hunk").is_none());
        h.get_by_label_contains("src/main.rs");
        h.get_by_label_contains("new()");
    });
    assert!(
        intents.is_empty(),
        "clicking a line in a read-only diff emits no intent, got {intents:?}"
    );
}

#[test]
fn read_only_diff_close_button_requests_close() {
    let (_, closed) = drive_read_only(sample_diff(), |h| h.get_by_label("Close").click());
    assert!(
        closed,
        "Close in a read-only commit diff requests a return to the graph"
    );
}

#[test]
fn drag_then_cmd_c_copies_diff_text_to_clipboard() {
    let text = drive_copy_diff("hello world\n", |harness| {
        let start = text_cell(harness, "hello world", 0);
        let end = text_cell(harness, "hello world", 4);
        harness.event(egui::Event::PointerMoved(start));
        harness.event(egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
        harness.run();
        harness.event(egui::Event::PointerMoved(end));
        harness.run();
        harness.event(egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        });
        harness.run();
    });

    assert_eq!(text.as_deref(), Some("hello"));
}

#[test]
fn double_click_then_cmd_c_copies_the_word() {
    let text = drive_copy_diff("hello world\n", |harness| {
        let pos = text_cell(harness, "hello world", 7);
        click_text_at(harness, pos, 2);
        assert!(harness.query_by_label("Stage lines").is_none());
    });

    assert_eq!(text.as_deref(), Some("world"));
}

#[test]
fn triple_click_then_cmd_c_copies_the_whole_line() {
    let text = drive_copy_diff("    hello world\n", |harness| {
        let pos = text_cell(harness, "hello world", 6);
        click_text_at(harness, pos, 3);
    });

    assert_eq!(text.as_deref(), Some("    hello world"));
}

#[test]
fn binary_file_offers_no_line_staging() {
    let palette = Palette::light();
    let diff = FileDiff {
        path: "logo.png".into(),
        binary: true,
        oversize: false,
        hunks: vec![],
        source_lines: vec![],
        image: None,
    };
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    harness.get_by_label_contains("Binary file");
    assert!(
        harness.query_by_label("Stage hunk").is_none(),
        "a binary file shows no per-hunk staging button"
    );
}

#[test]
fn oversize_diff_shows_summary_and_no_line_staging() {
    let palette = Palette::light();
    let diff = FileDiff {
        path: "huge.txt".into(),
        binary: false,
        oversize: true,
        hunks: vec![],
        source_lines: vec![],
        image: None,
    };
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    harness.get_by_label_contains("Large diff");
    assert!(
        harness.query_by_label("Stage hunk").is_none(),
        "an oversize diff shows no per-hunk staging button"
    );
}

#[test]
fn gutter_shows_old_and_new_line_numbers() {
    let palette = Palette::light();
    let diff = FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -96,2 +96,2 @@".into(),
            old_start: 96,
            old_lines: 2,
            new_start: 96,
            new_lines: 2,
            lines: vec![
                DiffLine {
                    origin: LineOrigin::Context,
                    content: "fn main() {\n".into(),
                    old_lineno: Some(96),
                    new_lineno: Some(96),
                },
                DiffLine {
                    origin: LineOrigin::Deletion,
                    content: "    old();\n".into(),
                    old_lineno: Some(97),
                    new_lineno: None,
                },
                DiffLine {
                    origin: LineOrigin::Addition,
                    content: "    new();\n".into(),
                    old_lineno: None,
                    new_lineno: Some(97),
                },
            ],
        }],
        source_lines: Vec::new(),
        image: None,
    };
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    // Context: both numbers; deletion: old only; addition: new only.
    harness.get_by_label_contains("96 96  fn main() {");
    harness.get_by_label_contains("97  -    old();");
    harness.get_by_label_contains(" 97 +    new();");
}

#[test]
fn header_shows_addition_and_deletion_totals() {
    let palette = Palette::light();
    let diff = sample_diff();
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    harness.get_by_label("+1");
    harness.get_by_label("−1");
}

#[test]
fn extend_context_reveals_five_more_lines_above_and_below() {
    let (_, _) = drive(extendable_diff(), false, |h| {
        assert!(
            h.query_by_label_contains("src-line-2").is_none(),
            "before extension, only the hunk lines are rendered"
        );
        h.get_by_label("Extend context").click();
        h.run();
        // Above: 5 lines (2..6); below: clamped to the end of the file (8..9).
        h.get_by_label_contains("src-line-2");
        h.get_by_label_contains("src-line-6");
        h.get_by_label_contains("src-line-8");
        h.get_by_label_contains("src-line-9");
        assert!(
            h.query_by_label_contains("src-line-1").is_none(),
            "the first line only enters at the next extension"
        );
        h.get_by_label("Extend context").click();
        h.run();
        h.get_by_label_contains("src-line-1");
        assert!(
            h.query_by_label("Extend context").is_none(),
            "the whole file is shown: the button disappears"
        );
    });
}

#[test]
fn extend_context_is_absent_without_source_lines() {
    let (_, _) = drive(sample_diff(), false, |h| {
        assert!(
            h.query_by_label("Extend context").is_none(),
            "without source content (deleted file...), nothing to extend"
        );
    });
}

#[test]
fn read_only_diff_still_offers_context_extension() {
    // Extending context is a view action, not a staging one: also available on
    // the full-screen commit diff (M9-7).
    let (intents, _) = drive_read_only(extendable_diff(), |h| {
        h.get_by_label("Extend context").click();
        h.run();
        h.get_by_label_contains("src-line-2");
        assert!(h.query_by_label("Stage hunk").is_none());
    });
    assert!(intents.is_empty());
}

#[test]
fn reloading_a_shrunk_diff_drops_a_stale_selection_and_signals_it() {
    let palette = Palette::light();
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    // The selection targets line 2 of hunk 0; after reload the file has no hunk
    // ⇒ the selection no longer applies.
    let diff = Rc::new(RefCell::new(sample_diff()));
    let diff_in_ui = diff.clone();

    let mut harness = Harness::new_ui(move |ui| {
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff_in_ui.borrow(),
            DiffSurface::WorkingTree { staged: false },
            &mut state_in_ui.borrow_mut(),
            &mut sink,
            None,
        );
    });
    harness.run();
    harness.get_by_label_contains("new()").click();
    harness.run();

    // Disk edit: the diff reloads empty ⇒ reconciliation on the app side.
    let reloaded = FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![],
        source_lines: vec![],
        image: None,
    };
    let dropped = state.borrow_mut().reconcile(&reloaded);
    *diff.borrow_mut() = reloaded;
    assert!(
        dropped,
        "the selection on a now-absent hunk no longer applies"
    );
    harness.run();

    harness.get_by_label_contains("selection no longer applies");
}

fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 40, 40, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode a 2x2 PNG");
    bytes
}

#[test]
fn an_image_diff_shows_a_zoomable_preview_instead_of_the_binary_placeholder() {
    let palette = Palette::light();
    let diff = FileDiff {
        path: "assets/logo.png".into(),
        binary: true,
        oversize: false,
        hunks: Vec::new(),
        source_lines: Vec::new(),
        image: Some(ImageBlob {
            bytes: tiny_png(),
            fingerprint: 1,
        }),
    };
    let mut harness = Harness::new_ui(move |ui| {
        let mut state = DiffViewState::default();
        let mut sink = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state,
            &mut sink,
            None,
        );
    });
    harness.run();

    // The zoom toolbar only exists in the image branch — its presence proves the
    // preview replaced the binary placeholder.
    harness.get_by_label("Fit");
    harness.get_by_label("+");
    assert!(
        harness.query_by_label_contains("Binary file").is_none(),
        "an image file must render a preview, not the binary placeholder",
    );
}

/// Addition line carrying a real `new_lineno`, so the inline editor anchors to a
/// single line (unlike the `None`/`None` lines of `sample_diff`).
fn review_diff() -> FileDiff {
    FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1,1 +1,2 @@".into(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            lines: vec![
                DiffLine {
                    origin: LineOrigin::Context,
                    content: "fn main() {\n".into(),
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                },
                DiffLine {
                    origin: LineOrigin::Addition,
                    content: "    work();\n".into(),
                    old_lineno: None,
                    new_lineno: Some(2),
                },
            ],
        }],
        source_lines: Vec::new(),
        image: None,
    }
}

/// Drives `diff_view` with review enabled and shared state, returns the emitted
/// review intents after the scripted actions.
fn drive_review(
    diff: FileDiff,
    comments: FileComments,
    actions: impl Fn(&mut Harness<'_, ()>) + 'static,
) -> Vec<ReviewIntent> {
    let palette = Palette::light();
    let git = Rc::new(RefCell::new(Vec::new()));
    let review = Rc::new(RefCell::new(Vec::new()));
    let review_sink = review.clone();
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let comments = Rc::new(comments);
    let comments_in_ui = comments.clone();
    let threads_in_ui = ForgeThreads::new();

    let mut harness = Harness::new_ui(move |ui| {
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::WorkingTree { staged: false },
            &mut state_in_ui.borrow_mut(),
            &mut git.borrow_mut(),
            Some(&mut DiffReview {
                comments: comments_in_ui.as_ref(),
                forge: None,
                existing: &threads_in_ui,
                agent: "claude",
                intents: &mut review_sink.borrow_mut(),
            }),
        );
    });
    harness.run();
    actions(&mut harness);
    harness.run();

    let out = review.borrow().clone();
    out
}

#[test]
fn clicking_the_note_icon_then_validating_emits_save_comment() {
    let intents = drive_review(review_diff(), FileComments::new(), |h| {
        // The note (✦) icon for the added line opens its inline editor.
        h.get_all_by_label("Comment line").last().unwrap().click();
        h.run();
        h.get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
            .type_text("needs rename");
        h.run();
        h.get_by_label("Validate note").click();
    });

    assert!(
        intents.iter().any(|i| matches!(
            i,
            ReviewIntent::SaveComment { pool, file, comment }
                if *pool == ReviewPool::Agent
                    && file == "src/main.rs"
                    && comment.new_lineno == Some(2)
                    && comment.note == "needs rename"
        )),
        "Validate must emit SaveComment for the annotated line, got {intents:?}",
    );
}

/// Drives the PR review surface (two pools) and returns the emitted intents after
/// clicking the gutter button labelled `button`, typing `note`, and validating.
fn drive_pr_review(button: &'static str, note: &'static str) -> Vec<ReviewIntent> {
    let palette = Palette::light();
    let diff = review_diff();
    let review = Rc::new(RefCell::new(Vec::new()));
    let review_sink = review.clone();
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let agent_notes = Rc::new(FileComments::new());
    let agent_in_ui = agent_notes.clone();
    let forge = Rc::new(FileComments::new());
    let forge_in_ui = forge.clone();
    let threads = ForgeThreads::new();

    let mut harness = Harness::new_ui(move |ui| {
        let mut git: Vec<GitIntent> = Vec::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::PrReview,
            &mut state_in_ui.borrow_mut(),
            &mut git,
            Some(&mut DiffReview {
                comments: agent_in_ui.as_ref(),
                forge: Some(forge_in_ui.as_ref()),
                existing: &threads,
                agent: "claude",
                intents: &mut review_sink.borrow_mut(),
            }),
        );
    });
    harness.run();
    harness.get_all_by_label(button).last().unwrap().click();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .type_text(note);
    harness.run();
    harness.get_by_label("Validate note").click();
    harness.run();

    let out = review.borrow().clone();
    out
}

#[test]
fn pr_forge_button_records_a_forge_pool_comment() {
    // The MessageSquarePlus button (slot 0) feeds the forge pool — the comments
    // posted to GitHub / Bitbucket on submit, never sent to the agent.
    let intents = drive_pr_review("Comment for review", "needs rename");
    assert!(
        intents.iter().any(|i| matches!(
            i,
            ReviewIntent::SaveComment { pool, file, comment }
                if *pool == ReviewPool::Forge
                    && file == "src/main.rs"
                    && comment.new_lineno == Some(2)
                    && comment.note == "needs rename"
        )),
        "the forge note button must record a Forge-pool comment, got {intents:?}",
    );
}

#[test]
fn pr_agent_button_records_an_agent_pool_note() {
    // The Sparkles button (slot 1) feeds the separate agent pool — batched to the
    // agent via "Send to …", never posted to the forge.
    let intents = drive_pr_review("Comment line", "ask claude");
    assert!(
        intents.iter().any(|i| matches!(
            i,
            ReviewIntent::SaveComment { pool, file, comment }
                if *pool == ReviewPool::Agent
                    && file == "src/main.rs"
                    && comment.new_lineno == Some(2)
                    && comment.note == "ask claude"
        )),
        "the agent note button must record an Agent-pool note, got {intents:?}",
    );
}

#[test]
fn existing_pr_thread_renders_anchored_read_only() {
    let palette = Palette::light();
    let diff = review_diff();
    let mut existing = ForgeThreads::new();
    existing.insert(
        "src/main.rs".into(),
        std::iter::once((
            (None, Some(2u32)),
            vec![helm::review::ThreadComment {
                author: "octocat".into(),
                body: "please rename work()".into(),
                id: Some(11),
            }],
        ))
        .collect(),
    );
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let mut git: Vec<GitIntent> = Vec::new();
        let mut intents: Vec<ReviewIntent> = Vec::new();
        let empty = FileComments::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::Commit,
            &mut state_in_ui.borrow_mut(),
            &mut git,
            Some(&mut DiffReview {
                comments: &empty,
                forge: None,
                existing: &existing,
                agent: "claude",
                intents: &mut intents,
            }),
        );
    });
    harness.run();
    // The posted comment shows anchored under its line (author + body), read-only.
    harness.get_by_label("octocat");
    harness.get_by_label("please rename work()");
}

#[test]
fn new_side_thread_on_a_modified_line_renders_once_not_on_the_deleted_row() {
    let palette = Palette::light();
    // A line modified in place: the deleted row (old 2) and the added row (new 2)
    // share the number 2. A new-side thread keyed (None, Some(2)) must land on the
    // added row only — not duplicate onto the deleted row whose old line is also 2.
    let diff = FileDiff {
        path: "src/main.rs".into(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1,2 +1,2 @@".into(),
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            lines: vec![
                DiffLine {
                    origin: LineOrigin::Context,
                    content: "fn main() {\n".into(),
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                },
                DiffLine {
                    origin: LineOrigin::Deletion,
                    content: "    old();\n".into(),
                    old_lineno: Some(2),
                    new_lineno: None,
                },
                DiffLine {
                    origin: LineOrigin::Addition,
                    content: "    new();\n".into(),
                    old_lineno: None,
                    new_lineno: Some(2),
                },
            ],
        }],
        source_lines: Vec::new(),
        image: None,
    };
    let mut existing = ForgeThreads::new();
    existing.insert(
        "src/main.rs".into(),
        std::iter::once((
            (None, Some(2u32)),
            vec![helm::review::ThreadComment {
                author: "octocat".into(),
                body: "please rename new()".into(),
                id: Some(22),
            }],
        ))
        .collect(),
    );
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let mut git: Vec<GitIntent> = Vec::new();
        let mut intents: Vec<ReviewIntent> = Vec::new();
        let empty = FileComments::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::Commit,
            &mut state_in_ui.borrow_mut(),
            &mut git,
            Some(&mut DiffReview {
                comments: &empty,
                forge: None,
                existing: &existing,
                agent: "claude",
                intents: &mut intents,
            }),
        );
    });
    harness.run();
    assert_eq!(
        harness.get_all_by_label("please rename new()").count(),
        1,
        "a new-side thread on a modified line must render once, not on both rows",
    );
}

#[test]
fn ask_agent_pill_on_a_thread_emits_the_intent() {
    let palette = Palette::light();
    let diff = review_diff();
    let mut existing = ForgeThreads::new();
    existing.insert(
        "src/main.rs".into(),
        std::iter::once((
            (None, Some(2u32)),
            vec![helm::review::ThreadComment {
                author: "octocat".into(),
                body: "please rename work()".into(),
                id: Some(33),
            }],
        ))
        .collect(),
    );
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let intents = Rc::new(RefCell::new(Vec::<ReviewIntent>::new()));
    let intents_in_ui = intents.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let mut git: Vec<GitIntent> = Vec::new();
        let empty = FileComments::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::Commit,
            &mut state_in_ui.borrow_mut(),
            &mut git,
            Some(&mut DiffReview {
                comments: &empty,
                forge: None,
                existing: &existing,
                agent: "claude",
                intents: &mut intents_in_ui.borrow_mut(),
            }),
        );
    });
    harness.run();
    harness.get_by_label("Ask claude").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::AskAgentOnThread { file, old, new }
                if file == "src/main.rs" && old.is_none() && *new == Some(2)
        )),
        "the Ask pill must emit AskAgentOnThread anchored at the thread, got {:?}",
        intents.borrow(),
    );
}

#[test]
fn reply_pill_on_a_thread_emits_reply_to_thread() {
    let palette = Palette::light();
    let diff = review_diff();
    let mut existing = ForgeThreads::new();
    existing.insert(
        "src/main.rs".into(),
        std::iter::once((
            (None, Some(2u32)),
            vec![helm::review::ThreadComment {
                author: "octocat".into(),
                body: "please rename work()".into(),
                id: Some(77),
            }],
        ))
        .collect(),
    );
    let state = Rc::new(RefCell::new(DiffViewState::default()));
    let state_in_ui = state.clone();
    let intents = Rc::new(RefCell::new(Vec::<ReviewIntent>::new()));
    let intents_in_ui = intents.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let mut git: Vec<GitIntent> = Vec::new();
        let empty = FileComments::new();
        diff_view(
            ui,
            &palette,
            &diff,
            DiffSurface::PrReview,
            &mut state_in_ui.borrow_mut(),
            &mut git,
            Some(&mut DiffReview {
                comments: &empty,
                forge: Some(&empty),
                existing: &existing,
                agent: "claude",
                intents: &mut intents_in_ui.borrow_mut(),
            }),
        );
    });
    harness.run();
    // Open the reply editor, type a reply, send it.
    harness.get_by_label("Reply").click();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .type_text("on it");
    harness.run();
    harness.get_by_label("Send reply").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::ReplyToThread { comment_id, body }
                if *comment_id == 77 && body == "on it"
        )),
        "the Reply editor must emit ReplyToThread for the thread root, got {:?}",
        intents.borrow(),
    );
}

#[test]
fn review_chip_opens_a_popover_listing_comments_with_send() {
    let mut comments = FileComments::new();
    helm::review::add_comment(
        &mut comments,
        "src/main.rs",
        LineComment {
            old_lineno: None,
            new_lineno: Some(2),
            code: "    work();".into(),
            note: "needs rename".into(),
        },
    );

    let intents = drive_review(review_diff(), comments, |h| {
        // The recap chip carries the count and toggles the popover.
        h.get_by_label("Review notes").click();
        h.run();
        assert!(
            h.query_all_by_label_contains("needs rename")
                .next()
                .is_some(),
            "the popover lists the stored note",
        );
        h.get_by_label("Delete review note");
        h.get_by_label("Send to claude").click();
    });

    assert!(
        intents
            .iter()
            .any(|i| matches!(i, ReviewIntent::SendToAgent)),
        "the popover Send button must emit SendToAgent, got {intents:?}",
    );
}

#[test]
fn the_editor_delete_icon_removes_the_comment() {
    let mut comments = FileComments::new();
    helm::review::add_comment(
        &mut comments,
        "src/main.rs",
        LineComment {
            old_lineno: None,
            new_lineno: Some(2),
            code: "    work();".into(),
            note: "needs rename".into(),
        },
    );

    let intents = drive_review(review_diff(), comments, |h| {
        // Click the saved note card to re-open its editor, then ✕ deletes it.
        h.get_by_label("Edit review note").click();
        h.run();
        h.get_by_label("Delete note").click();
    });

    assert!(
        intents.iter().any(|i| matches!(
            i,
            ReviewIntent::DeleteComment { file, line, .. }
                if file == "src/main.rs" && *line == Some(2)
        )),
        "the editor ✕ must emit DeleteComment for the line, got {intents:?}",
    );
}

#[test]
fn clicking_outside_the_editor_validates_the_note() {
    let intents = drive_review(review_diff(), FileComments::new(), |h| {
        // Open the added line's editor, type, then click outside (the header
        // Close button): the field loses focus, which validates.
        h.get_all_by_label("Comment line").last().unwrap().click();
        h.run();
        h.get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
            .type_text("outside save");
        h.run();
        h.get_by_label("Close").click();
        h.run();
        h.run();
    });

    assert!(
        intents.iter().any(|i| matches!(
            i,
            ReviewIntent::SaveComment { file, comment, .. }
                if file == "src/main.rs"
                    && comment.new_lineno == Some(2)
                    && comment.note == "outside save"
        )),
        "losing focus must emit SaveComment for the annotated line, got {intents:?}",
    );
}
