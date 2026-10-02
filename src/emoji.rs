//! Color emoji (terminal.md §4, design-system.md §2): which characters ask for one,
//! and its bitmap. egui's font backend only rasterizes monochrome outlines, so the
//! system emoji font is drawn through CoreText instead.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

const TEXT_PRESENTATION_SELECTOR: char = '\u{FE0E}';
const EMOJI_PRESENTATION_SELECTOR: char = '\u{FE0F}';

pub fn is_variation_selector(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}')
}

/// Whether a character is to be drawn as a color emoji: a variation selector among
/// its combining marks decides, else its default presentation.
pub fn has_emoji_presentation(base: char, marks: &[char]) -> bool {
    if marks.contains(&TEXT_PRESENTATION_SELECTOR) {
        return false;
    }
    marks.contains(&EMOJI_PRESENTATION_SELECTOR) || is_emoji_by_default(base)
}

/// `base` forced to its emoji presentation, as the rasterizer takes it.
pub fn emoji_cluster(base: char) -> String {
    String::from_iter([base, EMOJI_PRESENTATION_SELECTOR])
}

/// Whether the system emoji font draws `cluster` — remembered, the answer is asked
/// for every emoji of every frame.
pub fn has_color_glyph(cluster: &str) -> bool {
    static KNOWN: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let mut known = KNOWN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(&drawable) = known.get(cluster) {
        return drawable;
    }
    let drawable = system_font_draws(cluster);
    known.insert(cluster.to_owned(), drawable);
    drawable
}

#[cfg(target_os = "macos")]
fn system_font_draws(cluster: &str) -> bool {
    core_text_emoji::draws(cluster)
}

#[cfg(not(target_os = "macos"))]
fn system_font_draws(_cluster: &str) -> bool {
    false
}

/// Unicode `Emoji_Presentation` for the BMP; the emoji planes are taken whole —
/// the rasterizer turns down what the emoji font does not carry.
fn is_emoji_by_default(c: char) -> bool {
    matches!(
        c,
        '\u{231A}'..='\u{231B}'
            | '\u{23E9}'..='\u{23EC}'
            | '\u{23F0}'
            | '\u{23F3}'
            | '\u{25FD}'..='\u{25FE}'
            | '\u{2614}'..='\u{2615}'
            | '\u{2648}'..='\u{2653}'
            | '\u{267F}'
            | '\u{2693}'
            | '\u{26A1}'
            | '\u{26AA}'..='\u{26AB}'
            | '\u{26BD}'..='\u{26BE}'
            | '\u{26C4}'..='\u{26C5}'
            | '\u{26CE}'
            | '\u{26D4}'
            | '\u{26EA}'
            | '\u{26F2}'..='\u{26F3}'
            | '\u{26F5}'
            | '\u{26FA}'
            | '\u{26FD}'
            | '\u{2705}'
            | '\u{270A}'..='\u{270B}'
            | '\u{2728}'
            | '\u{274C}'
            | '\u{274E}'
            | '\u{2753}'..='\u{2755}'
            | '\u{2757}'
            | '\u{2795}'..='\u{2797}'
            | '\u{27B0}'
            | '\u{27BF}'
            | '\u{2B1B}'..='\u{2B1C}'
            | '\u{2B50}'
            | '\u{2B55}'
            | '\u{1F000}'..='\u{1FAFF}'
    )
}

/// An emoji drawn to fit a pixel box, centered in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmojiBitmap {
    pub width: usize,
    pub height: usize,
    pub rgba_premultiplied: Vec<u8>,
}

/// `None` when the system emoji font has no color glyph for `cluster`.
#[cfg(target_os = "macos")]
pub fn rasterize(cluster: &str, width: usize, height: usize) -> Option<EmojiBitmap> {
    core_text_emoji::rasterize(cluster, width, height)
}

#[cfg(not(target_os = "macos"))]
pub fn rasterize(_cluster: &str, _width: usize, _height: usize) -> Option<EmojiBitmap> {
    None
}

#[cfg(target_os = "macos")]
mod core_text_emoji {
    use super::EmojiBitmap;
    use core_foundation::attributed_string::CFMutableAttributedString;
    use core_foundation::base::{CFRange, TCFType};
    use core_foundation::string::CFString;
    use core_graphics::base::kCGImageAlphaPremultipliedLast;
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use core_text::font::CTFont;
    use core_text::font_descriptor::kCTFontColorGlyphsTrait;
    use core_text::line::CTLine;
    use core_text::string_attributes::kCTFontAttributeName;

