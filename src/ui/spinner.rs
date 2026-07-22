use egui::{
    lerp, vec2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, Widget,
};

/// Animation wakeup deadline, same rationale as `TERMINAL_REDRAW_INTERVAL`
/// (app.rs): a deadline keeps eframe in `ControlFlow::WaitUntil`, where the
/// runloop sleeps and drains input evenly. 33 ms because egui subtracts a
/// constant 1/60 s `predicted_dt` from every deadline — 16 ms would saturate
/// to zero and become an immediate request again (~60 FPS effective).
const SPINNER_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

/// Animation cadence for the **sidebar** agent pinwheel — slower than
/// `SPINNER_FRAME` on purpose. The badge sits in the always-visible sidebar, so
/// its booked repaint paces the whole app even for an agent in a background repo
/// whose terminal is off-screen; at 33 ms that pinned the window at ~30 FPS for
/// the lifetime of every background run. ~100 ms (≈12 FPS after egui's 1/60 s
/// `predicted_dt` subtraction) still reads as live motion on an 11 px badge at
/// ~3× less cost. A foreground streaming pane still paces at 33 ms via the
/// reader wakeup (egui coalesces to the minimum booked delay).
const SIDEBAR_SPINNER_FRAME: std::time::Duration = std::time::Duration::from_millis(100);

/// Drop-in replacement for `egui::Spinner` — identical paint, but the animation
/// wakeup is `request_repaint_after` instead of the immediate `request_repaint`
/// egui's widget fires every frame it is painted. An immediate request pins the
/// runloop in `ControlFlow::Poll`, and macOS then delivers scroll events in
/// clumps: any visible spinner (the sidebar agent badge during a whole streaming
/// run, a sync chip…) made the entire app judder while scrolling.
#[derive(Default)]
pub struct Spinner {
    /// Uses the style's `interact_size` if `None`.
    size: Option<f32>,
    color: Option<Color32>,
}

impl Spinner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn color(mut self, color: impl Into<Color32>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn paint_at(&self, ui: &Ui, rect: Rect) {
        if ui.is_rect_visible(rect) {
            ui.ctx().request_repaint_after(SPINNER_FRAME);

            let color = self
                .color
                .unwrap_or_else(|| ui.visuals().strong_text_color());
            let radius = (rect.height().min(rect.width()) / 2.0) - 2.0;
            let n_points = (radius.round() as u32).clamp(8, 128);
            let time = ui.input(|i| i.time);
            let start_angle = time * std::f64::consts::TAU;
            let end_angle = start_angle + 240f64.to_radians() * time.sin();
            let points: Vec<Pos2> = (0..n_points)
                .map(|i| {
                    let angle = lerp(start_angle..=end_angle, i as f64 / n_points as f64);
                    let (sin, cos) = angle.sin_cos();
                    rect.center() + radius * egui::vec2(cos as f32, sin as f32)
                })
                .collect();
            ui.painter()
                .add(Shape::line(points, Stroke::new(3.0_f32, color)));
        }
    }
}

/// How far the static halo ring extends beyond the dot, in points.
const DONE_HALO_GROW: f32 = 2.5;
/// Opacity of the static halo ring (a steady, faint glow — never animated).
const DONE_HALO_ALPHA: u8 = 70;

/// The agent `Done` indicator: a solid dot inside a faint, **static** halo. `Done`
/// is a persistent until-acknowledged state, not a one-off event, so it is not
/// animated: a looping pulse booked a repaint every frame it was painted, and
/// because the aggregate badge sits in the always-visible sidebar that pinned the
/// whole app at the animation cadence (~30 FPS) — never letting eframe return to
/// idle — for as long as an unacknowledged completion lingered. A static dot lets
/// the app sleep again. Shared by the repo row, the project-header aggregate and
/// the agents dashboard.
pub fn paint_done_dot(ui: &Ui, center: Pos2, dot_radius: f32, color: Color32) {
    let painter = ui.painter();
    let [r, g, b, _] = color.to_array();
    painter.circle_filled(
        center,
        dot_radius + DONE_HALO_GROW,
        Color32::from_rgba_unmultiplied(r, g, b, DONE_HALO_ALPHA),
    );
    painter.circle_filled(center, dot_radius, color);
}

/// Seconds for the pinwheel highlight to make one full turn.
const PINWHEEL_PERIOD: f64 = 1.1;
/// Gap between adjacent tiles, in points.
const PINWHEEL_GAP: f32 = 1.0;
/// Opacity floor of an unlit tile (lit tiles ramp from here to 1.0).
const PINWHEEL_BASE: f32 = 0.28;
/// Roughly how many perimeter cells the highlight lights at once (trail length).
const PINWHEEL_LIT: f32 = 1.4;
/// Opacity of an interior `core` tile — a steady hub the ring sweeps around.
const PINWHEEL_CORE_ALPHA: f32 = 0.85;

/// Perimeter cells `(row, col)` of an `n`×`n` grid in clockwise order from the
/// top-left — the path the highlight sweeps.
fn perimeter_cells(n: usize) -> Vec<(usize, usize)> {
    let last = n - 1;
    let mut cells = Vec::with_capacity(4 * last);
    for c in 0..n {
        cells.push((0, c));
    }
    for r in 1..n {
        cells.push((r, last));
    }
    for c in (0..last).rev() {
        cells.push((last, c));
    }
    for r in (1..last).rev() {
        cells.push((r, 0));
    }
    cells
}

