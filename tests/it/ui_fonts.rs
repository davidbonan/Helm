use ab_glyph::Font;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::theme;

#[test]
fn system_fonts_render_the_command_glyph() {
    let mut installed = false;
    let mut harness = Harness::new_ui(move |ui| {
        if !installed {
            theme::install_fonts(ui.ctx());
            installed = true;
        }
        ui.label("⌘1");
    });
    harness.run();

    harness.get_by_label("⌘1");
}

// Glyphs displayed by Claude Code in the terminal: Dingbats spinner, tool-call
// bullet, result connector. Tofu if no font in the mono chain serves them (bug
// report: white squares).
#[test]
fn mono_chain_covers_claude_code_glyphs() {
    let defs = theme::font_definitions();
    let order = &defs.families[&egui::FontFamily::Monospace];
    for ch in "✢✳✶✻✽✘⏺⎿".chars() {
        let covered = order.iter().any(|name| {
            let data = &defs.font_data[name.as_str()];
            ab_glyph::FontRef::try_from_slice_and_index(&data.font, data.index)
                .is_ok_and(|font| font.glyph_id(ch).0 != 0)
        });
        assert!(
            covered,
            "{ch} U+{:04X} without a glyph in the mono chain",
            ch as u32
        );
    }
}
