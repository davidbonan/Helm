//! What the Agents dashboard's **Terminals** view shows (specs/agents.md §5): the
//! agents picked from the header strip and the geometry of their mirrored panes.
//!
//! The tree is the terminal's own [`Layout`], so the wall inherits the workspace
//! splits' behaviour wholesale — seam resize, drag-and-drop reorg, geometric focus
//! navigation. A leaf is a **slot**: a wall-local [`PaneId`] mapped to the agent key
//! it mirrors, so relayouting the wall never touches the panes it borrows.
//!
//! Session state: the composition is deliberately not persisted — an agent key only
//! means something while that pane runs.

use crate::terminal::layout::{Layout, Orient, PaneId, Rect};

/// Terminals watchable at once. Past it the header's remaining chips read disabled:
/// a fifth pane would leave none of them big enough to follow.
pub const MAX_SHOWN: usize = 4;

/// The Terminals view's composition, generic over the key identifying an agent (the
/// app uses its `(repo, tab, pane)` triple).
pub struct AgentWall<K> {
    /// `None` while nothing is shown — [`Layout`] never holds zero leaves (it mints
    /// a fresh one instead), which would leave a slot mirroring no agent.
    layout: Option<Layout>,
    /// Slot → the agent it mirrors, in the order the slots were opened.
    slots: Vec<(PaneId, K)>,
}

impl<K> Default for AgentWall<K> {
    fn default() -> Self {
        Self {
            layout: None,
            slots: Vec::new(),
        }
    }
}

impl<K: Clone + PartialEq> AgentWall<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Every slot is taken: no further agent can be shown until one is hidden.
    pub fn full(&self) -> bool {
        self.slots.len() >= MAX_SHOWN
    }

    /// The split tree to render, `None` while the wall is empty.
    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    /// The tree, mutable — the app applies a seam drag / a pane drop straight to it
    /// (`resize_split`, `move_pane`, `swap_panes`), which only moves slots around.
    pub fn layout_mut(&mut self) -> Option<&mut Layout> {
        self.layout.as_mut()
    }

    pub fn slots(&self) -> &[(PaneId, K)] {
        &self.slots
    }

    pub fn key_of(&self, slot: PaneId) -> Option<&K> {
        self.slots
            .iter()
            .find(|(id, _)| *id == slot)
            .map(|(_, key)| key)
    }

    pub fn slot_of(&self, key: &K) -> Option<PaneId> {
        self.slots.iter().find(|(_, k)| k == key).map(|(id, _)| *id)
    }

    pub fn shows(&self, key: &K) -> bool {
        self.slot_of(key).is_some()
    }

    /// The agent whose slot holds the tree's focus — the one the keyboard drives.
    pub fn focused(&self) -> Option<&K> {
        self.key_of(self.layout.as_ref()?.focus())
    }

    /// Moves the tree's focus onto `key`'s slot; no-op when it isn't shown (so the
    /// app can mirror its selection here every frame without checking first).
    pub fn set_focus(&mut self, key: &K) {
        if let (Some(slot), Some(layout)) = (self.slot_of(key), self.layout.as_mut()) {
            layout.set_focus(slot);
        }
    }

    /// Adds `key`'s terminal to the wall, splitting the **roomiest** pane across its
    /// **longer** axis: 1 fills the area, 2 sit side by side, the 3rd halves whichever
    /// of them has the most room, the 4th lands a 2×2 — and a wall the user has
    /// resized or rearranged keeps its shape. Returns the new slot, or `None` when
    /// already shown or the wall is full.
    pub fn show(&mut self, key: K, area: Rect) -> Option<PaneId> {
        if self.shows(&key) || self.full() {
            return None;
        }
        let slot = match self.layout.as_mut() {
            None => {
                let layout = Layout::new();
                let slot = layout.focus();
                self.layout = Some(layout);
                slot
            }
            Some(layout) => {
                let (target, orient) = roomiest(layout, area);
                layout.set_focus(target);
                layout.split(orient)
            }
        };
        self.slots.push((slot, key));
        Some(slot)
    }

    /// Removes `key`'s terminal; its sibling takes the freed room (the tree's own
    /// close rule) and inherits the focus.
    pub fn hide(&mut self, key: &K) {
        let Some(slot) = self.slot_of(key) else {
            return;
        };
        self.slots.retain(|(id, _)| *id != slot);
        if self.slots.is_empty() {
            self.layout = None;
            return;
        }
        if let Some(layout) = self.layout.as_mut() {
            layout.set_focus(slot);
            layout.close();
        }
    }

    /// Header chip click: shows the agent, or hides it when it is already on the wall.
    pub fn toggle(&mut self, key: K, area: Rect) {
        if self.shows(&key) {
            self.hide(&key);
        } else {
            self.show(key, area);
        }
    }

    /// Drops the slots whose agent stopped running (its pane closed, or the agent
    /// left the foreground): the wall only ever mirrors live panes.
    pub fn retain<F: Fn(&K) -> bool>(&mut self, live: F) {
        let gone: Vec<K> = self
            .slots
            .iter()
            .filter(|(_, key)| !live(key))
            .map(|(_, key)| key.clone())
            .collect();
        for key in &gone {
            self.hide(key);
        }
    }
}

