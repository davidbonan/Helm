//! The in-house markdown renderer (pull-requests.md §11, files.md §4.3): PR bodies,
//! comments and worktree `.md` previews, laid out with their own line-height and
//! letter-spacing.

use std::collections::HashMap;
use std::path::Path;

use lucide_icons::Icon;

use crate::theme::{Palette, RADIUS_BUTTON};
use crate::ui::{paint_icon, with_alpha};

/// Markdown reading tweaks for the in-house renderer (`markdown`): the body reads
/// smaller than the egui_commonmark default but with looser line-height and a touch
/// of letter-spacing, so long prose blocks don't read as a dense wall.
pub(crate) const MD_TEXT_SIZE: f32 = 14.0;
pub(crate) const MD_CODE_SIZE: f32 = 13.0;
const MD_LINE_HEIGHT: f32 = MD_TEXT_SIZE * 1.55;
const MD_LETTER_SPACING: f32 = 0.4;
const MD_PARAGRAPH_GAP: f32 = 7.0;
const MD_LIST_INDENT: f32 = 18.0;
const MD_QUOTE_INDENT: f32 = 12.0;

/// Reading measure of the Conversation tab's **prose** (pull-requests.md §11). The
/// cards themselves span the area's full width — only the text inside them wraps at
/// this measure, so a wide window leaves breathing room *inside* a surface rather than
/// a hole between two floating blocks.
const PROSE_MEASURE: f32 = 900.0;

/// The inline style flags carried while walking the markdown events, so a run can be
/// rendered with the right weight/shape/family.
#[derive(Clone, Copy, Default)]
struct MdInline {
    strong: bool,
    emphasis: bool,
    code: bool,
    strike: bool,
    link: bool,
}

/// A worktree file shown as markdown: its relative links and images lead into the
/// worktree, from its own folder (files.md §4.3).
#[derive(Clone, Copy)]
pub struct WorktreeDoc<'a> {
    pub text: &'a str,
    /// The worktree, absolute.
    pub root: &'a Path,
    /// The document's folder, relative to `root` and `/`-separated (`""` at the root).
    pub dir: &'a str,
}

/// The text to render, and how its link and image URLs turn into what they open.
struct Source<'a> {
    text: &'a str,
    resolve: &'a dyn Fn(&str) -> String,
}

/// Scheme of a target resolved inside the worktree: the image loader reads it from
/// disk, a click opens it in the file viewer.
pub const LOCAL_SCHEME: &str = "file://";

/// One block as drawn: the source lines it was written on (1-based) and where it
/// landed on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct MdBlock {
    pub lines: std::ops::RangeInclusive<u32>,
    pub y: egui::Rangef,
}

/// Renders a PR body / comment as markdown, its URLs as written.
pub fn markdown(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    let resolve = |url: &str| url.to_owned();
    render(
        ui,
        palette,
        Source {
            text,
            resolve: &resolve,
        },
    );
}

/// Whether `event` opens or closes a block a preview can point at: the leaves of the
/// layout, each drawn whole (a list item, not its list).
fn block_edge(event: &pulldown_cmark::Event<'_>) -> Option<bool> {
    use pulldown_cmark::{Event, Tag, TagEnd};
    match event {
        Event::Start(
            Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::CodeBlock(_)
            | Tag::Table(_)
            | Tag::Item
            | Tag::HtmlBlock,
        ) => Some(true),
        Event::End(
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::CodeBlock
            | TagEnd::Table
            | TagEnd::Item
            | TagEnd::HtmlBlock,
        ) => Some(false),
        _ => None,
    }
}

/// The 1-based line of byte `offset`, given where each line starts.
fn line_at(line_starts: &[usize], offset: usize) -> u32 {
    line_starts.partition_point(|start| *start <= offset) as u32
}

/// Renders a worktree file as markdown, its relative links and images resolved to
/// `LOCAL_SCHEME` targets; returns its blocks as drawn.
pub fn worktree_markdown(
    ui: &mut egui::Ui,
    palette: &Palette,
    doc: WorktreeDoc<'_>,
) -> Vec<MdBlock> {
    let resolve = |url: &str| match crate::files::referenced_path(doc.dir, url) {
        Some(relative) => format!("{LOCAL_SCHEME}{}", doc.root.join(relative).display()),
        None => url.to_owned(),
    };
    render(
        ui,
        palette,
        Source {
            text: doc.text,
            resolve: &resolve,
        },
    )
}

