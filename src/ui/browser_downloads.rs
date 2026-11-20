//! The downloads shelf at the foot of the page area (browser.md §13): one row
//! per file the tab's page handed over, until dismissed.

use std::path::PathBuf;
use std::time::Duration;

use lucide_icons::Icon;

use crate::browser::download::{Download, DownloadStatus};
use crate::theme::Palette;
use crate::ui::browser_panel::BrowserIntent;

const ROW_HEIGHT: f32 = 34.0;
const MAX_ROWS: usize = 3;
const PAD_X: f32 = 12.0;
const ICON_SIZE: f32 = 14.0;
const TEXT_SIZE: f32 = 12.5;
const PROGRESS_HEIGHT: f32 = 2.0;
const DISMISS_SIZE: f32 = 22.0;
/// How often a running download's progress is read again.
const PROGRESS_REFRESH: Duration = Duration::from_millis(250);
pub const SHOW_IN_FINDER_LABEL: &str = "Show in Finder";

/// The rows the shelf shows: the latest downloads not dismissed.
pub(crate) fn shelved<'a>(downloads: &'a [Download], dismissed: &[u64]) -> Vec<&'a Download> {
    let mut shown: Vec<&Download> = downloads
        .iter()
        .filter(|download| !dismissed.contains(&download.id))
        .collect();
    let hidden = shown.len().saturating_sub(MAX_ROWS);
    shown.drain(..hidden);
    shown
}

pub(crate) fn shelf_height(rows: usize) -> f32 {
    rows as f32 * ROW_HEIGHT
}

/// The shelf's rows, and the downloads dismissed, where a row's `×` lands.
pub(crate) struct Shelf<'a> {
    pub(crate) rows: &'a [&'a Download],
    pub(crate) dismissed: &'a mut Vec<u64>,
    /// Rounding of its bottom corners, those of the block it ends.
    pub(crate) bottom_radius: u8,
}

/// Fills `ui`'s rect.
pub(crate) fn download_shelf(
    ui: &mut egui::Ui,
    palette: &Palette,
    shelf: Shelf<'_>,
) -> Vec<BrowserIntent> {
    let Shelf {
        rows,
        dismissed,
        bottom_radius,
    } = shelf;
    let area = ui.max_rect();
    let radius = egui::CornerRadius {
        sw: bottom_radius,
        se: bottom_radius,
        ..egui::CornerRadius::ZERO
    };
    ui.painter().rect_filled(area, radius, palette.bg_sidebar);
    ui.painter().hline(
        area.x_range(),
        area.top(),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let mut intents = Vec::new();
    for (index, download) in rows.iter().enumerate() {
        let top = area.top() + index as f32 * ROW_HEIGHT;
        let row = egui::Rect::from_x_y_ranges(area.x_range(), top..=top + ROW_HEIGHT);
        match shelf_row(ui, palette, download, row) {
            Some(RowAction::Reveal(path)) => intents.push(BrowserIntent::RevealDownload(path)),
            Some(RowAction::Dismiss) => dismissed.push(download.id),
            None => {}
        }
    }
    if rows
        .iter()
        .any(|download| matches!(download.status, DownloadStatus::Running(_)))
    {
        ui.ctx().request_repaint_after(PROGRESS_REFRESH);
    }
    intents
}

enum RowAction {
    Reveal(PathBuf),
    Dismiss,
}

fn shelf_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    download: &Download,
    row: egui::Rect,
) -> Option<RowAction> {
    let mut action = None;
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row.shrink2(egui::vec2(PAD_X, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut row_ui;
    let (icon, icon_color) = match download.status {
        DownloadStatus::Running(_) => (Icon::Download, palette.accent),
        DownloadStatus::Done => (Icon::CheckCircle2, palette.git_added),
        DownloadStatus::Failed(_) => (Icon::AlertTriangle, palette.git_deleted),
    };
    ui.label(
        egui::RichText::new(icon.unicode().to_string())
            .size(ICON_SIZE)
            .color(icon_color),
    );
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !matches!(download.status, DownloadStatus::Running(_))
            && dismiss_button(ui, palette).clicked()
        {
            action = Some(RowAction::Dismiss);
        }
        if let (DownloadStatus::Done, Some(path)) = (&download.status, &download.path) {
            if ui.button(SHOW_IN_FINDER_LABEL).clicked() {
                action = Some(RowAction::Reveal(path.clone()));
            }
        }
        ui.label(
            egui::RichText::new(status_text(&download.status))
                .size(TEXT_SIZE)
                .color(palette.text_muted),
        );
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&download.file_name)
                        .size(TEXT_SIZE)
                        .color(palette.text_primary),
                )
                .truncate(),
            );
        });
    });
    if let DownloadStatus::Running(Some(progress)) = download.status {
        let track = egui::Rect::from_x_y_ranges(
            row.x_range(),
            (row.bottom() - PROGRESS_HEIGHT)..=row.bottom(),
        );
        let mut done = track;
        done.set_width(track.width() * progress.clamp(0.0, 1.0));
        ui.painter().rect_filled(done, 0.0, palette.accent);
    }
    action
}

fn status_text(status: &DownloadStatus) -> String {
    match status {
        DownloadStatus::Running(Some(progress)) => format!("{:.0} %", progress * 100.0),
        DownloadStatus::Running(None) => "Downloading…".to_owned(),
        DownloadStatus::Done => "Done".to_owned(),
        DownloadStatus::Failed(reason) => reason.clone(),
    }
}

fn dismiss_button(ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(DISMISS_SIZE, DISMISS_SIZE), true);
    if hovered {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), palette.bg_surface_hover);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        Icon::X.unicode(),
        egui::FontId::proportional(13.0),
        palette.text_muted,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Dismiss"));
    response
}
