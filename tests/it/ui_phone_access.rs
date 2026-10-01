use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::remote::firewall::FirewallBlock;
use helm::remote::qr::QrMatrix;
use helm::theme::Palette;
use helm::ui::phone_access_modal::{
    firewall_message, phone_access_modal, PhoneAccessView, CLOSE_LABEL, FIREWALL_SETTINGS_LABEL,
    FIREWALL_TITLE, LIVE_WARNING, QR_LABEL, STOP_LABEL,
};

const URL: &str = "http://192.168.1.20:5123/pair?t=0123456789abcdef0123456789abcdef";

struct ModalState {
    qr: Option<QrMatrix>,
    clients: usize,
    firewall: Option<FirewallBlock>,
    stop: bool,
    dismiss: bool,
    open_firewall_settings: bool,
}

fn harness(clients: usize) -> Harness<'static, ModalState> {
    harness_with(clients, None)
}

fn harness_with(clients: usize, firewall: Option<FirewallBlock>) -> Harness<'static, ModalState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 700.0))
        .build_ui_state(
            |ui, state| {
                let view = PhoneAccessView {
                    pairing_url: URL,
                    qr: state.qr.as_ref(),
                    clients: state.clients,
                    firewall: state.firewall,
                };
                let action = phone_access_modal(ui, &Palette::dark(), &view);
                state.stop |= action.stop;
                state.dismiss |= action.dismiss;
                state.open_firewall_settings |= action.open_firewall_settings;
            },
            ModalState {
                qr: QrMatrix::encode(URL),
                clients,
                firewall,
                stop: false,
                dismiss: false,
                open_firewall_settings: false,
            },
        )
}

#[test]
fn the_modal_shows_the_code_the_link_and_that_access_is_live() {
    let mut harness = harness(1);
    harness.run();

    harness.get_by_label(QR_LABEL);
    harness.get_by_label(URL);
    harness.get_by_label(LIVE_WARNING);
    harness.get_by_label("1 device connected");
}

#[test]
fn stop_asks_to_end_access_and_close_only_dismisses() {
    let mut harness = harness(0);
    harness.run();

    harness.get_by_label(CLOSE_LABEL).click();
    harness.run();
    assert!(harness.state().dismiss && !harness.state().stop);

    harness.get_by_label(STOP_LABEL).click();
    harness.run();
    assert!(harness.state().stop, "Stop ends phone access");
}

#[test]
fn no_firewall_banner_while_nothing_blocks() {
    let mut harness = harness(0);
    harness.run();

    assert!(harness.query_by_label(FIREWALL_SETTINGS_LABEL).is_none());
}

#[test]
fn a_blocking_firewall_shows_why_and_offers_its_settings() {
    let mut harness = harness_with(0, Some(FirewallBlock::AllIncoming));
    harness.run();

    harness.get_by_label(FIREWALL_TITLE);
    harness.get_by_label(firewall_message(FirewallBlock::AllIncoming));
    harness.get_by_label(FIREWALL_SETTINGS_LABEL).click();
    harness.run();
    assert!(harness.state().open_firewall_settings);
    assert!(!harness.state().dismiss, "the modal stays open");
}
