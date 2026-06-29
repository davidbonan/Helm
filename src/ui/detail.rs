//! Shared leaf primitives for the right-hand detail panels — the commit-detail
//! sidebar (git.md §9) and the PR review rail (pull-requests.md §11) — so both
//! render the author avatar and the count chip in one visual language.

use crate::pull_requests::model::{SnippetKind, SnippetLine};
use crate::theme::{Palette, RADIUS_PILL};
use crate::ui::graph_view::initials;
use crate::ui::syntax_highlight::{highlight_buffer, HighlightedSpan};
use crate::ui::with_alpha;

const AVATAR_SIZE: f32 = 26.0;
const AVATAR_SIZE_SMALL: f32 = 20.0;
const AVATAR_INITIALS_SIZE: f32 = 10.5;
const AVATAR_INITIALS_SIZE_SMALL: f32 = 9.0;
const COUNT_CHIP_SIZE: f32 = 11.0;
const SNIPPET_TEXT_SIZE: f32 = 11.5;
const SNIPPET_NUM_SIZE: f32 = 10.5;
const SNIPPET_NUM_CHAR_W: f32 = 6.5;

/// Avatar dot: the author's initials (same rules as the graph bubble) on a
/// stable color derived from the name — drawn from the lane palette, whose
/// `lane_node_text` ink is already designed for this background.
pub fn author_avatar(ui: &mut egui::Ui, palette: &Palette, author: &str) {
    avatar(ui, palette, author, AVATAR_SIZE, AVATAR_INITIALS_SIZE);
}

/// A lighter avatar for a thread reply, so the root reads as the head of the thread
/// and the answers below it sit a notch quieter (pull-requests.md §11).
pub fn author_avatar_small(ui: &mut egui::Ui, palette: &Palette, author: &str) {
    avatar(
        ui,
        palette,
        author,
        AVATAR_SIZE_SMALL,
        AVATAR_INITIALS_SIZE_SMALL,
    );
}

fn avatar(ui: &mut egui::Ui, palette: &Palette, author: &str, size: f32, initials_size: f32) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let hash = author.bytes().fold(0usize, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, palette.lane_color(hash));
    let text = initials(author);
    if !text.is_empty() {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &text,
            egui::FontId::proportional(initials_size),
            palette.lane_node_text,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
}

/// A few lines of the code a comment was left on (pull-requests.md §5), as a compact
/// monospace block embedded in a comment card: a right-aligned line-number gutter, the
/// +/- sign and the text, tinted green/red/neutral by add/delete/context — the diff's
/// own grammar shrunk to fit. The code text is syntax-highlighted from `path`'s language
/// (the +/- sign keeps the kind colour). Returns the block's response so the center card
/// can use it as the "open in diff" click target. Assumes a non-empty `lines`.
pub fn code_snippet(
    ui: &mut egui::Ui,
    palette: &Palette,
    path: &str,
    lines: &[SnippetLine],
) -> egui::Response {
    let digits = lines
        .iter()
        .filter_map(|l| l.new_no.or(l.old_no))
        .map(|n| n.to_string().len())
        .max()
        .unwrap_or(1);
    let gutter_w = digits as f32 * SNIPPET_NUM_CHAR_W;
    let mono = egui::FontId::monospace(SNIPPET_TEXT_SIZE);
    let num_font = egui::FontId::monospace(SNIPPET_NUM_SIZE);
    let highlights = cached_snippet_highlights(ui.ctx(), palette, path, lines);
    egui::Frame::new()
        .fill(palette.bg_canvas)
        .stroke(egui::Stroke::new(1.0, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            let row_h = SNIPPET_TEXT_SIZE + 4.0;
            for (i, line) in lines.iter().enumerate() {
                let (bg, fg, sign) = match line.kind {
                    SnippetKind::Added => {
                        (with_alpha(palette.git_added, 32), palette.git_added, '+')
                    }
                    SnippetKind::Deleted => (
                        with_alpha(palette.git_deleted, 32),
                        palette.git_deleted,
                        '-',
                    ),
                    SnippetKind::Context => (egui::Color32::TRANSPARENT, palette.text_muted, ' '),
                };
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Sense::hover(),
                );
                if bg != egui::Color32::TRANSPARENT {
                    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, bg);
                }
                let center_y = rect.center().y;
                if let Some(n) = line.new_no.or(line.old_no) {
                    ui.painter().text(
                        egui::pos2(rect.left() + gutter_w, center_y),
                        egui::Align2::RIGHT_CENTER,
                        n.to_string(),
                        num_font.clone(),
                        palette.text_muted,
                    );
                }
                let code_left = rect.left() + gutter_w + 8.0;
                let code_rect = egui::Rect::from_min_max(
                    egui::pos2(code_left, rect.top()),
                    rect.right_bottom(),
                );
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &format!("{sign} "),
                    0.0,
                    egui::text::TextFormat::simple(mono.clone(), fg),
                );
                match highlights.as_deref().and_then(|h| h.get(i)) {
                    Some(spans) if !spans.is_empty() => {
                        for span in spans {
                            job.append(
                                &span.text,
                                0.0,
                                egui::text::TextFormat::simple(mono.clone(), span.color),
                            );
                        }
                    }
                    _ => job.append(
                        &line.text,
                        0.0,
                        egui::text::TextFormat::simple(mono.clone(), fg),
                    ),
                }
                let galley = ui.painter().layout_job(job);
                ui.painter().with_clip_rect(code_rect).galley(
                    egui::pos2(code_left, center_y - galley.size().y / 2.0),
                    galley,
                    fg,
                );
            }
        })
        .response
}

