//! How a tab shows its page (browser.md §4 *More*): its zoom and the width its
//! viewport keeps.

/// Safari's zoom steps.
const ZOOM_STEPS: [f64; 13] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageView {
    pub zoom: f64,
    pub viewport: Viewport,
}

impl Default for PageView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            viewport: Viewport::Desktop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomChange {
    In,
    Out,
    Reset,
}

impl PageView {
    /// The next step up or down, kept within the steps; `Reset` is 100 %.
    pub fn zoomed(self, change: ZoomChange) -> f64 {
        let zoom = self.zoom;
        match change {
            ZoomChange::Reset => 1.0,
            ZoomChange::In => ZOOM_STEPS
                .into_iter()
                .find(|step| *step > zoom + f64::EPSILON)
                .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1]),
            ZoomChange::Out => ZOOM_STEPS
                .into_iter()
                .rev()
                .find(|step| *step < zoom - f64::EPSILON)
                .unwrap_or(ZOOM_STEPS[0]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewport {
    Desktop,
    Tablet,
    Mobile,
}

impl Viewport {
    pub const ALL: [Self; 3] = [Self::Desktop, Self::Tablet, Self::Mobile];

    pub fn name(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Tablet => "Tablet",
            Self::Mobile => "Mobile",
        }
    }

    /// The page's width in points; `None` takes the pane's.
    pub fn width(self) -> Option<f32> {
        match self {
            Self::Desktop => None,
            Self::Tablet => Some(768.0),
            Self::Mobile => Some(390.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(zoom: f64) -> PageView {
        PageView {
            zoom,
            ..PageView::default()
        }
    }

    #[test]
    fn zoom_walks_the_steps_and_stops_at_their_ends() {
        assert_eq!(at(1.0).zoomed(ZoomChange::In), 1.1);
        assert_eq!(at(1.0).zoomed(ZoomChange::Out), 0.9);
        assert_eq!(at(3.0).zoomed(ZoomChange::In), 3.0);
        assert_eq!(at(0.5).zoomed(ZoomChange::Out), 0.5);
        assert_eq!(at(2.5).zoomed(ZoomChange::Reset), 1.0);
    }
}