/// The pane with the most room, and the orientation that halves it across its longer
/// axis (a wide pane splits into two columns, a tall one into two rows). Equal room
/// goes to the **last** in tree order, so a newcomer subdivides the youngest region:
/// it appears at the far end of the wall and the tiles already there keep their place.
fn roomiest(layout: &Layout, area: Rect) -> (PaneId, Orient) {
    let rects = layout.rects(area);
    let (id, rect) = rects
        .iter()
        .max_by(|(_, a), (_, b)| (a.w * a.h).total_cmp(&(b.w * b.h)))
        .copied()
        .unwrap_or((layout.focus(), area));
    let orient = if rect.w >= rect.h {
        Orient::Vertical
    } else {
        Orient::Horizontal
    };
    (id, orient)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1600.0,
        h: 900.0,
    };

    fn wall(count: usize) -> AgentWall<u32> {
        let mut wall = AgentWall::new();
        for key in 0..count as u32 {
            wall.show(key, AREA);
        }
        wall
    }

    /// Rects of the shown agents, keyed by their agent (not by their slot).
    fn tiles(wall: &AgentWall<u32>) -> Vec<(u32, Rect)> {
        let layout = wall.layout().expect("a wall with slots has a tree");
        let mut out: Vec<(u32, Rect)> = layout
            .rects(AREA)
            .into_iter()
            .map(|(slot, rect)| (*wall.key_of(slot).expect("slot maps to an agent"), rect))
            .collect();
        out.sort_by_key(|(key, _)| *key);
        out
    }

    #[test]
    fn one_agent_fills_the_area() {
        let wall = wall(1);
        assert_eq!(tiles(&wall), vec![(0, AREA)]);
    }

    #[test]
    fn two_agents_sit_side_by_side_in_a_wide_area() {
        let tiles = tiles(&wall(2));
        assert_eq!(tiles[0].1.w, 800.0);
        assert_eq!(tiles[0].1.h, 900.0);
        assert_eq!(tiles[1].1.x, 800.0);
        assert_eq!(tiles[1].1.w, 800.0);
    }

    #[test]
    fn two_agents_stack_in_a_tall_area() {
        let tall = Rect {
            x: 0.0,
            y: 0.0,
            w: 900.0,
            h: 1600.0,
        };
        let mut wall = AgentWall::new();
        wall.show(0_u32, tall);
        wall.show(1_u32, tall);
        let rects = wall.layout().unwrap().rects(tall);
        assert_eq!(rects[0].1.h, 800.0);
        assert_eq!(rects[1].1.y, 800.0);
    }

    #[test]
    fn the_third_agent_halves_the_roomiest_pane() {
        let tiles = tiles(&wall(3));
        // Both 800×900 halves are equally roomy, so the newcomer subdivides the
        // youngest one across its longer axis (height): the first agent keeps its
        // whole column, the second one stacks with the third.
        assert_eq!(
            tiles[0].1,
            (Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 900.0
            })
        );
        assert_eq!(
            tiles[1].1,
            (Rect {
                x: 800.0,
                y: 0.0,
                w: 800.0,
                h: 450.0
            })
        );
        assert_eq!(
            tiles[2].1,
            (Rect {
                x: 800.0,
                y: 450.0,
                w: 800.0,
                h: 450.0
            })
        );
    }

    #[test]
    fn the_fourth_agent_lands_a_two_by_two() {
        let tiles = tiles(&wall(4));
        let sizes: Vec<(f32, f32)> = tiles.iter().map(|(_, r)| (r.w, r.h)).collect();
        assert_eq!(sizes, vec![(800.0, 450.0); 4]);
        let corners: std::collections::HashSet<(u32, u32)> = tiles
            .iter()
            .map(|(_, r)| (r.x as u32, r.y as u32))
            .collect();
        assert_eq!(
            corners.len(),
            4,
            "each tile owns its own quadrant: {tiles:?}"
        );
    }

    #[test]
    fn a_fifth_agent_is_refused() {
        let mut wall = wall(MAX_SHOWN);
        assert!(wall.full());
        assert_eq!(wall.show(99, AREA), None);
        assert_eq!(wall.len(), MAX_SHOWN);
        assert!(!wall.shows(&99));
    }

    #[test]
    fn showing_the_same_agent_twice_is_a_no_op() {
        let mut wall = wall(2);
        assert_eq!(wall.show(1, AREA), None);
        assert_eq!(wall.len(), 2);
    }

    #[test]
    fn hiding_gives_the_room_to_the_sibling() {
        let mut wall = wall(2);
        wall.hide(&0);
        assert_eq!(tiles(&wall), vec![(1, AREA)]);
    }

    #[test]
    fn hiding_the_last_one_empties_the_wall() {
        let mut wall = wall(1);
        wall.hide(&0);
        assert!(wall.is_empty());
        assert!(wall.layout().is_none());
        // And the wall takes agents again afterwards.
        wall.show(7, AREA);
        assert_eq!(tiles(&wall), vec![(7, AREA)]);
    }

    #[test]
    fn a_freed_slot_is_available_again() {
        let mut wall = wall(MAX_SHOWN);
        wall.hide(&0);
        assert!(!wall.full());
        assert!(wall.show(99, AREA).is_some());
        assert_eq!(wall.len(), MAX_SHOWN);
        assert!(wall.shows(&99));
    }

    #[test]
    fn toggle_shows_then_hides() {
        let mut wall = AgentWall::new();
        wall.toggle(3_u32, AREA);
        assert!(wall.shows(&3));
        wall.toggle(3_u32, AREA);
        assert!(!wall.shows(&3));
    }

    #[test]
    fn a_new_slot_takes_the_focus_and_hiding_hands_it_to_the_sibling() {
        let mut wall = wall(2);
        assert_eq!(wall.focused(), Some(&1));
        wall.set_focus(&0);
        assert_eq!(wall.focused(), Some(&0));
        wall.hide(&0);
        assert_eq!(wall.focused(), Some(&1));
    }

    #[test]
    fn focusing_an_agent_that_is_not_shown_changes_nothing() {
        let mut wall = wall(2);
        wall.set_focus(&42);
        assert_eq!(wall.focused(), Some(&1));
    }

    #[test]
    fn an_agent_that_stopped_running_loses_its_slot() {
        let mut wall = wall(3);
        wall.retain(|key| *key != 1);
        assert!(!wall.shows(&1));
        assert_eq!(wall.len(), 2);
        assert_eq!(tiles(&wall).len(), 2);
        wall.retain(|_| false);
        assert!(wall.is_empty());
        assert!(wall.layout().is_none());
    }
}
