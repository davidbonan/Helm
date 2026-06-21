use super::{Edge, GraphCommit, GraphRow};

/// Assign a lane (column) to each commit and the edges descending to the next
/// row. Pure function over the display-ordered list of `(oid, parents)` so it
/// is testable on synthetic topologies without a real repository.
///
/// Commits are expected in display order (children before parents, as a topo
/// revwalk yields). A lane holds the oid the renderer expects to reach next on
/// that column; when a commit is processed its lane is handed to its first
/// parent — even if another lane already awaits the same parent: lines
/// converge only at the parent's node, never mid-air.
/// Extra parents (merges) are the exception: they join the lane already
/// awaiting them, or claim a free one.
pub fn assign_lanes(commits: &[(git2::Oid, Vec<git2::Oid>)]) -> Vec<GraphRow> {
    assign_lanes_with_wip(commits, None).1
}

/// Variant with a **virtual WIP node** (dirty tree, M10-7): `wip_parent` (the
/// HEAD commit) ⇒ a head row claims the **dedicated lane 0** for the WIP → HEAD
/// link — other branches shift over, the link is never covered by a solid line.
/// Its edges are `dashed` while the lane stays exclusive to it; if a merge link
/// (2nd+ parent = HEAD) joins it, the lane then carries a real branch and
/// becomes solid down to the HEAD node. Returns `(WIP row, commit rows)`.
pub fn assign_lanes_with_wip(
    commits: &[(git2::Oid, Vec<git2::Oid>)],
    wip_parent: Option<git2::Oid>,
) -> (Option<GraphRow>, Vec<GraphRow>) {
    let mut lanes: Vec<Option<git2::Oid>> = Vec::new();
    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());

    // Lane still exclusive to the WIP → HEAD link (its pass-throughs are dashed).
    let mut wip_lane: Option<usize> = None;
    let wip_row = wip_parent.map(|parent| {
        lanes.push(Some(parent));
        wip_lane = Some(0);
        GraphRow {
            lane: 0,
            edges: vec![Edge {
                from_lane: 0,
                to_lane: 0,
                dashed: true,
                merge: false,
            }],
        }
    });

    for (oid, parents) in commits {
        // Every lane awaiting this commit converges into its node: the node
        // takes the leftmost, and the previous row's edges into the other
        // awaiting lanes are redirected into it.
        let waiting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| (slot.as_ref() == Some(oid)).then_some(index))
            .collect();
        let lane = match waiting.first() {
            Some(&first) => first,
            None => {
                let free = free_lane(&mut lanes);
                lanes[free] = Some(*oid);
                free
            }
        };
        for &extra in waiting.iter().skip(1) {
            lanes[extra] = None;
            if let Some(prev) = rows.last_mut() {
                for edge in prev.edges.iter_mut().filter(|e| e.to_lane == extra) {
                    edge.to_lane = lane;
                }
            }
        }
        // The WIP link ends at the HEAD node; only a merge link (2nd+ parent =
        // HEAD) can still join its lane and take its exclusivity away.
        if Some(*oid) == wip_parent || parents.iter().skip(1).any(|p| Some(*p) == wip_parent) {
            wip_lane = None;
        }

        // Lanes other than this commit's pass straight through to the next row.
        let mut next: Vec<Option<git2::Oid>> = lanes.clone();
        next[lane] = None;
        let mut edges: Vec<Edge> = next
            .iter()
            .enumerate()
            .filter_map(|(l, slot)| {
                slot.map(|_| Edge {
                    from_lane: l,
                    to_lane: l,
                    dashed: Some(l) == wip_lane,
                    merge: false,
                })
            })
            .collect();

        // Route this commit's parents: the first parent keeps the freed lane —
        // even if another lane already awaits it, converging early would stack
        // unrelated tips on one column. Extra parents (merges) join the lane
        // already awaiting them, or open one.
        for (index, parent) in parents.iter().enumerate() {
            let to_lane = if index == 0 {
                next[lane] = Some(*parent);
                lane
            } else {
                match next.iter().position(|slot| slot.as_ref() == Some(parent)) {
                    Some(existing) => existing,
                    None => {
                        let free = free_lane(&mut next);
                        next[free] = Some(*parent);
                        free
                    }
                }
            };
            edges.push(Edge {
                from_lane: lane,
                to_lane,
                dashed: false,
                merge: index > 0,
            });
        }

        rows.push(GraphRow { lane, edges });
        compact(&mut next);
        lanes = next;
    }

    (wip_row, rows)
}

