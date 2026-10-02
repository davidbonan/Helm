//! Color emoji in egui text: egui fonts only carry monochrome glyphs, so the bitmap of
//! an emoji is painted over its glyph, itself hidden. [`install`] does it for every
//! text of the app; [`append_text`] + [`paint_emoji`] also drop the variation
//! selectors, for the text a view lays out itself.

use std::collections::HashMap;
use std::sync::Arc;

use egui::emath::GuiRounding as _;
use egui::epaint::text::{Glyph, Row};
use egui::epaint::TextShape;
use egui::layers::ShapeIdx;
use egui::text::{LayoutJob, TextFormat};

use crate::emoji::{emoji_cluster, has_color_glyph, has_emoji_presentation, is_variation_selector};

/// Bitmap of `cluster` fitted to a pixel box, uploaded once and cached in egui memory
/// (keyed per cluster and box). `None` — no color glyph — is cached too.
pub fn emoji_texture(
    ctx: &egui::Context,
    cluster: &str,
    box_px: [usize; 2],
) -> Option<egui::TextureId> {
    let id = egui::Id::new(("emoji", cluster, box_px));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return cached.map(|texture| texture.id());
    }
    let handle = crate::emoji::rasterize(cluster, box_px[0], box_px[1]).map(|bitmap| {
        let image = egui::ColorImage::from_rgba_premultiplied(
            [bitmap.width, bitmap.height],
            &bitmap.rgba_premultiplied,
        );
        ctx.load_texture(
            format!("emoji-{cluster}"),
            image,
            egui::TextureOptions::LINEAR,
        )
    });
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle.map(|texture| texture.id())
}

/// `LayoutJob::append` for text that may hold emoji: each one lands in a transparent
/// section of its own — the placeholder [`paint_emoji`] covers — and the variation
/// selectors, which egui would paint as a missing-glyph box, are dropped.
pub fn append_text(job: &mut LayoutJob, text: &str, format: TextFormat) {
    let placeholder = TextFormat {
        color: egui::Color32::TRANSPARENT,
        ..format.clone()
    };
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if is_variation_selector(c) {
            continue;
        }
        let selector = chars.next_if(|next| is_variation_selector(*next));
        if !is_color_emoji(c, selector) {
            plain.push(c);
            continue;
        }
        job.append(&std::mem::take(&mut plain), 0.0, format.clone());
        job.append(c.encode_utf8(&mut [0; 4]), 0.0, placeholder.clone());
    }
    job.append(&plain, 0.0, format);
}

fn is_color_emoji(base: char, selector: Option<char>) -> bool {
    has_emoji_presentation(base, selector.as_slice()) && has_color_glyph(&emoji_cluster(base))
}

