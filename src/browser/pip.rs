//! The browser floating over the window as a panel (browser.md §11): its mode
//! and its frame, kept per worktree.

use serde::{Deserialize, Serialize};

const DEFAULT_WIDTH: f32 = 480.0;
const DEFAULT_HEIGHT: f32 = 320.0;
const MIN_WIDTH: f32 = 240.0;
const MIN_HEIGHT: f32 = 160.0;
/// Kept from the terminals' corner by a first panel.
const CORNER_INSET: f32 = 16.0;

/// One worktree's floating browser.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserPip {
    /// Shown as the panel rather than the sidebar.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floating: bool,
    /// Where the panel was left; `None` ⇒ the terminals' corner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<PipFrame>,
}

/// A rectangle in window points, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PipFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The side of an axis a resize drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Left, or top.
    Start,
    /// Right, or bottom.
    End,
}

/// The edges a resize drags: one side per axis, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grip {
    pub horizontal: Option<Side>,
    pub vertical: Option<Side>,
}

/// One axis of a frame.
#[derive(Clone, Copy)]
struct Span {
    start: f32,
    length: f32,
}

impl Span {
    fn end(self) -> f32 {
        self.start + self.length
    }

    /// Within `bounds`, shrunk to it when longer.
    fn fitted(self, bounds: Span) -> Self {
        let length = self.length.min(bounds.length);
        let start = self.start.min(bounds.end() - length).max(bounds.start);
        Self { start, length }
    }

    /// `side` moved by `delta`, the other side fixed, `min` long at least and
    /// within `bounds`.
    fn resized(self, side: Option<Side>, delta: f32, bounds: Span, min: f32) -> Self {
        match side {
            None => self,
            Some(Side::End) => Self {
                start: self.start,
                length: (self.length + delta)
                    .min(bounds.end() - self.start)
                    .max(min),
            },
            Some(Side::Start) => {
                let start = (self.start + delta).min(self.end() - min).max(bounds.start);
                Self {
                    start,
                    length: self.end() - start,
                }
            }
        }
    }
}

impl PipFrame {
    /// A first panel: in `area`'s bottom-right corner, inset.
    pub fn in_corner_of(area: PipFrame) -> Self {
        Self {
            x: area.x + area.width - DEFAULT_WIDTH - CORNER_INSET,
            y: area.y + area.height - DEFAULT_HEIGHT - CORNER_INSET,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }

    pub fn moved(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    /// The `grip`'s edges dragged by (`dx`, `dy`), kept to the least size and
    /// within `bounds`.
    pub fn resized(self, grip: Grip, (dx, dy): (f32, f32), bounds: PipFrame) -> Self {
        let horizontal =
            self.horizontal()
                .resized(grip.horizontal, dx, bounds.horizontal(), MIN_WIDTH);
        let vertical = self
            .vertical()
            .resized(grip.vertical, dy, bounds.vertical(), MIN_HEIGHT);
        Self::from_spans(horizontal, vertical)
    }

    /// Pushed back within `bounds`, then shrunk to them when still too big.
    pub fn fitted(self, bounds: PipFrame) -> Self {
        Self::from_spans(
            self.horizontal().fitted(bounds.horizontal()),
            self.vertical().fitted(bounds.vertical()),
        )
    }

    fn horizontal(self) -> Span {
        Span {
            start: self.x,
            length: self.width,
        }
    }

    fn vertical(self) -> Span {
        Span {
            start: self.y,
            length: self.height,
        }
    }

    fn from_spans(horizontal: Span, vertical: Span) -> Self {
        Self {
            x: horizontal.start,
            y: vertical.start,
            width: horizontal.length,
            height: vertical.length,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: PipFrame = PipFrame {
        x: 0.0,
        y: 40.0,
        width: 1200.0,
        height: 760.0,
    };

    fn frame(x: f32, y: f32, width: f32, height: f32) -> PipFrame {
        PipFrame {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_first_panel_sits_in_the_corner_of_the_terminals() {
        assert_eq!(
            PipFrame::in_corner_of(frame(280.0, 40.0, 600.0, 760.0)),
            frame(384.0, 464.0, 480.0, 320.0)
        );
    }

    #[test]
    fn a_panel_dragged_off_the_window_comes_back_in() {
        let dragged = frame(100.0, 100.0, 480.0, 320.0).moved(-300.0, 600.0);
        assert_eq!(dragged.fitted(WINDOW), frame(0.0, 480.0, 480.0, 320.0));
    }

    #[test]
    fn a_shrinking_window_shrinks_the_panel_once_it_cannot_push_it() {
        let window = frame(0.0, 40.0, 400.0, 300.0);
        assert_eq!(
            frame(100.0, 100.0, 480.0, 320.0).fitted(window),
            frame(0.0, 40.0, 400.0, 300.0)
        );
    }

    #[test]
    fn a_resize_keeps_the_least_size_its_opposite_edges_fixed() {
        let grip = Grip {
            horizontal: Some(Side::Start),
            vertical: Some(Side::End),
        };
        assert_eq!(
            frame(100.0, 100.0, 480.0, 320.0).resized(grip, (400.0, -300.0), WINDOW),
            frame(340.0, 100.0, 240.0, 160.0)
        );
    }

    #[test]
    fn a_resize_stops_at_the_window_edge() {
        let grip = Grip {
            horizontal: Some(Side::End),
            vertical: Some(Side::Start),
        };
        assert_eq!(
            frame(800.0, 100.0, 300.0, 320.0).resized(grip, (500.0, -500.0), WINDOW),
            frame(800.0, 40.0, 400.0, 380.0)
        );
    }
}
