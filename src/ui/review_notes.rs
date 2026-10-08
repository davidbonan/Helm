//! Review notes left on a line (pull-requests.md §11): the note editor, the saved
//! note's card and the header's recap chip, shared by the diff and the file viewer
//! (files.md §4.2) so a note reads and behaves the same on both.

use crate::review::{count, FileComments, LineComment, ReviewIntent, ReviewPool};
use crate::theme::{Palette, PILL_SIZE, RADIUS_PILL};
use crate::ui::diff_view::{
    bar_button, card_width, editor_hairline, icon_button, pill_button, EDITOR_BAR_HEIGHT,
    EDITOR_PAD_X, EDITOR_PAD_Y, EDITOR_RADIUS, EDITOR_TEXT_SIZE, LINE_SIZE,
};
use crate::ui::with_alpha;

/// The line a note belongs to: its pool and its `(old, new)` numbers. The pool tells
/// the PR surface's two gutter buttons apart, so each opens its own editor on a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoteAnchor {
    pub pool: ReviewPool,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

/// The note editors of one surface, kept across frames: the one open under a line,
/// or the one open in the recap popover — never both.
#[derive(Debug, Default)]
pub struct NoteSession {
    /// Line whose note editor is open; `buffer` holds its in-progress text.
    active: Option<NoteAnchor>,
    buffer: String,
    /// Note being edited from the recap popover, keyed by `(file, line_ref)`;
    /// `popover_buffer` holds its in-progress text.
    popover_edit: Option<(String, Option<u32>)>,
    popover_buffer: String,
    /// One-shot: focus the editor on its next frame, so the caret lands in the field
    /// without an extra click.
    focus: bool,
}

impl NoteSession {
    /// Whether a note editor is open (under a line, or in the recap popover): it holds
    /// the text input, so the app disarms the sidebar keys that would otherwise land in
    /// the buffer — `Cmd+Enter` above all, which sends the review batch here
    /// (keybindings.md §4).
    pub fn is_open(&self) -> bool {
        self.active.is_some() || self.popover_edit.is_some()
    }

    /// The line whose note editor is open, if one is.
    pub(crate) fn editing_line(&self) -> Option<NoteAnchor> {
        self.active
    }

    /// Opens the note editor under `anchor`, prefilled with its saved note, and focuses
    /// it. Closes a popover edit so a single editor is live.
    pub(crate) fn open(&mut self, anchor: NoteAnchor, saved: Option<&str>) {
        self.buffer = saved.unwrap_or_default().to_owned();
        self.active = Some(anchor);
        self.popover_edit = None;
        self.focus = true;
    }

    /// Closes whichever note editor is open, its draft dropped (`Esc`).
    pub(crate) fn cancel(&mut self) {
        self.active = None;
        self.buffer.clear();
        self.popover_edit = None;
        self.popover_buffer.clear();
    }

    fn close_line_editor(&mut self) {
        self.active = None;
        self.buffer.clear();
    }

    fn close_popover_editor(&mut self) {
        self.popover_edit = None;
        self.popover_buffer.clear();
    }

    fn open_in_popover(&mut self, file: &str, comment: &LineComment) {
        self.popover_edit = Some((file.to_owned(), comment.line_ref()));
        self.popover_buffer = comment.note.clone();
        self.active = None;
        self.focus = true;
    }
}

/// The agent pool's batch as a surface shows it: every note of the worktree, and the
/// agent its *Send to* hands them to.
pub struct NoteBatch<'a> {
    pub comments: &'a FileComments,
    pub agent: &'a str,
}

/// What the note widgets of a surface draw with, keep their editors in, and raise
/// their actions into.
pub(crate) struct NoteCtx<'a> {
    pub palette: &'a Palette,
    pub session: &'a mut NoteSession,
    pub out: &'a mut Vec<ReviewIntent>,
}

/// A line that can carry a note: where it is, its code (the note's anchor text), and
/// the note already saved there, if any.
pub(crate) struct NoteLine<'a> {
    pub path: &'a str,
    pub anchor: NoteAnchor,
    pub code: &'a str,
    pub saved: Option<&'a str>,
}

