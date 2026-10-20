//! Drag-and-drop reordering of a tab row (terminal tabs, browser tabs): where a
//! dropped tab lands. Pure: the owners move their own tabs.

/// Tab `from` dropped before (or `after`) tab `anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabDrop {
    pub from: usize,
    pub anchor: usize,
    pub after: bool,
}

impl TabDrop {
    /// The gap (0..=len, before the tab leaves its place) the tab lands in, or
    /// `None` when out of bounds or dropped onto its own edge.
    pub fn slot(self, len: usize) -> Option<usize> {
        if self.from >= len || self.anchor >= len {
            return None;
        }
        let slot = if self.after {
            self.anchor + 1
        } else {
            self.anchor
        };
        (slot != self.from && slot != self.from + 1).then_some(slot)
    }

    /// Moves the dropped tab in `tabs`; returns where the tab at `followed` now
    /// sits, or `None` when nothing moved.
    pub fn apply<T>(self, tabs: &mut Vec<T>, followed: usize) -> Option<usize> {
        let slot = self.slot(tabs.len())?;
        let dest = if slot > self.from { slot - 1 } else { slot };
        let tab = tabs.remove(self.from);
        tabs.insert(dest, tab);
        Some(match followed {
            f if f == self.from => dest,
            f if self.from < f && f <= dest => f - 1,
            f if dest <= f && f < self.from => f + 1,
            f => f,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_dropped_further_shifts_the_ones_it_passes_back() {
        let mut tabs = vec!['a', 'b', 'c', 'd'];
        let drop = TabDrop {
            from: 0,
            anchor: 2,
            after: true,
        };
        assert_eq!(drop.apply(&mut tabs, 2), Some(1));
        assert_eq!(tabs, ['b', 'c', 'a', 'd']);
    }

    #[test]
    fn a_tab_dropped_earlier_shifts_the_ones_it_passes_forward() {
        let mut tabs = vec!['a', 'b', 'c', 'd'];
        let drop = TabDrop {
            from: 3,
            anchor: 1,
            after: false,
        };
        assert_eq!(
            drop.apply(&mut tabs, 3),
            Some(1),
            "the dropped tab is followed"
        );
        assert_eq!(tabs, ['a', 'd', 'b', 'c']);
    }

    #[test]
    fn a_tab_dropped_onto_its_own_edge_stays() {
        let mut tabs = vec!['a', 'b', 'c'];
        for after in [false, true] {
            let drop = TabDrop {
                from: 1,
                anchor: 1,
                after,
            };
            assert_eq!(drop.apply(&mut tabs, 0), None);
        }
        let next_left_edge = TabDrop {
            from: 1,
            anchor: 2,
            after: false,
        };
        assert_eq!(next_left_edge.slot(3), None);
        assert_eq!(tabs, ['a', 'b', 'c']);
    }
}
