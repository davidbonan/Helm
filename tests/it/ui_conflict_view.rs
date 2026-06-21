use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::conflict::{ConflictFile, ConflictKind, Region};
use helm::theme::Palette;
use helm::ui::conflict_view::{conflict_view, ConflictEditorState, ResolveRequest};

fn both_modified(path: &str) -> ConflictFile {
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::BothModified,
        ours_label: "Current · ours".to_owned(),
        theirs_label: "Incoming · theirs".to_owned(),
        regions: vec![
            Region::Stable(vec!["fn run() {".to_owned()]),
            Region::Conflict {
                ours: vec!["    ours_a".to_owned(), "    ours_b".to_owned()],
                theirs: vec!["    theirs_a".to_owned()],
                base: vec!["    base_line".to_owned()],
            },
            Region::Stable(vec!["}".to_owned()]),
        ],
        has_base: true,
    }
}

fn two_conflicts(path: &str) -> ConflictFile {
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::BothModified,
        ours_label: "Current · ours".to_owned(),
        theirs_label: "Incoming · theirs".to_owned(),
        regions: vec![
            Region::Stable(vec!["fn run() {".to_owned()]),
            Region::Conflict {
                ours: vec!["    a_ours".to_owned()],
                theirs: vec!["    a_theirs".to_owned()],
                base: vec!["    a_base".to_owned()],
            },
            Region::Stable(vec!["    mid();".to_owned()]),
            Region::Conflict {
                ours: vec!["    b_ours".to_owned()],
                theirs: vec!["    b_theirs".to_owned()],
                base: vec!["    b_base".to_owned()],
            },
            Region::Stable(vec!["}".to_owned()]),
        ],
        has_base: true,
    }
}

/// One conflict preceded by a long (8-line) context run — the panes show it in
/// full now (no folding), so every context line is present.
fn long_context(path: &str) -> ConflictFile {
    let ctx: Vec<String> = (0..8).map(|i| format!("ctx_{i}")).collect();
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::BothModified,
        ours_label: "Current · ours".to_owned(),
        theirs_label: "Incoming · theirs".to_owned(),
        regions: vec![
            Region::Stable(ctx),
            Region::Conflict {
                ours: vec!["    ours".to_owned()],
                theirs: vec!["    theirs".to_owned()],
                base: vec!["    base".to_owned()],
            },
        ],
        has_base: true,
    }
}

fn deleted_by_them(path: &str) -> ConflictFile {
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::DeletedByThem,
        ours_label: "Current · ours".to_owned(),
        theirs_label: "Incoming · theirs".to_owned(),
        regions: vec![],
        has_base: true,
    }
}

fn binary(path: &str) -> ConflictFile {
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::Binary,
        ours_label: "Current · ours".to_owned(),
        theirs_label: "Incoming · theirs".to_owned(),
        regions: vec![],
        has_base: true,
    }
}

/// A single conflict with plain (no leading whitespace, extension-less) content so
/// each side's line is queryable by its accessibility label.
fn single_conflict(path: &str) -> ConflictFile {
    ConflictFile {
        path: path.to_owned(),
        kind: ConflictKind::BothModified,
        ours_label: "Current · ours".to_owned(),
        theirs_label: "Incoming · theirs".to_owned(),
        regions: vec![Region::Conflict {
            ours: vec!["OURS_LINE".to_owned()],
            theirs: vec!["THEIRS_LINE".to_owned()],
            base: vec!["BASE_LINE".to_owned()],
        }],
        has_base: true,
    }
}

struct PageState {
    state: ConflictEditorState,
    busy: bool,
    resolve: Option<ResolveRequest>,
    close: bool,
}

fn editor(files: Vec<ConflictFile>) -> Harness<'static, PageState> {
    Harness::builder()
        .with_size(egui::vec2(960.0, 720.0))
        .build_ui_state(
            |ui, s: &mut PageState| {
                let palette = Palette::dark();
                let action = conflict_view(ui, &palette, &mut s.state, s.busy);
                if let Some(resolve) = action.resolve {
                    s.resolve = Some(resolve);
                }
                s.close |= action.close;
            },
            PageState {
                state: ConflictEditorState::new(files),
                busy: false,
                resolve: None,
                close: false,
            },
        )
}

