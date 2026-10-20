pub mod agents_view;
pub mod browser_panel;
pub(crate) mod browser_pip;
pub(crate) mod browser_start_page;
pub mod command_palette;
pub mod commit_detail;
pub mod conflict_view;
pub mod detail;
pub mod diff_view;
pub mod emoji;
pub mod feedback_modal;
pub mod file_list;
pub mod file_tree;
pub mod file_viewer;
pub mod git_panel;
pub mod graph_toolbar;
pub mod graph_view;
pub mod inline_editor;
pub mod phone_access_modal;
pub mod preferences;
pub mod pull_requests_view;
pub mod rebase_view;
pub mod release_notes;
pub mod repo_sidebar;
pub mod review_notes;
pub mod run_panel;
pub(crate) mod sidebar_block;
pub(crate) mod sidebar_footer;
pub mod spinner;
pub mod syntax_highlight;
pub mod tab_bar;
pub mod terminal_view;
pub mod text_selection;
pub mod toast;

use std::path::Path;

use egui::emath::GuiRounding as _;

use crate::agent_watch::AgentBadge;
use crate::git::commit_detail::CommitDetail;
use crate::git::status::RepoStatus;
use crate::keybindings::{Action, Keymap};
use crate::theme::{Palette, RADIUS_MENU_ITEM, RADIUS_PILL, SHORTCUT_BADGE_SIZE};
use crate::ui::commit_detail::commit_detail_panel;
use crate::ui::file_list::{FileMenuOutput, FileViewMode};
use crate::ui::file_tree::{files_sidebar, FileTreeState};
use crate::ui::git_panel::{git_panel, GitIntent, GitPanelState};
use crate::ui::repo_sidebar::{
    repo_sidebar, DoneAgentRow, ProjectVisibility, SidebarAction, SidebarItem,
};
use crate::ui::sidebar_block::{block_panel_frame, sidebar_block, BLOCK_GAP};
use crate::ui::sidebar_footer::{sidebar_footer, SidebarFooter, FOOTER_HEIGHT};
pub use crate::ui::sidebar_footer::{PhoneIndicator, PHONE_ACCESS_LABEL};
use crate::workspace_launcher::WorkspaceOpener;

const LEFT_SIDEBAR_ID: &str = "repos";
const LEFT_SIDEBAR_FOOTER_ID: &str = "repos_footer";
const RIGHT_SIDEBAR_ID: &str = "git";
const BROWSER_SIDEBAR_ID: &str = "browser";
const BROWSER_SIDEBAR_MIN_WIDTH: f32 = 320.0;
const FILES_SIDEBAR_ID: &str = "files";
const FILES_SIDEBAR_MIN_WIDTH: f32 = 200.0;
const GIT_SIDEBAR_MIN_WIDTH: f32 = 260.0;
/// Width the terminals keep beside the browser and files sidebars: their title
/// row still holds the sidebar actions and a compact switch.
const SIDEBAR_CENTRAL_RESERVE: f32 = 280.0;
const RUN_PANEL_ID: &str = "run_terminal_panel";
/// Folded Run strip: a separate panel id so collapsing leaves the expanded
/// panel's remembered height untouched (git.md §3).
const RUN_PANEL_COLLAPSED_ID: &str = "run_terminal_panel_collapsed";
/// Smallest expanded Run strip: header + a few lines of output (git.md §3).
const RUN_PANEL_MIN_HEIGHT: f32 = 120.0;
/// Height kept for the git content above the Run strip when it's dragged tall.
const RUN_PANEL_RESERVE: f32 = 200.0;

const SECTION_HEADER_SIZE: f32 = 11.0;
const SECTION_HEADER_TRACKING: f32 = 0.04;
pub(crate) const SECTION_TOP_MARGIN: f32 = 16.0;
/// `⌃⌘1..9` / `⌘1..9` badge cap (keybindings §1), shared by the repo sidebar
/// and the tab bar.
pub(crate) const MAX_SHORTCUT: usize = 9;
pub(crate) const SIDEBAR_PAD_X: i8 = 12;
const SIDEBAR_PAD_Y: i8 = 8;
// Hidden titlebar (design-system §3): the macOS traffic lights float over the
// top-left corner. MACOS_TITLEBAR_INSET is their vertical band; the header
// controls share that line (centered on it) rather than dropping to a second
// row, so the whole title row is a single TITLEBAR_HEIGHT strip and the content
// reserves only that. TRAFFIC_LIGHTS_RESERVE keeps the sidebar toggle clear of
// the native lights.
pub(crate) const MACOS_TITLEBAR_INSET: i8 = 28;
pub(crate) const TITLEBAR_HEIGHT: i8 = 40;
const TRAFFIC_LIGHTS_RESERVE: f32 = 78.0;

pub(crate) fn section_label(palette: &Palette, text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .size(SECTION_HEADER_SIZE)
        .extra_letter_spacing(SECTION_HEADER_SIZE * SECTION_HEADER_TRACKING)
        .color(palette.text_muted)
}

pub(crate) fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

fn h_scroll_room_id() -> egui::Id {
    egui::Id::new("h_scroll_room")
}

/// Raised by a horizontally-scrollable surface when the pointer sits over it **and** it
/// still has room to its left. A rightward trackpad swipe there is that surface's
/// scroll, not a navigation gesture — the same rule Safari uses before it will go back.
/// Set while the frame's body renders, read once after it, which clears it.
pub(crate) fn note_h_scroll_room(ctx: &egui::Context, viewport: egui::Rect, offset_x: f32) {
    if offset_x <= 0.5
        || !ctx
            .pointer_latest_pos()
            .is_some_and(|at| viewport.contains(at))
    {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(h_scroll_room_id(), true));
}

/// Whether [`note_h_scroll_room`] fired this frame, clearing it for the next.
pub(crate) fn h_scroll_owns_swipe(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(h_scroll_room_id()))
        .unwrap_or(false)
}

/// Shared interaction acquisition for custom clickable widgets: allocates
/// `size` with `Sense::click` when `enabled` (otherwise `hover` — the widget
/// stays hoverable for its tooltip), sets the pointer cursor on hover and
/// returns the hover flag already filtered by `enabled`.
pub(crate) fn clickable(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    enabled: bool,
) -> (egui::Rect, egui::Response, bool) {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(size, sense);
    if enabled {
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let hovered = enabled && response.hovered();
    (rect, response, hovered)
}

/// Centered Lucide glyph, rendered as text (the font is a fallback of the
/// proportional family — `theme::font_definitions`). `size` = font size: the
/// glyph occupies ~80% of the em.
pub(crate) fn paint_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    icon: lucide_icons::Icon,
    color: egui::Color32,
) {
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        icon.unicode().to_string(),
        egui::FontId::proportional(size),
        color,
    );
}

/// Share of the em a Lucide glyph's ink spans — what a Nerd Font glyph is fitted to.
const LUCIDE_INK: f32 = 0.8;

/// What stands before a file's name (files.md §3.1).
#[derive(Debug, Clone, Copy)]
pub(crate) enum FileIcon {
    Lucide {
        icon: lucide_icons::Icon,
        color: egui::Color32,
    },
    /// A file type's glyph, drawn from the mono family: there the Nerd Font face,
    /// not Lucide, serves the private-use area they share.
    NerdFont { glyph: char, color: egui::Color32 },
}