/// Unlike `egui_commonmark` (which emits plain `RichText`, so line-height and
/// letter-spacing are fixed at the font row height), this builds the `LayoutJob`s
/// itself — the prose carries a looser line-height and a little letter-spacing, the
/// levers that make a long description or reply readable rather than a dense wall
/// (pull-requests.md §11).
fn render(ui: &mut egui::Ui, palette: &Palette, source: Source<'_>) -> Vec<MdBlock> {
    let Source { text, resolve } = source;
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let mut blocks = Vec::new();
    let mut open_blocks: Vec<f32> = Vec::new();
    use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    // The card around the prose spans its area; the *text* stops at its measure, so a
    // wide window leaves margin inside the surface instead of 150-character lines (§11).
    let max_width = ui.available_width().min(PROSE_MEASURE);
    let new_job = |indent: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = (max_width - indent).max(80.0);
        job
    };
    // Emphasis is carried by the medium face rather than by a brighter ink: prose that
    // only reaches full contrast where it is bold leaves the rest reading as disabled,
    // which is what greys a whole description in dark mode (§11).
    let medium = crate::theme::medium_family(ui.ctx());
    let append = |job: &mut egui::text::LayoutJob,
                  run: &str,
                  s: MdInline,
                  heading: Option<HeadingLevel>,
                  quote: bool| {
        let size = match heading {
            Some(level) => md_heading_size(level),
            None if s.code => MD_CODE_SIZE,
            None => MD_TEXT_SIZE,
        };
        let family = if s.code {
            egui::FontFamily::Monospace
        } else if s.strong || heading.is_some() {
            medium.clone()
        } else {
            egui::FontFamily::Proportional
        };
        let mut color = palette.text_primary;
        if quote {
            color = palette.text_muted;
        }
        if s.link {
            color = palette.accent;
        }
        let format = egui::text::TextFormat {
            font_id: egui::FontId::new(size, family),
            color,
            background: if s.code {
                with_alpha(palette.text_muted, 28)
            } else {
                egui::Color32::TRANSPARENT
            },
            italics: s.emphasis,
            extra_letter_spacing: if s.code { 0.0 } else { MD_LETTER_SPACING },
            line_height: Some(if heading.is_some() {
                size * 1.3
            } else {
                MD_LINE_HEIGHT
            }),
            underline: if s.link {
                egui::Stroke::new(1.0_f32, palette.accent)
            } else {
                egui::Stroke::NONE
            },
            strikethrough: if s.strike {
                egui::Stroke::new(1.0_f32, color)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        };
        crate::ui::emoji::append_text(job, run, format);
    };
    let flush = |ui: &mut egui::Ui,
                 job: &mut egui::text::LayoutJob,
                 links: &mut Vec<MdLink>,
                 indent: f32| {
        if job.text.trim().is_empty() {
            *job = new_job(indent);
            links.clear();
            return;
        }
        let done = std::mem::replace(job, new_job(indent));
        let runs = std::mem::take(links);
        if indent > 0.0 {
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                prose(ui, done, &runs);
            });
        } else {
            prose(ui, done, &runs);
        }
        ui.add_space(MD_PARAGRAPH_GAP);
    };

    let mut job = new_job(0.0);
    let mut style = MdInline::default();
    let mut heading: Option<HeadingLevel> = None;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut quote_depth: usize = 0;
    let mut in_code_block = false;
    let mut code_block = String::new();
    let mut table: Option<MdTable> = None;
    let mut cell: Option<MdCell> = None;
    let mut in_head = false;
    // The image being described: its alt text arrives as ordinary text events between
    // the tags, and belongs to the image, not to the paragraph around it.
    let mut image: Option<(String, String)> = None;
    // The link runs of the block being built: where each one sits in the text, so the
    // laid-out galley can be hit-tested (a `LayoutJob` carries no link of its own).
    let mut links: Vec<MdLink> = Vec::new();
    let mut link: Option<(usize, String)> = None;
    // Bitbucket writes a smart link as `[text](url){: data-inline-card='' }`: without
    // the attribute-list extension the brace run lands in the text right after the
    // link, and reads as garbage in the middle of a sentence.
    let mut after_link = false;

    let indent_now = |list: &[Option<u64>], quote: usize| {
        list.len() as f32 * MD_LIST_INDENT + quote as f32 * MD_QUOTE_INDENT
    };

    // GFM tables and task lists: a PR description that carries a result table is a
    // table, not a paragraph of pipes.
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        let edge = block_edge(&event);
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level),
            Event::Start(Tag::Strong) => style.strong = true,
            Event::Start(Tag::Emphasis) => style.emphasis = true,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::Start(Tag::Link { dest_url, .. }) => {
                style.link = true;
                let block = cell.as_ref().map_or(&job, |c| &c.job);
                link = Some((block.text.chars().count(), resolve(&dest_url)));
            }
            Event::Start(Tag::List(start)) => list_stack.push(start),
            Event::Start(Tag::Item) => {
                let prefix = match list_stack.last_mut() {
                    Some(Some(n)) => {
                        let p = format!("{n}. ");
                        *n += 1;
                        p
                    }
                    _ => "•  ".to_owned(),
                };
                job = new_job(indent_now(&list_stack, quote_depth));
                append(&mut job, &prefix, MdInline::default(), None, false);
            }
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_block.clear();
            }
            Event::Start(Tag::Table(aligns)) => {
                flush(
                    ui,
                    &mut job,
                    &mut links,
                    indent_now(&list_stack, quote_depth),
                );
                table = Some(MdTable {
                    aligns,
                    head: Vec::new(),
                    rows: Vec::new(),
                });
            }
            Event::Start(Tag::TableHead) => in_head = true,
            Event::Start(Tag::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => cell = Some(MdCell::new(new_job(0.0))),
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some((resolve(&dest_url), String::new()));
            }
            Event::End(TagEnd::Image) => {
                if let Some((url, alt)) = image.take() {
                    // A cell hangs its pictures under its text — a smoke-test table
                    // carries its evidence in the Evidence column, and naming the file
                    // there is not the evidence (§11).
                    if let Some(cell) = cell.as_mut() {
                        cell.images.push((url, alt));
                    } else {
                        flush(
                            ui,
                            &mut job,
                            &mut links,
                            indent_now(&list_stack, quote_depth),
                        );
                        md_image(ui, palette, &url, &alt);
                    }
                }
            }
            // A pasted screenshot often arrives as raw `<img src="…">` rather than
            // markdown; the tag names an image all the same.
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some((url, alt)) = html_img(&html).map(|(src, alt)| (resolve(&src), alt)) {
                    if let Some(cell) = cell.as_mut() {
                        cell.images.push((url, alt));
                    } else {
                        flush(
                            ui,
                            &mut job,
                            &mut links,
                            indent_now(&list_stack, quote_depth),
                        );
                        md_image(ui, palette, &url, &alt);
                    }
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let (Some(t), Some(done)) = (table.as_mut(), cell.take()) {
                    if in_head {
                        t.head.push(done);
                    } else if let Some(row) = t.rows.last_mut() {
                        row.push(done);
                    }
                }
            }
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    md_table(ui, palette, t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(
                    ui,
                    &mut job,
                    &mut links,
                    indent_now(&list_stack, quote_depth),
                );
                heading = None;
            }
            Event::End(TagEnd::Paragraph) => {
                if list_stack.is_empty() {
                    flush(
                        ui,
                        &mut job,
                        &mut links,
                        quote_depth as f32 * MD_QUOTE_INDENT,
                    );
                }
            }
            Event::End(TagEnd::Item) => flush(
                ui,
                &mut job,
                &mut links,
                indent_now(&list_stack, quote_depth),
            ),
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
            Event::End(TagEnd::CodeBlock) => {
                md_code_block(ui, palette, code_block.trim_end());
                in_code_block = false;
            }
            Event::End(TagEnd::Strong) => style.strong = false,
            Event::End(TagEnd::Emphasis) => style.emphasis = false,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::End(TagEnd::Link) => {
                style.link = false;
                after_link = true;
                if let Some((start, url)) = link.take() {
                    let target = cell.as_ref().map_or(&job, |c| &c.job);
                    let end = target.text.chars().count();
                    let run = MdLink {
                        range: start..end,
                        url,
                    };
                    match cell.as_mut() {
                        Some(cell) => cell.links.push(run),
                        None => links.push(run),
                    }
                }
            }
            Event::Text(t) => {
                let attribute_run = after_link;
                after_link = false;
                if in_code_block {
                    code_block.push_str(&t);
                } else if let Some((_, alt)) = image.as_mut() {
                    alt.push_str(&t);
                } else {
                    // Header cells carry no `Strong` of their own — the grammar makes
                    // them headers, so the weight is ours to add.
                    let mut s = style;
                    s.strong |= in_head;
                    let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                    let text = if attribute_run {
                        strip_link_attributes(&t)
                    } else {
                        &t
                    };
                    append(target, text, s, heading, quote_depth > 0);
                }
            }
            Event::Code(t) => {
                let mut s = style;
                s.code = true;
                s.strong |= in_head;
                let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                append(target, &t, s, heading, quote_depth > 0);
            }
            Event::SoftBreak => {
                let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                append(target, " ", style, heading, quote_depth > 0);
            }
            Event::HardBreak => {
                let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                append(target, "\n", style, heading, quote_depth > 0);
            }
            _ => {}
        }
        match edge {
            Some(true) => open_blocks.push(ui.cursor().top()),
            Some(false) => {
                let Some(top) = open_blocks.pop() else {
                    continue;
                };
                // Every block is followed by its paragraph gap: the gap is not the block.
                let bottom = ui.cursor().top() - MD_PARAGRAPH_GAP;
                if bottom > top {
                    let last = range.end.saturating_sub(1).max(range.start);
                    blocks.push(MdBlock {
                        lines: line_at(&line_starts, range.start)..=line_at(&line_starts, last),
                        y: egui::Rangef::new(top, bottom),
                    });
                }
            }
            None => {}
        }
    }
    flush(
        ui,
        &mut job,
        &mut links,
        quote_depth as f32 * MD_QUOTE_INDENT,
    );
    blocks
}

