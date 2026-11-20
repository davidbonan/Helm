//! What the page area shows when its page could not load (browser.md §14).

use lucide_icons::Icon;

use crate::browser::favicons;
use crate::browser::load_failure::{FailureReason, LoadFailure};
use crate::browser::Navigation;
use crate::theme::Palette;
use crate::ui::browser_panel::{BrowserChrome, BrowserIntent};

const COLUMN_WIDTH: f32 = 420.0;
const ICON_SIZE: f32 = 34.0;
const TITLE_SIZE: f32 = 18.0;
const TEXT_SIZE: f32 = 13.0;
const URL_SIZE: f32 = 11.5;
pub const TRY_AGAIN_LABEL: &str = "Try again";
pub const RUN_SERVER_LABEL: &str = "Run server";

/// Fills `ui`'s rect.
pub(crate) fn failure_page(
    ui: &mut egui::Ui,
    chrome: &BrowserChrome<'_>,
    failure: &LoadFailure,
) -> Vec<BrowserIntent> {
    let area = ui.max_rect();
    let palette = chrome.palette;
    let site = favicons::site_of(&failure.url).unwrap_or(failure.url.as_str());
    let width = COLUMN_WIDTH.min(area.width() - 32.0).max(0.0);
    let column = egui::Rect::from_center_size(
        egui::pos2(area.center().x, area.top() + area.height() * 0.4),
        egui::vec2(width, 0.0),
    );
    let mut intents = Vec::new();
    let mut column_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(column.with_max_y(area.bottom()))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut column_ui;
    ui.label(
        egui::RichText::new(icon(&failure.reason).unicode().to_string())
            .size(ICON_SIZE)
            .color(palette.text_muted),
    );
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(failure.reason.title(site))
            .size(TITLE_SIZE)
            .color(palette.text_primary)
            .strong(),
    );
    ui.add_space(6.0);
    centered_text(
        ui,
        failure.reason.detail(),
        TEXT_SIZE,
        palette.text_secondary,
    );
    ui.add_space(4.0);
    centered_text(ui, &failure.url, URL_SIZE, palette.text_muted);
    ui.add_space(18.0);
    let run = runs_server(chrome, failure);
    let labels: &[&str] = if run {
        &[RUN_SERVER_LABEL, TRY_AGAIN_LABEL]
    } else {
        &[TRY_AGAIN_LABEL]
    };
    let row = egui::vec2(buttons_width(ui, labels), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        row,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if run && primary_button(ui, palette, RUN_SERVER_LABEL).clicked() {
                intents.push(BrowserIntent::RunServer);
            }
            let retry = if run {
                ui.button(TRY_AGAIN_LABEL)
            } else {
                primary_button(ui, palette, TRY_AGAIN_LABEL)
            };
            if retry.clicked() {
                intents.push(BrowserIntent::Navigate(Navigation::Reload));
            }
        },
    );
    intents
}

/// Nothing answers on the worktree's own server, and its Run strip is not running.
fn runs_server(chrome: &BrowserChrome<'_>, failure: &LoadFailure) -> bool {
    let failed_site = favicons::site_of(&failure.url);
    failure.reason == FailureReason::Refused
        && !chrome.server_running
        && chrome
            .server
            .is_some_and(|server| favicons::site_of(server) == failed_site)
}

fn icon(reason: &FailureReason) -> Icon {
    match reason {
        FailureReason::Refused => Icon::ServerOff,
        FailureReason::Offline => Icon::WifiOff,
        FailureReason::Insecure(_) => Icon::ShieldAlert,
        FailureReason::Crashed => Icon::Frown,
        FailureReason::HostNotFound | FailureReason::TimedOut | FailureReason::Other(_) => {
            Icon::AlertTriangle
        }
    }
}

fn centered_text(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).size(size).color(color))
            .wrap()
            .halign(egui::Align::Center),
    );
}

/// The row of `labels` as buttons: what centers it.
fn buttons_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let padding = 2.0 * ui.spacing().button_padding.x;
    let gaps = ui.spacing().item_spacing.x * (labels.len().saturating_sub(1)) as f32;
    let text: f32 = labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap(
                    (*label).to_owned(),
                    font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
        })
        .sum();
    text + padding * labels.len() as f32 + gaps
}

fn primary_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE))
            .fill(palette.primary_button_fill()),
    )
}