/// Lane-computation cache (M10-8): `assign_lanes` is pure but the renderer
/// requested it every frame. Memoizes on the topological identity (oid +
/// parents, in display order) **and** the WIP link's parent — an identically
/// reloaded graph does not recompute.
#[derive(Default)]
pub struct LaneCache {
    topo: Vec<(git2::Oid, Vec<git2::Oid>)>,
    wip_parent: Option<git2::Oid>,
    wip_row: Option<GraphRow>,
    rows: Vec<GraphRow>,
    computed: bool,
    computes: usize,
}

impl LaneCache {
    /// Graph lanes (`(virtual WIP row, commit rows)`), recomputed only if the
    /// topology or the WIP link's parent has changed.
    pub fn rows(
        &mut self,
        commits: &[GraphCommit],
        wip_parent: Option<git2::Oid>,
    ) -> (Option<&GraphRow>, &[GraphRow]) {
        let unchanged = self.computed
            && self.wip_parent == wip_parent
            && self.topo.len() == commits.len()
            && self
                .topo
                .iter()
                .zip(commits)
                .all(|(t, c)| t.0 == c.oid && t.1 == c.parents);
        if !unchanged {
            self.topo = commits.iter().map(|c| (c.oid, c.parents.clone())).collect();
            self.wip_parent = wip_parent;
            (self.wip_row, self.rows) = assign_lanes_with_wip(&self.topo, wip_parent);
            // Stash → base commit link: dashed, same visual language as the
            // WIP → HEAD link (off-branch content hooked onto its commit). A
            // row's parent edge is the only one leaving its own lane.
            for (commit, row) in commits.iter().zip(&mut self.rows) {
                if commit.stash {
                    let lane = row.lane;
                    for edge in row.edges.iter_mut().filter(|e| e.from_lane == lane) {
                        edge.dashed = true;
                    }
                }
            }
            self.computed = true;
            self.computes += 1;
        }
        (self.wip_row.as_ref(), &self.rows)
    }

    /// Test seam: number of computations performed since creation.
    pub fn computes(&self) -> usize {
        self.computes
    }
}

