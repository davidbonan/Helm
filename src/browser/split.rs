//! Two tabs side by side (browser.md §16): which, and how they share the width.

/// A tab of a worktree's browser, for as long as it lives.
pub type TabId = u64;

/// The narrowest a half gets.
pub const MIN_HALF: f32 = 320.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitView {
    pub left: TabId,
    pub right: TabId,
    /// The left half's share of the width the halves split.
    pub share: f32,
}

impl SplitView {
    /// `joining` on the right of `shown`, halves at first.
    pub fn new(shown: TabId, joining: TabId) -> Self {
        Self {
            left: shown,
            right: joining,
            share: 0.5,
        }
    }

    pub fn holds(&self, tab: TabId) -> bool {
        tab == self.left || tab == self.right
    }

    /// The other half's tab, and the side it shows on.
    pub fn partner_of(&self, tab: TabId) -> Option<(TabId, Side)> {
        match tab {
            _ if tab == self.left => Some((self.right, Side::Right)),
            _ if tab == self.right => Some((self.left, Side::Left)),
            _ => None,
        }
    }
}

/// The halves' widths in `room` (the width they split, the gap left out); `None`
/// where both cannot keep `MIN_HALF`.
pub fn widths(room: f32, share: f32) -> Option<(f32, f32)> {
    if room < 2.0 * MIN_HALF {
        return None;
    }
    let left = (room * share).clamp(MIN_HALF, room - MIN_HALF);
    Some((left, room - left))
}

/// The left half's share once the divider leaves it `left` wide, both halves
/// kept at `MIN_HALF` at least.
pub fn share_at(left: f32, room: f32) -> f32 {
    if room < 2.0 * MIN_HALF {
        return 0.5;
    }
    left.clamp(MIN_HALF, room - MIN_HALF) / room
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_share_the_width_but_never_shrink_under_their_minimum() {
        assert_eq!(widths(1000.0, 0.5), Some((500.0, 500.0)));
        assert_eq!(widths(1000.0, 0.1), Some((320.0, 680.0)));
        assert_eq!(widths(600.0, 0.5), None);
    }

    #[test]
    fn the_divider_stops_where_a_half_would_get_too_narrow() {
        let share = share_at(900.0, 1000.0);
        assert_eq!(widths(1000.0, share), Some((680.0, 320.0)));
    }

    #[test]
    fn each_half_s_partner_shows_on_the_other_side() {
        let split = SplitView::new(1, 2);
        assert_eq!(split.partner_of(1), Some((2, Side::Right)));
        assert_eq!(split.partner_of(2), Some((1, Side::Left)));
        assert_eq!(split.partner_of(3), None);
    }
}
