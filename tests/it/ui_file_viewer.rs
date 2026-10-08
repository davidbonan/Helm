use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::files::content::{self, Content, FileSnapshot};
use helm::files::file_type::FileType;
use helm::git::diff::ImageBlob;
use helm::git::edit::EditRequest;
use helm::git::status::ChangeKind;
use helm::review::{FileComments, LineComment, ReviewIntent, ReviewPool};
use helm::theme::Palette;
use helm::ui::file_viewer::{file_viewer, FileViewerState, ViewedFile, ViewerIntents};
use helm::ui::git_panel::GitIntent;
use helm::ui::review_notes::NoteBatch;

/// The viewer over `snapshot` with the worktree's notes, what it emitted, and whether
/// closing was asked.
struct Viewer {
    snapshot: FileSnapshot,
    change: Option<ChangeKind>,
    comments: FileComments,
    view: FileViewerState,
    out: ViewerIntents,
    closed: bool,
}

fn snapshot(path: &str, size: u64, content: Content) -> FileSnapshot {
    FileSnapshot {
        path: path.to_owned(),
        size,
        stamp: None,
        writable: true,
        content,
    }
}

fn harness_on(snapshot: FileSnapshot, change: Option<ChangeKind>) -> Harness<'static, Viewer> {
    harness_with_notes(snapshot, change, FileComments::new())
}

fn harness_with_notes(
    snapshot: FileSnapshot,
    change: Option<ChangeKind>,
    comments: FileComments,
) -> Harness<'static, Viewer> {
    let palette = Palette::dark();
    let mut harness = Harness::new_ui_state(
        move |ui, viewer: &mut Viewer| {
            let file = ViewedFile {
                palette: &palette,
                snapshot: &viewer.snapshot,
                change: viewer.change,
                batch: NoteBatch {
                    comments: &viewer.comments,
                    agent: "claude",
                },
            };
            viewer.closed |= file_viewer(ui, &file, &mut viewer.view, &mut viewer.out);
        },
        Viewer {
            snapshot,
            change,
            comments,
            view: FileViewerState::default(),
            out: ViewerIntents::default(),
            closed: false,
        },
    );
    harness.run();
    harness
}

/// The viewer on `content` at `path`.
fn viewer(
    path: &'static str,
    size: u64,
    content: Content,
    change: Option<ChangeKind>,
) -> Harness<'static, Viewer> {
    harness_on(snapshot(path, size, content), change)
}

fn text(lines: &[&str]) -> Content {
    Content::Text(lines.iter().map(|line| (*line).to_owned()).collect())
}

fn copied_text(harness: &Harness<'_, Viewer>) -> Option<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
}

#[test]
fn text_shows_each_line_after_its_number_under_the_path_size_and_change() {
    let harness = viewer(
        "src/main.rs",
        840,
        text(&["fn main() {", "}"]),
        Some(ChangeKind::Modified),
    );

    harness.get_by_label("1 fn main() {");
    harness.get_by_label("2 }");
    harness.get_by_label("src/main.rs");
    harness.get_by_label("840 B");
    harness.get_by_label("Modified");
}

#[test]
fn the_header_shows_the_file_type_glyph_in_its_color() {
    let palette = Palette::dark();
    let harness = viewer("src/main.rs", 1, text(&["fn main() {}"]), None);

    let glyph = FileType::Rust.glyph().to_string();
    let ink = harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == glyph => {
                Some(shape.fallback_color)
            }
            _ => None,
        });
    assert_eq!(ink, Some(palette.file_type_color(FileType::Rust)));
}

#[test]
fn esc_and_the_close_button_ask_to_close() {
    let mut escaped = viewer("a.txt", 1, text(&["a"]), None);
    escaped.key_press(egui::Key::Escape);
    escaped.run();
    assert!(escaped.state().closed);

    let mut clicked = viewer("a.txt", 1, text(&["a"]), None);
    clicked.get_by_label("Close").click();
    clicked.run();
    assert!(clicked.state().closed);
}

#[test]
fn the_close_control_is_a_square_icon_button_not_a_text_pill() {
    let viewer = viewer("a.txt", 1, text(&["a"]), None);
    let rect = viewer.get_by_label("Close").rect();
    assert_eq!(
        rect.width(),
        rect.height(),
        "icon button is square: {rect:?}"
    );
}

