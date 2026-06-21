use std::collections::HashMap;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::keybindings::Shortcut;
use helm::terminal::emu::{feed, shared_term, SharedTerm, DEFAULT_FONT_SIZE};
use helm::terminal::layout::{Layout, Node, Orient, PaneId, Rect as PaneRect};
use helm::terminal::palette::{TermPalette, TermTheme};
use helm::theme::Palette;
use helm::ui::terminal_view::{terminal_tree, terminal_view};

const CLEAR: Option<Shortcut> = Some(Shortcut::cmd(egui::Key::K));

struct TreeState {
    layout: Layout,
    grids: HashMap<PaneId, SharedTerm>,
    term_palette: TermPalette,
    chrome: Palette,
    last_focus: Option<PaneId>,
    area: PaneRect,
}

impl TreeState {
    fn two_panes() -> Self {
        let mut layout = Layout::new();
        let left = layout.focus();
        let right = layout.split(Orient::Vertical);

        let left_grid = shared_term(6, 40);
        feed(&left_grid, b"LEFTPANE");
        let right_grid = shared_term(6, 40);
        feed(&right_grid, b"RIGHTPANE");

        let mut grids = HashMap::new();
        grids.insert(left, left_grid);
        grids.insert(right, right_grid);

        Self {
            layout,
            grids,
            term_palette: TermPalette::variant(TermTheme::Dark),
            chrome: Palette::dark(),
            last_focus: None,
            area: PaneRect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
        }
    }

    // ((A | C) | B): two vertical seams sharing an orientation. Dragging the
    // root seam (between C and B) must move the root split, not the inner one.
    fn left_nested() -> Self {
        let mut layout = Layout::new();
        let a = layout.focus();
        layout.split(Orient::Vertical);
        layout.set_focus(a);
        layout.split(Orient::Vertical);

        let mut grids = HashMap::new();
        for id in layout.pane_ids() {
            let grid = shared_term(6, 40);
            feed(&grid, format!("PANE{}", id.0).as_bytes());
            grids.insert(id, grid);
        }

        Self {
            layout,
            grids,
            term_palette: TermPalette::variant(TermTheme::Dark),
            chrome: Palette::dark(),
            last_focus: None,
            area: PaneRect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
        }
    }

    fn nested_ratios(&self) -> (f32, f32) {
        let Node::Split { ratio, first, .. } = self.layout.root() else {
            panic!("expected a split");
        };
        let Node::Split { ratio: inner, .. } = first.as_ref() else {
            panic!("expected a nested split");
        };
        (*ratio, *inner)
    }
}

fn draw(ui: &mut egui::Ui, state: &mut TreeState) {
    let a = ui.available_rect_before_wrap();
    state.area = PaneRect {
        x: a.min.x,
        y: a.min.y,
        w: a.width(),
        h: a.height(),
    };
    let grids = &state.grids;
    let term_palette = &state.term_palette;
    let output = terminal_tree(ui, &state.layout, &state.chrome, |ui, id, focused| {
        let grid = &grids[&id];
        let input = terminal_view(
            ui,
            grid,
            term_palette,
            DEFAULT_FONT_SIZE,
            focused,
            false,
            CLEAR,
            None,
        );
        input.clicked
    });
    if let Some(id) = output.focus {
        state.layout.set_focus(id);
        state.last_focus = Some(id);
    }
    if let Some(drag) = output.resize {
        state
            .layout
            .resize_split(drag.first, drag.second, drag.delta, state.area, 8.0, 14.0);
    }
}

#[test]
fn two_panes_render_side_by_side() {
    let mut harness = Harness::new_ui_state(draw, TreeState::two_panes());
    harness.run();

    harness.get_by_label_contains("LEFTPANE");
    harness.get_by_label_contains("RIGHTPANE");
}

#[test]
fn clicking_a_pane_moves_focus_to_it() {
    let mut harness = Harness::new_ui_state(draw, TreeState::two_panes());
    harness.run();
    let right = harness.state().layout.focus();

    harness.get_by_label_contains("LEFTPANE").click();
    harness.run();
    let left = harness.state().layout.focus();
    assert_ne!(
        left, right,
        "clicking the left pane moves focus off the right"
    );
    assert_eq!(harness.state().last_focus, Some(left));

    harness.get_by_label_contains("RIGHTPANE").click();
    harness.run();
    assert_eq!(
        harness.state().layout.focus(),
        right,
        "clicking the right pane brings focus back to it"
    );
}

#[test]
fn dragging_the_root_seam_resizes_the_root_split_not_the_inner_one() {
    let mut harness = Harness::new_ui_state(draw, TreeState::left_nested());
    harness.run();

    let (root_before, inner_before) = harness.state().nested_ratios();
    let area = harness.state().area;
    let seam_x = area.x + area.w * root_before;
    let mid_y = area.y + area.h * 0.5;

    let start = egui::pos2(seam_x, mid_y);
    harness.event(egui::Event::PointerMoved(start));
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    for step in 1..=10 {
        let p = egui::pos2(seam_x + step as f32 * 12.0, mid_y);
        harness.event(egui::Event::PointerMoved(p));
        harness.run();
    }
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(seam_x + 120.0, mid_y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();

    let (root_after, inner_after) = harness.state().nested_ratios();
    // A 120px drag over the full width must track the whole way, not stall after
    // the first frame (the seam-position-derived id used to drop the drag at ~1px).
    let expected = 120.0 / area.w;
    assert!(
        root_after - root_before > expected * 0.7,
        "the root split tracks the full drag (expected ~{expected}): {root_before} -> {root_after}"
    );
    assert!(
        (inner_after - inner_before).abs() < 1e-3,
        "the inner split is untouched: {inner_before} -> {inner_after}"
    );
}
