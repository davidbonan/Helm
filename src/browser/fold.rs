//! The toolbar folding away while the page scrolls down (browser.md §4).

/// This close to the top, the toolbar stays full.
const TOP_ZONE: f64 = 16.0;
/// Downward travel that folds the toolbar.
const FOLD_TRAVEL: f64 = 8.0;
/// Upward travel that unfolds it: more than the toolbar's height change, so a
/// page resized at its bottom, its scroll clamped back, cannot unfold it.
const UNFOLD_TRAVEL: f64 = 30.0;

#[derive(Debug, Default)]
pub struct ToolbarFold {
    page_url: String,
    last_y: f64,
    /// Travel since the scroll last changed direction: positive downward.
    travel: f64,
    folded: bool,
}

impl ToolbarFold {
    /// Follows the page's scroll offset; whether the toolbar is folded now. A new
    /// page starts unfolded.
    pub fn follow(&mut self, page_url: &str, scroll_y: f64) -> bool {
        if page_url != self.page_url {
            *self = Self {
                page_url: page_url.to_owned(),
                last_y: scroll_y,
                ..Self::default()
            };
            return false;
        }
        let delta = scroll_y - self.last_y;
        self.last_y = scroll_y;
        if delta != 0.0 {
            let turned = (delta > 0.0) != (self.travel > 0.0);
            self.travel = if turned { delta } else { self.travel + delta };
        }
        if scroll_y < TOP_ZONE || self.travel <= -UNFOLD_TRAVEL {
            self.folded = false;
        } else if self.travel >= FOLD_TRAVEL {
            self.folded = true;
        }
        self.folded
    }

    /// Unfolds the toolbar until the page scrolls down again.
    pub fn unfold(&mut self) {
        self.folded = false;
        self.travel = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "https://docs.rs/egui";

    fn scrolled(steps: &[f64]) -> ToolbarFold {
        let mut fold = ToolbarFold::default();
        for &y in steps {
            fold.follow(PAGE, y);
        }
        fold
    }

    #[test]
    fn scrolling_down_folds_the_toolbar() {
        assert!(scrolled(&[0.0, 40.0, 80.0]).follow(PAGE, 120.0));
    }

    #[test]
    fn near_the_top_the_toolbar_stays_full() {
        assert!(!scrolled(&[0.0, 4.0, 10.0]).follow(PAGE, 14.0));
    }

    #[test]
    fn a_short_climb_keeps_it_folded_a_long_one_unfolds_it() {
        let mut fold = scrolled(&[0.0, 200.0, 400.0]);
        assert!(
            fold.follow(PAGE, 386.0),
            "a clamp of the bar's height change"
        );
        assert!(!fold.follow(PAGE, 360.0));
    }

    #[test]
    fn a_new_page_starts_unfolded() {
        let mut fold = scrolled(&[0.0, 200.0, 400.0]);
        assert!(!fold.follow("https://docs.rs/eframe", 400.0));
    }

    #[test]
    fn unfold_holds_until_the_page_scrolls_down_again() {
        let mut fold = scrolled(&[0.0, 200.0, 400.0]);
        fold.unfold();
        assert!(!fold.follow(PAGE, 400.0));
        assert!(fold.follow(PAGE, 420.0));
    }
}