/// An embedded image, as far as the renderer is concerned: fetched off-thread by the
/// app (`PrReviewRequest::Image`) and decoded into a texture it hands back here.
#[derive(Clone)]
pub enum MdImage {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

/// Cache of embedded images, by URL. Lives in egui's temp memory rather than in the
/// view struct: `markdown` is called from three surfaces (body, conversation card,
/// diff overlay), and threading a media borrow through all of them — the diff view
/// included — would buy nothing the URL key does not already give.
pub fn md_image_cache_id() -> egui::Id {
    egui::Id::new("pr_md_images")
}

/// Links clicked in a body this frame: the view has no business opening a browser, so
/// it names the URL here and the app drains it.
pub fn md_link_clicked_id() -> egui::Id {
    egui::Id::new("pr_md_links_clicked")
}

/// Images a body asked for this frame but has no bytes for yet — the app drains it,
/// fetches, and writes the result back into `md_image_cache_id`.
pub fn md_image_wanted_id() -> egui::Id {
    egui::Id::new("pr_md_images_wanted")
}

/// The reading width an embedded image is allowed: wide enough for a screenshot to be
/// legible, capped so a full-page capture does not push everything else off screen.
const MD_IMAGE_MAX_H: f32 = 420.0;

/// The open image viewer: which picture, and how the user has moved it since (§11).
#[derive(Clone)]
struct MdViewer {
    url: String,
    /// Multiplier over the fit-to-screen size — 1.0 is "the whole picture on screen".
    zoom: f32,
    /// Pan, in screen points, from the centred position.
    offset: egui::Vec2,
}

impl MdViewer {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }
}