/// The per-region take checkboxes are the only `CheckBox`-role nodes. They render in
/// order: pane A's conflicts top-to-bottom, then pane B's — so index `i` is conflict
/// `i` on A, and `n + i` is conflict `i` on B for an `n`-conflict file.
fn tick(harness: &mut Harness<'static, PageState>, index: usize) {
    harness
        .get_all_by(|n| format!("{:?}", n.role()) == "CheckBox")
        .nth(index)
        .expect("take checkbox present")
        .click();
    harness.run();
}

fn checkbox_count(harness: &mut Harness<'static, PageState>) -> usize {
    harness
        .get_all_by(|n| format!("{:?}", n.role()) == "CheckBox")
        .count()
}

#[test]
fn renders_the_two_panes_and_the_output() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();

    harness.get_by_label_contains("CURRENT · OURS");
    harness.get_by_label_contains("INCOMING · THEIRS");
    harness.get_by_label_contains("OUTPUT");
    // one take checkbox per side for the single conflict.
    assert_eq!(checkbox_count(&mut harness), 2);
}

#[test]
fn ticking_a_takes_ours() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    tick(&mut harness, 0); // A = ours
    harness.get_by_label("Save").click();
    harness.run();

    let ResolveRequest::Compose { path, content } = harness
        .state()
        .resolve
        .as_ref()
        .expect("Save emits a resolve")
    else {
        panic!("expected Compose");
    };
    assert_eq!(path, "x.rs");
    assert!(content.contains("ours_a"), "kept the A side: {content:?}");
    assert!(
        !content.contains("theirs_a"),
        "dropped the B side: {content:?}"
    );
}

#[test]
fn ticking_b_takes_theirs() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    tick(&mut harness, 1); // B = theirs
    harness.get_by_label("Save").click();
    harness.run();

    let ResolveRequest::Compose { content, .. } =
        harness.state().resolve.as_ref().expect("resolve emitted")
    else {
        panic!("expected Compose");
    };
    assert!(content.contains("theirs_a"), "kept the B side: {content:?}");
    assert!(
        !content.contains("ours_a"),
        "dropped the A side: {content:?}"
    );
}

#[test]
fn ticking_a_then_b_concatenates_ours_then_theirs() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    tick(&mut harness, 0);
    tick(&mut harness, 1);
    harness.get_by_label("Save").click();
    harness.run();

    let ResolveRequest::Compose { content, .. } =
        harness.state().resolve.as_ref().expect("resolve emitted")
    else {
        panic!("expected Compose");
    };
    assert_eq!(
        content,
        "fn run() {\n    ours_a\n    ours_b\n    theirs_a\n}\n"
    );
}

#[test]
fn ticking_b_then_a_concatenates_theirs_then_ours() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    tick(&mut harness, 1);
    tick(&mut harness, 0);
    harness.get_by_label("Save").click();
    harness.run();

    let ResolveRequest::Compose { content, .. } =
        harness.state().resolve.as_ref().expect("resolve emitted")
    else {
        panic!("expected Compose");
    };
    assert_eq!(
        content,
        "fn run() {\n    theirs_a\n    ours_a\n    ours_b\n}\n"
    );
}

#[test]
fn the_output_is_editable_and_saves_hand_edits() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    // The Output is always a text field; a hand edit saves once the region is resolved.
    tick(&mut harness, 0); // A = ours, so Save is enabled

    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .type_text("HAND_EDITED");
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();

    let ResolveRequest::Compose { content, .. } =
        harness.state().resolve.as_ref().expect("resolve emitted")
    else {
        panic!("expected Compose");
    };
    assert!(
        content.contains("HAND_EDITED"),
        "saved the hand edit: {content:?}"
    );
}

#[test]
fn save_emits_nothing_while_a_region_is_unresolved() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();

    assert!(
        harness.state().resolve.is_none(),
        "an unresolved file cannot be saved"
    );
}