#[test]
fn binary_too_large_and_gone_files_show_their_placeholder() {
    let binary = viewer("blob.bin", 3 * 1024 * 1024, Content::Binary, None);
    let large = viewer("dump.sql", 2_621_440, Content::TooLarge, None);
    let gone = viewer("old.rs", 0, Content::Missing, None);

    binary.get_by_label("Binary file · 3.0 MB");
    large.get_by_label("File too large to display · 2.5 MB");
    gone.get_by_label("File no longer exists");
}

#[test]
fn an_image_opens_in_the_zoomable_preview() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::new(4, 3)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let bytes = png.into_inner();

    let harness = viewer(
        "assets/logo.png",
        bytes.len() as u64,
        Content::Image(ImageBlob::new(bytes)),
        None,
    );

    harness.get_by_label("Fit");
    harness.get_by_label("4×3");
}

#[test]
fn a_triple_click_selects_the_line_and_cmd_c_copies_it() {
    let mut harness = viewer(
        "src/main.rs",
        26,
        text(&["fn main() {", "    run();", "}"]),
        None,
    );
    let pos = text_cell(&mut harness, "2     run();", 6);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    for _ in 0..3 {
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

    harness.event(egui::Event::Copy);
    harness.step();

    assert_eq!(copied_text(&harness).as_deref(), Some("    run();"));
}

const LINES: [&str; 3] = ["fn main() {", "    run();", "}"];

fn main_rs() -> Harness<'static, Viewer> {
    viewer("src/main.rs", 26, text(&LINES), None)
}

/// Pointer position at column `col` of the text of the row labelled `label`.
fn text_cell(harness: &mut Harness<'_, Viewer>, label: &str, col: usize) -> egui::Pos2 {
    let row = harness.get_by_label(label).rect();
    let char_w = harness.ctx.fonts_mut(|fonts| {
        fonts
            .glyph_width(&egui::FontId::monospace(12.0), ' ')
            .max(1.0)
    });
    // The note button's slot, the three-digit gutter, its padding, then the content's own.
    let content_left = row.left() + 22.0 + 3.0 * char_w + 12.0 + 8.0;
    egui::pos2(content_left + (col as f32 + 0.5) * char_w, row.center().y)
}

fn press_at(harness: &mut Harness<'_, Viewer>, pos: egui::Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
}

fn click_at(harness: &mut Harness<'_, Viewer>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    press_at(harness, pos, true);
    press_at(harness, pos, false);
}

fn type_text(harness: &mut Harness<'_, Viewer>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    harness.run();
}

/// A click on the header, away from the text: the editor's exit gesture.
fn click_away(harness: &mut Harness<'_, Viewer>) {
    let pos = harness.get_by_label("26 B").rect().center();
    click_at(harness, pos);
}