impl FileIcon {
    /// Centred in `icon_box`, as large as a Lucide glyph of the box's height.
    pub(crate) fn paint(self, painter: &egui::Painter, icon_box: egui::Rect) {
        match self {
            Self::Lucide { icon, color } => {
                paint_icon(painter, icon_box.center(), icon_box.height(), icon, color);
            }
            Self::NerdFont { glyph, color } => {
                let Some((galley, ink)) = fitted_nerd_glyph(painter, glyph, icon_box.height())
                else {
                    return;
                };
                let pos = (icon_box.center() - ink.center().to_vec2())
                    .round_to_pixels(painter.pixels_per_point());
                painter.galley(pos, galley, color);
            }
        }
    }
}

/// `glyph` laid out so its ink spans what a Lucide glyph's does at `size`, with
/// that ink's rect in the galley; `None` when the glyph has no ink.
fn fitted_nerd_glyph(
    painter: &egui::Painter,
    glyph: char,
    size: f32,
) -> Option<(std::sync::Arc<egui::Galley>, egui::Rect)> {
    let layout = |font_size| {
        painter.layout_no_wrap(
            glyph.to_string(),
            egui::FontId::monospace(font_size),
            egui::Color32::PLACEHOLDER,
        )
    };
    let probe = glyph_ink(&layout(size))?;
    let galley = layout(size * size * LUCIDE_INK / probe.width().max(probe.height()));
    let ink = glyph_ink(&galley)?;
    Some((galley, ink))
}

/// Where the first glyph of `galley` leaves ink, in galley coordinates.
fn glyph_ink(galley: &egui::Galley) -> Option<egui::Rect> {
    let row = galley.rows.first()?;
    let glyph = row.glyphs.first()?;
    (!glyph.uv_rect.is_nothing()).then(|| {
        egui::Rect::from_min_size(
            row.pos + glyph.pos.to_vec2() + glyph.uv_rect.offset,
            glyph.uv_rect.size,
        )
    })
}

/// `YYYY-MM-DD` date (UTC) from Unix seconds.
pub fn format_date(epoch_secs: i64) -> String {
    let (year, month, day) = civil_from_epoch(epoch_secs);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `DD/MM/YYYY @ HH:MM` — "authored" line of the commit detail, same calendar
/// as [`format_date`].
pub fn format_date_time(epoch_secs: i64) -> String {
    let (year, month, day) = civil_from_epoch(epoch_secs);
    let secs = epoch_secs.rem_euclid(86_400);
    format!(
        "{day:02}/{month:02}/{year:04} @ {:02}:{:02}",
        secs / 3_600,
        (secs % 3_600) / 60
    )
}

/// Civil (year, month, day) in UTC, with no calendar dependency (roadmap M9: no
/// new crate). Howard Hinnant's algorithm (inverse `days_from_civil`, valid
/// domain ±~5.8M years).
fn civil_from_epoch(epoch_secs: i64) -> (i64, i64, i64) {
    let days = epoch_secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

fn workspace_sidebar_fill(palette: &Palette) -> egui::Color32 {
    palette.bg_sidebar
}

/// Keyboard navigation step within a list (keybindings §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowNav {
    Up,
    Down,
}

/// Arrow ↑/↓ pressed **without modifier** this frame — list navigation (graph,
/// files of a commit; keybindings §3).
pub fn arrow_nav_pressed(ui: &egui::Ui) -> Option<ArrowNav> {
    ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if modifiers.is_none() => match key {
                egui::Key::ArrowUp => Some(ArrowNav::Up),
                egui::Key::ArrowDown => Some(ArrowNav::Down),
                _ => None,
            },
            _ => None,
        })
    })
}

pub fn section_header(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.add_space(SECTION_TOP_MARGIN);
    ui.label(section_label(palette, text));
}

const EMPTY_STATE_TITLE: &str = "Open a project to get started";
const EMPTY_STATE_SUBTITLE: &str = "Terminal splits and Git staging, per project";
const EMPTY_STATE_LABEL: &str = "Open Folder…";
const EMPTY_STATE_TITLE_SIZE: f32 = 24.0;
const EMPTY_STATE_SUBTITLE_SIZE: f32 = 15.0;
const EMPTY_STATE_LABEL_SIZE: f32 = 15.0;
const EMPTY_STATE_BUTTON_HEIGHT: f32 = 30.0;
const EMPTY_STATE_TEXT_MARGIN: f32 = 32.0;
const EMPTY_STATE_ICON_SIZE: f32 = 40.0;
const EMPTY_STATE_DROP_HINT: &str = "or drop a folder here";

/// Central empty state (design-system §2): a helm glyph, welcome title + tagline,
/// primary folder-open button + shortcut reminder, and a drag-and-drop hint. A
/// folder dragged over the window highlights the zone; the import itself lands in
/// `ui()`. Returns `true` on click (same path as ⌘O).
pub fn central_empty_state(ui: &mut egui::Ui, palette: &Palette, keymap: &Keymap) -> bool {
    let mut clicked = false;
    let dragging_folder = ui.input(|i| !i.raw.hovered_files.is_empty());
    if dragging_folder {
        ui.painter().rect(
            ui.max_rect().shrink(16.0),
            egui::CornerRadius::same(12),
            with_alpha(palette.accent, 18),
            egui::Stroke::new(1.5_f32, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - EMPTY_STATE_BUTTON_HEIGHT * 2.0).max(0.0));
        ui.label(
            egui::RichText::new(lucide_icons::Icon::ShipWheel.unicode().to_string())
                .font(egui::FontId::proportional(EMPTY_STATE_ICON_SIZE))
                .color(palette.text_muted),
        );
        ui.add_space(12.0);
        // Both sidebars open on a small window can squeeze the central zone below
        // the title's intrinsic width: the texts yield (instead of wrapping letter
        // by letter), the button + shortcut keep the action reachable.
        let title_width = ui
            .painter()
            .layout_no_wrap(
                EMPTY_STATE_TITLE.to_owned(),
                egui::FontId::proportional(EMPTY_STATE_TITLE_SIZE),
                egui::Color32::TRANSPARENT,
            )
            .size()
            .x;
        if ui.available_width() >= title_width + EMPTY_STATE_TEXT_MARGIN {
            ui.label(
                egui::RichText::new(EMPTY_STATE_TITLE)
                    .size(EMPTY_STATE_TITLE_SIZE)
                    .color(palette.text_primary),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(EMPTY_STATE_SUBTITLE)
                    .size(EMPTY_STATE_SUBTITLE_SIZE)
                    .color(palette.text_secondary),
            );
            ui.add_space(20.0);
        }
        clicked = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(EMPTY_STATE_LABEL)
                        .size(EMPTY_STATE_LABEL_SIZE)
                        .color(egui::Color32::WHITE),
                )
                .fill(palette.primary_button_fill())
                .min_size(egui::vec2(0.0, EMPTY_STATE_BUTTON_HEIGHT))
                // Below the button's own width, truncate rather than wrap the
                // label letter by letter (same squeeze as the title above).
                .wrap_mode(egui::TextWrapMode::Truncate),
            )
            .clicked();
        if let Some(shortcut) = keymap.shortcut_for(Action::OpenFolder) {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(shortcut.display()).color(palette.text_muted));
        }
        ui.add_space(8.0);
        let (hint, color) = if dragging_folder {
            ("Drop to open", palette.accent)
        } else {
            (EMPTY_STATE_DROP_HINT, palette.text_muted)
        };
        ui.label(egui::RichText::new(hint).color(color));
    });
    clicked
}

