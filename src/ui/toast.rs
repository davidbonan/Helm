use std::path::PathBuf;

use crate::theme::{Palette, RADIUS_PILL};
use crate::ui::{paint_icon, with_alpha};

/// Lifetime of an auto-expiring toast (errors and successes alike); action
/// toasts persist until dismissed or acted on.
pub const TOAST_TTL: f64 = 4.0;
/// Stack cap: beyond it, the oldest is dropped.
pub const MAX_TOASTS: usize = 5;

const TOAST_WIDTH: f32 = 340.0;
const MARGIN: f32 = 12.0;
const PAD_X: i8 = 10;
const PAD_Y: i8 = 8;
const ICON_GLYPH: f32 = 14.0;
const CLOSE_HIT: f32 = 20.0;
const TEXT_SIZE: f32 = 12.5;
const BORDER_ALPHA: u8 = 96;
const DISMISS: &str = "Dismiss notification";
const ACTION_HEIGHT: f32 = 24.0;
const ACTION_PAD_X: f32 = 10.0;
const ACTION_GAP: f32 = 8.0;
const ACTION_FILL_ALPHA: u8 = 24;
const ACTION_FILL_HOVER_ALPHA: u8 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Error,
    Success,
    Info,
}

/// What a toast's action button does. Typed rather than a label the caller
/// re-matches: the overlay hands the deed back, so a second kind of action cannot be
/// mistaken for the updater's Install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastAction {
    /// Installs the downloaded update and relaunches (update.md §6).
    InstallUpdate,
    /// Opens the file in the configured external editor (git.md §4): the inline
    /// editor refused it, so the toast hands it to the real one.
    OpenInEditor(PathBuf),
}

impl ToastAction {
    fn label(&self) -> &'static str {
        match self {
            ToastAction::InstallUpdate => "Install",
            ToastAction::OpenInEditor(_) => "Open in editor",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
    /// egui clock (`ctx.input.time`) at creation — drives expiration.
    pub born: f64,
    /// Action offered under the message; a toast carrying one persists until
    /// dismissed or acted on (update.md §6 — the Install of the Available toast).
    pub action: Option<ToastAction>,
}

impl Toast {
    fn expires(&self) -> bool {
        self.action.is_none()
    }
}

/// Notification stack (git.md §10): errors and successes auto-expire (cross to
/// close early); action toasts persist until dismissed or acted on. An identical
/// message already shown is **refreshed** instead of being stacked — a repeated
/// failure (poll, re-click) never spams.
#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn error(&mut self, message: impl Into<String>, now: f64) {
        self.push(ToastKind::Error, message.into(), None, now);
    }

    pub fn success(&mut self, message: impl Into<String>, now: f64) {
        self.push(ToastKind::Success, message.into(), None, now);
    }

    /// Informational toast with an action button (e.g. "Update available vX" +
    /// Install): persists until dismissed or acted on.
    pub fn info_with_action(&mut self, message: impl Into<String>, action: ToastAction, now: f64) {
        self.push(ToastKind::Info, message.into(), Some(action), now);
    }

    fn push(&mut self, kind: ToastKind, message: String, action: Option<ToastAction>, now: f64) {
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|t| t.kind == kind && t.message == message)
        {
            existing.born = now;
            return;
        }
        self.items.push(Toast {
            kind,
            message,
            born: now,
            action,
        });
        if self.items.len() > MAX_TOASTS {
            self.items.remove(0);
        }
    }

    /// Expires errors and successes past [`TOAST_TTL`]; action toasts remain.
    pub fn tick(&mut self, now: f64) {
        self.items
            .retain(|t| !t.expires() || now - t.born < TOAST_TTL);
    }

    pub fn dismiss(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    pub fn items(&self) -> &[Toast] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Next expiration deadline (seconds from `now`), if at least one expiring
    /// toast is shown — the caller schedules a repaint on it.
    fn next_expiry(&self, now: f64) -> Option<f64> {
        self.items
            .iter()
            .filter(|t| t.expires())
            .map(|t| (t.born + TOAST_TTL - now).max(0.0))
            .min_by(|a, b| a.total_cmp(b))
    }
}

/// Toast overlay, anchored bottom-left above everything (all views, not just the
/// graph). Tick + render + repaint scheduled on the next expiration: a toast
/// disappears without interaction. Returns the action of the toast whose button was
/// clicked (the toast is then dismissed) — the caller carries it out.
pub fn toast_overlay(
    ctx: &egui::Context,
    palette: &Palette,
    toasts: &mut Toasts,
) -> Option<ToastAction> {
    let now = ctx.input(|i| i.time);
    toasts.tick(now);
    if toasts.is_empty() {
        return None;
    }
    if let Some(delay) = toasts.next_expiry(now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
    }
    let mut dismissed = None;
    let mut actioned = None;
    egui::Area::new(egui::Id::new("toast_overlay"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(MARGIN, -MARGIN))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for (index, toast) in toasts.items.iter().enumerate() {
                let (close, action) = toast_card(ui, palette, toast, index);
                if close {
                    dismissed = Some(index);
                }
                if action {
                    actioned = Some(index);
                }
            }
        });
    let action = actioned
        .and_then(|index| toasts.items.get(index))
        .and_then(|toast| toast.action.clone());
    if let Some(index) = actioned.or(dismissed) {
        toasts.dismiss(index);
    }
    action
}