fn writes(harness: &Harness<'_, Viewer>) -> Vec<EditRequest> {
    harness
        .state()
        .out
        .git
        .iter()
        .filter_map(|intent| match intent {
            GitIntent::FlushEdit(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

fn buffer(harness: &Harness<'_, Viewer>) -> Option<String> {
    harness
        .state()
        .view
        .editor()
        .map(|edit| edit.buffer.clone())
}

#[test]
fn a_click_in_the_text_opens_the_whole_file_editor_with_the_caret_there() {
    let mut harness = main_rs();

    let pos = text_cell(&mut harness, "2     run();", 4);
    click_at(&mut harness, pos);
    type_text(&mut harness, "X");

    let edit = harness.state().view.editor().cloned().expect("editor open");
    assert_eq!(
        edit.target.range,
        0..3,
        "the editor writes back the whole file"
    );
    assert_eq!(edit.target.original, LINES);
    assert_eq!(
        edit.buffer, "fn main() {\n    Xrun();\n}",
        "the character must land at the clicked column of the clicked line"
    );
}

#[test]
fn a_drag_selects_the_text_instead_of_opening_the_editor() {
    let mut harness = main_rs();
    let start = text_cell(&mut harness, "2     run();", 4);
    let end = text_cell(&mut harness, "2     run();", 8);

    harness.event(egui::Event::PointerMoved(start));
    press_at(&mut harness, start, true);
    harness.event(egui::Event::PointerMoved(end));
    harness.run();
    press_at(&mut harness, end, false);
    harness.event(egui::Event::Copy);
    harness.step();

    assert!(!harness.state().view.is_editing());
    assert_eq!(copied_text(&harness).as_deref(), Some("run()"));
}

#[test]
fn clicking_away_writes_the_buffer_once_and_keeps_it_on_screen() {
    let mut harness = main_rs();
    let pos = text_cell(&mut harness, "2     run();", 4);
    click_at(&mut harness, pos);
    type_text(&mut harness, "X");

    click_away(&mut harness);

    assert!(!harness.state().view.is_editing(), "the editor is left");
    let writes = writes(&harness);
    assert_eq!(writes.len(), 1, "got {writes:?}");
    assert_eq!(writes[0].path, "src/main.rs");
    assert_eq!(writes[0].range, 0..3);
    assert_eq!(writes[0].original, LINES);
    assert_eq!(writes[0].replacement, "fn main() {\n    Xrun();\n}");
    assert!(!writes[0].stage_after && !writes[0].force);
    harness.get_by_label("2     Xrun();");
}

#[test]
fn leaving_an_untouched_editor_writes_nothing() {
    let mut harness = main_rs();
    let pos = text_cell(&mut harness, "2     run();", 4);
    click_at(&mut harness, pos);

    click_away(&mut harness);

    assert!(!harness.state().view.is_editing());
    assert!(writes(&harness).is_empty());
}

#[test]
fn cmd_s_writes_and_leaves_the_editor() {
    let mut harness = main_rs();
    let pos = text_cell(&mut harness, "1 fn main() {", 0);
    click_at(&mut harness, pos);
    type_text(&mut harness, "pub ");

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();

    assert!(!harness.state().view.is_editing());
    let writes = writes(&harness);
    assert_eq!(writes.len(), 1, "got {writes:?}");
    assert_eq!(writes[0].replacement, "pub fn main() {\n    run();\n}");
}

#[test]
fn esc_drops_the_buffer_then_a_second_esc_closes_the_viewer() {
    let mut harness = main_rs();
    let pos = text_cell(&mut harness, "2     run();", 4);
    click_at(&mut harness, pos);
    type_text(&mut harness, "X");

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(
        !harness.state().view.is_editing(),
        "the first Esc leaves the editor"
    );
    assert!(!harness.state().closed, "…without closing the viewer");
    assert!(writes(&harness).is_empty(), "nothing is written");
    harness.get_by_label("2     run();");

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().closed);
}

#[test]
fn a_file_that_cannot_be_edited_takes_no_caret() {
    let read_only = FileSnapshot {
        writable: false,
        ..snapshot("src/main.rs", 26, text(&LINES))
    };
    let long: Vec<String> = (0..3_001).map(|i| format!("line {i}")).collect();
    let cases = [
        (read_only, "1 fn main() {"),
        (
            snapshot("long.txt", 30_000, Content::Text(long)),
            "1 line 0",
        ),
    ];
    for (snapshot, first_row) in cases {
        let path = snapshot.path.clone();
        let mut harness = harness_on(snapshot, None);
        let pos = text_cell(&mut harness, first_row, 3);
        click_at(&mut harness, pos);

        assert!(!harness.state().view.is_editing(), "{path}: no caret");
    }
}

#[test]
fn the_longest_editable_file_still_opens_the_editor() {
    let lines: Vec<String> = (0..3_000).map(|i| format!("line {i}")).collect();
    let mut harness = harness_on(snapshot("long.txt", 30_000, Content::Text(lines)), None);

    // Four-digit gutter: a column past the three-digit one `text_cell` measures.
    let pos = text_cell(&mut harness, "1 line 0", 2);
    click_at(&mut harness, pos);

    assert!(harness.state().view.is_editing());
}

#[test]
fn the_view_keeps_its_scroll_as_the_editor_opens_and_closes() {
    let lines: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
    let mut harness = harness_on(snapshot("long.txt", 3_000, Content::Text(lines)), None);
    let pos = harness.get_by_label("1 line 0").rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -1_700.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    for _ in 0..30 {
        harness.step();
    }
    let label = "110 line 109";
    let before = harness.get_by_label(label).rect().top();

    let line_height = harness.get_by_label("111 line 110").rect().height();
    let pos = text_cell(&mut harness, label, 2);
    click_at(&mut harness, pos);
    let editor = harness
        .get_by(|node| format!("{:?}", node.role()) == "MultilineTextInput")
        .rect();
    let editing = editor.top() + 109.0 * line_height;

    harness.key_press(egui::Key::Escape);
    harness.run();
    let after = harness.get_by_label(label).rect().top();

    assert!(
        (editing - before).abs() <= 1.0,
        "the clicked line must stay put as the editor opens: {before} → {editing}"
    );
    assert!(
        (after - before).abs() <= 1.0,
        "…and as it closes: {before} → {after}"
    );
}

#[test]
fn a_refused_write_offers_reload_and_overwrite() {
    let refused = EditRequest {
        path: "src/main.rs".to_owned(),
        range: 0..3,
        original: LINES.iter().map(|line| (*line).to_owned()).collect(),
        replacement: "changed".to_owned(),
        stage_after: false,
        whole_file: false,
        force: false,
    };
    let mut overwrite = main_rs();
    overwrite.state_mut().view.edit_diverged(refused.clone());
    overwrite.run();
    overwrite.get_by_label("Overwrite").click();
    overwrite.run();
    assert_eq!(
        writes(&overwrite),
        vec![EditRequest {
            force: true,
            ..refused.clone()
        }]
    );

    let mut reload = main_rs();
    reload.state_mut().view.edit_diverged(refused);
    reload.run();
    reload.get_by_label("Reload").click();
    reload.run();
    assert!(reload
        .state()
        .out
        .git
        .contains(&GitIntent::OpenFile("src/main.rs".to_owned())));
    assert!(reload.query_by_label("Overwrite").is_none(), "answered");
}

#[test]
fn the_read_of_what_was_written_replaces_it_without_a_change() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "one\ntwo\n").unwrap();
    let read = |root: &std::path::Path| match content::read(root, "a.txt", None) {
        content::ReadOutcome::Read(snapshot) => snapshot,
        content::ReadOutcome::Unchanged => unreachable!("no stamp was known"),
    };
    let mut harness = harness_on(read(tmp.path()), None);
    let pos = text_cell(&mut harness, "2 two", 3);
    click_at(&mut harness, pos);
    type_text(&mut harness, "!");
    let pos = harness.get_by_label("8 B").rect().center();
    click_at(&mut harness, pos);
    harness.get_by_label("2 two!");

    std::fs::write(tmp.path().join("a.txt"), "one\ntwo!\n").unwrap();
    harness.state_mut().snapshot = read(tmp.path());
    harness.run();

    harness.get_by_label("2 two!");
    let pos = text_cell(&mut harness, "2 two!", 4);
    click_at(&mut harness, pos);
    assert_eq!(
        harness
            .state()
            .view
            .editor()
            .map(|edit| edit.target.original.clone()),
        Some(vec!["one".to_owned(), "two!".to_owned()]),
        "the next editor opens on what the disk now holds"
    );
}

fn note_on(path: &str, line: u32, code: &str, note: &str) -> FileComments {
    let mut comments = FileComments::new();
    helm::review::add_comment(
        &mut comments,
        path,
        LineComment {
            old_lineno: None,
            new_lineno: Some(line),
            code: code.to_owned(),
            note: note.to_owned(),
        },
    );
    comments
}

fn notes_on_main_rs(comments: FileComments) -> Harness<'static, Viewer> {
    harness_with_notes(snapshot("src/main.rs", 26, text(&LINES)), None, comments)
}

