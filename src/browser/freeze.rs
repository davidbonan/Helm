//! The page frozen under what egui draws over it (browser.md §4): the native view
//! hides only once a snapshot of it can stand in its place.

use std::time::{Duration, Instant};

/// How long the view stays up waiting for its snapshot before hiding anyway.
const SNAPSHOT_WAIT: Duration = Duration::from_millis(200);

/// What egui draws over the page area this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cover {
    None,
    /// Something drawn over the page (suggestions, a modal, a popup, a drag): the
    /// page shows frozen beneath it.
    Overlay,
    /// No page to show: the start page, a background tab, or the pane not drawn.
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeStep {
    Show,
    /// Ask for a snapshot; the view stays up meanwhile.
    Snapshot,
    /// The snapshot is not in yet; the view stays up.
    Wait,
    /// Hide the view; `pictured`: the last snapshot still pictures the page, else
    /// it is dropped.
    Hide {
        pictured: bool,
    },
    /// Ask for a last snapshot and hide the view at once: the page goes off screen.
    Leave,
}

#[derive(Debug, Default)]
pub struct PageFreeze {
    hidden: bool,
    asked_at: Option<Instant>,
}

impl PageFreeze {
    /// What the view does this frame; `snapshot_in`: a snapshot arrived.
    pub fn step(&mut self, cover: Cover, snapshot_in: bool, now: Instant) -> FreezeStep {
        match cover {
            Cover::None => {
                *self = Self::default();
                FreezeStep::Show
            }
            Cover::Away => {
                self.asked_at = None;
                if std::mem::replace(&mut self.hidden, true) {
                    FreezeStep::Hide { pictured: true }
                } else {
                    FreezeStep::Leave
                }
            }
            Cover::Overlay => self.overlay(snapshot_in, now),
        }
    }

    fn overlay(&mut self, snapshot_in: bool, now: Instant) -> FreezeStep {
        if self.hidden {
            return FreezeStep::Hide { pictured: true };
        }
        let Some(asked_at) = self.asked_at else {
            self.asked_at = Some(now);
            return FreezeStep::Snapshot;
        };
        if snapshot_in {
            self.hidden = true;
            return FreezeStep::Hide { pictured: true };
        }
        if now.duration_since(asked_at) >= SNAPSHOT_WAIT {
            self.hidden = true;
            return FreezeStep::Hide { pictured: false };
        }
        FreezeStep::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn an_overlay_keeps_the_view_up_until_its_snapshot_is_in() {
        let mut freeze = PageFreeze::default();
        let start = Instant::now();
        let steps = [
            freeze.step(Cover::Overlay, false, start),
            freeze.step(Cover::Overlay, false, start + FRAME),
            freeze.step(Cover::Overlay, true, start + 2 * FRAME),
            freeze.step(Cover::Overlay, false, start + 3 * FRAME),
        ];
        let hidden = FreezeStep::Hide { pictured: true };
        assert_eq!(
            steps,
            [FreezeStep::Snapshot, FreezeStep::Wait, hidden, hidden]
        );
    }

    #[test]
    fn a_snapshot_that_never_comes_hides_the_view_bare() {
        let mut freeze = PageFreeze::default();
        let start = Instant::now();
        freeze.step(Cover::Overlay, false, start);
        assert_eq!(
            freeze.step(Cover::Overlay, false, start + SNAPSHOT_WAIT),
            FreezeStep::Hide { pictured: false }
        );
    }

    #[test]
    fn a_snapshot_arriving_before_any_overlay_is_not_taken_as_fresh() {
        let mut freeze = PageFreeze::default();
        let start = Instant::now();
        freeze.step(Cover::Overlay, false, start);
        freeze.step(Cover::None, false, start + FRAME);
        assert_eq!(
            freeze.step(Cover::Overlay, true, start + 2 * FRAME),
            FreezeStep::Snapshot
        );
    }

    #[test]
    fn going_off_screen_hides_the_view_at_once_keeping_a_last_snapshot() {
        let mut freeze = PageFreeze::default();
        let start = Instant::now();
        let steps = [
            freeze.step(Cover::Away, false, start),
            freeze.step(Cover::Away, true, start + FRAME),
        ];
        assert_eq!(
            steps,
            [FreezeStep::Leave, FreezeStep::Hide { pictured: true }]
        );
    }
}
