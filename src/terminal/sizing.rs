//! Who sizes a pane's PTY (specs/remote.md §7): a PTY has a single size, so the
//! Mac's widget and a phone driving the agent take turns — the latest to act wins.

use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;

use crate::terminal::activity::{now_ms, PaneActivity};
use crate::terminal::emu::{resize_term, SharedTerm};
use crate::terminal::pty::PtyResizer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub rows: u16,
    pub cols: u16,
}

impl GridSize {
    /// A phone's screen, bounded: below the floor an agent's TUI is unusable, above
    /// the ceiling the grid would only cost memory.
    pub fn phone(self) -> Self {
        Self {
            rows: self.rows.clamp(8, 300),
            cols: self.cols.clamp(20, 500),
        }
    }
}

/// The turn-taking itself; every change returns the size the PTY must take, if any.
#[derive(Debug)]
pub struct SizeOwnership {
    desktop: GridSize,
    phone: Option<GridSize>,
    applied: GridSize,
}

impl SizeOwnership {
    pub fn new(size: GridSize) -> Self {
        Self {
            desktop: size,
            phone: None,
            applied: size,
        }
    }

    pub fn size(&self) -> GridSize {
        self.applied
    }

    pub fn is_sized_by_phone(&self) -> bool {
        self.phone.is_some()
    }

    /// The Mac widget's measured size: applied only while the Mac holds the turn.
    pub fn fit_desktop(&mut self, size: GridSize) -> Option<GridSize> {
        self.desktop = size;
        self.settle()
    }

    pub fn claim_desktop(&mut self) -> Option<GridSize> {
        self.phone = None;
        self.settle()
    }

    pub fn claim_phone(&mut self, size: GridSize) -> Option<GridSize> {
        self.phone = Some(size.phone());
        self.settle()
    }

    pub fn release_phone(&mut self) -> Option<GridSize> {
        self.phone = None;
        self.settle()
    }

    fn settle(&mut self) -> Option<GridSize> {
        let target = self.phone.unwrap_or(self.desktop);
        (target != self.applied).then(|| {
            self.applied = target;
            target
        })
    }
}

/// A pane's size for every thread that may change it: the UI's widget, and the
/// phone server, which must work while helm draws no frame (specs/remote.md §4).
pub struct PaneSizing {
    ownership: Mutex<SizeOwnership>,
    resizer: PtyResizer,
    term: SharedTerm,
    activity: Arc<PaneActivity>,
}

impl PaneSizing {
    pub fn new(
        size: GridSize,
        resizer: PtyResizer,
        term: SharedTerm,
        activity: Arc<PaneActivity>,
    ) -> Self {
        Self {
            ownership: Mutex::new(SizeOwnership::new(size)),
            resizer,
            term,
            activity,
        }
    }

    pub fn size(&self) -> GridSize {
        self.ownership().size()
    }

    pub fn is_sized_by_phone(&self) -> bool {
        self.ownership().is_sized_by_phone()
    }

    pub fn fit_desktop(&self, size: GridSize) -> Result<()> {
        self.settle(|ownership| ownership.fit_desktop(size))
    }

    pub fn claim_desktop(&self) -> Result<()> {
        self.settle(SizeOwnership::claim_desktop)
    }

    pub fn claim_phone(&self, size: GridSize) -> Result<()> {
        self.settle(|ownership| ownership.claim_phone(size))
    }

    pub fn release_phone(&self) -> Result<()> {
        self.settle(SizeOwnership::release_phone)
    }

    /// Held across the resize, so two threads' turns land in the order they took them.
    fn settle(&self, change: impl FnOnce(&mut SizeOwnership) -> Option<GridSize>) -> Result<()> {
        let mut ownership = self.ownership();
        let Some(size) = change(&mut ownership) else {
            return Ok(());
        };
        // Stamp before the PTY resize so the window is open when the program's
        // SIGWINCH repaint lands: that burst is helm's doing, not agent work,
        // and must not re-arm the activity badge (specs/agents.md).
        self.activity.stamp_resize(now_ms());
        resize_term(&mut self.term.lock(), size.rows, size.cols);
        self.resizer.resize(size)
    }

    fn ownership(&self) -> MutexGuard<'_, SizeOwnership> {
        self.ownership
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: GridSize = GridSize {
        rows: 50,
        cols: 200,
    };
    const PHONE: GridSize = GridSize { rows: 40, cols: 52 };

    fn phone_driving() -> SizeOwnership {
        let mut ownership = SizeOwnership::new(DESKTOP);
        ownership.claim_phone(PHONE);
        ownership
    }

    #[test]
    fn a_phone_claim_resizes_to_the_phone() {
        let mut ownership = SizeOwnership::new(DESKTOP);

        assert_eq!(ownership.claim_phone(PHONE), Some(PHONE));
    }

    #[test]
    fn the_mac_widget_does_not_undo_a_phone_claim() {
        let mut ownership = phone_driving();

        let widened = GridSize {
            rows: 60,
            cols: 240,
        };
        assert_eq!(ownership.fit_desktop(widened), None);
        assert_eq!(ownership.size(), PHONE);
    }

    #[test]
    fn acting_on_the_mac_takes_back_its_latest_size() {
        let mut ownership = phone_driving();
        let widened = GridSize {
            rows: 60,
            cols: 240,
        };
        ownership.fit_desktop(widened);

        assert_eq!(ownership.claim_desktop(), Some(widened));
    }

    #[test]
    fn a_phone_leaving_gives_the_size_back_to_the_mac() {
        let mut ownership = phone_driving();

        assert_eq!(ownership.release_phone(), Some(DESKTOP));
    }

    #[test]
    fn a_phone_cannot_shrink_the_grid_below_a_usable_screen() {
        let mut ownership = SizeOwnership::new(DESKTOP);

        let applied = ownership.claim_phone(GridSize { rows: 1, cols: 1 });

        assert_eq!(applied, Some(GridSize { rows: 8, cols: 20 }));
    }
}
