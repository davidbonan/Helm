//! A worktree markdown file rendered in place of its lines (files.md §4.3): the file
//! viewer's and the diff view's Preview, the latter marking in its margin the blocks
//! the diff touches.

use std::path::Path;

use crate::git::diff::NewSideChanges;
use crate::theme::Palette;
use crate::ui::markdown::{
    md_link_clicked_id, worktree_markdown, MdBlock, WorktreeDoc, LOCAL_SCHEME,
};

const PAD: i8 = 16;
/// Room left of the prose for the change bars.
const PAD_LEFT: i8 = 24;
const BAR_W: f32 = 3.0;
const BAR_INSET: f32 = 12.0;
const REMOVED_SIZE: f32 = 5.0;

/// How a markdown file is shown: its source, or rendered.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownView {
    #[default]
    Code,
    Preview,
}

/// The header's Code | Preview segments.
pub const VIEW_SEGMENTS: [(MarkdownView, &str); 2] = [
    (MarkdownView::Code, "Code"),
    (MarkdownView::Preview, "Preview"),
];

/// What a preview draws: the document, the identity its scroll is kept under, and the
/// diff to mark in the margin, if any.
pub struct PreviewPane<'a> {
    pub doc: WorktreeDoc<'a>,
    pub scroll_salt: egui::Id,
    pub changes: Option<&'a NewSideChanges>,
}

/// Renders the pane; returns the worktree files a click on a link asked to open. A
/// bare anchor goes nowhere; any other URL stays queued for the app.
pub fn preview_pane(ui: &mut egui::Ui, palette: &Palette, pane: PreviewPane<'_>) -> Vec<String> {
    egui::ScrollArea::vertical()
        .id_salt(pane.scroll_salt)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let margin = egui::Margin {
                left: PAD_LEFT,
                right: PAD,
                top: PAD,
                bottom: PAD,
            };
            let drawn = egui::Frame::new()
                .inner_margin(margin)
                .show(ui, |ui| worktree_markdown(ui, palette, pane.doc));
            if let Some(changes) = pane.changes {
                let x = drawn.response.rect.left() + f32::from(PAD_LEFT) - BAR_INSET;
                for mark in change_marks(&drawn.inner, changes) {
                    paint_mark(ui, palette, x, &mark);
                }
            }
        });
    opened_files(ui, pane.doc.root)
}

/// Takes this frame's link clicks: the worktree files among them, the anchors
/// dropped, the rest put back for the app.
fn opened_files(ui: &egui::Ui, root: &Path) -> Vec<String> {
    let clicked: Vec<String> =
        ui.data_mut(|d| std::mem::take(d.get_temp_mut_or_default(md_link_clicked_id())));
    let mut opened = Vec::new();
    let mut outside = Vec::new();
    for url in clicked {
        if let Some(relative) = worktree_file(root, &url) {
            opened.push(relative);
        } else if !url.starts_with('#') {
            outside.push(url);
        }
    }
    ui.data_mut(|d| d.insert_temp(md_link_clicked_id(), outside));
    opened
}

/// The worktree-relative path of a `LOCAL_SCHEME` target inside `root`.
fn worktree_file(root: &Path, url: &str) -> Option<String> {
    let path = Path::new(url.strip_prefix(LOCAL_SCHEME)?);
    Some(path.strip_prefix(root).ok()?.to_str()?.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeKind {
    Added,
    Modified,
    Removed,
}

impl ChangeKind {
    fn label(self) -> &'static str {
        match self {
            ChangeKind::Added => "Added",
            ChangeKind::Modified => "Modified",
            ChangeKind::Removed => "Removed",
        }
    }
}

/// A margin mark: a bar beside a changed block, or a notch where lines went away.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChangeMark {
    pub kind: ChangeKind,
    pub y: egui::Rangef,
}

