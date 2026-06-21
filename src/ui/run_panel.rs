//! Run terminal strip at the bottom of the git sidebar (git.md §3): a header with
//! the project's run command + Run/Stop/Relaunch controls and a read-only terminal
//! mirroring the server's output. The app owns the process and persistence; this
//! view only paints and emits intents.

use lucide_icons::Icon;

use crate::theme::{Palette, RADIUS_PILL};

/// Header strip height; also the collapsed panel height (git.md §3).
pub const HEADER_HEIGHT: f32 = 36.0;

const ICON_HIT: f32 = 24.0;
const ICON_GLYPH: f32 = 15.0;
const LABEL_SIZE: f32 = 12.0;
const COMMAND_SIZE: f32 = 12.0;
const STATUS_DOT_RADIUS: f32 = 4.0;
const PORT_FIELD_WIDTH: f32 = 48.0;

/// Live state of the Run process, derived by the app from `caches.run_panes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    /// No process: never started, or stopped.
    Stopped,
    /// Process running.
    Running,
    /// Command returned on its own.
    Exited,
    /// The PTY failed to spawn.
    Failed(String),
}

/// Intents emitted by the Run panel for the app to apply (git.md §3).
#[derive(Default)]
pub struct RunPanelAction {
    pub run: bool,
    pub stop: bool,
    pub relaunch: bool,
    pub toggle_collapsed: bool,
    /// Pencil clicked: open the inline command editor.
    pub begin_edit: bool,
    /// Inline edit confirmed (Enter / check): write the buffer to the project.
    pub commit_edit: bool,
    /// Inline edit abandoned (Esc / focus lost): drop the buffer unchanged.
    pub cancel_edit: bool,
    /// Port chip clicked: open the inline `$PORT` override editor (git.md §3).
    pub begin_port_edit: bool,
    /// Port edit confirmed: write the override to the worktree (empty ⇒ auto).
    pub commit_port_edit: bool,
    /// Port edit abandoned: drop the buffer unchanged.
    pub cancel_port_edit: bool,
}

impl RunPanelAction {
    /// Any intent was emitted this frame — lets the app skip the apply path when idle.
    pub fn any(&self) -> bool {
        self.run
            || self.stop
            || self.relaunch
            || self.toggle_collapsed
            || self.begin_edit
            || self.commit_edit
            || self.cancel_edit
            || self.begin_port_edit
            || self.commit_port_edit
            || self.cancel_port_edit
    }
}

/// Renders the Run strip. `command` is the resolved command shown when not editing
/// (empty ⇒ a "set a command" hint); `port` is the worktree's resolved `$PORT`
/// value, shown as a clickable chip when the command consumes it. `edit` /
/// `port_edit` are `Some` while their inline editor is open (mutually exclusive).
/// `body` paints the terminal viewer and is called only when expanded.
#[allow(clippy::too_many_arguments)]
pub fn run_panel(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RunStatus,
    command: &str,
    port: Option<u16>,
    collapsed: bool,
    edit: Option<&mut String>,
    port_edit: Option<&mut String>,
    body: impl FnOnce(&mut egui::Ui),
) -> RunPanelAction {
    let mut action = RunPanelAction::default();
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), HEADER_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let chevron = if collapsed {
                Icon::ChevronRight
            } else {
                Icon::ChevronDown
            };
            if icon_button(
                ui,
                palette,
                palette.text_primary,
                true,
                "Toggle run panel",
                chevron,
            ) {
                action.toggle_collapsed = true;
            }
            paint_status_dot(ui, status_color(palette, status));
            ui.label(
                egui::RichText::new("Run")
                    .size(LABEL_SIZE)
                    .color(palette.text_secondary),
            );

            // Right cluster first (right-to-left) so the command field takes the
            // remaining middle width; the port chip sits just left of the controls.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                run_controls(ui, palette, status, &mut action);
                port_field(ui, palette, port, port_edit, &mut action);
                command_field(ui, palette, command, edit, &mut action);
            });
        },
    );

    if !collapsed {
        ui.separator();
        // Pin the body to exactly the remaining height in a clipped child. A bottom
        // Panel persists its *content* rect: a shorter body snaps the strip back on
        // release, a taller one (the 24-row grid before it resizes to fit) ratchets
        // it open at launch. Reserving the space up front decouples the panel height
        // from the terminal content.
        let avail = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(rect);
        body(&mut child);
    }
    action
}