/// [`snippet_highlights`] memoized in egui memory (keyed by path + theme + the snippet
/// text): syntect rebuilds its highlighter and re-parses on every call (~1ms for a
/// known-extension snippet), so a handful of inline-comment cards re-highlighting each
/// frame dropped frames while scrolling the PR overview. Mirrors `HighlightedDiffCache`,
/// which already spares the full diff this cost. The `None` outcome is cached too.
fn cached_snippet_highlights(
    ctx: &egui::Context,
    palette: &Palette,
    path: &str,
    lines: &[SnippetLine],
) -> Option<Vec<Vec<HighlightedSpan>>> {
    let id = snippet_cache_id(palette, path, lines);
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<Vec<Vec<HighlightedSpan>>>>(id)) {
        return cached;
    }
    let highlights = snippet_highlights(palette, path, lines);
    ctx.data_mut(|d| d.insert_temp(id, highlights.clone()));
    highlights
}

/// The memory key for a snippet's highlights — the syntax (`path` extension), theme and
/// the line texts that the spans depend on.
fn snippet_cache_id(palette: &Palette, path: &str, lines: &[SnippetLine]) -> egui::Id {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    palette.syntax.hash(&mut hasher);
    for line in lines {
        line.text.hash(&mut hasher);
    }
    egui::Id::new(("snippet_highlights", hasher.finish()))
}

/// Per-line syntax spans for a snippet, from `path`'s language and the palette's theme.
/// `None` when the path has no known syntax (the caller renders the code flat). The line
/// count is verified to match so the painter can index spans by row.
fn snippet_highlights(
    palette: &Palette,
    path: &str,
    lines: &[SnippetLine],
) -> Option<Vec<Vec<HighlightedSpan>>> {
    if lines.is_empty() {
        return None;
    }
    let joined = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let spans = highlight_buffer(path, palette.syntax, &joined)?;
    (spans.len() == lines.len()).then_some(spans)
}

/// Bordered count chip beside a section title (e.g. the "Files changed" band).
pub fn count_chip(ui: &mut egui::Ui, palette: &Palette, count: usize) {
    let text = count.to_string();
    let galley = ui.painter().layout_no_wrap(
        text.clone(),
        egui::FontId::proportional(COUNT_CHIP_SIZE),
        egui::Color32::PLACEHOLDER,
    );
    let size = galley.size() + egui::vec2(10.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        palette.bg_surface,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_line(no: u32, text: &str) -> SnippetLine {
        SnippetLine {
            old_no: None,
            new_no: Some(no),
            kind: SnippetKind::Context,
            text: text.to_owned(),
        }
    }

    #[test]
    fn cached_snippet_highlights_matches_uncached_and_serves_from_memory() {
        let ctx = egui::Context::default();
        let palette = Palette::dark();
        let path = "src/lib.rs";
        let lines = vec![
            ctx_line(1, "fn main() {"),
            ctx_line(2, "    let answer = 42;"),
            ctx_line(3, "}"),
        ];

        let direct = snippet_highlights(&palette, path, &lines);
        assert!(direct.is_some(), "Rust is highlighted by the syntax set");
        assert_eq!(
            cached_snippet_highlights(&ctx, &palette, path, &lines),
            direct
        );

        // A later frame must read egui memory, not re-run syntect: poison the slot and
        // check the poisoned value comes back instead of a fresh highlight.
        let id = snippet_cache_id(&palette, path, &lines);
        let poison: Option<Vec<Vec<HighlightedSpan>>> = Some(vec![vec![HighlightedSpan {
            text: "poison".to_owned(),
            color: egui::Color32::RED,
        }]]);
        ctx.data_mut(|d| d.insert_temp(id, poison.clone()));
        assert_eq!(
            cached_snippet_highlights(&ctx, &palette, path, &lines),
            poison
        );
    }
}