/// Zoom bounds of the viewer: out to the fit size, in to 8× — past that a screenshot is
/// a grid of texels, not a detail.
const MD_VIEWER_MIN_ZOOM: f32 = 1.0;
const MD_VIEWER_MAX_ZOOM: f32 = 8.0;

fn md_viewer_id() -> egui::Id {
    egui::Id::new("pr_md_image_viewer")
}

/// Whether the image viewer is up — the surfaces around it read this rather than the
/// state itself (which is the viewer's own business).
pub fn md_viewer_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<MdViewer>(md_viewer_id()).is_some())
}

/// The clicked picture, full surface: scroll (or pinch) zooms, drag moves it, double
/// click resets, `Esc` / a click on the backdrop / the ✕ closes. Drawn over the review
/// and owning `Esc` while it is up, so the press that closes it never also drops the
/// file or leaves the review (pull-requests.md §11).
pub(crate) fn md_image_viewer(ui: &mut egui::Ui, palette: &Palette, screen: egui::Rect) {
    let Some(mut state) = ui.data(|d| d.get_temp::<MdViewer>(md_viewer_id())) else {
        return;
    };
    let texture = ui.data(|d| {
        d.get_temp::<HashMap<String, MdImage>>(md_image_cache_id())
            .and_then(|images| match images.get(&state.url) {
                Some(MdImage::Ready(texture)) => Some(texture.clone()),
                _ => None,
            })
    });
    // The bytes went away (a reload dropped the cache): nothing to look at.
    let Some(texture) = texture else {
        ui.data_mut(|d| d.remove::<MdViewer>(md_viewer_id()));
        return;
    };

    let mut close = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    egui::Area::new(md_viewer_id().with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(screen);
            let backdrop = ui.interact(
                screen,
                md_viewer_id().with("backdrop"),
                egui::Sense::click_and_drag(),
            );
            ui.painter()
                .rect_filled(screen, 0, with_alpha(palette.bg_canvas, 235));

            // Fit first, then the user's own zoom on top of it.
            let size = texture.size_vec2();
            let fit = (screen.width() * 0.9 / size.x)
                .min(screen.height() * 0.9 / size.y)
                .min(1.0);
            let pointer = ui.input(|i| i.pointer.hover_pos());
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (1.0 + scroll * 0.002);
            if (factor - 1.0).abs() > f32::EPSILON {
                let before = state.zoom;
                state.zoom = (state.zoom * factor).clamp(MD_VIEWER_MIN_ZOOM, MD_VIEWER_MAX_ZOOM);
                // Zoom about the pointer, so the detail under it stays under it.
                if let Some(pointer) = pointer {
                    let anchor = pointer - screen.center() - state.offset;
                    state.offset -= anchor * (state.zoom / before - 1.0);
                }
            }
            if backdrop.dragged() {
                state.offset += backdrop.drag_delta();
            }
            if backdrop.double_clicked() {
                state = MdViewer::new(&state.url);
            } else if backdrop.clicked() {
                close = true;
            }
            if state.zoom <= MD_VIEWER_MIN_ZOOM {
                state.offset = egui::Vec2::ZERO;
            }
            let shown = size * fit * state.zoom;
            let rect = egui::Rect::from_center_size(screen.center() + state.offset, shown);
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if backdrop.hovered() {
                ui.ctx()
                    .set_cursor_icon(if state.zoom > MD_VIEWER_MIN_ZOOM {
                        egui::CursorIcon::Grab
                    } else {
                        egui::CursorIcon::ZoomIn
                    });
            }
            md_viewer_bar(ui, palette, screen, &state, &mut close);
        });

    if close {
        ui.data_mut(|d| d.remove::<MdViewer>(md_viewer_id()));
    } else {
        ui.data_mut(|d| d.insert_temp(md_viewer_id(), state));
    }
}