/// Stored note anchored at the `(old, new)` row of `path`, if any. Matches the full
/// pair — not `line_ref()` — so a deleted row (old N) and an added row (new N) sharing
/// a number don't collide and render the same note twice.
pub(crate) fn note_at<'a>(
    comments: &'a FileComments,
    path: &str,
    old: Option<u32>,
    new: Option<u32>,
) -> Option<&'a str> {
    comments
        .get(path)?
        .iter()
        .find(|c| c.old_lineno == old && c.new_lineno == new)
        .map(|c| c.note.as_str())
}

/// Outcome of a note editor frame.
#[derive(Clone, Copy)]
enum NoteEdit {
    Idle,
    Delete,
    Save,
    /// Validate the note *and* hand the whole agent batch to the agent (⌘↵ or the
    /// Sparkles button) — agent pool only.
    SaveAndSend,
}

/// Visual identity of a review pool — the color, icon, header label and editor
/// hint that tell a forge review comment (`accent`) apart from an agent note
/// (`accent_ai`) wherever the two share the diff gutter (pull-requests.md §11).
struct PoolStyle {
    color: egui::Color32,
    icon: lucide_icons::Icon,
    hint: &'static str,
}

fn pool_style(palette: &Palette, pool: ReviewPool) -> PoolStyle {
    match pool {
        ReviewPool::Forge => PoolStyle {
            color: palette.accent,
            icon: lucide_icons::Icon::MessageSquarePlus,
            hint: "Leave a review comment…",
        },
        ReviewPool::Agent => PoolStyle {
            color: palette.accent_ai,
            icon: lucide_icons::Icon::Sparkles,
            hint: "Describe what the agent should inspect…",
        },
    }
}

/// Shared note field, built as **one framed object** — a padded input over a hairline
/// and an action bar — the same shape as the PR reply editor and the conversation
/// composer, so a review note reads like every other authoring surface of the app.
/// Enter validates, `Shift+Enter` inserts a newline, a click outside validates too.
/// `focus` is a one-shot that lands the caret in the field the frame the editor opens;
/// `style` colors the focus ring and the caret to the pool's identity. `can_send` adds
/// the *Send review* action and its `⌘↩`, which validate then flush the batch: agent
/// pool only, so a forge review is never posted on a keystroke.
fn note_editor(
    ui: &mut egui::Ui,
    palette: &Palette,
    style: &PoolStyle,
    buffer: &mut String,
    focus: &mut bool,
    width: f32,
    can_send: bool,
) -> NoteEdit {
    // Consume the bare Enter before the field sees it so it validates instead of
    // inserting a newline; Shift+Enter falls through to the field as a newline.
    let (submit_key, send_key) = ui.input_mut(|i| {
        let (mut submit, mut send) = (false, false);
        i.events.retain(|e| {
            let egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                modifiers,
                ..
            } = e
            else {
                return true;
            };
            if modifiers.shift {
                return true;
            }
            if can_send && modifiers.command {
                send = true;
            } else {
                submit = true;
            }
            false
        });
        (submit, send)
    });
    let mut edit = if send_key {
        NoteEdit::SaveAndSend
    } else if submit_key {
        NoteEdit::Save
    } else {
        NoteEdit::Idle
    };
    // The focus ring is read *before* the field is added, so the frame around it can
    // carry the ring — egui's own widget stroke sits inside the frame and is invisible
    // once the field is frameless.
    let field_id = ui.id().with("note_editor_field");
    let ring = if ui.memory(|m| m.has_focus(field_id)) {
        egui::Stroke::new(1.5_f32, style.color)
    } else {
        egui::Stroke::new(1.0_f32, palette.border_input)
    };
    let response = egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(ring)
        .corner_radius(egui::CornerRadius::same(EDITOR_RADIUS))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.visuals_mut().selection.stroke = egui::Stroke::new(1.5_f32, style.color);
                let response = ui.add(
                    egui::TextEdit::multiline(buffer)
                        .id(field_id)
                        // `TextEdit::margin` is ignored once a custom frame is given, so
                        // the padding rides on the frame itself (as in `reply_editor`).
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
                            EDITOR_PAD_X as i8,
                            EDITOR_PAD_Y as i8,
                        )))
                        .desired_rows(2)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::proportional(EDITOR_TEXT_SIZE))
                        .hint_text(style.hint),
                );
                // A click outside the field (it loses focus) validates, like Enter or
                // *Save note* — but the bar below is evaluated after, so clicking one of
                // its actions, which blurs the field too, still raises that action.
                if matches!(edit, NoteEdit::Idle) && response.lost_focus() {
                    edit = NoteEdit::Save;
                }
                editor_hairline(ui, palette);
                // Pin the bar to its own height: a bare layout would inherit the parent's
                // remaining height and drop the buttons out of reach in a tall scroll area.
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), EDITOR_BAR_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(8.0);
                        // The destructive action sits alone on the far side of the bar:
                        // it must not share an edge with the two confirmations.
                        if bar_button(ui, palette, "Delete note", None, false, true) {
                            edit = NoteEdit::Delete;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if can_send
                                && bar_button(
                                    ui,
                                    palette,
                                    "Send review",
                                    Some(SHORTCUT_SEND),
                                    true,
                                    true,
                                )
                            {
                                edit = NoteEdit::SaveAndSend;
                            }
                            // Primary only where it is the sole confirmation — with a
                            // Send beside it, two filled buttons would compete.
                            if bar_button(
                                ui,
                                palette,
                                "Save note",
                                Some(SHORTCUT_SAVE),
                                !can_send,
                                true,
                            ) {
                                edit = NoteEdit::Save;
                            }
                        });
                    },
                );
                response
            })
            .inner
        })
        .inner;
    if *focus {
        response.request_focus();
        *focus = false;
    }
    edit
}

