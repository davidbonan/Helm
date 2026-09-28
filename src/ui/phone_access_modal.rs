//! Pairing modal of phone access (specs/remote.md §2): the QR code of the pairing
//! URL, the URL itself, the connected devices and a Stop. Pure rendering — the app
//! owns the server; the modal reports the user's intent.

use crate::remote::qr::QrMatrix;
use crate::theme::Palette;

const MODAL_WIDTH: f32 = 380.0;
const TITLE_SIZE: f32 = 14.0;
const TEXT_SIZE: f32 = 13.0;
const QR_SIDE: f32 = 220.0;
/// Blank modules around the code: scanners need them to find its edges.
const QR_QUIET_MODULES: usize = 4;
const TITLE: &str = "Open on phone";
pub const QR_LABEL: &str = "Pairing QR code";
pub const LIVE_WARNING: &str =
    "Phone access is on — anyone with this code can type into your agents.";
const NETWORK_NOTE: &str = "Plain HTTP on your local network: use it on a network you trust.";
pub const COPY_LABEL: &str = "Copy link";
pub const CLOSE_LABEL: &str = "Close";
pub const STOP_LABEL: &str = "Stop phone access";

pub struct PhoneAccessView<'a> {
    pub pairing_url: &'a str,
    pub qr: Option<&'a QrMatrix>,
    pub clients: usize,
}

#[derive(Default)]
pub struct PhoneAccessAction {
    pub copy_url: bool,
    pub stop: bool,
    /// Close the modal, access stays on (Close, `Esc`, click outside).
    pub dismiss: bool,
}

pub fn phone_access_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &PhoneAccessView,
) -> PhoneAccessAction {
    let mut action = PhoneAccessAction::default();
    let modal = egui::Modal::new(egui::Id::new("phone_access_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(MODAL_WIDTH);
            ui.label(
                egui::RichText::new(TITLE)
                    .size(TITLE_SIZE)
                    .color(palette.text_primary)
                    .strong(),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Scan with the phone's camera, on the same Wi-Fi.")
                    .size(TEXT_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                if let Some(qr) = view.qr {
                    qr_code(ui, qr);
                }
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(view.pairing_url)
                        .monospace()
                        .size(TEXT_SIZE - 1.0)
                        .color(palette.text_secondary),
                );
                if ui.button(COPY_LABEL).clicked() {
                    action.copy_url = true;
                }
            });
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(LIVE_WARNING)
                    .size(TEXT_SIZE)
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new(NETWORK_NOTE)
                    .size(TEXT_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(devices_label(view.clients))
                    .size(TEXT_SIZE)
                    .color(palette.text_secondary),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(CLOSE_LABEL).clicked() {
                    action.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(crate::ui::danger_button(palette, STOP_LABEL))
                        .clicked()
                    {
                        action.stop = true;
                    }
                });
            });
        });
    if modal.should_close() {
        action.dismiss = true;
    }
    action
}

pub fn devices_label(clients: usize) -> String {
    match clients {
        0 => "No device connected".to_owned(),
        1 => "1 device connected".to_owned(),
        n => format!("{n} devices connected"),
    }
}

/// Black on white whatever the theme: a scanner needs the contrast.
fn qr_code(ui: &mut egui::Ui, qr: &QrMatrix) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(QR_SIDE, QR_SIDE), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, QR_LABEL));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::WHITE);
    let modules = qr.width() + 2 * QR_QUIET_MODULES;
    // Whole points per module: fractional edges antialias into gray seams.
    let module = (QR_SIDE / modules as f32).floor();
    let origin = rect.center() - egui::Vec2::splat(module * modules as f32 / 2.0);
    for y in 0..qr.width() {
        for x in 0..qr.width() {
            if !qr.is_dark(x, y) {
                continue;
            }
            let min = origin
                + egui::vec2(
                    (x + QR_QUIET_MODULES) as f32 * module,
                    (y + QR_QUIET_MODULES) as f32 * module,
                );
            painter.rect_filled(
                egui::Rect::from_min_size(min, egui::vec2(module, module)),
                0.0,
                egui::Color32::BLACK,
            );
        }
    }
}