/// Paints the emoji of a galley built with [`append_text`], drawn at `origin`: a square
/// of one em at most, centered on each placeholder glyph.
pub fn paint_emoji(painter: &egui::Painter, origin: egui::Pos2, galley: &egui::Galley) {
    let pixels_per_point = painter.pixels_per_point();
    let mut chars = galley.job.text.char_indices();
    for row in &galley.rows {
        for glyph in &row.glyphs {
            // An elided tail no longer follows the text: nothing past it is an emoji.
            let Some((byte, _)) = chars.next().filter(|(_, c)| *c == glyph.chr) else {
                return;
            };
            let Some(font_size) = placeholder_font_size(&galley.job, byte) else {
                continue;
            };
            // The font's own glyph for an emoji can be narrower than its em (U+2764).
            let side_px = (font_size.min(glyph.advance_width) * pixels_per_point).round();
            let cluster = emoji_cluster(glyph.chr);
            let Some(texture) = emoji_texture(painter.ctx(), &cluster, [side_px as usize; 2])
            else {
                continue;
            };
            let center = origin + row.pos.to_vec2() + glyph.logical_rect().center().to_vec2();
            painter.image(
                texture,
                egui::Rect::from_center_size(center, egui::Vec2::splat(side_px / pixels_per_point))
                    .round_to_pixels(pixels_per_point),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        if row.ends_with_newline {
            chars.next();
        }
    }
}

fn placeholder_font_size(job: &LayoutJob, byte: usize) -> Option<f32> {
    if job.text.as_bytes()[byte].is_ascii() {
        return None;
    }
    format_at(job, byte)
        .filter(|format| format.color == egui::Color32::TRANSPARENT)
        .map(|format| format.font_id.size)
}

/// Colors the emoji of every text painted in `ctx`, whatever widget laid it out.
pub fn install(ctx: &egui::Context) {
    ctx.add_plugin(ColorEmoji::default());
}

/// An emoji and the side, in pixels, of the square it is drawn in.
type SpriteKey = (char, usize);

#[derive(Default)]
struct ColorEmoji {
    sprites: HashMap<SpriteKey, Option<egui::TextureId>>,
}

impl egui::plugin::Plugin for ColorEmoji {
    fn debug_name(&self) -> &'static str {
        "color_emoji"
    }

    fn on_end_pass(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx();
        let missing = self.colorize_painted_text(ctx);
        if missing.is_empty() {
            return;
        }
        for (base, side_px) in missing {
            let texture = emoji_texture(ctx, &emoji_cluster(base), [side_px; 2]);
            self.sprites.insert((base, side_px), texture);
        }
        self.colorize_painted_text(ctx);
    }
}

impl ColorEmoji {
    /// Returns the sprites the pass lacked: a texture cannot be uploaded while the
    /// painted shapes are borrowed, so their emoji wait for a second pass.
    fn colorize_painted_text(&self, ctx: &egui::Context) -> Vec<SpriteKey> {
        let layers: Vec<egui::LayerId> = ctx.memory(|memory| {
            std::iter::once(egui::LayerId::background())
                .chain(memory.layer_ids())
                .collect()
        });
        let mut pass = ColorizePass {
            sprites: &self.sprites,
            pixels_per_point: ctx.pixels_per_point(),
            missing: Vec::new(),
        };
        ctx.graphics_mut(|graphics| {
            for layer in layers {
                let Some(shapes) = graphics.get_mut(layer) else {
                    continue;
                };
                for idx in 0..shapes.next_idx().0 {
                    shapes.mutate_shape(ShapeIdx(idx), |clipped| pass.colorize(&mut clipped.shape));
                }
            }
        });
        pass.missing
    }
}

struct ColorizePass<'a> {
    sprites: &'a HashMap<SpriteKey, Option<egui::TextureId>>,
    pixels_per_point: f32,
    missing: Vec<SpriteKey>,
}

/// An emoji glyph of a galley, and the selector glyph after it if any.
struct EmojiGlyph {
    row: usize,
    base: Glyph,
    selector: Option<Glyph>,
    font_size: f32,
}

impl EmojiGlyph {
    /// What the layout gave the emoji: its glyph, plus the box egui lays a selector as.
    fn slot_width(&self) -> f32 {
        self.base.advance_width + self.selector.map_or(0.0, |glyph| glyph.advance_width)
    }
}

impl ColorizePass<'_> {
    fn colorize(&mut self, shape: &mut egui::Shape) {
        let sprites = match shape {
            egui::Shape::Vec(shapes) => {
                shapes.iter_mut().for_each(|shape| self.colorize(shape));
                return;
            }
            egui::Shape::Text(text) if text.angle == 0.0 => self.colorize_text(text),
            _ => return,
        };
        if sprites.is_empty() {
            return;
        }
        let text = std::mem::replace(shape, egui::Shape::Noop);
        *shape = egui::Shape::Vec(std::iter::once(text).chain(sprites).collect());
    }

    /// Hides the emoji glyphs of `text` and returns the sprites to paint over them.
    fn colorize_text(&mut self, text: &mut TextShape) -> Vec<egui::Shape> {
        let mut drawn = Vec::new();
        for emoji in emoji_glyphs(&text.galley) {
            let side_px = (emoji.font_size.min(emoji.slot_width()) * self.pixels_per_point).round();
            let key = (emoji.base.chr, side_px as usize);
            match self.sprites.get(&key) {
                Some(Some(texture)) => drawn.push((emoji, *texture, side_px)),
                Some(None) => {}
                None => self.missing.push(key),
            }
        }
        if drawn.is_empty() {
            return Vec::new();
        }
        let tint = egui::Color32::WHITE.gamma_multiply(text.opacity_factor);
        let galley = Arc::make_mut(&mut text.galley);
        drawn
            .into_iter()
            .map(|(emoji, texture, side_px)| {
                let placed = &mut galley.rows[emoji.row];
                let slot = emoji.base.logical_rect();
                let center = text.pos
                    + placed.pos.to_vec2()
                    + egui::vec2(slot.left() + emoji.slot_width() / 2.0, slot.center().y);
                let row = Arc::make_mut(&mut placed.row);
                hide_glyph(row, &emoji.base);
                if let Some(selector) = &emoji.selector {
                    hide_glyph(row, selector);
                }
                egui::Shape::image(
                    texture,
                    egui::Rect::from_center_size(
                        center,
                        egui::Vec2::splat(side_px / self.pixels_per_point),
                    )
                    .round_to_pixels(self.pixels_per_point),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    tint,
                )
            })
            .collect()
    }
}