/// The two note-editor shortcuts, in the badge convention the rest of the app displays
/// (`keybindings::Shortcut::display` — `↩` for Enter, modifiers in `⌃⌥⇧⌘` order).
const SHORTCUT_SAVE: &str = "↩";
const SHORTCUT_SEND: &str = "⌘↩";

/// Under a line: its open note editor, or its saved note as a clickable card that
/// reopens the editor, left-aligned just below the line.
pub(crate) fn note_block(ui: &mut egui::Ui, line: &NoteLine<'_>, notes: &mut NoteCtx<'_>) {
    if notes.session.active == Some(line.anchor) {
        open_note_block(ui, line, notes);
        return;
    }
    let Some(note) = line.saved else {
        return;
    };
    let style = pool_style(notes.palette, line.anchor.pool);
    ui.add_space(3.0);
    let clicked = ui
        .horizontal(|ui| {
            note_card(
                ui,
                notes.palette,
                &style,
                note,
                line.anchor.new.or(line.anchor.old),
            )
        })
        .inner;
    ui.add_space(3.0);
    if clicked {
        notes.session.open(line.anchor, Some(note));
    }
}

/// The note editor under its line; validating or deleting closes it.
fn open_note_block(ui: &mut egui::Ui, line: &NoteLine<'_>, notes: &mut NoteCtx<'_>) {
    let pool = line.anchor.pool;
    let style = pool_style(notes.palette, pool);
    let session = &mut *notes.session;
    ui.add_space(3.0);
    let edit = ui
        .horizontal(|ui| {
            ui.vertical(|ui| {
                let width = card_width(ui);
                note_editor(
                    ui,
                    notes.palette,
                    &style,
                    &mut session.buffer,
                    &mut session.focus,
                    width,
                    pool == ReviewPool::Agent,
                )
            })
            .inner
        })
        .inner;
    ui.add_space(3.0);
    match edit {
        NoteEdit::Save | NoteEdit::SaveAndSend => {
            save_note(notes.out, line, session.buffer.trim());
            if matches!(edit, NoteEdit::SaveAndSend) {
                notes.out.push(ReviewIntent::SendToAgent);
            }
            session.close_line_editor();
        }
        NoteEdit::Delete => {
            notes.out.push(ReviewIntent::DeleteComment {
                pool,
                file: line.path.to_owned(),
                line: line.anchor.new.or(line.anchor.old),
            });
            session.close_line_editor();
        }
        NoteEdit::Idle => {}
    }
}