#[test]
fn close_warns_on_an_unsaved_composition_then_discard_leaves() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    tick(&mut harness, 0);

    harness.get_by_label_contains("Close").click();
    harness.run();
    assert!(!harness.state().close, "the first Close only warns");
    harness.get_by_label_contains("Unsaved resolution");

    harness.get_by_label("Discard").click();
    harness.run();
    assert!(
        harness.state().close,
        "confirming the discard leaves the editor"
    );
}

#[test]
fn close_leaves_immediately_with_nothing_unsaved() {
    let mut harness = editor(vec![both_modified("x.rs")]);
    harness.run();
    harness.get_by_label_contains("Close").click();
    harness.run();

    assert!(harness.state().close);
}

#[test]
fn the_nav_readout_jumps_between_conflicts() {
    let mut harness = editor(vec![two_conflicts("x.rs")]);
    harness.run();
    harness.get_by_label("1/2");

    harness.get_by_label("▼").click();
    harness.run();
    harness.get_by_label("2/2");
}

#[test]
fn long_context_runs_show_in_full() {
    let mut harness = editor(vec![long_context("x.rs")]);
    harness.run();

    // every context line is visible (no folding) — the panes keep them all as rows.
    assert!(
        harness.get_all_by_label("ctx_0").count() >= 1,
        "the far context line is kept"
    );
    assert!(
        harness.get_all_by_label("ctx_7").count() >= 1,
        "the near context line is kept"
    );
    assert!(
        harness.query_by_label_contains("lines hidden").is_none(),
        "nothing is folded away"
    );
}

#[test]
fn clicking_a_pane_row_body_takes_that_side() {
    let mut harness = editor(vec![single_conflict("note")]);
    harness.run();
    // Click the code line itself (not its checkbox): the body takes that side.
    harness.get_by_label("OURS_LINE").click();
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();

    let ResolveRequest::Compose { content, .. } =
        harness.state().resolve.as_ref().expect("resolve emitted")
    else {
        panic!("expected Compose");
    };
    assert!(
        content.contains("OURS_LINE"),
        "clicking the body kept ours: {content:?}"
    );
    assert!(
        !content.contains("THEIRS_LINE"),
        "dropped theirs: {content:?}"
    );
}

#[test]
fn the_output_header_shows_the_resolved_count() {
    let mut harness = editor(vec![two_conflicts("x.rs")]);
    harness.run();
    harness.get_by_label_contains("0/2 resolved");

    tick(&mut harness, 0); // A on the first conflict
    harness.get_by_label_contains("1/2 resolved");
}

#[test]
fn the_binary_card_takes_a_whole_side() {
    let mut harness = editor(vec![binary("logo.png")]);
    harness.run();
    harness.get_by_label("Current · ours").click();
    harness.run();
    assert_eq!(
        harness.state().resolve,
        Some(ResolveRequest::UseSide {
            path: "logo.png".to_owned(),
            ours: true,
        })
    );

    let mut harness = editor(vec![binary("logo.png")]);
    harness.run();
    harness.get_by_label("Incoming · theirs").click();
    harness.run();
    assert_eq!(
        harness.state().resolve,
        Some(ResolveRequest::UseSide {
            path: "logo.png".to_owned(),
            ours: false,
        })
    );
}

#[test]
fn the_delete_modify_card_keeps_or_deletes() {
    let mut harness = editor(vec![deleted_by_them("doomed.txt")]);
    harness.run();
    harness.get_by_label("Keep the modified version").click();
    harness.run();
    assert_eq!(
        harness.state().resolve,
        Some(ResolveRequest::Keep {
            path: "doomed.txt".to_owned()
        })
    );

    let mut harness = editor(vec![deleted_by_them("doomed.txt")]);
    harness.run();
    harness.get_by_label("Delete the file").click();
    harness.run();
    assert_eq!(
        harness.state().resolve,
        Some(ResolveRequest::Delete {
            path: "doomed.txt".to_owned()
        })
    );
}
