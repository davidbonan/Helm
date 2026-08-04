use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::keybindings::Shortcut;
use helm::terminal::emu::{clear, feed, scroll, shared_term, DEFAULT_FONT_SIZE};
use helm::terminal::links::LinkAction;
use helm::terminal::palette::{TermPalette, TermTheme};
use helm::ui::terminal_view::{
    cell_metrics, terminal_view, terminal_view_readonly, PROCESS_ENDED_BANNER,
};

const CLEAR: Option<Shortcut> = Some(Shortcut::cmd(egui::Key::K));

const CMD: egui::Modifiers = egui::Modifiers {
    alt: false,
    ctrl: false,
    shift: false,
    mac_cmd: true,
    command: true,
};

/// The PTY grid is sized with `cell_metrics`; if its line height differs from the
/// one `ui.label` actually allocates (rounded to the physical pixel), the gap
/// accumulates and truncates the bottom of the terminal. The label is built like
/// `line_job`: `line_height` forced to the cell height.
#[test]
fn cell_metrics_height_matches_rendered_row() {
    use std::sync::{Arc, Mutex};

    for ppp in [1.0_f32, 2.0] {
        for font_size in [11.0_f32, DEFAULT_FONT_SIZE, 14.0] {
            let probe: Arc<Mutex<(f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0)));
            let sink = probe.clone();
            let mut harness = Harness::new_ui(move |ui| {
                ui.ctx().set_pixels_per_point(ppp);
                let (_, row_h) = cell_metrics(ui.ctx(), font_size);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    "x",
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::monospace(font_size),
                        line_height: Some(row_h),
                        ..Default::default()
                    },
                );
                let resp = ui.label(job);
                *sink.lock().unwrap() = (row_h, resp.rect.height());
            });
            harness.run();

            let (row_h, label_h) = *probe.lock().unwrap();
            assert!(
                (row_h - label_h).abs() < 0.001,
                "ppp={ppp} size={font_size}: cell_metrics gives {row_h} but the rendered line is {label_h}"
            );
        }
    }
}

/// Descenders (p, g, y) hugged the bottom of the cell: SF Mono has no lineGap and
/// its `descent` sits flush with the bottom of the descenders — and the slack from
/// `ceil()` alone is absorbed by baseline rounding + hinting (measured at the pixel
/// via headless-verify). The cell must be `ceil(font height) + 1`, the gained space
/// going below the baseline.
#[test]
fn cell_height_gives_descenders_breathing_room() {
    use std::sync::{Arc, Mutex};

    for font_size in [11.0_f32, DEFAULT_FONT_SIZE, 14.0] {
        let probe: Arc<Mutex<(f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0)));
        let sink = probe.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let (_, row_h) = cell_metrics(ui.ctx(), font_size);
            let font = egui::FontId::monospace(font_size);
            let font_h = ui.ctx().fonts_mut(|f| {
                let plain = f.layout_no_wrap("p".to_owned(), font, egui::Color32::WHITE);
                plain.rows[0].glyphs[0].font_height
            });
            *sink.lock().unwrap() = (row_h, font_h);
        });
        harness.run();

        let (row_h, font_h) = *probe.lock().unwrap();
        assert!(
            row_h >= font_h.ceil() + 1.0 - 0.001,
            "size={font_size}: cell {row_h} < ceil(font height {font_h}) + 1"
        );
    }
}

#[test]
fn fixed_grid_renders_cell_text() {
    let term = shared_term(6, 40);
    feed(&term, b"helm");

    let palette = TermPalette::variant(TermTheme::Dark);
    let mut harness = Harness::new_ui(move |ui| {
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
    });
    harness.run();

    harness.get_by_label_contains("helm");
}

#[test]
fn styled_grid_renders_with_attributes() {
    let term = shared_term(4, 40);
    feed(
        &term,
        b"\x1b[1mBOLD \x1b[3mITAL \x1b[4mUND \x1b[7mINV\x1b[0m",
    );

    let palette = TermPalette::variant(TermTheme::Light);
    let mut harness = Harness::new_ui(move |ui| {
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            false,
            false,
            CLEAR,
            None,
        );
    });
    harness.run();

    harness.get_by_label_contains("BOLD");
    harness.get_by_label_contains("ITAL");
    harness.get_by_label_contains("UND");
    harness.get_by_label_contains("INV");
}