/// Destructive button for confirmation modals (Discard / Delete): white label
/// on `git.deleted` (design-system §4).
pub(crate) fn danger_button(palette: &Palette, label: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE))
        .fill(palette.git_deleted)
}

/// Padding between the modal border and its content (design-system §4) — egui's
/// `Frame::popup` default (`menu_margin` = 6) is too tight for a dialog.
const MODAL_PADDING: i8 = 16;
/// Controls inside a modal keep the discreet rounding of the mockup instead of
/// the pill default (design-system §4).
const MODAL_CONTROL_RADIUS: u8 = 6;

/// Modal chrome (design-system §4): popup colors with comfortable inner padding.
pub(crate) fn modal_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style).inner_margin(MODAL_PADDING)
}

/// Confirmation modals treat `Enter`/`Return` as the primary action — the
/// danger-button equivalent, mirroring `Esc`/click-outside for Cancel.
pub(crate) fn modal_confirm_pressed(ui: &egui::Ui) -> bool {
    ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Applies [`MODAL_CONTROL_RADIUS`] to the interactive widgets (buttons, inputs,
/// rows) of the modal's content `Ui`.
pub(crate) fn modal_controls_style(ui: &mut egui::Ui) {
    let widgets = &mut ui.style_mut().visuals.widgets;
    for ws in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
    ] {
        ws.corner_radius = egui::CornerRadius::same(MODAL_CONTROL_RADIUS);
    }
}

const SWITCH_HEIGHT: f32 = 28.0;
const SWITCH_SEG_PAD_X: f32 = 12.0;
const SWITCH_LABEL_SIZE: f32 = 13.0;
const SWITCH_ICON_SIZE: f32 = 15.0;
const SWITCH_ICON_GAP: f32 = 6.0;
// Accent bottom border of the active segment — the selection indicator
// (design-system §4); slightly thicker than the container's 1px frame.
const SWITCH_ACTIVE_BORDER: f32 = 2.0;
// How far the accent border rides up the rounded corner: shallow so it reads as
// a bottom border, not a corner wrap.
const SWITCH_BORDER_CLIMB: f32 = 2.5;
// Width reserved for the badge, aligned with the tab bar's BADGE_W.
const SWITCH_BADGE_W: f32 = 38.0;
// Reserve on each side of the centered icon+label: the badge shows in the
// target segment's right reserve so nothing shifts when Cmd toggles
// (keybindings §5).
const SWITCH_SEG_RESERVE: f32 = SWITCH_SEG_PAD_X + SWITCH_BADGE_W;
const SWITCH_TERMINAL: &str = "Terminal";
const SWITCH_GRAPH: &str = "Git";
// Gap kept between the project/worktree reminder and the centered switch so the
// reminder never reaches it when the panel narrows.
const SWITCH_LABEL_GAP: f32 = 12.0;
// Reminder reads as quiet metadata: a notch below the switch labels' size.
const REMINDER_SIZE: f32 = 11.0;

/// Segmented "Terminal ⇄ Git" switch in the central area header
/// (design-system §4, M9-4), horizontally centered. `graph_active` marks the
/// current segment; clicking the other segment returns `Some(true|false)` (new
/// Graph state). Pure rendering: the mode toggle is arbitrated by the caller
/// (`HelmApp`).
pub fn central_switch(
    ui: &mut egui::Ui,
    palette: &Palette,
    graph_active: bool,
    keymap: &Keymap,
    project: Option<&str>,
    worktree: Option<&str>,
    workspace_shown: bool,
) -> Option<bool> {
    let mut requested = None;
    // The switch shares the traffic-light line like the side controls; the row
    // then pads down to TITLEBAR_HEIGHT so the central content aligns with the
    // side panels.
    let row_top = (TITLE_LINE_Y - SWITCH_HEIGHT / 2.0 - ui.cursor().top()).max(0.0);
    ui.add_space(row_top);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), SWITCH_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            // Cmd held (alone) ⇒ toggle badge in the toggle's target segment;
            // unbound ⇒ no badge (keybindings §5).
            let room = room_before_title_actions(ui);
            let fit = switch_fit(ui, room);
            // A compact segment has no room for the badge (keybindings §5).
            let badge = keymap
                .shortcut_for(Action::ToggleGraph)
                .filter(|_| cmd_held(ui) && fit == SwitchFit::Labeled)
                .map(|s| s.display());
            let terminal_w = segment_width(ui, SWITCH_TERMINAL, fit);
            let total = terminal_w + segment_width(ui, SWITCH_GRAPH, fit);
            let centered = (ui.available_width() - total) / 2.0;
            let gutter = centered.min(room - total).max(0.0);
            if let Some(project) = project {
                paint_project_reminder(ui, palette, project, worktree, workspace_shown, gutter);
            }
            // A single rounded frame holds both segments; the active one is marked
            // by an accent bottom border, not a fill (design-system §4).
            let row = ui.max_rect();
            let container = egui::Rect::from_min_size(
                egui::pos2(row.left() + gutter, row.top()),
                egui::vec2(total, SWITCH_HEIGHT),
            );
            ui.painter().rect(
                container,
                egui::CornerRadius::same(RADIUS_PILL),
                palette.bg_surface,
                egui::Stroke::new(1.0_f32, palette.border_subtle),
                egui::StrokeKind::Inside,
            );
            ui.add_space(gutter);
            let terminal = switch_segment(
                ui,
                palette,
                Segment {
                    label: SWITCH_TERMINAL,
                    icon: lucide_icons::Icon::SquareTerminal,
                    active: !graph_active,
                    badge: badge.as_deref().filter(|_| graph_active),
                    fit,
                },
            );
            if terminal.clicked() && graph_active {
                requested = Some(false);
            }
            let graph = switch_segment(
                ui,
                palette,
                Segment {
                    label: SWITCH_GRAPH,
                    icon: lucide_icons::Icon::GitBranch,
                    active: graph_active,
                    badge: badge.as_deref().filter(|_| !graph_active),
                    fit,
                },
            );
            if graph.clicked() && !graph_active {
                requested = Some(true);
            }
            paint_active_border(ui, palette, container, terminal_w, graph_active);
        },
    );
    ui.add_space(f32::from(TITLEBAR_HEIGHT) - row_top - SWITCH_HEIGHT);
    requested
}

/// How the switch's segments are drawn: labeled, or icons only when the labels
/// would run under the title-row actions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SwitchFit {
    Labeled,
    Compact,
}

fn switch_fit(ui: &egui::Ui, room: f32) -> SwitchFit {
    let labeled = segment_width(ui, SWITCH_TERMINAL, SwitchFit::Labeled)
        + segment_width(ui, SWITCH_GRAPH, SwitchFit::Labeled);
    if labeled <= room {
        SwitchFit::Labeled
    } else {
        SwitchFit::Compact
    }
}

struct Segment<'a> {
    label: &'a str,
    icon: lucide_icons::Icon,
    active: bool,
    badge: Option<&'a str>,
    fit: SwitchFit,
}

/// Width from the row's left edge to the title-row actions drawn last frame, less
/// the gap the switch keeps from them.
fn room_before_title_actions(ui: &egui::Ui) -> f32 {
    let row = ui.max_rect();
    let actions = ui.ctx().memory(|m| m.area_rect(title_actions_id()));
    let right = actions.map_or(row.right(), |actions| {
        actions.left().min(row.right()) - SWITCH_LABEL_GAP
    });
    right - row.left()
}