/// Saved note rendered as a compact identity-tinted card — the pool's icon beside
/// the note body, with an accent left edge — the whole surface clickable to
/// re-open its editor. Returns `true` on click.
fn note_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    style: &PoolStyle,
    note: &str,
    line: Option<u32>,
) -> bool {
    let inner = egui::Frame::new()
        .fill(with_alpha(style.color, 20))
        .inner_margin(egui::Margin::symmetric(9, 6))
        .corner_radius(egui::CornerRadius::same(RADIUS_PILL))
        .stroke(egui::Stroke::new(1.0_f32, with_alpha(style.color, 70)))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(LINE_SIZE, LINE_SIZE), egui::Sense::hover());
                crate::ui::paint_icon(
                    ui.painter(),
                    r.center(),
                    LINE_SIZE - 1.0,
                    style.icon,
                    style.color,
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(note)
                        .size(LINE_SIZE)
                        .color(palette.text_secondary),
                );
            });
        });
    let rect = inner.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
        egui::CornerRadius::same(RADIUS_PILL),
        style.color,
    );
    let response = ui
        .interact(
            rect,
            ui.id().with(("note_card", line, rect.min.y.to_bits())),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Edit review note")
    });
    response.clicked()
}

/// Pushes a save of `note` on `line` — or a delete when the note is blank.
fn save_note(out: &mut Vec<ReviewIntent>, line: &NoteLine<'_>, note: &str) {
    let NoteAnchor { pool, old, new } = line.anchor;
    if note.is_empty() {
        out.push(ReviewIntent::DeleteComment {
            pool,
            file: line.path.to_owned(),
            line: new.or(old),
        });
        return;
    }
    out.push(ReviewIntent::SaveComment {
        pool,
        file: line.path.to_owned(),
        comment: LineComment {
            old_lineno: old,
            new_lineno: new,
            code: line.code.to_owned(),
            note: note.to_owned(),
        },
    });
}

/// The header's recap of the agent batch: a chip with its count, toggling a popover
/// that lists every note and sends them. Nothing while the batch is empty.
pub(crate) fn review_recap(ui: &mut egui::Ui, notes: &mut NoteCtx<'_>, batch: &NoteBatch<'_>) {
    let n = count(batch.comments);
    if n == 0 {
        return;
    }
    let chip = review_chip(ui, notes.palette, n);
    egui::Popup::from_toggle_button_response(&chip)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| review_popover(ui, notes, batch));
}

/// Header chip — a Sparkles glyph and the comment count — that toggles the review
/// recap popover. Returns its response so the popover can anchor to it.
fn review_chip(ui: &mut egui::Ui, palette: &Palette, n: usize) -> egui::Response {
    let label = n.to_string();
    let font = egui::FontId::proportional(PILL_SIZE);
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font.clone(), egui::Color32::PLACEHOLDER);
    let icon_w = LINE_SIZE;
    let size = egui::vec2(icon_w + 4.0 + galley.size().x + 16.0, PILL_SIZE + 10.0);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let (fill, content) = if hovered {
        (with_alpha(palette.accent_ai, 36), palette.accent_ai)
    } else {
        (palette.bg_surface, palette.text_secondary)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        fill,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    crate::ui::paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 8.0 + icon_w / 2.0, rect.center().y),
        icon_w,
        lucide_icons::Icon::Sparkles,
        content,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 8.0 + icon_w + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        content,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Review notes"));
    response
}

/// Review recap popover: every stored note grouped by file, each editable in
/// place (click) and deletable (✕), with a Send-to-agent footer.
fn review_popover(ui: &mut egui::Ui, notes: &mut NoteCtx<'_>, batch: &NoteBatch<'_>) {
    let palette = notes.palette;
    ui.set_max_width(360.0);
    ui.spacing_mut().item_spacing.y = 6.0;
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for (file, file_comments) in batch.comments {
                ui.label(
                    egui::RichText::new(file)
                        .size(PILL_SIZE)
                        .color(palette.text_muted),
                );
                for comment in file_comments {
                    popover_note(ui, notes, file, comment);
                }
            }
        });
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if send_pill(ui, palette, batch.agent) {
            notes.out.push(ReviewIntent::SendToAgent);
        }
    });
}