#[test]
fn focused_pane_listens_to_keyboard_without_a_click() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.bytes);
    });

    harness.run();
    harness.event(egui::Event::Text("a".to_string()));
    harness.run();

    assert!(
        captured.lock().unwrap().contains(&b'a'),
        "a focused pane receives keyboard input without a prior click"
    );
}

/// Shift+Tab is used by Claude Code (mode change): egui must not consume it for
/// its focus navigation — the terminal keeps focus and the PTY receives the
/// backtab (CSI Z).
#[test]
fn shift_tab_is_forwarded_and_keeps_the_focus() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let mut harness = Harness::new_ui(move |ui| {
        // Alternative target for focus navigation: without the lock
        // (set_focus_lock_filter), Shift+Tab would leave the terminal for it.
        let _ = ui.button("decoy");
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.bytes);
    });
    harness.run();

    harness.event(egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    });
    harness.run();

    assert!(
        captured.lock().unwrap().windows(3).any(|w| w == b"\x1b[Z"),
        "Shift+Tab forwards the backtab to the PTY"
    );

    // Focus has not moved: a subsequent keystroke still reaches the terminal.
    captured.lock().unwrap().clear();
    harness.event(egui::Event::Text("a".to_string()));
    harness.run();
    assert!(
        captured.lock().unwrap().contains(&b'a'),
        "the terminal keeps focus after Shift+Tab"
    );
}

/// Shift+Enter in an agent harness (Claude Code, Codex): encoded as `CSI 13;2u`
/// **without negotiation** (kitty/Ghostty convention — Claude Code never pushes
/// the protocol, it parses the sequence unconditionally).
#[test]
fn shift_enter_sends_kitty_csi_u_without_negotiation() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.bytes);
    });
    harness.run();

    harness.event(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    });
    harness.run();

    let bytes = captured.lock().unwrap().clone();
    assert!(
        bytes.windows(7).any(|w| w == b"\x1b[13;2u"),
        "Shift+Enter goes out as CSI 13;2u from the first frame, without negotiation"
    );
    assert!(
        !bytes.contains(&b'\r'),
        "Shift+Enter must no longer emit \\r (otherwise the harness submits the prompt)"
    );
}

#[test]
fn cmd_k_clears_above_the_prompt_but_keeps_it() {
    let term = shared_term(6, 40);
    feed(&term, b"old-output\r\nuser@mac helm %");

    let palette = TermPalette::variant(TermTheme::Dark);
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        if input.clear {
            clear(&render_term);
        }
    });

    harness.run();
    harness.get_by_label_contains("old-output");
    harness.get_by_label_contains("user@mac helm %");

    harness.event(egui::Event::Key {
        key: egui::Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        },
    });
    harness.run();

    assert!(
        harness.query_by_label_contains("old-output").is_none(),
        "Cmd+K clears the output above the prompt"
    );
    harness.get_by_label_contains("user@mac helm %");
}

fn feed_history(term: &helm::terminal::emu::SharedTerm) {
    feed(term, b"top-marker");
    for i in 0..30 {
        feed(term, format!("\r\nline-{i}").as_bytes());
    }
    feed(term, b"\r\nbottom-marker");
}

#[test]
fn shift_pageup_reveals_scrollback_history() {
    let term = shared_term(4, 40);
    feed_history(&term);

    let palette = TermPalette::variant(TermTheme::Dark);
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        if let Some(kind) = input.scroll {
            scroll(&render_term, kind);
        }
    });

    harness.run();
    harness.get_by_label_contains("bottom-marker");
    assert!(
        harness.query_by_label_contains("top-marker").is_none(),
        "the top of the history is off-screen before scrolling"
    );

    for _ in 0..20 {
        harness.event(egui::Event::Key {
            key: egui::Key::PageUp,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                shift: true,
                ..Default::default()
            },
        });
        harness.run();
    }

    harness.get_by_label_contains("top-marker");
}