/// Accent bottom border under the active segment: a thicker restroke of the
/// container's rounded path, clipped to the active segment's lower band so the
/// accent stays on the bottom edge. The band is shallow (`SWITCH_BORDER_CLIMB`)
/// so the accent barely rides up the rounded corner; it ends straight at the
/// segment boundary, reaching both sides.
fn paint_active_border(
    ui: &egui::Ui,
    palette: &Palette,
    container: egui::Rect,
    terminal_w: f32,
    graph_active: bool,
) {
    let boundary = container.left() + terminal_w;
    let (left, right) = if graph_active {
        (boundary, container.right())
    } else {
        (container.left(), boundary)
    };
    let band = egui::Rect::from_min_max(
        egui::pos2(left, container.bottom() - SWITCH_BORDER_CLIMB),
        egui::pos2(right, container.bottom()),
    );
    ui.painter().with_clip_rect(band).rect_stroke(
        container,
        egui::CornerRadius::same(RADIUS_PILL),
        egui::Stroke::new(SWITCH_ACTIVE_BORDER, palette.accent),
        egui::StrokeKind::Inside,
    );
}

/// Left inset of central content that shares the **title row** with the floating window
/// controls: the sidebar's own padding while the workspace sidebar holds the window's
/// left edge, else clear of the macOS traffic lights and of the sidebar toggle, which then
/// float over the central area (in fullscreen the lights are hidden and the toggle hugs
/// the edge — mirrors `root_layout`'s `toggle_x`).
pub(crate) fn titlebar_content_inset(ui: &egui::Ui, workspace_shown: bool) -> f32 {
    if workspace_shown {
        return f32::from(SIDEBAR_PAD_X);
    }
    let fullscreen = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
    (if fullscreen {
        8.0
    } else {
        TRAFFIC_LIGHTS_RESERVE
    }) + TOGGLE_HIT.x
        + 8.0
}

/// Project (and worktree, when the active entry is one) reminder painted in the
/// left gutter of the switch row, left-aligned and truncated so it never reaches
/// the centered switch. A painter overlay: it doesn't consume layout, so the
/// switch stays centered regardless of the name's length. Inside a worktree the
/// reminder reads `project / worktree`: the worktree is the current location
/// (prominent) and survives truncation — the project context is truncated first.
fn paint_project_reminder(
    ui: &egui::Ui,
    palette: &Palette,
    project: &str,
    worktree: Option<&str>,
    workspace_shown: bool,
    gutter: f32,
) {
    let inset = titlebar_content_inset(ui, workspace_shown);
    let max_width = gutter - inset - SWITCH_LABEL_GAP;
    if max_width <= 0.0 {
        return;
    }
    let font = egui::FontId::proportional(REMINDER_SIZE);
    let truncated = |text: &str, color, width: f32| {
        let mut job = egui::text::LayoutJob::single_section(
            text.to_owned(),
            egui::text::TextFormat::simple(font.clone(), color),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        ui.painter().layout_job(job)
    };
    let row = ui.max_rect();
    let cy = row.center().y;
    let left = row.left() + inset;

    let Some(worktree) = worktree else {
        // No worktree: the project alone is the location — truncate from the end.
        let galley = truncated(project, palette.text_secondary, max_width);
        ui.painter().galley(
            egui::pos2(left, cy - galley.size().y / 2.0),
            galley,
            palette.text_secondary,
        );
        return;
    };

    // The worktree (prominent) must survive; the project context (muted) and its
    // separator are what get truncated first to keep the worktree readable.
    let tail = truncated(worktree, palette.text_secondary, max_width);
    let sep = truncated(" / ", palette.text_muted, max_width);
    let mut x = left;
    if max_width - tail.size().x - sep.size().x > 0.0 {
        let head = truncated(
            project,
            palette.text_muted,
            max_width - tail.size().x - sep.size().x,
        );
        let head_w = head.size().x;
        ui.painter().galley(
            egui::pos2(x, cy - head.size().y / 2.0),
            head,
            palette.text_muted,
        );
        x += head_w;
        let sep_w = sep.size().x;
        ui.painter().galley(
            egui::pos2(x, cy - sep.size().y / 2.0),
            sep,
            palette.text_muted,
        );
        x += sep_w;
    }
    ui.painter().galley(
        egui::pos2(x, cy - tail.size().y / 2.0),
        tail,
        palette.text_secondary,
    );
}

fn segment_width(ui: &egui::Ui, label: &str, fit: SwitchFit) -> f32 {
    if fit == SwitchFit::Compact {
        return SWITCH_ICON_SIZE + SWITCH_SEG_PAD_X * 2.0;
    }
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(SWITCH_LABEL_SIZE),
        egui::Color32::TRANSPARENT,
    );
    SWITCH_ICON_SIZE + SWITCH_ICON_GAP + galley.size().x + SWITCH_SEG_RESERVE * 2.0
}

fn switch_segment(ui: &mut egui::Ui, palette: &Palette, segment: Segment<'_>) -> egui::Response {
    // Stable id per segment: the badge's `new_child` (conditional on Cmd) would
    // otherwise shift the next segment's auto-id — egui then paints a red
    // "widget changed id between passes" box (same trap as the tab bar).
    ui.push_id(segment.label, |ui| {
        switch_segment_body(ui, palette, segment)
    })
    .inner
}

fn switch_segment_body(
    ui: &mut egui::Ui,
    palette: &Palette,
    segment: Segment<'_>,
) -> egui::Response {
    let Segment {
        label,
        icon,
        active,
        badge,
        fit,
    } = segment;
    let (rect, response, hovered) = clickable(
        ui,
        egui::vec2(segment_width(ui, label, fit), SWITCH_HEIGHT),
        true,
    );
    let label_color = switch_label_color(palette, active, hovered);
    let icon_color = switch_icon_color(palette, active, hovered);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label));
    if fit == SwitchFit::Compact {
        paint_icon(
            ui.painter(),
            rect.center(),
            SWITCH_ICON_SIZE,
            icon,
            icon_color,
        );
        return response.on_hover_text(label);
    }
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(SWITCH_LABEL_SIZE),
        label_color,
    );
    let content_width = SWITCH_ICON_SIZE + SWITCH_ICON_GAP + galley.size().x;
    let content_left = rect.center().x - content_width / 2.0;
    paint_icon(
        ui.painter(),
        egui::pos2(content_left + SWITCH_ICON_SIZE / 2.0, rect.center().y),
        SWITCH_ICON_SIZE,
        icon,
        icon_color,
    );
    ui.painter().galley(
        egui::pos2(
            content_left + SWITCH_ICON_SIZE + SWITCH_ICON_GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        label_color,
    );
    if let Some(badge) = badge {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("shortcut_badge_central_switch")
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(rect.right() - SWITCH_SEG_RESERVE, rect.top()),
                    egui::pos2(rect.right() - SWITCH_SEG_PAD_X, rect.bottom()),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        child.label(
            egui::RichText::new(badge)
                .size(SHORTCUT_BADGE_SIZE)
                .color(palette.text_muted),
        );
    }
    response
}

fn switch_label_color(palette: &Palette, active: bool, hovered: bool) -> egui::Color32 {
    if active || hovered {
        palette.text_primary
    } else {
        palette.text_secondary
    }
}

fn switch_icon_color(palette: &Palette, active: bool, hovered: bool) -> egui::Color32 {
    if active {
        palette.accent
    } else if hovered {
        palette.text_primary
    } else {
        palette.text_secondary
    }
}

/// The title bar's browser toggle, and whether the browser it shows floats
/// rather than fills the sidebar (browser.md §3, §11).
pub struct BrowserToggle {
    pub shown: bool,
    pub floating: bool,
}