/// Run / Stop / Relaunch buttons by state (laid out right-to-left).
fn run_controls(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: &RunStatus,
    action: &mut RunPanelAction,
) {
    match status {
        RunStatus::Running => {
            if icon_button(
                ui,
                palette,
                palette.text_primary,
                true,
                "Relaunch",
                Icon::RotateCcw,
            ) {
                action.relaunch = true;
            }
            if icon_button(ui, palette, palette.git_deleted, true, "Stop", Icon::Square) {
                action.stop = true;
            }
        }
        _ => {
            if icon_button(ui, palette, palette.git_added, true, "Run", Icon::Play) {
                action.run = true;
            }
        }
    }
}

/// The command label, or — while editing — a text field confirmed by Enter / the
/// check button and abandoned on focus loss (Esc included).
fn command_field(
    ui: &mut egui::Ui,
    palette: &Palette,
    command: &str,
    edit: Option<&mut String>,
    action: &mut RunPanelAction,
) {
    match edit {
        Some(buffer) => {
            if icon_button(
                ui,
                palette,
                palette.accent,
                true,
                "Save command",
                Icon::Check,
            ) {
                action.commit_edit = true;
            }
            let field = egui::TextEdit::singleline(buffer)
                .desired_width(ui.available_width())
                .font(egui::TextStyle::Monospace)
                .hint_text("npm run dev");
            let resp = ui.add(field);
            // Read focus loss before re-grabbing: requesting focus back the same
            // frame would clear `lost_focus`, swallowing the Enter commit.
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action.commit_edit = true;
                } else {
                    action.cancel_edit = true;
                }
            } else if ui.memory(|m| m.focused().is_none()) {
                resp.request_focus();
            }
        }
        None => {
            if icon_button(
                ui,
                palette,
                palette.text_primary,
                true,
                "Edit run command",
                Icon::Pencil,
            ) {
                action.begin_edit = true;
            }
            let (text, color) = if command.is_empty() {
                ("Set a run command".to_owned(), palette.text_muted)
            } else {
                (command.to_owned(), palette.text_secondary)
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .monospace()
                        .size(COMMAND_SIZE)
                        .color(color),
                )
                .truncate(),
            );
        }
    }
}

/// The `:PORT` chip for this worktree, or — while editing — a small numeric field
/// confirmed by Enter / the check button and abandoned on focus loss. Shown only
/// when the command consumes `$PORT` (`port` is `Some`) (git.md §3).
fn port_field(
    ui: &mut egui::Ui,
    palette: &Palette,
    port: Option<u16>,
    port_edit: Option<&mut String>,
    action: &mut RunPanelAction,
) {
    match port_edit {
        Some(buffer) => {
            if icon_button(ui, palette, palette.accent, true, "Save port", Icon::Check) {
                action.commit_port_edit = true;
            }
            let field = egui::TextEdit::singleline(buffer)
                .desired_width(PORT_FIELD_WIDTH)
                .font(egui::TextStyle::Monospace)
                .hint_text("3000");
            let resp = ui.add(field);
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action.commit_port_edit = true;
                } else {
                    action.cancel_port_edit = true;
                }
            } else if ui.memory(|m| m.focused().is_none()) {
                resp.request_focus();
            }
        }
        None => {
            if let Some(port) = port {
                let chip = egui::Label::new(
                    egui::RichText::new(format!(":{port}"))
                        .monospace()
                        .size(COMMAND_SIZE)
                        .color(palette.text_secondary),
                )
                .sense(egui::Sense::click());
                if ui
                    .add(chip)
                    .on_hover_text("Set this worktree's port")
                    .clicked()
                {
                    action.begin_port_edit = true;
                }
            }
        }
    }
}

fn status_color(palette: &Palette, status: &RunStatus) -> egui::Color32 {
    match status {
        RunStatus::Running => palette.git_added,
        RunStatus::Failed(_) => palette.git_deleted,
        RunStatus::Stopped | RunStatus::Exited => palette.text_muted,
    }
}

fn paint_status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(STATUS_DOT_RADIUS * 2.0, ICON_HIT),
        egui::Sense::hover(),
    );
    ui.painter()
        .circle_filled(rect.center(), STATUS_DOT_RADIUS, color);
}

/// Lucide icon button, tinted to `intent` on hover — same affordance as the git
/// panel's, kept local so the Run strip doesn't depend on `git_panel` internals.
fn icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    intent: egui::Color32,
    enabled: bool,
    tooltip: &str,
    icon: Icon,
) -> bool {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(ICON_HIT, ICON_HIT), enabled);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.bg_surface_hover,
        );
    }
    let color = if !enabled {
        palette.state_disabled
    } else if hovered {
        intent
    } else {
        palette.text_muted
    };
    crate::ui::paint_icon(ui.painter(), rect.center(), ICON_GLYPH, icon, color);
    let response = response.on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tooltip));
    enabled && response.clicked()
}