fn emoji_glyphs(galley: &egui::Galley) -> Vec<EmojiGlyph> {
    let mut found = Vec::new();
    let mut chars = galley.job.text.char_indices();
    for (row_idx, row) in galley.rows.iter().enumerate() {
        for (idx, base) in row.glyphs.iter().enumerate() {
            // An elided tail no longer follows the text: nothing past it is an emoji.
            let Some((byte, _)) = chars.next().filter(|(_, c)| *c == base.chr) else {
                return found;
            };
            if base.chr.is_ascii() || is_hidden(row, base) {
                continue;
            }
            let selector = row
                .glyphs
                .get(idx + 1)
                .filter(|next| is_variation_selector(next.chr));
            if !is_color_emoji(base.chr, selector.map(|glyph| glyph.chr)) {
                continue;
            }
            if let Some(format) = format_at(&galley.job, byte) {
                found.push(EmojiGlyph {
                    row: row_idx,
                    base: *base,
                    selector: selector.copied(),
                    font_size: format.font_id.size,
                });
            }
        }
        if row.ends_with_newline {
            chars.next();
        }
    }
    found
}

fn format_at(job: &LayoutJob, byte: usize) -> Option<&TextFormat> {
    job.sections
        .iter()
        .find(|section| section.byte_range.contains(&byte))
        .map(|section| &section.format)
}

/// The four vertices of a glyph's quad; a glyph without ink has none.
fn glyph_quad(glyph: &Glyph) -> Option<std::ops::Range<usize>> {
    let first = glyph.first_vertex as usize;
    (!glyph.uv_rect.is_nothing()).then_some(first..first + 4)
}

/// Not painted: no ink, a transparent placeholder ([`append_text`]) or already hidden.
fn is_hidden(row: &Row, glyph: &Glyph) -> bool {
    glyph_quad(glyph)
        .and_then(|quad| row.visuals.mesh.vertices.get(quad))
        .is_none_or(|quad| quad[0].color.a() == 0 || quad[0].pos == quad[3].pos)
}

fn hide_glyph(row: &mut Row, glyph: &Glyph) {
    let Some(quad) = glyph_quad(glyph).and_then(|quad| row.visuals.mesh.vertices.get_mut(quad))
    else {
        return;
    };
    let collapsed = quad[0].pos;
    quad.iter_mut().for_each(|vertex| vertex.pos = collapsed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_of(text: &str) -> LayoutJob {
        let mut job = LayoutJob::default();
        let format = TextFormat::simple(egui::FontId::proportional(14.0), egui::Color32::WHITE);
        append_text(&mut job, text, format);
        job
    }

    fn placeholders(job: &LayoutJob) -> Vec<&str> {
        job.sections
            .iter()
            .filter(|section| section.format.color == egui::Color32::TRANSPARENT)
            .map(|section| &job.text[section.byte_range.clone()])
            .collect()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_emoji_becomes_a_placeholder_and_its_selector_is_dropped() {
        let job = job_of("a \u{26A0}\u{FE0F} b \u{1F4A1}c");

        assert_eq!(job.text, "a \u{26A0} b \u{1F4A1}c");
        assert_eq!(placeholders(&job), ["\u{26A0}", "\u{1F4A1}"]);
    }

    /// The sprites of one frame painting `text` as a label, the plugin installed.
    fn label_sprites(text: &'static str) -> usize {
        let ctx = egui::Context::default();
        install(&ctx);
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.label(text);
        });
        output
            .shapes
            .iter()
            .map(|clipped| sprite_count(&clipped.shape))
            .sum()
    }

    fn sprite_count(shape: &egui::Shape) -> usize {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().map(sprite_count).sum(),
            egui::Shape::Mesh(mesh) => usize::from(mesh.texture_id != Default::default()),
            _ => 0,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn any_painted_text_gets_a_sprite_per_emoji() {
        assert_eq!(
            label_sprites("done \u{2705} \u{26A0}\u{FE0F} ship \u{1F680}"),
            3
        );
    }

    #[test]
    fn painted_text_without_emoji_gets_no_sprite() {
        assert_eq!(label_sprites("plain \u{26A0} text"), 0);
    }

    #[test]
    fn text_presentation_stays_a_font_glyph() {
        let job = job_of("\u{26A0} \u{2705}\u{FE0E} ok");

        assert_eq!(job.text, "\u{26A0} \u{2705} ok");
        assert!(placeholders(&job).is_empty());
    }
}