fn reviews<'a>(harness: &'a Harness<'_, Viewer>) -> &'a [ReviewIntent] {
    &harness.state().out.review
}

fn type_note(harness: &mut Harness<'_, Viewer>, note: &str) {
    harness
        .get_by(|node| format!("{:?}", node.role()) == "MultilineTextInput")
        .type_text(note);
    harness.run();
}

fn press_enter(harness: &mut Harness<'_, Viewer>, modifiers: egui::Modifiers) {
    harness.event(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    harness.run();
}

/// Opens the note editor under line `line` (1-based) through its gutter button.
fn open_note(harness: &mut Harness<'_, Viewer>, line: usize) {
    harness
        .get_all_by_label("Comment line")
        .nth(line - 1)
        .unwrap()
        .click();
    harness.run();
}

fn sparkles_painted(harness: &Harness<'_, Viewer>) -> usize {
    let glyph = lucide_icons::Icon::Sparkles.unicode().to_string();
    harness
        .output()
        .shapes
        .iter()
        .filter(|clipped| matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.job.text == glyph))
        .count()
}

#[test]
fn hovering_a_line_shows_its_note_button() {
    let mut harness = main_rs();
    assert_eq!(sparkles_painted(&harness), 0, "no button at rest");

    let pos = text_cell(&mut harness, "2     run();", 2);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();

    assert_eq!(
        sparkles_painted(&harness),
        1,
        "the hovered line's button only"
    );
}