/// The title bar's files toggle and the tree its sidebar shows (files.md §2).
pub struct FilesPanel<'a> {
    pub shown: bool,
    pub tree: &'a mut FileTreeState,
}

#[allow(clippy::too_many_arguments)]
pub fn root_layout(
    ui: &mut egui::Ui,
    palette: &Palette,
    items: &[SidebarItem],
    child_flags: &[bool],
    projects: &[ProjectVisibility],
    active_repo: Option<usize>,
    branch: &str,
    status: &RepoStatus,
    op_in_progress: bool,
    op: Option<&crate::git::status::OpSummary>,
    git_state: &mut GitPanelState,
    intents: &mut Vec<GitIntent>,
    show_workspace: &mut bool,
    show_git: &mut bool,
    // `None` without an active worktree: no browser sidebar nor toggle.
    show_browser: Option<&mut BrowserToggle>,
    // `None` without an active worktree: no files sidebar nor toggle.
    mut files: Option<&mut FilesPanel>,
    show_commit_detail: bool,
    commit_detail: Option<&CommitDetail>,
    // The selected commit is HEAD ⇒ its message can be amended from the detail panel.
    can_amend_head: bool,
    commit_diff_file: Option<&(git2::Oid, String)>,
    open_commit_file: &mut Option<(git2::Oid, String)>,
    repo_root: Option<&Path>,
    file_menu: &mut FileMenuOutput,
    git_file_view: FileViewMode,
    default_workspace_opener: WorkspaceOpener,
    installed_openers: &[WorkspaceOpener],
    open_workspace: &mut Option<WorkspaceOpener>,
    open_preferences: &mut bool,
    open_feedback: &mut bool,
    phone: Option<&PhoneIndicator>,
    open_phone: &mut bool,
    agents_badge: AgentBadge,
    agents_active: bool,
    done_agents: &[DoneAgentRow],
    pr_to_review: usize,
    pr_active: bool,
    sidebar: &mut SidebarAction,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    browser_sidebar_width: f32,
    files_sidebar_width: f32,
    keymap: &Keymap,
    // Run terminal strip at the bottom of the git sidebar (git.md §3): mounted only
    // when `show_run`, folded to its header when `run_collapsed`. `run_panel` paints
    // the strip (header + viewer); it's left unused when the strip is hidden.
    show_run: bool,
    run_collapsed: bool,
    run_panel_height: f32,
    run_panel: impl FnOnce(&mut egui::Ui),
    browser_panel: impl FnOnce(&mut egui::Ui),
    central: impl FnOnce(&mut egui::Ui),
) {
    // A Helm central mode (the dashboard or the PR cockpit) is cross-repo, so the
    // per-repo git panel has nothing to show — its mount and toggle are dropped.
    let helm_central = agents_active || pr_active;
    let sidebar_frame = egui::Frame::side_top_panel(ui.style())
        .fill(workspace_sidebar_fill(palette))
        .inner_margin(egui::Margin {
            left: SIDEBAR_PAD_X,
            right: SIDEBAR_PAD_X,
            top: TITLEBAR_HEIGHT,
            bottom: SIDEBAR_PAD_Y,
        });

    egui::Panel::left(LEFT_SIDEBAR_ID)
        .resizable(true)
        .default_size(left_sidebar_width)
        .min_size(200.0)
        .frame(sidebar_frame)
        .show_animated_inside(ui, *show_workspace, |ui| {
            ui.set_min_width(ui.available_width());
            let preferences_badge = keymap
                .shortcut_for(Action::TogglePreferences)
                .filter(|_| cmd_held(ui))
                .map(|shortcut| shortcut.display());
            egui::Panel::bottom(LEFT_SIDEBAR_FOOTER_ID)
                .frame(egui::Frame::NONE)
                .resizable(false)
                .show_separator_line(false)
                .exact_size(FOOTER_HEIGHT)
                .show_inside(ui, |ui| {
                    sidebar_footer(
                        ui,
                        palette,
                        SidebarFooter {
                            phone,
                            open_phone,
                            open_feedback,
                            open_preferences,
                            preferences_badge,
                        },
                    );
                });
            repo_sidebar(
                ui,
                palette,
                items,
                child_flags,
                projects,
                active_repo,
                agents_badge,
                agents_active,
                done_agents,
                pr_to_review,
                pr_active,
                keymap,
                sidebar,
            );
        });

    let git_shown = *show_git && !helm_central;
    let browser_shown = show_browser
        .as_deref()
        .is_some_and(|browser| browser.shown && !browser.floating)
        && !helm_central;
    let files_shown = files.as_ref().is_some_and(|files| files.shown) && !helm_central;
    let browser_min = if browser_shown {
        BROWSER_SIDEBAR_MIN_WIDTH
    } else {
        0.0
    };
    let files_min = if files_shown {
        FILES_SIDEBAR_MIN_WIDTH
    } else {
        0.0
    };
    // Each block leaves the terminals their reserve and the blocks laid out after
    // it their minimum width.
    let room_after_git = SIDEBAR_CENTRAL_RESERVE + browser_min + files_min;
    let git_max = (ui.available_width() - room_after_git).max(GIT_SIDEBAR_MIN_WIDTH);
    let git_block_padding = egui::Margin {
        left: SIDEBAR_PAD_X,
        right: SIDEBAR_PAD_X,
        top: 0,
        bottom: SIDEBAR_PAD_Y,
    };
    egui::Panel::right(RIGHT_SIDEBAR_ID)
        .resizable(true)
        .default_size(right_sidebar_width)
        .min_size(GIT_SIDEBAR_MIN_WIDTH)
        .max_size(git_max)
        .frame(block_panel_frame(palette))
        .show_separator_line(false)
        .show_animated_inside(ui, git_shown, |ui| {
            ui.set_min_width(ui.available_width());
            sidebar_block(ui, palette, git_block_padding, |ui| {
                // Run terminal strip pinned to the bottom; the status/detail content
                // fills the remaining height above it (git.md §3).
                if show_run {
                    let run_frame =
                        egui::Frame::new()
                            .fill(palette.bg_canvas)
                            .inner_margin(egui::Margin {
                                left: 0,
                                right: 0,
                                top: 4,
                                bottom: 0,
                            });
                    let panel = if run_collapsed {
                        // A distinct id while folded: egui prefers a panel's stored size
                        // over default_size, so sharing the id would overwrite the
                        // remembered height with the header height and reopen the strip
                        // clamped to its minimum (git.md §3).
                        egui::Panel::bottom(RUN_PANEL_COLLAPSED_ID)
                            .frame(run_frame)
                            .resizable(false)
                            .exact_size(crate::ui::run_panel::HEADER_HEIGHT)
                    } else {
                        let max =
                            (ui.available_height() - RUN_PANEL_RESERVE).max(RUN_PANEL_MIN_HEIGHT);
                        egui::Panel::bottom(RUN_PANEL_ID)
                            .frame(run_frame)
                            .resizable(true)
                            .default_size(run_panel_height)
                            .min_size(RUN_PANEL_MIN_HEIGHT)
                            .max_size(max)
                    };
                    panel.show_inside(ui, run_panel);
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        // A commit selected in Graph mode (M9-6) ⇒ its detail in place of
                        // the status sections (git.md §9); no selection ⇒ the status
                        // sections stay (WIP is the implicit selection — never a "select a
                        // commit" state).
                        if show_commit_detail {
                            let mut set_view = None;
                            let mut amend = None;
                            commit_detail_panel(
                                ui,
                                palette,
                                commit_detail,
                                commit_diff_file,
                                open_commit_file,
                                repo_root,
                                file_menu,
                                git_file_view,
                                &mut set_view,
                                can_amend_head,
                                &mut amend,
                            );
                            if let Some(view) = set_view {
                                intents.push(crate::ui::git_panel::GitIntent::SetFileView(view));
                            }
                            if let Some(message) = amend {
                                intents
                                    .push(crate::ui::git_panel::GitIntent::AmendMessage(message));
                            }
                        } else if active_repo.is_none() {
                            git_panel::no_repo(ui, palette, git_state);
                        } else {
                            git_panel(
                                ui,
                                palette,
                                branch,
                                status,
                                op_in_progress,
                                op,
                                git_state,
                                keymap,
                                intents,
                                repo_root,
                                file_menu,
                                git_file_view,
                            );
                        }
                    });
            });
        });

    let room_after_browser = SIDEBAR_CENTRAL_RESERVE + files_min;
    let browser_max = (ui.available_width() - room_after_browser).max(BROWSER_SIDEBAR_MIN_WIDTH);
    egui::Panel::right(BROWSER_SIDEBAR_ID)
        .resizable(true)
        .default_size(browser_sidebar_width)
        .min_size(BROWSER_SIDEBAR_MIN_WIDTH)
        .max_size(browser_max)
        .frame(block_panel_frame(palette))
        .show_separator_line(false)
        .show_animated_inside(ui, browser_shown, |ui| {
            ui.set_min_width(ui.available_width());
            sidebar_block(ui, palette, egui::Margin::ZERO, browser_panel);
        });

    let files_max = (ui.available_width() - SIDEBAR_CENTRAL_RESERVE).max(FILES_SIDEBAR_MIN_WIDTH);
    egui::Panel::right(FILES_SIDEBAR_ID)
        .resizable(true)
        .default_size(files_sidebar_width)
        .min_size(FILES_SIDEBAR_MIN_WIDTH)
        .max_size(files_max)
        .frame(block_panel_frame(palette))
        .show_separator_line(false)
        .show_animated_inside(ui, files_shown, |ui| {
            ui.set_min_width(ui.available_width());
            sidebar_block(ui, palette, git_block_padding, |ui| {
                if let Some(files) = files.as_deref_mut() {
                    intents.extend(files_sidebar(ui, palette, files.tree));
                }
            });
        });

    let terminals = egui::CentralPanel::default()
        .show_inside(ui, |ui| {
            central(ui);
        })
        .response
        .rect;

    // Single title row: both clusters center vertically in the title strip so the
    // left sidebar toggle and the right actions share one baseline. The macOS
    // traffic lights float in the top-left corner; TRAFFIC_LIGHTS_RESERVE keeps
    // the toggle clear of them horizontally — except in fullscreen, where the
    // lights are hidden: the toggle hugs the left edge, mirroring the right
    // cluster's margin.
    let controls_y = f32::from(TITLEBAR_HEIGHT) / 2.0 - TOGGLE_HIT.y / 2.0;
    let fullscreen = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
    let toggle_x = if fullscreen {
        8.0
    } else {
        TRAFFIC_LIGHTS_RESERVE
    };
    egui::Area::new(egui::Id::new("titlebar_left"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(toggle_x, controls_y))
        .show(ui.ctx(), |ui| {
            let workspace = workspace_toggle(ui, palette, show_workspace);
            if let Some(shortcut) = keymap
                .shortcut_for(Action::ToggleWorkspaceSidebar)
                .filter(|_| cmd_held(ui))
            {
                shortcut_badge(
                    ui,
                    palette,
                    "shortcut_badge_workspace",
                    &shortcut.display(),
                    workspace.rect,
                );
            }
        });

    // The right sidebars are blocks reaching the top of the window: their actions
    // ride the terminals' title row, against the first block.
    egui::Area::new(title_actions_id())
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            terminals.right() - TITLE_ACTIONS_MARGIN,
            TITLE_LINE_Y - TOGGLE_HIT.y / 2.0,
        ))
        .pivot(egui::Align2::RIGHT_TOP)
        .show(ui.ctx(), |ui| {
            let can_open_workspace = active_repo
                .and_then(|index| {
                    items.iter().find_map(|item| match item {
                        SidebarItem::Row(row) if row.index == index => Some(row),
                        _ => None,
                    })
                })
                .is_some_and(|row| !row.missing);
            let launcher = Launcher {
                shown: !items.is_empty(),
                enabled: can_open_workspace,
                default_opener: default_workspace_opener,
                installed: installed_openers,
                open: open_workspace,
            };
            title_actions(
                ui,
                palette,
                keymap,
                launcher,
                SidebarToggles {
                    helm_central,
                    show_git,
                    show_browser,
                    show_files: files.map(|files| &mut files.shown),
                },
            )
        });
}