/// The agent `Working` indicator: a `grid`×`grid` board of fixed-hue tiles whose
/// highlight sweeps clockwise around the perimeter like a pinwheel. Each tile keeps
/// its own color (cycled from `colors`); only its opacity is animated (composited
/// over whatever row background sits behind it), so the lit tile pops and the ones
/// trailing it fade. Interior cells, if any, hold a steady `core`. Books its next
/// frame on the slower `SIDEBAR_SPINNER_FRAME` cadence (it is the sidebar badge).
pub fn paint_pinwheel(
    ui: &Ui,
    center: Pos2,
    size: f32,
    grid: usize,
    colors: &[Color32],
    core: Option<Color32>,
) {
    let full = Rect::from_center_size(center, Vec2::splat(size));
    if !ui.is_rect_visible(full) {
        return;
    }
    ui.ctx().request_repaint_after(SIDEBAR_SPINNER_FRAME);

    let n = grid.max(2);
    let tile = (size - PINWHEEL_GAP * (n - 1) as f32) / n as f32;
    let step = tile + PINWHEEL_GAP;
    let origin = center - Vec2::splat((size - tile) / 2.0);
    let radius = CornerRadius::same((tile / 3.0).round().max(1.0) as u8);
    let cell_rect = |row: usize, col: usize| {
        Rect::from_center_size(
            origin + vec2(col as f32 * step, row as f32 * step),
            Vec2::splat(tile),
        )
    };
    let with_alpha = |color: Color32, alpha: f32| {
        let [r, g, b, _] = color.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0) as u8)
    };

    let ring = perimeter_cells(n);
    let window = PINWHEEL_LIT / ring.len() as f32;
    let phase = (ui.input(|i| i.time) / PINWHEEL_PERIOD).rem_euclid(1.0) as f32;
    let painter = ui.painter();
    for (i, &(row, col)) in ring.iter().enumerate() {
        let raw = (phase - i as f32 / ring.len() as f32).rem_euclid(1.0);
        let d = raw.min(1.0 - raw);
        let t = (1.0 - d / window).clamp(0.0, 1.0);
        let lit = t * t * (3.0 - 2.0 * t);
        let alpha = PINWHEEL_BASE + (1.0 - PINWHEEL_BASE) * lit;
        painter.rect_filled(
            cell_rect(row, col),
            radius,
            with_alpha(colors[i % colors.len()], alpha),
        );
    }
    if let Some(core) = core {
        for row in 1..n - 1 {
            for col in 1..n - 1 {
                painter.rect_filled(
                    cell_rect(row, col),
                    radius,
                    with_alpha(core, PINWHEEL_CORE_ALPHA),
                );
            }
        }
    }
}

impl Widget for Spinner {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = self
            .size
            .unwrap_or_else(|| ui.style().spacing.interact_size.y);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::new(egui::WidgetType::ProgressIndicator));
        self.paint_at(ui, rect);

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A painted spinner must book its next animation frame as a deadline —
    /// an immediate request would pin eframe in `ControlFlow::Poll` for the
    /// spinner's whole lifetime (the regression this module exists to prevent).
    #[test]
    fn spinner_frame_requests_no_immediate_repaint() {
        // Real-runtime simulation: eframe never sets `predicted_dt`, so it stays
        // at egui's 1/60 s default — kittest's 0.25 s default would make any
        // deadline saturate to zero and fail the assertion spuriously.
        let mut harness = egui_kittest::Harness::builder()
            .with_step_dt(1.0 / 60.0)
            .build_ui(|ui| {
                ui.add(Spinner::new().size(14.0));
            });
        for _ in 0..2 {
            harness.step();
            assert!(!harness.ctx.requested_repaint_last_pass());
            let delay = harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            assert!(delay <= SPINNER_FRAME, "animation wakeup not booked");
        }
    }

    /// `Done` is a persistent state, not an event: the dot is static and must book
    /// **no** repaint, so an unacknowledged completion lingering in the always-visible
    /// sidebar lets the app fall back to idle instead of being pinned at the animation
    /// cadence — the perpetual-30-FPS regression this change removes.
    #[test]
    fn done_dot_books_no_animation_wakeup() {
        let mut harness = egui_kittest::Harness::builder()
            .with_step_dt(1.0 / 60.0)
            .build_ui(|ui| {
                paint_done_dot(ui, ui.max_rect().center(), 3.5, Color32::GREEN);
            });
        for _ in 0..2 {
            harness.step();
            assert!(!harness.ctx.requested_repaint_last_pass());
            let delay = harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            assert!(
                delay > SPINNER_FRAME * 10,
                "a static Done dot must not book an animation wakeup (got {delay:?})"
            );
        }
    }

    /// The sidebar pinwheel animates on a deadline — never an immediate request
    /// (no `ControlFlow::Poll` pin) — and on the **slower** `SIDEBAR_SPINNER_FRAME`
    /// cadence, so a background agent's badge does not pin the app at ~30 FPS.
    #[test]
    fn pinwheel_books_slow_sidebar_cadence() {
        let mut harness = egui_kittest::Harness::builder()
            .with_step_dt(1.0 / 60.0)
            .build_ui(|ui| {
                paint_pinwheel(
                    ui,
                    ui.max_rect().center(),
                    11.0,
                    3,
                    &[Color32::RED, Color32::GREEN, Color32::BLUE, Color32::YELLOW],
                    None,
                );
            });
        for _ in 0..2 {
            harness.step();
            assert!(!harness.ctx.requested_repaint_last_pass());
            let delay = harness.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            assert!(delay <= SIDEBAR_SPINNER_FRAME, "pinwheel wakeup not booked");
            assert!(
                delay > SPINNER_FRAME,
                "sidebar pinwheel must pace slower than the 33 ms spinner (got {delay:?})"
            );
        }
    }
}