/// A block every line of which is new is **added**; one with a new line, or lines
/// removed inside it or right before it, is **modified**. A removal between two
/// untouched blocks is a notch on the seam.
pub(crate) fn change_marks(blocks: &[MdBlock], changes: &NewSideChanges) -> Vec<ChangeMark> {
    let mut marks = Vec::new();
    let mut claimed = Vec::new();
    for block in blocks {
        let (start, end) = (*block.lines.start(), *block.lines.end());
        let added = changes.added.range(start..=end).count();
        let removed_from = |from: u32| {
            changes
                .removed_before
                .iter()
                .filter(move |line| (from..=end).contains(*line))
        };
        let removed_inside = removed_from(start + 1).next().is_some();
        let removed_before = changes.removed_before.contains(&start);
        let kind = if added as u32 == end - start + 1 && !removed_before {
            ChangeKind::Added
        } else if added > 0 || removed_inside {
            ChangeKind::Modified
        } else {
            continue;
        };
        // A removal right before the block is its own once the block is rewritten.
        let from = if added > 0 { start } else { start + 1 };
        claimed.extend(removed_from(from));
        marks.push(ChangeMark { kind, y: block.y });
    }
    for &line in &changes.removed_before {
        if claimed.contains(&line) {
            continue;
        }
        let seam = match blocks.iter().position(|block| *block.lines.start() >= line) {
            Some(0) => blocks[0].y.min,
            Some(next) => (blocks[next - 1].y.max + blocks[next].y.min) / 2.0,
            None => match blocks.last() {
                Some(last) => last.y.max,
                None => continue,
            },
        };
        marks.push(ChangeMark {
            kind: ChangeKind::Removed,
            y: egui::Rangef::point(seam),
        });
    }
    marks
}

fn paint_mark(ui: &mut egui::Ui, palette: &Palette, x: f32, mark: &ChangeMark) {
    let rect = match mark.kind {
        ChangeKind::Removed => egui::Rect::from_center_size(
            egui::pos2(x + BAR_W / 2.0, mark.y.min),
            egui::vec2(REMOVED_SIZE * 2.0, REMOVED_SIZE * 2.0),
        ),
        _ => egui::Rect::from_x_y_ranges(egui::Rangef::new(x, x + BAR_W), mark.y),
    };
    match mark.kind {
        ChangeKind::Added => {
            ui.painter().rect_filled(rect, 1.0, palette.git_added);
        }
        ChangeKind::Modified => {
            ui.painter().rect_filled(rect, 1.0, palette.git_modified);
        }
        ChangeKind::Removed => {
            let tip = egui::pos2(rect.center().x + REMOVED_SIZE, rect.center().y);
            let points = vec![
                egui::pos2(rect.left(), rect.top()),
                tip,
                egui::pos2(rect.left(), rect.bottom()),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                palette.git_deleted,
                egui::Stroke::NONE,
            ));
        }
    }
    let label = mark.kind.label();
    let hover = ui.interact(
        rect.expand2(egui::vec2(3.0, 0.0)),
        ui.id().with(("md_change", mark.y.min.to_bits(), label)),
        egui::Sense::hover(),
    );
    hover.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    hover.on_hover_text(label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn block(lines: std::ops::RangeInclusive<u32>, top: f32, bottom: f32) -> MdBlock {
        MdBlock {
            lines,
            y: egui::Rangef::new(top, bottom),
        }
    }

    fn changes(added: &[u32], removed_before: &[u32]) -> NewSideChanges {
        NewSideChanges {
            added: added.iter().copied().collect::<BTreeSet<_>>(),
            removed_before: removed_before.iter().copied().collect::<BTreeSet<_>>(),
        }
    }

    fn kinds(marks: &[ChangeMark]) -> Vec<ChangeKind> {
        marks.iter().map(|mark| mark.kind).collect()
    }

    #[test]
    fn a_new_block_is_added_a_touched_one_modified_an_untouched_one_unmarked() {
        let blocks = [
            block(1..=1, 0.0, 20.0),
            block(3..=4, 30.0, 60.0),
            block(6..=7, 70.0, 90.0),
        ];

        let marks = change_marks(&blocks, &changes(&[3, 4, 6], &[]));

        assert_eq!(kinds(&marks), [ChangeKind::Added, ChangeKind::Modified]);
        assert_eq!(marks[0].y, blocks[1].y);
        assert_eq!(marks[1].y, blocks[2].y);
    }

    #[test]
    fn a_replaced_block_is_modified_not_added_beside_a_removal() {
        let blocks = [block(1..=1, 0.0, 20.0), block(3..=3, 30.0, 50.0)];

        let marks = change_marks(&blocks, &changes(&[3], &[3]));

        assert_eq!(kinds(&marks), [ChangeKind::Modified]);
    }

    #[test]
    fn a_removal_between_untouched_blocks_notches_the_seam() {
        let blocks = [block(1..=1, 0.0, 20.0), block(3..=3, 30.0, 50.0)];

        let marks = change_marks(&blocks, &changes(&[], &[2]));
        assert_eq!(kinds(&marks), [ChangeKind::Removed]);
        assert_eq!(marks[0].y, egui::Rangef::point(25.0));

        let marks = change_marks(&blocks, &changes(&[], &[4]));
        assert_eq!(marks[0].y, egui::Rangef::point(50.0));
    }
}