/// One note of the recap: its line and code over the note, or its editor once clicked.
fn popover_note(ui: &mut egui::Ui, notes: &mut NoteCtx<'_>, file: &str, comment: &LineComment) {
    let palette = notes.palette;
    let line = comment.line_ref();
    let editing = notes
        .session
        .popover_edit
        .as_ref()
        .is_some_and(|(f, l)| f == file && *l == line);
    if editing {
        popover_editor(ui, notes, file, comment);
        return;
    }
    let loc = match line {
        Some(n) => format!("L{n}"),
        None => "·".to_owned(),
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(loc)
                .monospace()
                .size(PILL_SIZE)
                .color(palette.text_muted),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(truncate_code(&comment.code))
                    .monospace()
                    .size(PILL_SIZE)
                    .color(palette.text_muted),
            )
            .truncate(),
        );
    });
    ui.horizontal(|ui| {
        if icon_button(
            ui,
            palette,
            lucide_icons::Icon::Trash2,
            palette.git_deleted,
            "Delete review note",
        )
        .clicked()
        {
            notes.out.push(ReviewIntent::DeleteComment {
                pool: ReviewPool::Agent,
                file: file.to_owned(),
                line,
            });
        }
        let note = ui.add(
            egui::Label::new(
                egui::RichText::new(&comment.note)
                    .size(LINE_SIZE)
                    .color(palette.text_secondary),
            )
            .sense(egui::Sense::click()),
        );
        if note.clicked() {
            notes.session.open_in_popover(file, comment);
        }
    });
}

/// A recap note's editor, in place in the popover.
fn popover_editor(ui: &mut egui::Ui, notes: &mut NoteCtx<'_>, file: &str, comment: &LineComment) {
    let style = pool_style(notes.palette, ReviewPool::Agent);
    let session = &mut *notes.session;
    let edit = note_editor(
        ui,
        notes.palette,
        &style,
        &mut session.popover_buffer,
        &mut session.focus,
        ui.available_width(),
        true,
    );
    let line = NoteLine {
        path: file,
        anchor: NoteAnchor {
            pool: ReviewPool::Agent,
            old: comment.old_lineno,
            new: comment.new_lineno,
        },
        code: &comment.code,
        saved: Some(&comment.note),
    };
    match edit {
        NoteEdit::Save | NoteEdit::SaveAndSend => {
            save_note(notes.out, &line, session.popover_buffer.trim());
            if matches!(edit, NoteEdit::SaveAndSend) {
                notes.out.push(ReviewIntent::SendToAgent);
            }
            session.close_popover_editor();
        }
        NoteEdit::Delete => {
            notes.out.push(ReviewIntent::DeleteComment {
                pool: ReviewPool::Agent,
                file: file.to_owned(),
                line: comment.line_ref(),
            });
            session.close_popover_editor();
        }
        NoteEdit::Idle => {}
    }
}

/// Recap footer action: a Sparkles glyph and a "Send to {agent}" label in a pill
/// (the AI-call icon used by the commit message), hover-tinted to the accent.
fn send_pill(ui: &mut egui::Ui, palette: &Palette, agent: &str) -> bool {
    pill_button(
        ui,
        palette,
        palette.accent_ai,
        lucide_icons::Icon::Sparkles,
        &format!("Send to {agent}"),
        RADIUS_PILL,
    )
}

/// Single-line, trimmed-and-capped code snippet used as the anchor shown beside a
/// note in the recap popover.
fn truncate_code(code: &str) -> String {
    const MAX: usize = 40;
    let trimmed = code.trim();
    if trimmed.chars().count() > MAX {
        let head: String = trimmed.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_editor_hints_follow_the_app_badge_convention() {
        // The captions are literals (the two keys are hard-wired here, not rebindable
        // `Action`s), so they must be pinned to the convention the badges elsewhere
        // render — a change to `key_display` must not leave this editor behind.
        let send = crate::keybindings::Shortcut::cmd(egui::Key::Enter).display();
        assert_eq!(SHORTCUT_SEND, send);
        assert_eq!(SHORTCUT_SAVE, send.trim_start_matches('⌘'));
    }

    #[test]
    fn note_on_a_deleted_row_does_not_show_on_an_added_row_with_the_same_number() {
        let mut store = FileComments::new();
        crate::review::add_comment(
            &mut store,
            "f",
            LineComment {
                old_lineno: Some(5),
                new_lineno: None,
                code: "removed".into(),
                note: "for claude".into(),
            },
        );

        assert_eq!(note_at(&store, "f", Some(5), None), Some("for claude"));
        assert_eq!(note_at(&store, "f", None, Some(5)), None);
    }
}