const VIEWER_BAR_PAD_X: f32 = 18.0;
const VIEWER_BAR_TEXT_SIZE: f32 = 12.5;
const VIEWER_CLOSE_ICON: f32 = 15.0;

/// The viewer's chrome: the zoom level and a ✕, in a strip at the top.
fn md_viewer_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    screen: egui::Rect,
    state: &MdViewer,
    close: &mut bool,
) {
    let bar = egui::Rect::from_min_size(
        egui::pos2(screen.left(), screen.top()),
        egui::vec2(screen.width(), 44.0),
    );
    ui.painter().text(
        egui::pos2(bar.left() + VIEWER_BAR_PAD_X, bar.center().y),
        egui::Align2::LEFT_CENTER,
        format!(
            "{:.0}%  ·  scroll to zoom, drag to move",
            state.zoom * 100.0
        ),
        egui::FontId::proportional(VIEWER_BAR_TEXT_SIZE),
        palette.text_muted,
    );
    let hit = egui::Rect::from_center_size(
        egui::pos2(bar.right() - VIEWER_BAR_PAD_X - 12.0, bar.center().y),
        egui::vec2(28.0, 28.0),
    );
    let response = ui.interact(hit, md_viewer_id().with("close"), egui::Sense::click());
    let ink = if response.hovered() {
        palette.text_primary
    } else {
        palette.text_muted
    };
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    paint_icon(ui.painter(), hit.center(), VIEWER_CLOSE_ICON, Icon::X, ink);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    *close |= response.clicked();
}

/// One embedded image: the texture once it has landed, else a muted placeholder line
/// carrying its alt text (and, on failure, why).
fn md_image(ui: &mut egui::Ui, palette: &Palette, url: &str, alt: &str) {
    let slot: Option<MdImage> = ui.data(|d| {
        d.get_temp::<HashMap<String, MdImage>>(md_image_cache_id())
            .and_then(|images| images.get(url).cloned())
    });
    match slot {
        Some(MdImage::Ready(texture)) => {
            let size = texture.size_vec2();
            let scale = (ui.available_width() / size.x)
                .min(MD_IMAGE_MAX_H / size.y.max(1.0))
                .min(1.0);
            // In the flow a screenshot is a thumbnail — the detail lives in the viewer,
            // one click away (§11).
            let response = ui
                .add(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(size * scale)
                        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
                        .sense(egui::Sense::click()),
                )
                .on_hover_text(format!("{url}\n\nClick to open"));
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                ui.data_mut(|d| d.insert_temp(md_viewer_id(), MdViewer::new(url)));
            }
            ui.add_space(MD_PARAGRAPH_GAP);
        }
        Some(MdImage::Failed(reason)) => {
            md_image_placeholder(
                ui,
                palette,
                url,
                alt,
                &format!("image unavailable — {reason}"),
            );
        }
        Some(MdImage::Loading) => md_image_placeholder(ui, palette, url, alt, "loading image…"),
        None => {
            // First sight: ask the app for it, and stand in until it lands.
            ui.data_mut(|d| {
                let wanted: &mut Vec<String> = d.get_temp_mut_or_default(md_image_wanted_id());
                if !wanted.iter().any(|u| u == url) {
                    wanted.push(url.to_owned());
                }
            });
            md_image_placeholder(ui, palette, url, alt, "loading image…");
        }
    }
}