/// The title-row actions; [`central_switch`] reads where they were last drawn.
fn title_actions_id() -> egui::Id {
    egui::Id::new("title_actions")
}

/// Cmd alone is held: shortcut badges show (keybindings §5).
fn cmd_held(ui: &egui::Ui) -> bool {
    ui.input(|i| {
        let m = i.modifiers;
        m.command && !m.shift && !m.alt && !m.ctrl
    })
}

/// Current left sidebar width after interaction (resize drag). `egui` stores the
/// panel state under its `Id` once rendered; we read it back to persist the width
/// in the TOML (architecture §4, M7-5).
pub fn left_sidebar_width(ctx: &egui::Context) -> Option<f32> {
    panel_width(ctx, LEFT_SIDEBAR_ID)
}

/// Current right sidebar width (see [`left_sidebar_width`]).
pub fn right_sidebar_width(ctx: &egui::Context) -> Option<f32> {
    panel_width(ctx, RIGHT_SIDEBAR_ID)
}

/// Current browser sidebar width (see [`left_sidebar_width`]).
pub fn browser_sidebar_width(ctx: &egui::Context) -> Option<f32> {
    panel_width(ctx, BROWSER_SIDEBAR_ID)
}

/// Current files sidebar width (see [`left_sidebar_width`]).
pub fn files_sidebar_width(ctx: &egui::Context) -> Option<f32> {
    panel_width(ctx, FILES_SIDEBAR_ID)
}

fn panel_width(ctx: &egui::Context, id: &str) -> Option<f32> {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new(id)).map(|s| s.size().x)
}

/// Current Run terminal strip height after a resize drag (git.md §3), read back to
/// persist it like the sidebar widths.
pub fn run_panel_height(ctx: &egui::Context) -> Option<f32> {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new(RUN_PANEL_ID)).map(|s| s.size().y)
}

/// The workspace launcher of the title row: hidden until a repository is imported,
/// disabled while the active folder is missing.
struct Launcher<'a> {
    shown: bool,
    enabled: bool,
    default_opener: WorkspaceOpener,
    installed: &'a [WorkspaceOpener],
    open: &'a mut Option<WorkspaceOpener>,
}

/// The right sidebars' toggles of the title row.
struct SidebarToggles<'a> {
    helm_central: bool,
    show_git: &'a mut bool,
    show_browser: Option<&'a mut BrowserToggle>,
    show_files: Option<&'a mut bool>,
}