#[test]
fn enter_in_the_note_editor_saves_it_on_the_working_tree_line() {
    let mut harness = main_rs();
    open_note(&mut harness, 2);
    assert!(harness.state().view.note_editing());

    type_note(&mut harness, "rename run");
    press_enter(&mut harness, egui::Modifiers::NONE);

    assert_eq!(
        reviews(&harness),
        [ReviewIntent::SaveComment {
            pool: ReviewPool::Agent,
            file: "src/main.rs".to_owned(),
            comment: LineComment {
                old_lineno: None,
                new_lineno: Some(2),
                code: "    run();".to_owned(),
                note: "rename run".to_owned(),
            },
        }],
        "a queued note, anchored as a WIP diff note on an added line, nothing sent"
    );
    assert!(!harness.state().view.note_editing());
}

#[test]
fn a_saved_note_is_a_card_under_its_line_that_pushes_the_next_one_down() {
    let harness = notes_on_main_rs(note_on("src/main.rs", 2, "    run();", "rename run"));

    let line = harness.get_by_label("2     run();").rect();
    let card = harness.get_by_label("Edit review note").rect();
    let next = harness.get_by_label("3 }").rect();
    assert!(card.top() >= line.bottom(), "{card:?} under {line:?}");
    assert!(next.top() >= card.bottom(), "{next:?} under {card:?}");
    assert!(harness
        .query_all_by_label_contains("rename run")
        .next()
        .is_some());
}

#[test]
fn clicking_a_card_edits_its_note_and_cmd_enter_saves_then_sends() {
    let mut harness = notes_on_main_rs(note_on("src/main.rs", 2, "    run();", "rename run"));
    harness.get_by_label("Edit review note").click();
    harness.run();

    press_enter(&mut harness, egui::Modifiers::COMMAND);

    let reviews = reviews(&harness);
    assert!(
        matches!(
            reviews,
            [ReviewIntent::SaveComment { comment, .. }, ReviewIntent::SendToAgent]
                if comment.note == "rename run" && comment.new_lineno == Some(2)
        ),
        "the editor opens on the saved note, ⌘↵ saves it then sends: {reviews:?}"
    );
}

#[test]
fn esc_closes_the_note_editor_then_the_viewer() {
    let mut harness = main_rs();
    open_note(&mut harness, 1);
    type_note(&mut harness, "dropped");

    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.run();

    assert!(
        !harness.state().view.note_editing(),
        "the first Esc closes the note"
    );
    assert!(!harness.state().closed, "…without closing the viewer");
    assert!(reviews(&harness).is_empty(), "nothing is saved");

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().closed);
}

#[test]
fn the_recap_chip_lists_the_whole_batch_and_sends_it() {
    let mut comments = note_on("src/main.rs", 2, "    run();", "rename run");
    helm::review::add_comment(
        &mut comments,
        "src/lib.rs",
        LineComment {
            old_lineno: None,
            new_lineno: Some(9),
            code: "pub fn run() {}".to_owned(),
            note: "from the diff".to_owned(),
        },
    );
    let mut harness = notes_on_main_rs(comments);

    harness.get_by_label("Review notes").click();
    harness.run();
    for note in ["rename run", "from the diff"] {
        assert!(
            harness.query_all_by_label_contains(note).next().is_some(),
            "the popover lists {note:?}"
        );
    }
    harness.get_by_label("Send to claude").click();
    harness.run();

    assert_eq!(reviews(&harness), [ReviewIntent::SendToAgent]);
}

#[test]
fn notes_are_hidden_while_the_whole_file_editor_is_open() {
    let mut harness = notes_on_main_rs(note_on("src/main.rs", 2, "    run();", "rename run"));
    harness.get_by_label("Edit review note");

    let pos = text_cell(&mut harness, "1 fn main() {", 0);
    click_at(&mut harness, pos);

    assert!(harness.state().view.is_editing());
    assert!(harness.query_by_label("Edit review note").is_none());
    assert!(harness.query_by_label("Comment line").is_none());
}

#[test]
fn below_a_note_far_down_a_click_still_lands_on_its_line() {
    let lines: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
    let comments = note_on("long.txt", 150, "line 149", "look here");
    let mut harness = harness_with_notes(
        snapshot("long.txt", 3_000, Content::Text(lines)),
        None,
        comments,
    );
    let pos = harness.get_by_label("1 line 0").rect().center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -2_400.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    for _ in 0..30 {
        harness.step();
    }
    let card = harness.get_by_label("Edit review note").rect();
    let below = harness.get_by_label("152 line 151").rect();
    assert!(below.top() >= card.bottom(), "{below:?} under {card:?}");

    let pos = text_cell(&mut harness, "152 line 151", 2);
    click_at(&mut harness, pos);
    type_text(&mut harness, "X");

    let buffer = buffer(&harness).expect("editor open");
    assert_eq!(buffer.lines().nth(151), Some("liXne 151"));
}