/// A toast card: typed icon + message (wrapped, action button below it when
/// present) + close cross. Returns `(cross clicked, action clicked)`.
fn toast_card(ui: &mut egui::Ui, palette: &Palette, toast: &Toast, index: usize) -> (bool, bool) {
    let (accent, icon) = match toast.kind {
        ToastKind::Error => (palette.git_deleted, lucide_icons::Icon::AlertCircle),
        ToastKind::Success => (palette.git_added, lucide_icons::Icon::CheckCircle),
        ToastKind::Info => (palette.accent, lucide_icons::Icon::Info),
    };
    let mut action_clicked = false;
    let close = egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0_f32, with_alpha(accent, BORDER_ALPHA)))
        .corner_radius(RADIUS_PILL)
        .inner_margin(egui::Margin::symmetric(PAD_X, PAD_Y))
        .show(ui, |ui| {
            ui.set_width(TOAST_WIDTH);
            ui.horizontal_top(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::vec2(ICON_GLYPH, CLOSE_HIT), egui::Sense::hover());
                paint_icon(ui.painter(), icon_rect.center(), ICON_GLYPH, icon, accent);
                let label_width = ui.available_width() - CLOSE_HIT - ui.spacing().item_spacing.x;
                ui.scope(|ui| {
                    ui.set_width(label_width);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&toast.message)
                                .size(TEXT_SIZE)
                                .color(palette.text_primary),
                        )
                        .wrap(),
                    );
                    if let Some(action) = &toast.action {
                        ui.add_space(ACTION_GAP);
                        action_clicked = action_button(ui, accent, action.label());
                    }
                });
                let (close_rect, close, hovered) =
                    crate::ui::clickable(ui, egui::vec2(CLOSE_HIT, CLOSE_HIT), true);
                let ink = if hovered {
                    palette.text_primary
                } else {
                    palette.text_secondary
                };
                paint_icon(
                    ui.painter(),
                    close_rect.center(),
                    ICON_GLYPH,
                    lucide_icons::Icon::X,
                    ink,
                );
                close.widget_info(move || {
                    let mut info =
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, DISMISS);
                    info.label = Some(format!("{DISMISS} {index}"));
                    info
                });
                close
            })
            .inner
        })
        .inner;
    (close.clicked(), action_clicked)
}

/// Accent pill button under the message, sized to its label.
fn action_button(ui: &mut egui::Ui, accent: egui::Color32, label: &str) -> bool {
    let font = egui::FontId::proportional(TEXT_SIZE);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), accent)
        .size()
        .x;
    let size = egui::vec2(ACTION_PAD_X * 2.0 + text_width, ACTION_HEIGHT);
    let (rect, response, hovered) = crate::ui::clickable(ui, size, true);
    let fill = if hovered {
        with_alpha(accent, ACTION_FILL_HOVER_ALPHA)
    } else {
        with_alpha(accent, ACTION_FILL_ALPHA)
    };
    ui.painter().rect(
        rect,
        RADIUS_PILL,
        fill,
        egui::Stroke::new(1.0_f32, with_alpha(accent, BORDER_ALPHA)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        accent,
    );
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_successes_both_expire() {
        let mut toasts = Toasts::default();
        toasts.error("Stash failed — nothing to stash", 0.0);
        toasts.success("Pushed", 0.0);
        toasts.tick(TOAST_TTL + 0.1);
        assert!(toasts.is_empty());
    }

    #[test]
    fn duplicate_message_refreshes_instead_of_stacking() {
        let mut toasts = Toasts::default();
        toasts.success("Pulled — already up to date", 0.0);
        toasts.success("Pulled — already up to date", 3.0);
        assert_eq!(toasts.items().len(), 1);
        assert_eq!(toasts.items()[0].born, 3.0);
        // Refreshed at t=3 ⇒ alive at t=5 (would expire at t=4 without refresh).
        toasts.tick(5.0);
        assert_eq!(toasts.items().len(), 1);
    }

    #[test]
    fn action_toasts_persist() {
        let mut toasts = Toasts::default();
        toasts.info_with_action("Update available v0.2.0", ToastAction::InstallUpdate, 0.0);
        toasts.tick(TOAST_TTL + 1.0);
        assert_eq!(toasts.items().len(), 1);
        assert_eq!(
            toasts.items()[0].action,
            Some(ToastAction::InstallUpdate),
            "the deed travels with the toast, not a label to re-match"
        );
        assert_eq!(toasts.next_expiry(0.0), None, "action toasts do not expire");
    }

    #[test]
    fn same_message_different_kind_stacks() {
        let mut toasts = Toasts::default();
        toasts.error("Pulled", 0.0);
        toasts.success("Pulled", 0.0);
        assert_eq!(toasts.items().len(), 2);
    }

    #[test]
    fn stack_is_capped_dropping_the_oldest() {
        let mut toasts = Toasts::default();
        for i in 0..=MAX_TOASTS {
            toasts.error(format!("error {i}"), i as f64);
        }
        assert_eq!(toasts.items().len(), MAX_TOASTS);
        assert_eq!(toasts.items()[0].message, "error 1");
    }

    #[test]
    fn dismiss_removes_the_target_and_tolerates_stale_index() {
        let mut toasts = Toasts::default();
        toasts.error("a", 0.0);
        toasts.error("b", 0.0);
        toasts.dismiss(0);
        assert_eq!(toasts.items()[0].message, "b");
        toasts.dismiss(7);
        assert_eq!(toasts.items().len(), 1);
    }

    #[test]
    fn next_expiry_tracks_the_earliest_expiring_toast() {
        let mut toasts = Toasts::default();
        assert_eq!(toasts.next_expiry(0.0), None);
        toasts.info_with_action("Update available", ToastAction::InstallUpdate, 0.0);
        assert_eq!(toasts.next_expiry(0.0), None, "action toasts do not expire");
        toasts.success("late", 2.0);
        toasts.error("early", 1.0);
        assert_eq!(toasts.next_expiry(2.0), Some(TOAST_TTL - 1.0));
    }
}
