use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::files::content::{Content, FileSnapshot};
use helm::files::file_type::FileType;
use helm::git::diff::ImageBlob;
use helm::git::status::ChangeKind;
use helm::theme::Palette;
use helm::ui::file_viewer::{file_viewer, FileViewerState, ViewedFile};

/// The viewer on `content` at `path`; the state is `true` once closing was asked.
fn viewer(
    path: &'static str,
    size: u64,
    content: Content,
    change: Option<ChangeKind>,
) -> Harness<'static, bool> {
    let palette = Palette::dark();
    let snapshot = FileSnapshot {
        path: path.to_owned(),
        size,
        stamp: None,
        content,
    };
    let mut view = FileViewerState::default();
    let mut harness = Harness::new_ui_state(
        move |ui, closed: &mut bool| {
            let file = ViewedFile {
                palette: &palette,
                snapshot: &snapshot,
                change,
            };
            *closed |= file_viewer(ui, &file, &mut view);
        },
        false,
    );
    harness.run();
    harness
}

fn text(lines: &[&str]) -> Content {
    Content::Text(lines.iter().map(|line| (*line).to_owned()).collect())
}

fn copied_text(harness: &Harness<'_, bool>) -> Option<String> {
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
    assert!(*escaped.state());

    let mut clicked = viewer("a.txt", 1, text(&["a"]), None);
    clicked.get_by_label("Close").click();
    clicked.run();
    assert!(*clicked.state());
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
    let row = harness.get_by_label("2     run();").rect();
    let char_w = harness.ctx.fonts_mut(|fonts| {
        fonts
            .glyph_width(&egui::FontId::monospace(12.0), ' ')
            .max(1.0)
    });
    // Three-digit gutter, its padding, then the content's own.
    let content_left = row.left() + 3.0 * char_w + 12.0 + 8.0;
    let pos = egui::pos2(content_left + 6.5 * char_w, row.center().y);
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