fn title_actions(
    ui: &mut egui::Ui,
    palette: &Palette,
    keymap: &Keymap,
    launcher: Launcher<'_>,
    toggles: SidebarToggles<'_>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TOP_ACTION_GAP;
        if launcher.shown {
            workspace_launcher(
                ui,
                palette,
                launcher.enabled,
                launcher.default_opener,
                launcher.installed,
                launcher.open,
            );
        }
        let shown = sidebar_toggles(ui, palette, toggles);
        // Badges are painted as an overlay below their icon (not inserted into the
        // flow): a `new_child` does not advance the cursor, so the row keeps its
        // width and the icons do not shift when Cmd toggles the display.
        if !cmd_held(ui) {
            return;
        }
        for (toggle, action, id_salt) in shown {
            if let Some(shortcut) = keymap.shortcut_for(action) {
                shortcut_badge(ui, palette, id_salt, &shortcut.display(), toggle.rect);
            }
        }
    });
}

/// Draws the toggles that apply; each drawn one with the action its badge names.
fn sidebar_toggles(
    ui: &mut egui::Ui,
    palette: &Palette,
    toggles: SidebarToggles<'_>,
) -> Vec<(egui::Response, Action, &'static str)> {
    let mut shown = Vec::new();
    if let Some(files) = toggles.show_files.filter(|_| !toggles.helm_central) {
        let toggle = title_toggle(
            ui,
            palette,
            files,
            "Toggle files sidebar",
            lucide_icons::Icon::Folder,
        );
        shown.push((toggle, Action::ToggleFilesSidebar, "shortcut_badge_files"));
    }
    if let Some(browser) = toggles.show_browser.filter(|_| !toggles.helm_central) {
        let toggle = title_toggle(
            ui,
            palette,
            &mut browser.shown,
            "Toggle browser sidebar",
            lucide_icons::Icon::Globe,
        );
        shown.push((
            toggle,
            Action::ToggleBrowserSidebar,
            "shortcut_badge_browser",
        ));
    }
    if !toggles.helm_central {
        let toggle = git_toggle(ui, palette, toggles.show_git);
        shown.push((toggle, Action::ToggleGitSidebar, "shortcut_badge_git"));
    }
    shown
}

const TOP_ACTION_GAP: f32 = 6.0;
/// Height of the line the terminals' switch, the title-row actions and the right
/// blocks' headers are centered on, from the window's top.
const TITLE_LINE_Y: f32 = BLOCK_GAP as f32 + git_panel::HEADER_BAND_H / 2.0;
/// Room between the title-row actions and the first right sidebar block.
const TITLE_ACTIONS_MARGIN: f32 = 8.0;
const SHORTCUT_BADGE_DROP: f32 = 3.0;
const SHORTCUT_BADGE_BOX: egui::Vec2 = egui::vec2(40.0, SHORTCUT_BADGE_SIZE + 4.0);

fn shortcut_badge(
    ui: &mut egui::Ui,
    palette: &Palette,
    id_salt: &str,
    label: &str,
    anchor: egui::Rect,
) {
    let mut badge = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id_salt)
            .max_rect(egui::Rect::from_center_size(
                egui::pos2(anchor.center().x, anchor.bottom() + SHORTCUT_BADGE_DROP),
                SHORTCUT_BADGE_BOX,
            ))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    badge.label(
        egui::RichText::new(label)
            .size(SHORTCUT_BADGE_SIZE)
            .color(palette.text_muted),
    );
}
const LAUNCHER_MAIN_HIT: egui::Vec2 = egui::vec2(32.0, 24.0);
const LAUNCHER_MENU_HIT: egui::Vec2 = egui::vec2(18.0, 24.0);
const LAUNCHER_ICON_SIZE: f32 = 20.0;
const LAUNCHER_CHEVRON_SIZE: f32 = 13.0;
const LAUNCHER_MENU_W: f32 = 156.0;
const LAUNCHER_MENU_ROW_H: f32 = 34.0;
const LAUNCHER_MENU_ICON_SIZE: f32 = 24.0;
const LAUNCHER_MENU_ICON_CENTER_X: f32 = 18.0;
const LAUNCHER_MENU_TEXT_X: f32 = 36.0;

fn workspace_launcher(
    ui: &mut egui::Ui,
    palette: &Palette,
    has_workspace: bool,
    default_opener: WorkspaceOpener,
    installed: &[WorkspaceOpener],
    out: &mut Option<WorkspaceOpener>,
) -> egui::Rect {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let default_label = format!("Open workspace in {}", default_opener.label());
        let hover = if has_workspace {
            default_label.as_str()
        } else {
            "Project folder not found"
        };
        let response = launcher_segment(
            ui,
            palette,
            has_workspace,
            &default_label,
            hover,
            LauncherSegment::Left,
            |painter, rect, color| {
                let icon = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(LAUNCHER_ICON_SIZE, LAUNCHER_ICON_SIZE),
                );
                paint_opener_icon(painter, icon, default_opener, has_workspace, color);
            },
        );
        if response.clicked() {
            *out = Some(default_opener);
        }

        let menu_response = launcher_segment(
            ui,
            palette,
            has_workspace,
            "Open workspace menu",
            if has_workspace {
                "Choose an app"
            } else {
                "Project folder not found"
            },
            LauncherSegment::Right,
            |painter, rect, color| {
                paint_icon(
                    painter,
                    rect.center(),
                    LAUNCHER_CHEVRON_SIZE,
                    lucide_icons::Icon::ChevronDown,
                    color,
                );
            },
        );
        if has_workspace {
            egui::Popup::menu(&menu_response)
                .style(crate::theme::menu_style)
                .show(|ui| {
                    ui.set_width(LAUNCHER_MENU_W);
                    for &opener in installed {
                        if opener_menu_row(ui, opener).clicked() {
                            *out = Some(opener);
                            ui.close();
                        }
                    }
                });
        }
    })
    .response
    .rect
}

#[derive(Clone, Copy)]
enum LauncherSegment {
    Left,
    Right,
}

fn launcher_segment(
    ui: &mut egui::Ui,
    palette: &Palette,
    enabled: bool,
    label: &str,
    hover: &str,
    segment: LauncherSegment,
    paint: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    let (rect, response, hovered) = clickable(ui, segment_size(segment), enabled);
    let fill = if hovered {
        palette.bg_surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        segment_radius(segment),
        fill,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let color = if enabled {
        palette.text_secondary
    } else {
        palette.state_disabled
    };
    paint(ui.painter(), rect, color);

    let label = label.to_owned();
    let response = response.on_hover_text(hover);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.clone())
    });
    response
}

fn segment_size(segment: LauncherSegment) -> egui::Vec2 {
    match segment {
        LauncherSegment::Left => LAUNCHER_MAIN_HIT,
        LauncherSegment::Right => LAUNCHER_MENU_HIT,
    }
}

fn segment_radius(segment: LauncherSegment) -> egui::CornerRadius {
    match segment {
        LauncherSegment::Left => egui::CornerRadius {
            nw: RADIUS_PILL,
            ne: 0,
            sw: RADIUS_PILL,
            se: 0,
        },
        LauncherSegment::Right => egui::CornerRadius {
            nw: 0,
            ne: RADIUS_PILL,
            sw: 0,
            se: RADIUS_PILL,
        },
    }
}