fn md_image_placeholder(ui: &mut egui::Ui, palette: &Palette, url: &str, alt: &str, status: &str) {
    ui.horizontal(|ui| {
        paint_icon_at_cursor(ui, palette, Icon::Image);
        let label = format!("{} — {status}", image_label(url, alt));
        ui.label(
            egui::RichText::new(label)
                .size(MD_CODE_SIZE)
                .color(palette.text_muted),
        );
    });
    ui.add_space(MD_PARAGRAPH_GAP);
}

fn paint_icon_at_cursor(ui: &mut egui::Ui, palette: &Palette, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    paint_icon(ui.painter(), rect.center(), 14.0, icon, palette.text_muted);
}

/// How an image names itself while it is not on screen (loading, or unavailable): its
/// alt text, else the file name the URL ends on.
fn image_label(url: &str, alt: &str) -> String {
    if !alt.trim().is_empty() {
        return alt.to_owned();
    }
    url.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(url)
        .to_owned()
}

/// `src` and `alt` of an `<img …>` tag in a raw-HTML chunk, if it is one. A body's
/// HTML is not parsed beyond this: the one tag that carries a picture is worth
/// reading, a general HTML renderer is not.
fn html_img(html: &str) -> Option<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<img")?;
    let end = lower[start..].find('>').map(|i| start + i)?;
    let attr = |name: &str| {
        let at = lower[start..end].find(&format!("{name}="))? + start + name.len() + 1;
        let rest = html.get(at..end)?;
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        rest[1..].split(quote).next().map(str::to_owned)
    };
    let src = attr("src")?;
    Some((src, attr("alt").unwrap_or_default()))
}

/// A GFM table collected while parsing: the header cells, then one job per cell per
/// row, with the column alignments the delimiter row declared.
struct MdTable {
    aligns: Vec<pulldown_cmark::Alignment>,
    head: Vec<MdCell>,
    rows: Vec<Vec<MdCell>>,
}

/// A link run inside a laid-out block: where it sits in the text, and where it points.
struct MdLink {
    range: std::ops::Range<usize>,
    url: String,
}

/// Draws a prose block from its job and makes its link runs clickable.
///
/// Laid out **here** rather than handed to `Label` as a job: `Label` relayouts a bare
/// `LayoutJob` at the width of the `Ui` it lands in (egui `label.rs`), which would drop
/// the reading measure — and the galley is what the link hit-boxes are measured on. A
/// clicked URL goes to the app through `md_link_clicked_id`, which owns the opening.
fn prose(ui: &mut egui::Ui, job: egui::text::LayoutJob, links: &[MdLink]) {
    let galley = ui.painter().layout_job(job);
    let response = ui.add(egui::Label::new(galley.clone()));
    crate::ui::emoji::paint_emoji(ui.painter(), response.rect.min, &galley);
    let origin = response.rect.min.to_vec2();
    for (i, link) in links.iter().enumerate() {
        for (j, rect) in link_rects(&galley, &link.range).into_iter().enumerate() {
            let hit = ui.interact(
                rect.translate(origin),
                response.id.with(("md_link", i, j)),
                egui::Sense::click(),
            );
            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if hit.clicked() {
                ui.data_mut(|d| {
                    let clicked: &mut Vec<String> = d.get_temp_mut_or_default(md_link_clicked_id());
                    clicked.push(link.url.clone());
                });
            }
            // A run of a paragraph is a widget of its own here: name it, or the link is
            // invisible to the accessibility tree (and to the tests driving it).
            let url = link.url.clone();
            hit.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Link, true, url.clone())
            });
            hit.on_hover_text(&link.url);
        }
    }
}

