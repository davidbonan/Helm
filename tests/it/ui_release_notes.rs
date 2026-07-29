use egui_commonmark::CommonMarkCache;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

struct ModalState {
    cache: CommonMarkCache,
    dismissed: bool,
}

/// A text fragment from the oldest bundled section, read from the notes rather
/// than hardcoded: the 10-version cap (update.md §9.1) drops the oldest section
/// on release, which would otherwise break these assertions every time.
pub fn oldest_notes_fragment() -> String {
    let oldest = helm::release_notes::RELEASE_NOTES
        .rsplit("\n## ")
        .next()
        .expect("a bundled version section");
    let last = oldest
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .expect("a non-empty line in the oldest section");
    last.strip_prefix("- ").unwrap_or(last).to_owned()
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
    harness.get_by_label_contains(&oldest_notes_fragment());
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