fn free_lane(lanes: &mut Vec<Option<git2::Oid>>) -> usize {
    match lanes.iter().position(|slot| slot.is_none()) {
        Some(index) => index,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

fn compact(lanes: &mut Vec<Option<git2::Oid>>) {
    while matches!(lanes.last(), Some(None)) {
        lanes.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> git2::Oid {
        git2::Oid::from_bytes(&[byte; 20]).unwrap()
    }

    fn graph_commit(byte: u8, parents: Vec<git2::Oid>) -> GraphCommit {
        GraphCommit {
            oid: oid(byte),
            short_id: format!("{byte:07x}"),
            summary: String::new(),
            body: String::new(),
            author: String::new(),
            time: 0,
            parents,
            refs: vec![],
            stash: false,
        }
    }

    fn stash_commit(byte: u8, base: git2::Oid) -> GraphCommit {
        GraphCommit {
            stash: true,
            ..graph_commit(byte, vec![base])
        }
    }

    #[test]
    fn lane_cache_computes_once_for_unchanged_topology() {
        let commits = vec![graph_commit(2, vec![oid(1)]), graph_commit(1, vec![])];
        let mut cache = LaneCache::default();

        let rows = cache.rows(&commits, None).1.to_vec();
        assert_eq!(rows, cache.rows(&commits, None).1);
        assert_eq!(cache.computes(), 1);
        assert_eq!(
            rows,
            assign_lanes(&[(oid(2), vec![oid(1)]), (oid(1), vec![])])
        );
    }

    #[test]
    fn lane_cache_recomputes_when_topology_changes() {
        let two = vec![graph_commit(2, vec![oid(1)]), graph_commit(1, vec![])];
        let mut cache = LaneCache::default();
        cache.rows(&two, None);

        // Same length, different parent ⇒ recompute (identity = oid + parents).
        let reparented = vec![graph_commit(2, vec![oid(3)]), graph_commit(1, vec![])];
        cache.rows(&reparented, None);
        assert_eq!(cache.computes(), 2);

        let one = vec![graph_commit(1, vec![])];
        assert_eq!(cache.rows(&one, None).1.len(), 1);
        assert_eq!(cache.computes(), 3);
    }

    #[test]
    fn lane_cache_recomputes_when_the_wip_link_changes() {
        let commits = vec![graph_commit(2, vec![oid(1)]), graph_commit(1, vec![])];
        let mut cache = LaneCache::default();

        assert!(cache.rows(&commits, None).0.is_none());
        // The tree becomes dirty (WIP → HEAD link) ⇒ recompute, WIP row present.
        let (wip_row, _) = cache.rows(&commits, Some(oid(2)));
        assert_eq!(wip_row.map(|r| r.lane), Some(0));
        assert_eq!(cache.computes(), 2);
        // Same topology + same link ⇒ memoized.
        cache.rows(&commits, Some(oid(2)));
        assert_eq!(cache.computes(), 2);
    }

    #[test]
    fn lane_cache_dashes_the_stash_link_to_its_base() {
        // stash(9) → base(1) alone: the base inherits the stash's lane, the
        // link is a dashed vertical (like the WIP → HEAD link).
        let commits = vec![stash_commit(9, oid(1)), graph_commit(1, vec![])];
        let mut cache = LaneCache::default();

        let rows = cache.rows(&commits, None).1;
        assert_eq!(
            rows[0].edges,
            vec![Edge {
                from_lane: 0,
                to_lane: 0,
                dashed: true,
                merge: false
            }]
        );
    }

    #[test]
    fn stash_collapse_onto_an_awaited_lane_is_dashed_and_passthrough_stays_solid() {
        // c2 awaits the base on lane 0; the stash, a tip shifted to lane 1,
        // collapses onto it: dashed collapse, solid branch pass-through.
        let commits = vec![
            graph_commit(2, vec![oid(1)]),
            stash_commit(9, oid(1)),
            graph_commit(1, vec![]),
        ];
        let mut cache = LaneCache::default();

        let rows = cache.rows(&commits, None).1;
        assert_eq!(rows[1].lane, 1, "the stash is a shifted tip");
        assert!(rows[1].edges.contains(&Edge {
            from_lane: 1,
            to_lane: 0,
            dashed: true,
            merge: false
        }));
        assert!(rows[1].edges.contains(&Edge {
            from_lane: 0,
            to_lane: 0,
            dashed: false,
            merge: false
        }));
    }

    #[test]
    fn lane_cache_memoizes_the_empty_graph_too() {
        let mut cache = LaneCache::default();
        assert!(cache.rows(&[], None).1.is_empty());
        assert!(cache.rows(&[], None).1.is_empty());
        assert_eq!(cache.computes(), 1);
    }

    #[test]
    fn linear_history_stays_on_lane_zero() {
        // a -> b -> c (each child's only parent is the next commit).
        let a = oid(1);
        let b = oid(2);
        let c = oid(3);
        let rows = assign_lanes(&[(a, vec![b]), (b, vec![c]), (c, vec![])]);

        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.lane == 0));
        // a continues to b on lane 0, b continues to c on lane 0.
        assert_eq!(
            rows[0].edges,
            vec![Edge {
                from_lane: 0,
                to_lane: 0,
                dashed: false,
                merge: false
            }]
        );
        assert_eq!(
            rows[1].edges,
            vec![Edge {
                from_lane: 0,
                to_lane: 0,
                dashed: false,
                merge: false
            }]
        );
        // c is a root: no descending edge.
        assert!(rows[2].edges.is_empty());
    }

    #[test]
    fn branch_then_merge_uses_two_lanes() {
        // m (merge) has parents [a, b]; a and b both descend to base z.
        //   m
        //   |\
        //   a b
        //   |/
        //   z
        let m = oid(1);
        let a = oid(2);
        let b = oid(3);
        let z = oid(4);
        let rows = assign_lanes(&[(m, vec![a, b]), (a, vec![z]), (b, vec![z]), (z, vec![])]);

        assert_eq!(rows[0].lane, 0, "merge sits on lane 0");
        // The merge opens a second lane for its extra parent b — flagged
        // `merge` (the renderer bends that link at the merge row).
        assert!(rows[0]
            .edges
            .iter()
            .any(|e| e.to_lane == 1 && e.from_lane == 0 && e.merge));
        assert!(rows[0]
            .edges
            .iter()
            .any(|e| e.to_lane == 0 && e.from_lane == 0 && !e.merge));
        // a keeps lane 0, b keeps lane 1.
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[2].lane, 1);
        // Both a and b collapse back onto z's single lane.
        assert_eq!(rows[3].lane, 0);
        assert!(rows[3].edges.is_empty());
    }

    #[test]
    fn merge_collapses_extra_lane_after_join() {
        let m = oid(1);
        let a = oid(2);
        let b = oid(3);
        let z = oid(4);
        let rows = assign_lanes(&[(m, vec![a, b]), (a, vec![z]), (b, vec![z]), (z, vec![])]);

        // After b lands on z (already on lane 0), lane 1 collapses: b's row
        // routes its lane-1 node down to lane 0 and no lane 1 survives.
        let b_row = &rows[2];
        assert!(b_row.edges.iter().all(|e| e.to_lane == 0));
        assert!(b_row.edges.contains(&Edge {
            from_lane: 1,
            to_lane: 0,
            dashed: false,
            merge: false
        }));
    }

    #[test]
    fn diverging_branches_share_no_lane_collision() {
        // Two independent roots: lane 0 and lane 1 never share a slot.
        let a = oid(1);
        let b = oid(2);
        let rows = assign_lanes(&[(a, vec![]), (b, vec![])]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 0, "a's lane freed, so b reuses lane 0");
        assert!(rows[0].edges.is_empty());
        assert!(rows[1].edges.is_empty());
    }

    #[test]
    fn wip_link_takes_a_dedicated_lane_and_shifts_other_branches() {
        // A branch above HEAD descends down to it:
        //   (wip)        lane 0, dashed
        //   a            tip shifted to lane 1
        //   b → h        converges into the HEAD node (h = HEAD)
        //   h            HEAD node on lane 0
        let a = oid(1);
        let b = oid(2);
        let h = oid(3);
        let (wip_row, rows) =
            assign_lanes_with_wip(&[(a, vec![b]), (b, vec![h]), (h, vec![])], Some(h));

        let wip_row = wip_row.expect("WIP row present");
        assert_eq!(wip_row.lane, 0, "the WIP link reserves lane 0");
        assert_eq!(
            wip_row.edges,
            vec![Edge {
                from_lane: 0,
                to_lane: 0,
                dashed: true,
                merge: false
            }]
        );
        // a is shifted: lane 0 belongs to the link, still exclusive here.
        assert_eq!(rows[0].lane, 1);
        assert!(rows[0].edges.contains(&Edge {
            from_lane: 0,
            to_lane: 0,
            dashed: true,
            merge: false
        }));
        // b converges into the HEAD node; the link keeps its lane (dashed)
        // down to it.
        assert!(rows[1].edges.contains(&Edge {
            from_lane: 0,
            to_lane: 0,
            dashed: true,
            merge: false
        }));
        assert!(rows[1].edges.contains(&Edge {
            from_lane: 1,
            to_lane: 0,
            dashed: false,
            merge: false
        }));
        assert_eq!(rows[2].lane, 0, "HEAD at the end of the link");
    }

    #[test]
    fn tips_sharing_a_parent_keep_their_own_lane_until_the_parent_row() {
        // Three tips all pointing at z: no mid-air collapse onto z's lane —
        // each keeps its own column, the lines converge at z's node.
        let t1 = oid(1);
        let t2 = oid(2);
        let t3 = oid(3);
        let z = oid(4);
        let rows = assign_lanes(&[(t1, vec![z]), (t2, vec![z]), (t3, vec![z]), (z, vec![])]);

        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1, "the tip keeps its own lane");
        assert_eq!(rows[2].lane, 2);
        // Above the parent row, each line is a plain vertical…
        assert!(rows[1].edges.contains(&Edge {
            from_lane: 1,
            to_lane: 1,
            dashed: false,
            merge: false
        }));
        // …and the row just above z redirects every awaiting lane into its node.
        assert_eq!(
            rows[2].edges,
            vec![
                Edge {
                    from_lane: 0,
                    to_lane: 0,
                    dashed: false,
                    merge: false
                },
                Edge {
                    from_lane: 1,
                    to_lane: 0,
                    dashed: false,
                    merge: false
                },
                Edge {
                    from_lane: 2,
                    to_lane: 0,
                    dashed: false,
                    merge: false
                },
            ]
        );
        assert_eq!(rows[3].lane, 0);
    }

    #[test]
    fn merge_link_joins_the_lane_already_awaiting_its_second_parent() {
        // t (lane 0) awaits b; the merge m then routes b as 2nd parent: the
        // merge link joins lane 0 mid-air (the one exception)
        // instead of opening a third lane.
        let t = oid(1);
        let m = oid(2);
        let a = oid(3);
        let b = oid(4);
        let z = oid(5);
        let rows = assign_lanes(&[
            (t, vec![b]),
            (m, vec![a, b]),
            (a, vec![z]),
            (b, vec![z]),
            (z, vec![]),
        ]);

        assert_eq!(rows[1].lane, 1);
        assert!(rows[1].edges.contains(&Edge {
            from_lane: 1,
            to_lane: 0,
            dashed: false,
            merge: true
        }));
    }

    #[test]
    fn wip_link_stays_dashed_down_to_head_without_other_children() {
        // Feature chain forked below HEAD (no child of HEAD in the page): the
        // dashing runs all the way to the HEAD node.
        //   (wip)              lane 0, dashed
        //   f2 → f1 → base     feature on lane 1
        //   h  → base          HEAD on lane 0 (= wip)
        //   base
        let f2 = oid(1);
        let f1 = oid(2);
        let h = oid(3);
        let base = oid(4);
        let (wip_row, rows) = assign_lanes_with_wip(
            &[
                (f2, vec![f1]),
                (f1, vec![base]),
                (h, vec![base]),
                (base, vec![]),
            ],
            Some(h),
        );

        assert_eq!(wip_row.unwrap().lane, 0);
        for row in &rows[..2] {
            assert_eq!(row.lane, 1, "the feature chain is shifted");
            assert!(
                row.edges.contains(&Edge {
                    from_lane: 0,
                    to_lane: 0,
                    dashed: true,
                    merge: false
                }),
                "the link stays dashed down to HEAD"
            );
        }
        assert_eq!(rows[2].lane, 0, "HEAD at the end of the link");
        assert!(
            rows[2].edges.iter().all(|e| !e.dashed),
            "past the HEAD node, no more dashing"
        );
    }

    #[test]
    fn assign_lanes_without_wip_emits_no_dashed_edge() {
        let rows = assign_lanes(&[(oid(1), vec![oid(2)]), (oid(2), vec![])]);
        assert!(rows.iter().flat_map(|r| &r.edges).all(|e| !e.dashed));
    }
}