fn opener_menu_row(ui: &mut egui::Ui, opener: WorkspaceOpener) -> egui::Response {
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(LAUNCHER_MENU_W, LAUNCHER_MENU_ROW_H), true);
    if hovered {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(2.0, 2.0)),
            egui::CornerRadius::same(RADIUS_MENU_ITEM),
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.left() + LAUNCHER_MENU_ICON_CENTER_X, rect.center().y),
        egui::vec2(LAUNCHER_MENU_ICON_SIZE, LAUNCHER_MENU_ICON_SIZE),
    );
    paint_opener_icon(
        ui.painter(),
        icon,
        opener,
        true,
        ui.visuals().widgets.inactive.fg_stroke.color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + LAUNCHER_MENU_TEXT_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        opener.label(),
        egui::FontId::proportional(14.0),
        ui.visuals().text_color(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, opener.label()));
    response
}

fn paint_opener_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    opener: WorkspaceOpener,
    enabled: bool,
    fallback: egui::Color32,
) {
    if let Some(texture) = opener_icon_texture(painter.ctx(), opener) {
        // The real app icon carries its own colors and rounded-rect shape: tint
        // white to leave them untouched, faded when the launcher is disabled.
        let tint = if enabled {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_white_alpha(110)
        };
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    } else {
        paint_icon(
            painter,
            rect.center(),
            rect.width(),
            lucide_icons::Icon::AppWindow,
            fallback,
        );
    }
}

/// Real macOS icon for `opener`, decoded from its `.app` bundle and uploaded as a
/// texture once, then cached in egui memory (keyed per opener) so the work runs
/// only on first paint. The `None` outcome is cached too, so a missing or
/// undecodable icon stays on the generic-glyph fallback without retrying.
fn opener_icon_texture(
    ctx: &egui::Context,
    opener: WorkspaceOpener,
) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("opener_icon", opener));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return cached;
    }
    let handle = crate::workspace_launcher::load_opener_icon(opener).map(|icon| {
        let image = egui::ColorImage::from_rgba_unmultiplied([icon.width, icon.height], &icon.rgba);
        ctx.load_texture(
            format!("opener-icon-{}", opener.label()),
            image,
            egui::TextureOptions::LINEAR,
        )
    });
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

const TOGGLE_HIT: egui::Vec2 = egui::vec2(28.0, 24.0);
const TOGGLE_GLYPH: egui::Vec2 = egui::vec2(16.0, 13.0);
const TITLE_TOGGLE_GLYPH: f32 = 15.0;

fn git_toggle(ui: &mut egui::Ui, palette: &Palette, show_git: &mut bool) -> egui::Response {
    title_toggle(
        ui,
        palette,
        show_git,
        "Toggle git sidebar",
        lucide_icons::Icon::GitBranch,
    )
}

/// An icon flipping `shown`, accent while on.
fn title_toggle(
    ui: &mut egui::Ui,
    palette: &Palette,
    shown: &mut bool,
    label: &str,
    icon: lucide_icons::Icon,
) -> egui::Response {
    let (rect, response, hovered) = clickable(ui, TOGGLE_HIT, true);
    if response.clicked() {
        *shown = !*shown;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, egui::CornerRadius::same(6), palette.bg_surface_hover);
    }
    let color = if *shown {
        palette.accent
    } else {
        palette.text_muted
    };
    paint_icon(painter, rect.center(), TITLE_TOGGLE_GLYPH, icon, color);
    response
}

/// A panel glyph whose left column fills while the workspace sidebar is shown.
fn workspace_toggle(ui: &mut egui::Ui, palette: &Palette, show: &mut bool) -> egui::Response {
    let (rect, response, hovered) = clickable(ui, TOGGLE_HIT, true);
    if response.clicked() {
        *show = !*show;
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Toggle workspace sidebar")
    });

    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, egui::CornerRadius::same(6), palette.bg_surface_hover);
    }

    let glyph = egui::Rect::from_center_size(rect.center(), TOGGLE_GLYPH);
    let color = if *show {
        palette.text_primary
    } else {
        palette.text_muted
    };
    let stroke = egui::Stroke::new(1.5_f32, color);
    let divider_x = glyph.left() + glyph.width() * 0.38;
    if *show {
        let column = egui::Rect::from_min_max(glyph.min, egui::pos2(divider_x, glyph.bottom()));
        painter.rect_filled(column, egui::CornerRadius::ZERO, palette.accent_subtle);
    }
    painter.rect_stroke(
        glyph,
        egui::CornerRadius::same(3),
        stroke,
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(divider_x, glyph.top()),
            egui::pos2(divider_x, glyph.bottom()),
        ],
        stroke,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_date_unix_epoch() {
        assert_eq!(format_date(0), "1970-01-01");
    }

    #[test]
    fn format_date_known_timestamps() {
        // 2021-01-01T00:00:00Z = 1_609_459_200; mid-day same date keeps the date.
        assert_eq!(format_date(1_609_459_200), "2021-01-01");
        assert_eq!(format_date(1_609_459_200 + 86_399), "2021-01-01");
        // 2024-02-29 (leap day) = 1_709_164_800.
        assert_eq!(format_date(1_709_164_800), "2024-02-29");
    }

    #[test]
    fn format_date_time_appends_hour_and_minute() {
        assert_eq!(format_date_time(0), "01/01/1970 @ 00:00");
        // 2021-01-01T14:32:05Z.
        assert_eq!(
            format_date_time(1_609_459_200 + 14 * 3_600 + 32 * 60 + 5),
            "01/01/2021 @ 14:32"
        );
        // Negative seconds wrap back cleanly (rem_euclid).
        assert_eq!(format_date_time(-60), "31/12/1969 @ 23:59");
    }

    /// Runs one frame with the pointer parked at `at`, letting the body raise the flag.
    fn frame_with_pointer(at: egui::Pos2, mut body: impl FnMut(&egui::Context)) -> egui::Context {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(at)],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| body(ui.ctx()));
        ctx
    }

    const SCROLL_ROOM_VIEWPORT: egui::Rect = egui::Rect {
        min: egui::pos2(100.0, 100.0),
        max: egui::pos2(300.0, 300.0),
    };

    #[test]
    fn a_scrolled_surface_under_the_pointer_claims_the_swipe() {
        let ctx = frame_with_pointer(egui::pos2(150.0, 150.0), |ctx| {
            note_h_scroll_room(ctx, SCROLL_ROOM_VIEWPORT, 40.0);
        });

        assert!(h_scroll_owns_swipe(&ctx));
        // Reading clears it, so the next frame starts from nothing claimed.
        assert!(!h_scroll_owns_swipe(&ctx));
    }

    #[test]
    fn a_surface_back_at_its_left_edge_leaves_the_swipe_alone() {
        let ctx = frame_with_pointer(egui::pos2(150.0, 150.0), |ctx| {
            note_h_scroll_room(ctx, SCROLL_ROOM_VIEWPORT, 0.0);
        });

        assert!(!h_scroll_owns_swipe(&ctx));
    }

    #[test]
    fn a_surface_the_pointer_is_not_over_leaves_the_swipe_alone() {
        let ctx = frame_with_pointer(egui::pos2(600.0, 150.0), |ctx| {
            note_h_scroll_room(ctx, SCROLL_ROOM_VIEWPORT, 40.0);
        });

        assert!(!h_scroll_owns_swipe(&ctx));
    }

    #[test]
    fn workspace_sidebar_fill_uses_the_sidebar_token() {
        // design-system §1: bg.sidebar is the left sidebar's background — the
        // tonal separation with the central zone comes from the token itself.
        let palette = Palette::light();
        let fill = workspace_sidebar_fill(&palette);

        assert_eq!(fill, palette.bg_sidebar);
        assert_eq!(fill.to_srgba_unmultiplied()[3], 255);
    }
}