#[test]
fn mouse_wheel_scrolls_into_history() {
    let term = shared_term(4, 40);
    feed_history(&term);

    let palette = TermPalette::variant(TermTheme::Dark);
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        if let Some(kind) = input.scroll {
            scroll(&render_term, kind);
        }
    });

    harness.run();
    let center = harness.ctx.content_rect().center();
    harness.event(egui::Event::PointerMoved(center));
    harness.run();

    for _ in 0..15 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 5.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        harness.run();
    }

    harness.get_by_label_contains("top-marker");
}

/// In a full-screen TUI (alt screen, e.g. Claude Code) there is no scrollback:
/// the wheel must be forwarded to the application as ↑/↓ arrows (alternate
/// scroll, on by default) instead of a silent local scroll.
#[test]
fn wheel_in_fullscreen_tui_sends_arrows_to_the_pty() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(4, 40);
    feed(&term, b"\x1b[?1049h");

    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.scroll_bytes);
        assert!(
            input.scroll.is_none(),
            "no local scrollback in the alt screen"
        );
    });

    harness.run();
    let center = harness.ctx.content_rect().center();
    harness.event(egui::Event::PointerMoved(center));
    harness.run();

    // egui low-passes a wheel notch across several frames, so the arrow *count*
    // is not a fixed multiple of the wheel delta; assert the direction forwarded
    // for each gesture, not the length.
    let drain = |sink: &Arc<Mutex<Vec<u8>>>| std::mem::take(&mut *sink.lock().unwrap());
    let only =
        |bytes: &[u8], arrow: &[u8]| !bytes.is_empty() && bytes.chunks(3).all(|c| c == arrow);

    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 2.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    let up = drain(&captured);
    assert!(only(&up, b"\x1b[A"), "wheel up -> up-arrows, got {up:?}");

    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    let down = drain(&captured);
    assert!(
        only(&down, b"\x1b[B"),
        "wheel down -> down-arrows, got {down:?}"
    );
}

/// An app that has enabled mouse reporting (DECSET 1000 + 1006) receives the
/// wheel as SGR mouse events, with the cell under the pointer.
#[test]
fn wheel_under_mouse_reporting_sends_sgr_events() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(4, 40);
    feed(&term, b"\x1b[?1000h\x1b[?1006h");

    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.scroll_bytes);
    });

    harness.run();
    let center = harness.ctx.content_rect().center();
    harness.event(egui::Event::PointerMoved(center));
    harness.run();

    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();

    let bytes = captured.lock().unwrap().clone();
    assert!(
        bytes.starts_with(b"\x1b[<64;") && bytes.ends_with(b"M"),
        "wheel up -> SGR mouse event button 64, got: {:?}",
        String::from_utf8_lossy(&bytes)
    );
}

/// An app in mouse reporting (DECSET 1000 + 1006, e.g. Claude Code) receives a left
/// click as an SGR button press (`M`) then release (`m`) — the piece that lets you
/// click a tool to expand it (terminal.md §7).
#[test]
fn click_under_mouse_reporting_sends_sgr_button() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(4, 40);
    feed(&term, b"\x1b[?1000h\x1b[?1006h");

    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.mouse_bytes);
    });

    harness.run();
    let pos = harness.ctx.content_rect().center();
    click(&mut harness, pos, egui::Modifiers::default());

    let bytes = captured.lock().unwrap().clone();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.starts_with("\u{1b}[<0;"),
        "left click -> SGR button-0 report, got: {text:?}"
    );
    assert!(text.contains('M'), "press present (M), got: {text:?}");
    assert!(text.ends_with('m'), "release present (m), got: {text:?}");
}

/// Without mouse reporting the click stays a local gesture (focus / selection,
/// terminal.md §6/§7): nothing is forwarded to the PTY.
#[test]
fn click_without_mouse_reporting_stays_local() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(4, 40);

    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.mouse_bytes);
    });

    harness.run();
    let pos = harness.ctx.content_rect().center();
    click(&mut harness, pos, egui::Modifiers::default());

    assert!(
        captured.lock().unwrap().is_empty(),
        "no mouse report without DECSET 1000"
    );
}

/// A primary-button press then release at `pos`, each in its own frame.
fn click(harness: &mut Harness, pos: egui::Pos2, modifiers: egui::Modifiers) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
        harness.run();
    }
}