/// The boxes a link run occupies in its galley — one per row it spans, since a link
/// that wraps is two boxes on two lines, not one rectangle across the paragraph.
fn link_rects(galley: &egui::Galley, range: &std::ops::Range<usize>) -> Vec<egui::Rect> {
    let start = galley.pos_from_cursor(egui::text::CCursor::new(range.start));
    let end = galley.pos_from_cursor(egui::text::CCursor::new(range.end));
    if (start.top() - end.top()).abs() < 0.5 {
        return vec![egui::Rect::from_min_max(start.min, end.max)];
    }
    galley
        .rows
        .iter()
        .filter(|row| row.max_y() > start.top() && row.min_y() < end.bottom())
        .map(|row| {
            let first = row.min_y() <= start.top() && start.top() < row.max_y();
            let last = row.min_y() <= end.top() && end.top() < row.max_y();
            let left = if first {
                start.left()
            } else {
                row.rect().left()
            };
            let right = if last {
                end.right()
            } else {
                row.rect().right()
            };
            egui::Rect::from_min_max(
                egui::pos2(left, row.min_y()),
                egui::pos2(right, row.max_y()),
            )
        })
        .collect()
}

/// Bitbucket's smart-link attribute run — `{: data-inline-card='' }` right after a
/// link — dropped: without the attribute-list extension it lands in the prose, and it
/// is markup, not something the author wrote to be read.
fn strip_link_attributes(text: &str) -> &str {
    let rest = text.strip_prefix("{:").unwrap_or(text);
    if std::ptr::eq(rest, text) {
        return text;
    }
    match rest.find('}') {
        Some(end) if !rest[..end].contains('\n') => &rest[end + 1..],
        _ => text,
    }
}

/// One table cell: its text, plus the pictures it embeds — drawn under that text
/// inside the column, so a table can carry a screenshot as evidence (§11).
struct MdCell {
    job: egui::text::LayoutJob,
    images: Vec<(String, String)>,
    links: Vec<MdLink>,
}

impl MdCell {
    fn new(job: egui::text::LayoutJob) -> Self {
        Self {
            job,
            images: Vec::new(),
            links: Vec::new(),
        }
    }
}

/// What a cell would take on a single unwrapped line — the width its column asks for.
fn md_cell_natural_width(ui: &egui::Ui, cell: &MdCell) -> f32 {
    let text = if cell.job.text.trim().is_empty() {
        0.0
    } else {
        let mut job = cell.job.clone();
        job.wrap.max_width = f32::INFINITY;
        ui.painter().layout_job(job).rect.width()
    };
    if cell.images.is_empty() {
        text
    } else {
        text.max(MD_CELL_IMAGE_W)
    }
}

const MD_CELL_PAD_X: f32 = 8.0;
const MD_CELL_PAD_Y: f32 = 5.0;
const MD_CELL_MIN_W: f32 = 72.0;
/// Ceiling on what a column may *ask* for: without it a single long line (a URL, a
/// stack trace) would claim the whole table and squeeze its neighbours to the floor.
const MD_CELL_MAX_W: f32 = 420.0;
/// What a cell holding an image asks for, since its text ("same screenshot") says
/// nothing about the thumbnail below it — and the thumbnail scales to the column.
const MD_CELL_IMAGE_W: f32 = 220.0;
/// The frame's horizontal inner margin, subtracted from the width the columns divide.
const MD_TABLE_MARGIN_X: f32 = 10.0;

