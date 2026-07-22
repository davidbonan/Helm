use egui_commonmark::CommonMarkCache;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

struct ModalState {
    cache: CommonMarkCache,
    dismissed: bool,
}

fn modal_harness() -> Harness<'static, ModalState> {
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, state| {
                state.dismissed |= helm::ui::release_notes::modal(ui, &mut state.cache);
            },
            ModalState {
                cache: CommonMarkCache::default(),
                dismissed: false,
            },
        )
}

#[test]
fn whats_new_modal_renders_heading_close_and_bundled_notes() {
    let mut harness = modal_harness();
    harness.run();

    harness.get_by_label("What's new");
    harness.get_by_label("Close");
    // Oldest section still bundled: proves the notes render down to the last one.
    harness.get_by_label_contains("Polished the workspace sidebar headers");
}

#[test]
fn whats_new_modal_close_button_dismisses() {
    let mut harness = modal_harness();
    harness.run();
    assert!(!harness.state().dismissed);

    harness.get_by_label("Close").click_accesskit();
    harness.run();

    assert!(harness.state().dismissed);
}