#[test]
fn keystroke_snaps_the_view_back_to_the_bottom() {
    let term = shared_term(4, 40);
    feed_history(&term);

    let palette = TermPalette::variant(TermTheme::Dark);
    let render_term = term.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &render_term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        if let Some(kind) = input.scroll {
            scroll(&render_term, kind);
        }
    });
    harness.run();

    for _ in 0..20 {
        harness.event(egui::Event::Key {
            key: egui::Key::PageUp,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                shift: true,
                ..Default::default()
            },
        });
        harness.run();
    }
    harness.get_by_label_contains("top-marker");

    harness.event(egui::Event::Text("a".to_string()));
    harness.run();

    harness.get_by_label_contains("bottom-marker");
    assert!(
        harness.query_by_label_contains("top-marker").is_none(),
        "a keystroke brings the view back to the bottom of the scrollback"
    );
}

#[test]
fn exited_pane_shows_process_terminated_banner() {
    let term = shared_term(4, 40);

    let palette = TermPalette::variant(TermTheme::Dark);
    let mut harness = Harness::new_ui(move |ui| {
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            true,
            CLEAR,
            None,
        );
    });
    harness.run();

    harness.get_by_label_contains(PROCESS_ENDED_BANNER);
}

fn copied_text(harness: &Harness) -> Option<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
}

#[test]
fn drag_then_cmd_c_copies_selection_to_clipboard() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    feed(&term, b"hello world");

    let palette = TermPalette::variant(TermTheme::Dark);
    // (grid origin x/y, cell width, line height) captured at render time.
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let probe = metrics.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let origin = ui.next_widget_position();
        let (w, h) = ui.ctx().fonts_mut(|f| {
            let font = egui::FontId::monospace(DEFAULT_FONT_SIZE);
            (
                f.glyph_width(&font, ' ').max(1.0),
                f.row_height(&font).max(1.0),
            )
        });
        *probe.lock().unwrap() = (origin.x, origin.y, w, h);
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    let cell = |col: usize| egui::pos2(ox + (col as f32 + 0.5) * w, oy + 0.5 * h);

    harness.event(egui::Event::PointerMoved(cell(0)));
    harness.event(egui::Event::PointerButton {
        pos: cell(0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    harness.event(egui::Event::PointerMoved(cell(4)));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: cell(4),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();

    // `step` (not `run`) leaves the frame that handled Copy in the output; `run`
    // chains frames until stable and the clipboard command would be lost in there.
    harness.event(egui::Event::Copy);
    harness.step();

    assert_eq!(
        copied_text(&harness).as_deref(),
        Some("hello"),
        "a char selection + Cmd+C copies the text to the clipboard"
    );
}

#[test]
fn cmd_c_without_selection_is_a_no_op() {
    let term = shared_term(6, 40);
    feed(&term, b"hello world");

    let palette = TermPalette::variant(TermTheme::Dark);
    let mut harness = Harness::new_ui(move |ui| {
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
    });
    harness.run();

    harness.event(egui::Event::Copy);
    harness.run();

    assert!(
        copied_text(&harness).is_none(),
        "Cmd+C without a selection does not touch the clipboard"
    );
}

#[test]
fn ctrl_c_stays_with_the_pty_and_does_not_copy() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    let palette = TermPalette::variant(TermTheme::Dark);
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        sink.lock().unwrap().extend_from_slice(&input.bytes);
    });
    harness.run();

    harness.event(egui::Event::Key {
        key: egui::Key::C,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    });
    harness.run();

    assert!(
        captured.lock().unwrap().contains(&0x03),
        "Ctrl+C stays forwarded to the PTY (ETX byte)"
    );
    assert!(
        copied_text(&harness).is_none(),
        "Ctrl+C does not trigger a clipboard copy"
    );
}

#[test]
fn cmd_v_paste_is_forwarded_to_the_pty() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    let palette = TermPalette::variant(TermTheme::Dark);
    let pasted: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let sink = pasted.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            None,
        );
        if let Some(text) = input.paste {
            *sink.lock().unwrap() = Some(text);
        }
    });
    harness.run();

    harness.event(egui::Event::Paste("pasted-text".to_string()));
    harness.run();

    assert_eq!(
        pasted.lock().unwrap().as_deref(),
        Some("pasted-text"),
        "Cmd+V (Event::Paste) is forwarded to the PTY"
    );
}