    const EMOJI_FONT: &str = "AppleColorEmoji";

    pub fn draws(cluster: &str) -> bool {
        emoji_line(cluster, 16.0).is_some()
    }

    pub fn rasterize(cluster: &str, width: usize, height: usize) -> Option<EmojiBitmap> {
        if width == 0 || height == 0 {
            return None;
        }
        let mut context = CGContext::create_bitmap_context(
            None,
            width,
            height,
            8,
            width * 4,
            &CGColorSpace::create_device_rgb(),
            kCGImageAlphaPremultipliedLast,
        );
        let probe = emoji_line(cluster, height as f64)?;
        let probe_ink = probe.get_image_bounds(&context).size;
        if probe_ink.width <= 0.0 || probe_ink.height <= 0.0 {
            return None;
        }
        let fit = (width as f64 / probe_ink.width).min(height as f64 / probe_ink.height);
        let line = emoji_line(cluster, height as f64 * fit)?;
        let ink = line.get_image_bounds(&context);
        context.set_text_position(
            (width as f64 - ink.size.width) / 2.0 - ink.origin.x,
            (height as f64 - ink.size.height) / 2.0 - ink.origin.y,
        );
        line.draw(&context);
        Some(EmojiBitmap {
            width,
            height,
            rgba_premultiplied: context.data().to_vec(),
        })
    }

    /// `None` when CoreText falls back to another font for part of the cluster.
    fn emoji_line(cluster: &str, font_size: f64) -> Option<CTLine> {
        let font = core_text::font::new_from_name(EMOJI_FONT, font_size).ok()?;
        let mut string = CFMutableAttributedString::new();
        string.replace_str(&CFString::new(cluster), CFRange::init(0, 0));
        let whole = CFRange::init(0, string.char_len());
        // SAFETY: CoreText constant, valid for the life of the process.
        string.set_attribute(whole, unsafe { kCTFontAttributeName }, &font);
        let line = CTLine::new_with_attributed_string(string.as_concrete_TypeRef());
        is_drawn_in_color(&line).then_some(line)
    }

    fn is_drawn_in_color(line: &CTLine) -> bool {
        let runs = line.glyph_runs();
        !runs.is_empty() && runs.iter().all(|run| run_font_has_color_glyphs(&run))
    }

    fn run_font_has_color_glyphs(run: &core_text::run::CTRun) -> bool {
        // SAFETY: CoreText constant, valid for the life of the process.
        let font_key = unsafe { CFString::wrap_under_get_rule(kCTFontAttributeName) };
        run.attributes()
            .and_then(|attributes| attributes.find(&font_key).map(|font| font.clone()))
            .and_then(|font| font.downcast::<CTFont>())
            .is_some_and(|font| font.symbolic_traits() & kCTFontColorGlyphsTrait != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variation_selector_overrides_the_default_presentation() {
        assert!(has_emoji_presentation('\u{26A0}', &['\u{FE0F}']));
        assert!(!has_emoji_presentation('\u{26A0}', &[]));
        assert!(has_emoji_presentation('\u{2705}', &[]));
        assert!(has_emoji_presentation('\u{1F680}', &[]));
        assert!(!has_emoji_presentation('\u{2705}', &['\u{FE0E}']));
        assert!(!has_emoji_presentation('a', &[]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_what_the_emoji_font_carries_has_a_color_glyph() {
        assert!(has_color_glyph(&emoji_cluster('\u{1F4A1}')));
        assert!(!has_color_glyph(&emoji_cluster('\u{1F000}')));
        assert!(!has_color_glyph("a"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_warning_sign_is_rasterized_in_color_inside_its_box() {
        let bitmap = rasterize("\u{26A0}\u{FE0F}", 32, 36).expect("system emoji font");

        assert_eq!(bitmap.rgba_premultiplied.len(), 32 * 36 * 4);
        let has_yellow_ink = bitmap
            .rgba_premultiplied
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] == 255 && px[0] > 200 && px[2] < 120);
        assert!(has_yellow_ink, "monochrome or empty bitmap");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_the_emoji_font_does_not_cover_is_not_rasterized() {
        assert_eq!(rasterize("\u{4E2D}", 32, 36), None);
    }
}
