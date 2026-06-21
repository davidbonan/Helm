pub const MIN_COLS: u16 = 8;
pub const MIN_LINES: u16 = 3;

const RESIZE_STEP: f32 = 0.05;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PaneId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orient {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Node {
    Leaf(PaneId),
    Split {
        orient: Orient,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Layout {
    root: Node,
    focus: PaneId,
    next_id: u32,
}

impl Layout {
    pub fn new() -> Self {
        Self {
            root: Node::Leaf(PaneId(0)),
            focus: PaneId(0),
            next_id: 1,
        }
    }

    pub fn focus(&self) -> PaneId {
        self.focus
    }

    pub fn set_focus(&mut self, id: PaneId) {
        if contains(&self.root, id) {
            self.focus = id;
        }
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        collect_ids(&self.root, &mut out);
        out
    }

    pub fn split(&mut self, orient: Orient) -> PaneId {
        let new_id = PaneId(self.next_id);
        self.next_id += 1;
        let focus = self.focus;
        replace_leaf(&mut self.root, focus, |leaf| Node::Split {
            orient,
            ratio: 0.5,
            first: Box::new(leaf),
            second: Box::new(Node::Leaf(new_id)),
        });
        self.focus = new_id;
        new_id
    }

    pub fn close(&mut self) {
        let focus = self.focus;
        if let Some(focus) = remove_leaf(&mut self.root, focus) {
            self.focus = focus;
        } else if matches!(&self.root, Node::Leaf(id) if *id == focus) {
            let fresh = PaneId(self.next_id);
            self.next_id += 1;
            self.root = Node::Leaf(fresh);
            self.focus = fresh;
        }
    }

    pub fn resize(&mut self, dir: Dir, area: Rect, cell_w: f32, cell_h: f32) {
        self.resize_pane_by(self.focus, dir, RESIZE_STEP, area, cell_w, cell_h);
    }

    pub fn resize_pane_by(
        &mut self,
        pane: PaneId,
        dir: Dir,
        fraction: f32,
        area: Rect,
        cell_w: f32,
        cell_h: f32,
    ) {
        let step = signed_step(dir, fraction);
        let orient = match dir {
            Dir::Left | Dir::Right => Orient::Vertical,
            Dir::Up | Dir::Down => Orient::Horizontal,
        };
        adjust_split(&mut self.root, pane, orient, step, area, cell_w, cell_h);
    }

    /// Adjusts the exact split sitting between `first` and `second` (the leaves
    /// pinning its two sides) by `delta` of its local extent. Unlike
    /// [`resize_pane_by`], which walks to the nearest same-orientation split,
    /// this targets the one split the dragged seam belongs to.
    pub fn resize_split(
        &mut self,
        first: PaneId,
        second: PaneId,
        delta: f32,
        area: Rect,
        cell_w: f32,
        cell_h: f32,
    ) {
        adjust_exact_split(&mut self.root, first, second, delta, area, cell_w, cell_h);
    }

    pub fn focus_neighbor(&mut self, dir: Dir, area: Rect) {
        let rects = self.rects(area);
        let Some(current) = rects.iter().find(|(id, _)| *id == self.focus) else {
            return;
        };
        if let Some(target) = nearest_neighbor(current.1, dir, &rects, self.focus) {
            self.focus = target;
        }
    }

    pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        layout_rects(&self.root, area, &mut out);
        out
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_ids(node: &Node, out: &mut Vec<PaneId>) {
    match node {
        Node::Leaf(id) => out.push(*id),
        Node::Split { first, second, .. } => {
            collect_ids(first, out);
            collect_ids(second, out);
        }
    }
}

pub(crate) fn first_leaf(node: &Node) -> PaneId {
    match node {
        Node::Leaf(id) => *id,
        Node::Split { first, .. } => first_leaf(first),
    }
}

fn replace_leaf(node: &mut Node, target: PaneId, build: impl FnOnce(Node) -> Node) {
    match node {
        Node::Leaf(id) if *id == target => {
            let leaf = std::mem::replace(node, Node::Leaf(target));
            *node = build(leaf);
        }
        Node::Leaf(_) => {}
        Node::Split { first, second, .. } => {
            if contains(first, target) {
                replace_leaf(first, target, build);
            } else {
                replace_leaf(second, target, build);
            }
        }
    }
}

fn remove_leaf(node: &mut Node, target: PaneId) -> Option<PaneId> {
    let Node::Split { first, second, .. } = node else {
        return None;
    };
    if matches!(first.as_ref(), Node::Leaf(id) if *id == target) {
        *node = (**second).clone();
        return Some(first_leaf(node));
    }
    if matches!(second.as_ref(), Node::Leaf(id) if *id == target) {
        *node = (**first).clone();
        return Some(first_leaf(node));
    }
    if contains(first, target) {
        remove_leaf(first, target)
    } else {
        remove_leaf(second, target)
    }
}

fn contains(node: &Node, target: PaneId) -> bool {
    match node {
        Node::Leaf(id) => *id == target,
        Node::Split { first, second, .. } => contains(first, target) || contains(second, target),
    }
}

fn signed_step(dir: Dir, fraction: f32) -> f32 {
    match dir {
        Dir::Right | Dir::Down => fraction,
        Dir::Left | Dir::Up => -fraction,
    }
}

#[allow(clippy::too_many_arguments)]
fn adjust_split(
    node: &mut Node,
    target: PaneId,
    orient: Orient,
    step: f32,
    area: Rect,
    cell_w: f32,
    cell_h: f32,
) -> bool {
    let Node::Split {
        orient: node_orient,
        ratio,
        first,
        second,
    } = node
    else {
        return false;
    };
    let (first_rect, second_rect) = split_rects(*node_orient, *ratio, area);
    let node_orient = *node_orient;
    let handled = if contains(first, target) {
        adjust_split(first, target, orient, step, first_rect, cell_w, cell_h)
    } else if contains(second, target) {
        adjust_split(second, target, orient, step, second_rect, cell_w, cell_h)
    } else {
        return false;
    };
    if handled {
        return true;
    }
    if node_orient == orient {
        *ratio = clamp_ratio(orient, *ratio + step, area, cell_w, cell_h);
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn adjust_exact_split(
    node: &mut Node,
    a: PaneId,
    b: PaneId,
    delta: f32,
    area: Rect,
    cell_w: f32,
    cell_h: f32,
) -> bool {
    let Node::Split {
        orient,
        ratio,
        first,
        second,
    } = node
    else {
        return false;
    };
    let (first_rect, second_rect) = split_rects(*orient, *ratio, area);
    if first_leaf(first) == a && first_leaf(second) == b {
        *ratio = clamp_ratio(*orient, *ratio + delta, area, cell_w, cell_h);
        return true;
    }
    if contains(first, a) {
        adjust_exact_split(first, a, b, delta, first_rect, cell_w, cell_h)
    } else if contains(second, a) {
        adjust_exact_split(second, a, b, delta, second_rect, cell_w, cell_h)
    } else {
        false
    }
}

fn clamp_ratio(orient: Orient, ratio: f32, area: Rect, cell_w: f32, cell_h: f32) -> f32 {
    let (extent, cell, min_cells) = match orient {
        Orient::Vertical => (area.w, cell_w, MIN_COLS),
        Orient::Horizontal => (area.h, cell_h, MIN_LINES),
    };
    let min_fraction = (min_cells as f32 * cell) / extent;
    let lo = min_fraction;
    let hi = 1.0 - min_fraction;
    if lo >= hi {
        return ratio;
    }
    ratio.clamp(lo, hi)
}

fn layout_rects(node: &Node, area: Rect, out: &mut Vec<(PaneId, Rect)>) {
    match node {
        Node::Leaf(id) => out.push((*id, area)),
        Node::Split {
            orient,
            ratio,
            first,
            second,
        } => {
            let (a, b) = split_rects(*orient, *ratio, area);
            layout_rects(first, a, out);
            layout_rects(second, b, out);
        }
    }
}

pub(crate) fn split_rects(orient: Orient, ratio: f32, area: Rect) -> (Rect, Rect) {
    match orient {
        Orient::Vertical => {
            let first_w = area.w * ratio;
            (
                Rect {
                    x: area.x,
                    y: area.y,
                    w: first_w,
                    h: area.h,
                },
                Rect {
                    x: area.x + first_w,
                    y: area.y,
                    w: area.w - first_w,
                    h: area.h,
                },
            )
        }
        Orient::Horizontal => {
            let first_h = area.h * ratio;
            (
                Rect {
                    x: area.x,
                    y: area.y,
                    w: area.w,
                    h: first_h,
                },
                Rect {
                    x: area.x,
                    y: area.y + first_h,
                    w: area.w,
                    h: area.h - first_h,
                },
            )
        }
    }
}

fn nearest_neighbor(
    from: Rect,
    dir: Dir,
    rects: &[(PaneId, Rect)],
    current: PaneId,
) -> Option<PaneId> {
    let from_cx = from.x + from.w / 2.0;
    let from_cy = from.y + from.h / 2.0;
    let mut best: Option<(f32, PaneId)> = None;
    for (id, rect) in rects {
        if *id == current {
            continue;
        }
        let cx = rect.x + rect.w / 2.0;
        let cy = rect.y + rect.h / 2.0;
        let in_dir = match dir {
            Dir::Left => cx < from_cx,
            Dir::Right => cx > from_cx,
            Dir::Up => cy < from_cy,
            Dir::Down => cy > from_cy,
        };
        if !in_dir {
            continue;
        }
        let dist = (cx - from_cx).powi(2) + (cy - from_cy).powi(2);
        if best.map(|(d, _)| dist < d).unwrap_or(true) {
            best = Some((dist, *id));
        }
    }
    best.map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 800.0,
        h: 600.0,
    };
    const CELL_W: f32 = 8.0;
    const CELL_H: f32 = 16.0;

    #[test]
    fn split_creates_two_leaves_and_focuses_new() {
        let mut layout = Layout::new();
        let original = layout.focus();
        let new = layout.split(Orient::Vertical);

        let ids = layout.pane_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&original));
        assert!(ids.contains(&new));
        assert_ne!(original, new);
        assert_eq!(layout.focus(), new);
        assert!(
            matches!(layout.root(), Node::Split { ratio, .. } if (*ratio - 0.5).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn split_orientation_places_new_pane_per_spec() {
        let mut layout = Layout::new();
        layout.split(Orient::Vertical);
        let rects = layout.rects(AREA);
        let new = layout.focus();
        let new_rect = rects.iter().find(|(id, _)| *id == new).unwrap().1;
        assert!(
            new_rect.x > 0.0,
            "vertical split puts the new pane to the right"
        );

        let mut layout = Layout::new();
        layout.split(Orient::Horizontal);
        let rects = layout.rects(AREA);
        let new = layout.focus();
        let new_rect = rects.iter().find(|(id, _)| *id == new).unwrap().1;
        assert!(
            new_rect.y > 0.0,
            "horizontal split puts the new pane at the bottom"
        );
    }

    #[test]
    fn close_reabsorbs_sibling_into_parent() {
        let mut layout = Layout::new();
        let original = layout.focus();
        let new = layout.split(Orient::Vertical);
        assert_eq!(layout.pane_ids().len(), 2);

        layout.close();

        let ids = layout.pane_ids();
        assert_eq!(ids, vec![original]);
        assert_eq!(layout.focus(), original);
        assert!(matches!(layout.root(), Node::Leaf(id) if *id == original));
        assert!(!ids.contains(&new));
    }

    #[test]
    fn close_on_single_leaf_replaces_it_with_a_fresh_pane() {
        let mut layout = Layout::new();
        let only = layout.focus();

        layout.close();

        let ids = layout.pane_ids();
        assert_eq!(ids.len(), 1, "workspace keeps exactly one pane");
        let fresh = ids[0];
        assert_ne!(fresh, only, "the last leaf is replaced by a fresh id");
        assert_eq!(layout.focus(), fresh);
        assert!(matches!(layout.root(), Node::Leaf(id) if *id == fresh));
    }

    #[test]
    fn close_down_to_last_leaf_yields_a_fresh_pane() {
        let mut layout = Layout::new();
        let original = layout.focus();
        layout.split(Orient::Vertical);

        layout.close();
        assert_eq!(layout.pane_ids(), vec![original], "sibling is reabsorbed");

        layout.close();
        let ids = layout.pane_ids();
        assert_eq!(ids.len(), 1, "never an empty workspace");
        let fresh = ids[0];
        assert_ne!(fresh, original, "closing the last leaf opens a fresh one");
        assert_eq!(layout.focus(), fresh);
    }

    #[test]
    fn resize_shifts_ratio_by_five_percent() {
        let mut layout = Layout::new();
        layout.split(Orient::Vertical);
        layout.resize(Dir::Left, AREA, CELL_W, CELL_H);
        let Node::Split { ratio, .. } = layout.root() else {
            panic!("expected a split");
        };
        assert!((*ratio - 0.45).abs() < 1e-5);
    }

    #[test]
    fn set_focus_only_accepts_existing_pane() {
        let mut layout = Layout::new();
        let original = layout.focus();
        let new = layout.split(Orient::Vertical);

        layout.set_focus(original);
        assert_eq!(layout.focus(), original);

        layout.set_focus(PaneId(999));
        assert_eq!(layout.focus(), original, "unknown id is a no-op");

        layout.set_focus(new);
        assert_eq!(layout.focus(), new);
    }

    #[test]
    fn resize_pane_by_shifts_targeted_split_proportionally() {
        let mut layout = Layout::new();
        let left = layout.focus();
        layout.split(Orient::Vertical);

        layout.resize_pane_by(left, Dir::Right, 0.1, AREA, CELL_W, CELL_H);
        let Node::Split { ratio, .. } = layout.root() else {
            panic!("expected a split");
        };
        assert!((*ratio - 0.6).abs() < 1e-5, "ratio={ratio}");
    }

    #[test]
    fn resize_adjusts_the_nearest_split_not_an_outer_one_of_same_orientation() {
        let mut layout = Layout::new();
        layout.split(Orient::Vertical);
        let inner = layout.split(Orient::Vertical);

        layout.resize_pane_by(inner, Dir::Left, 0.1, AREA, CELL_W, CELL_H);

        let Node::Split {
            ratio: root_ratio,
            second,
            ..
        } = layout.root()
        else {
            panic!("expected a split at the root");
        };
        assert!(
            (*root_ratio - 0.5).abs() < 1e-5,
            "outer boundary stays put, root_ratio={root_ratio}"
        );
        let Node::Split {
            ratio: inner_ratio, ..
        } = second.as_ref()
        else {
            panic!("expected a nested split");
        };
        assert!(
            (*inner_ratio - 0.4).abs() < 1e-5,
            "nearest boundary moves, inner_ratio={inner_ratio}"
        );
    }

    // ((A | C) | B): a left-nested vertical layout. Its two vertical seams share
    // an orientation, so the keyboard "nearest split" rule would conflate them —
    // a dragged seam must instead move exactly the split it borders.
    fn left_nested_vertical() -> (Layout, PaneId, PaneId, PaneId) {
        let mut layout = Layout::new();
        let a = layout.focus();
        let b = layout.split(Orient::Vertical);
        layout.set_focus(a);
        let c = layout.split(Orient::Vertical);
        (layout, a, b, c)
    }

    fn nested_ratios(layout: &Layout) -> (f32, f32) {
        let Node::Split {
            ratio: root, first, ..
        } = layout.root()
        else {
            panic!("expected a split at the root");
        };
        let Node::Split { ratio: inner, .. } = first.as_ref() else {
            panic!("expected a nested split in the first child");
        };
        (*root, *inner)
    }

    #[test]
    fn resize_split_moves_the_root_seam_not_the_inner_one() {
        let (mut layout, a, b, _c) = left_nested_vertical();
        layout.resize_split(a, b, 0.1, AREA, CELL_W, CELL_H);

        let (root, inner) = nested_ratios(&layout);
        assert!((root - 0.6).abs() < 1e-5, "root seam moves, root={root}");
        assert!(
            (inner - 0.5).abs() < 1e-5,
            "inner seam stays, inner={inner}"
        );
    }

    #[test]
    fn resize_split_moves_the_inner_seam_not_the_root_one() {
        let (mut layout, a, _b, c) = left_nested_vertical();
        layout.resize_split(a, c, 0.1, AREA, CELL_W, CELL_H);

        let (root, inner) = nested_ratios(&layout);
        assert!((root - 0.5).abs() < 1e-5, "root seam stays, root={root}");
        assert!(
            (inner - 0.6).abs() < 1e-5,
            "inner seam moves, inner={inner}"
        );
    }

    #[test]
    fn resize_split_is_bounded_by_minimum_cells() {
        let mut layout = Layout::new();
        let left = layout.focus();
        let right = layout.split(Orient::Vertical);

        layout.resize_split(left, right, -1.0, AREA, CELL_W, CELL_H);
        let Node::Split { ratio, .. } = layout.root() else {
            panic!("expected a split");
        };
        assert!(*ratio * AREA.w >= MIN_COLS as f32 * CELL_W - 1e-3);
    }

    #[test]
    fn resize_pane_by_is_bounded_by_minimum_cells() {
        let mut layout = Layout::new();
        let left = layout.focus();
        layout.split(Orient::Vertical);

        layout.resize_pane_by(left, Dir::Left, 1.0, AREA, CELL_W, CELL_H);
        let Node::Split { ratio, .. } = layout.root() else {
            panic!("expected a split");
        };
        assert!(*ratio * AREA.w >= MIN_COLS as f32 * CELL_W - 1e-3);
    }

    #[test]
    fn resize_is_bounded_by_minimum_cells() {
        let mut layout = Layout::new();
        layout.split(Orient::Vertical);
        for _ in 0..50 {
            layout.resize(Dir::Left, AREA, CELL_W, CELL_H);
        }
        let Node::Split { ratio, .. } = layout.root() else {
            panic!("expected a split");
        };
        let min_first_w = *ratio * AREA.w;
        let min_second_w = (1.0 - *ratio) * AREA.w;
        assert!(min_first_w >= MIN_COLS as f32 * CELL_W - 1e-3);
        assert!(min_second_w >= MIN_COLS as f32 * CELL_W - 1e-3);
    }

    #[test]
    fn focus_neighbor_picks_geometric_pane() {
        let mut layout = Layout::new();
        let left = layout.focus();
        let right = layout.split(Orient::Vertical);
        assert_eq!(layout.focus(), right);

        layout.focus_neighbor(Dir::Left, AREA);
        assert_eq!(layout.focus(), left);

        layout.focus_neighbor(Dir::Right, AREA);
        assert_eq!(layout.focus(), right);
    }

    #[test]
    fn focus_neighbor_crosses_split_boundaries_geometrically() {
        let mut layout = Layout::new();
        let top_left = layout.focus();
        let top_right = layout.split(Orient::Vertical);
        let bottom_right = layout.split(Orient::Horizontal);
        assert_eq!(layout.focus(), bottom_right);

        layout.focus_neighbor(Dir::Up, AREA);
        assert_eq!(layout.focus(), top_right);

        layout.focus_neighbor(Dir::Left, AREA);
        assert_eq!(layout.focus(), top_left);
    }

    #[test]
    fn focus_neighbor_without_pane_in_direction_keeps_focus() {
        let mut layout = Layout::new();
        let left = layout.focus();
        layout.split(Orient::Vertical);
        layout.focus_neighbor(Dir::Left, AREA);
        assert_eq!(layout.focus(), left);
        layout.focus_neighbor(Dir::Up, AREA);
        assert_eq!(layout.focus(), left);
    }
}
