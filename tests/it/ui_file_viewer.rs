use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::files::content::{self, Content, FileSnapshot};
use helm::files::file_type::FileType;
use helm::git::diff::ImageBlob;
use helm::git::edit::EditRequest;
use helm::git::status::ChangeKind;
use helm::theme::Palette;
use helm::ui::file_viewer::{file_viewer, FileViewerState, ViewedFile};
use helm::ui::git_panel::{EditRefusal, GitIntent};

/// The viewer over `snapshot`, what it emitted, and whether closing was asked.
struct Viewer {
    snapshot: FileSnapshot,
    change: Option<ChangeKind>,
    view: FileViewerState,
    intents: Vec<GitIntent>,
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
    let palette = Palette::dark();
    let mut harness = Harness::new_ui_state(
        move |ui, viewer: &mut Viewer| {
            let file = ViewedFile {
                palette: &palette,
                snapshot: &viewer.snapshot,
                change: viewer.change,
            };
            viewer.closed |= file_viewer(ui, &file, &mut viewer.view, &mut viewer.intents);
        },
        Viewer {
            snapshot,
            change,
            view: FileViewerState::default(),
            intents: Vec::new(),
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
    // Three-digit gutter, its padding, then the content's own.
    let content_left = row.left() + 3.0 * char_w + 12.0 + 8.0;
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
        .intents
        .iter()
        .filter_map(|intent| match intent {
            GitIntent::FlushEdit(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

fn refusals(harness: &Harness<'_, Viewer>) -> Vec<EditRefusal> {
    harness
        .state()
        .intents
        .iter()
        .filter_map(|intent| match intent {
            GitIntent::EditRefused { reason, .. } => Some(*reason),
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
fn cmd_e_opens_the_editor_on_the_hovered_line() {
    let mut harness = main_rs();
    let pos = text_cell(&mut harness, "2     run();", 7);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::E);
    harness.run();
    type_text(&mut harness, "X");

    assert_eq!(
        buffer(&harness).as_deref(),
        Some("fn main() {\nX    run();\n}"),
        "the caret opens at the start of the hovered line"
    );
}

#[test]
fn a_file_that_cannot_be_edited_takes_no_caret_and_cmd_e_names_why() {
    let read_only = FileSnapshot {
        writable: false,
        ..snapshot("src/main.rs", 26, text(&LINES))
    };
    let long: Vec<String> = (0..3_001).map(|i| format!("line {i}")).collect();
    let cases = [
        (read_only, Some("1 fn main() {"), EditRefusal::ReadOnly),
        (
            snapshot("blob.bin", 3, Content::Binary),
            None,
            EditRefusal::File,
        ),
        (snapshot("empty.txt", 0, text(&[])), None, EditRefusal::File),
        (
            snapshot("long.txt", 30_000, Content::Text(long)),
            Some("1 line 0"),
            EditRefusal::FileTooLong,
        ),
    ];
    for (snapshot, first_row, reason) in cases {
        let path = snapshot.path.clone();
        let mut harness = harness_on(snapshot, None);
        if let Some(label) = first_row {
            let pos = text_cell(&mut harness, label, 3);
            click_at(&mut harness, pos);
        }

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::E);
        harness.run();

        assert!(!harness.state().view.is_editing(), "{path}: no caret");
        assert_eq!(refusals(&harness), vec![reason], "{path}");
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
        .intents
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