/// Grid origin (x, y) + cell (width, height), captured during a render so the
/// pointer can be aimed at a specific cell.
fn grid_metrics(ui: &egui::Ui) -> (f32, f32, f32, f32) {
    let origin = ui.next_widget_position();
    let (w, h) = ui.ctx().fonts_mut(|f| {
        let font = egui::FontId::monospace(DEFAULT_FONT_SIZE);
        (
            f.glyph_width(&font, ' ').max(1.0),
            f.row_height(&font).max(1.0),
        )
    });
    (origin.x, origin.y, w, h)
}

fn cursor_icon(harness: &Harness) -> egui::CursorIcon {
    harness.output().platform_output.cursor_icon
}

/// Primary press + release at the same point on the first grid row — a click, not
/// a drag (the held modifiers come from the persistent `RawInput`, not the event).
fn click_cell(harness: &mut Harness, at: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
}

#[test]
fn cmd_hover_over_a_url_shows_the_pointing_hand() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    feed(&term, b"see https://example.com now");

    let palette = TermPalette::variant(TermTheme::Dark);
    let cwd = std::env::temp_dir();
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let probe = metrics.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            Some(cwd.as_path()),
        );
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    let on_url = egui::pos2(ox + 10.5 * w, oy + 0.5 * h);

    harness.event(egui::Event::PointerMoved(on_url));
    harness.run();
    assert_ne!(
        cursor_icon(&harness),
        egui::CursorIcon::PointingHand,
        "without Cmd the URL is ordinary text"
    );

    harness.input_mut().modifiers = CMD;
    harness.event(egui::Event::PointerMoved(on_url));
    harness.run();
    assert_eq!(
        cursor_icon(&harness),
        egui::CursorIcon::PointingHand,
        "Cmd+hover over a URL shows the pointing-hand cursor"
    );
}

#[test]
fn click_without_cmd_emits_no_link() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    feed(&term, b"see https://example.com now");

    let palette = TermPalette::variant(TermTheme::Dark);
    let cwd = std::env::temp_dir();
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let opened: Arc<Mutex<Option<LinkAction>>> = Arc::new(Mutex::new(None));
    let probe = metrics.clone();
    let sink = opened.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            Some(cwd.as_path()),
        );
        if let Some(action) = input.open_link {
            *sink.lock().unwrap() = Some(action);
        }
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    click_cell(&mut harness, egui::pos2(ox + 10.5 * w, oy + 0.5 * h));

    assert!(
        opened.lock().unwrap().is_none(),
        "a plain click over a URL never activates it (no Cmd held)"
    );
}

#[test]
fn cmd_click_on_a_url_emits_open_url() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    feed(&term, b"see https://example.com now");

    let palette = TermPalette::variant(TermTheme::Dark);
    let cwd = std::env::temp_dir();
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let opened: Arc<Mutex<Option<LinkAction>>> = Arc::new(Mutex::new(None));
    let probe = metrics.clone();
    let sink = opened.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            Some(cwd.as_path()),
        );
        if let Some(action) = input.open_link {
            *sink.lock().unwrap() = Some(action);
        }
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    harness.input_mut().modifiers = CMD;
    click_cell(&mut harness, egui::pos2(ox + 10.5 * w, oy + 0.5 * h));

    assert_eq!(
        *opened.lock().unwrap(),
        Some(LinkAction::Url("https://example.com".to_string())),
        "Cmd+click on a URL emits the open-URL action"
    );
}

#[test]
fn cmd_click_on_a_file_path_emits_open_file_with_line() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/x.rs"), b"fn main() {}").unwrap();

    let term = shared_term(6, 40);
    feed(&term, b"edit src/x.rs:7 now");

    let palette = TermPalette::variant(TermTheme::Dark);
    let cwd = dir.path().to_path_buf();
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let opened: Arc<Mutex<Option<LinkAction>>> = Arc::new(Mutex::new(None));
    let probe = metrics.clone();
    let sink = opened.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            Some(cwd.as_path()),
        );
        if let Some(action) = input.open_link {
            *sink.lock().unwrap() = Some(action);
        }
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    harness.input_mut().modifiers = CMD;
    click_cell(&mut harness, egui::pos2(ox + 8.5 * w, oy + 0.5 * h));

    let action = opened.lock().unwrap().clone();
    match action {
        Some(LinkAction::File { path, line, column }) => {
            assert!(path.ends_with("src/x.rs"), "resolved path was {path:?}");
            assert_eq!(line, Some(7));
            assert_eq!(column, None);
        }
        other => panic!("expected an open-file action, got {other:?}"),
    }
}