/// A markdown table: fixed-width columns over the reading width, each cell wrapping
/// inside its own column. Columns are sized on what their content asks for — a *Step*
/// column of single digits next to a sentence should not take the same quarter — then
/// scaled to the width available, so the table fills the card without ever pushing past
/// it and taking the horizontal scroll with it.
fn md_table(ui: &mut egui::Ui, palette: &Palette, mut table: MdTable) {
    let columns = table
        .rows
        .iter()
        .map(Vec::len)
        .chain(std::iter::once(table.head.len()))
        .max()
        .unwrap_or(0);
    if columns == 0 {
        return;
    }
    let natural: Vec<f32> = (0..columns)
        .map(|i| {
            table
                .rows
                .iter()
                .chain(std::iter::once(&table.head))
                .filter_map(|row| row.get(i))
                .map(|cell| md_cell_natural_width(ui, cell))
                .fold(0.0_f32, f32::max)
                .clamp(MD_CELL_MIN_W, MD_CELL_MAX_W)
        })
        .collect();
    let asked: f32 = natural.iter().sum();
    let free =
        (ui.available_width() - 2.0 * MD_TABLE_MARGIN_X - (columns as f32 - 1.0) * MD_CELL_PAD_X)
            .max(MD_CELL_MIN_W);
    let widths: Vec<f32> = natural
        .iter()
        .map(|w| (w / asked * free).max(MD_CELL_MIN_W))
        .collect();
    let align = |i: usize| match table.aligns.get(i) {
        Some(pulldown_cmark::Alignment::Center) => egui::Align::Center,
        Some(pulldown_cmark::Alignment::Right) => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let row_ui = |ui: &mut egui::Ui, cells: &mut Vec<MdCell>| {
        ui.horizontal_top(|ui| {
            let last = cells.len().saturating_sub(1);
            for (i, cell) in cells.iter_mut().enumerate() {
                let col_w = widths[i];
                cell.job.wrap.max_width = col_w;
                let done = std::mem::take(&mut cell.job);
                let images = std::mem::take(&mut cell.images);
                let links = std::mem::take(&mut cell.links);
                ui.allocate_ui_with_layout(
                    egui::vec2(col_w, 0.0),
                    egui::Layout::top_down(align(i)),
                    |ui| {
                        // Without a floor the cell shrinks to its text and the next
                        // column starts wherever this one happened to end — the rows
                        // would stop lining up under their headers.
                        ui.set_min_width(col_w);
                        if !done.text.trim().is_empty() {
                            prose(ui, done, &links);
                        }
                        for (url, alt) in &images {
                            md_image(ui, palette, url, alt);
                        }
                    },
                );
                if i != last {
                    ui.add_space(MD_CELL_PAD_X);
                }
            }
        });
    };

    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
        .inner_margin(egui::Margin::symmetric(MD_TABLE_MARGIN_X as i8, 8))
        .show(ui, |ui| {
            if !table.head.is_empty() {
                row_ui(ui, &mut table.head);
                ui.add_space(MD_CELL_PAD_Y);
                let line = ui.min_rect().x_range();
                ui.painter().hline(
                    line,
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, palette.border_subtle),
                );
                ui.add_space(MD_CELL_PAD_Y);
            }
            let last = table.rows.len().saturating_sub(1);
            for (index, row) in table.rows.iter_mut().enumerate() {
                row_ui(ui, row);
                if index != last {
                    ui.add_space(MD_CELL_PAD_Y);
                    ui.painter().hline(
                        ui.min_rect().x_range(),
                        ui.cursor().top(),
                        egui::Stroke::new(1.0_f32, with_alpha(palette.border_subtle, 120)),
                    );
                    ui.add_space(MD_CELL_PAD_Y);
                }
            }
        });
    ui.add_space(MD_PARAGRAPH_GAP);
}

/// Per-level heading size for `markdown`, scaling down from the body size.
fn md_heading_size(level: pulldown_cmark::HeadingLevel) -> f32 {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => MD_TEXT_SIZE + 6.0,
        H2 => MD_TEXT_SIZE + 4.0,
        H3 => MD_TEXT_SIZE + 2.0,
        H4 => MD_TEXT_SIZE + 1.0,
        H5 | H6 => MD_TEXT_SIZE,
    }
}

/// A fenced/indented code block in `markdown`: monospace lines on a subtle fill.
fn md_code_block(ui: &mut egui::Ui, palette: &Palette, code: &str) {
    egui::Frame::new()
        .fill(with_alpha(palette.text_muted, 24))
        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for line in code.lines() {
                ui.label(
                    egui::RichText::new(line)
                        .size(MD_CODE_SIZE)
                        .monospace()
                        .color(palette.text_secondary),
                );
            }
        });
    ui.add_space(MD_PARAGRAPH_GAP);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_img_reads_src_and_alt() {
        assert_eq!(
            html_img(r#"<img width="600" alt="Step 1" src="https://x.test/a.png" />"#),
            Some(("https://x.test/a.png".to_owned(), "Step 1".to_owned()))
        );
        assert_eq!(
            html_img("<img src='https://x.test/b.png'>"),
            Some(("https://x.test/b.png".to_owned(), String::new()))
        );
        assert_eq!(html_img("<p>no picture here</p>"), None);
    }

    /// In a table cell an image can only read as text: its alt, else the file name —
    /// never the whole URL, which would blow the column open.
    #[test]
    fn image_label_falls_back_to_the_file_name() {
        assert_eq!(image_label("https://x.test/a/step-1.png", ""), "step-1.png");
        assert_eq!(
            image_label("https://x.test/a/step-1.png", "Step 1"),
            "Step 1"
        );
    }
}