#[test]
fn cmd_click_on_plain_text_emits_nothing() {
    use std::sync::{Arc, Mutex};

    // An empty cwd so no bare word accidentally resolves to an existing file.
    let dir = tempfile::tempdir().unwrap();

    let term = shared_term(6, 40);
    feed(&term, b"plain words here");

    let palette = TermPalette::variant(TermTheme::Dark);
    let cwd = dir.path().to_path_buf();
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let opened: Arc<Mutex<Option<LinkAction>>> = Arc::new(Mutex::new(None));
    let probe = metrics.clone();
    let sink = opened.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        let input = terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            Some(cwd.as_path()),
        );
        if let Some(action) = input.open_link {
            *sink.lock().unwrap() = Some(action);
        }
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    harness.input_mut().modifiers = CMD;
    click_cell(&mut harness, egui::pos2(ox + 2.5 * w, oy + 0.5 * h));

    assert!(
        opened.lock().unwrap().is_none(),
        "Cmd+click on plain text is not a link"
    );
}

#[test]
fn drag_select_still_copies_with_link_detection_wired() {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    feed(&term, b"hello world");

    let palette = TermPalette::variant(TermTheme::Dark);
    let cwd = std::env::temp_dir();
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let probe = metrics.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        terminal_view(
            ui,
            &term,
            &palette,
            DEFAULT_FONT_SIZE,
            true,
            false,
            CLEAR,
            Some(cwd.as_path()),
        );
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    let cell = |col: usize| egui::pos2(ox + (col as f32 + 0.5) * w, oy + 0.5 * h);

    harness.event(egui::Event::PointerMoved(cell(0)));
    harness.event(egui::Event::PointerButton {
        pos: cell(0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    harness.event(egui::Event::PointerMoved(cell(4)));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: cell(4),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();

    harness.event(egui::Event::Copy);
    harness.step();

    assert_eq!(
        copied_text(&harness).as_deref(),
        Some("hello"),
        "without Cmd, a drag still selects and Cmd+C copies — link_cwd does not change it"
    );
}

/// Renders the Run panel's read-only viewer over `hello world` and returns the
/// harness plus a cell-center locator, ready for the selection gestures.
fn readonly_harness() -> (Harness<'static>, impl Fn(usize) -> egui::Pos2) {
    use std::sync::{Arc, Mutex};

    let term = shared_term(6, 40);
    feed(&term, b"hello world");

    let palette = TermPalette::variant(TermTheme::Dark);
    let metrics: Arc<Mutex<(f32, f32, f32, f32)>> = Arc::new(Mutex::new((0.0, 0.0, 8.0, 16.0)));
    let probe = metrics.clone();
    let mut harness = Harness::new_ui(move |ui| {
        *probe.lock().unwrap() = grid_metrics(ui);
        terminal_view_readonly(ui, &term, &palette, DEFAULT_FONT_SIZE, false);
    });
    harness.run();

    let (ox, oy, w, h) = *metrics.lock().unwrap();
    (harness, move |col: usize| {
        egui::pos2(ox + (col as f32 + 0.5) * w, oy + 0.5 * h)
    })
}

fn drag_cells(harness: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
    harness.event(egui::Event::PointerMoved(to));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run();
}

#[test]
fn readonly_viewer_drag_then_cmd_c_copies_selection() {
    let (mut harness, cell) = readonly_harness();

    drag_cells(&mut harness, cell(0), cell(4));
    harness.event(egui::Event::Copy);
    harness.step();

    assert_eq!(
        copied_text(&harness).as_deref(),
        Some("hello"),
        "the Run viewer selects on drag and copies the selection on Cmd+C"
    );
}

#[test]
fn readonly_viewer_copy_without_selection_is_a_no_op() {
    let (mut harness, _) = readonly_harness();

    harness.event(egui::Event::Copy);
    harness.step();

    assert!(
        copied_text(&harness).is_none(),
        "Cmd+C without a selection leaves the clipboard alone"
    );
}
