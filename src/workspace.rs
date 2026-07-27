use std::path::{Path, PathBuf};

use crate::persistence::Project;
use crate::terminal::layout::{Layout, PaneId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub path: PathBuf,
    pub name: String,
    /// Bare root (worktrees.md §8): shown but not selectable (v1).
    pub bare: bool,
}

impl Repo {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self {
            path,
            name,
            bare: false,
        }
    }
}

/// Stable tab identity (M17-11): minted once from a workspace-wide counter and
/// never reused — cache keys don't shift when a tab closes or a sibling moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

struct Tab {
    id: TabId,
    layout: Layout,
    /// User rename (`rename_tab`): wins over the auto name.
    name: Option<String>,
    /// Sticky default derived from the terminal's activity (OSC title / running
    /// process), refreshed at the agent-watch tick (terminal.md §4). Kept across
    /// idle periods — only a new activity replaces it.
    auto_name: Option<String>,
}

struct Entry {
    repo: Repo,
    /// Path of the group root when the entry is a linked worktree; `None` for a
    /// root or a standalone repo.
    parent: Option<PathBuf>,
    /// Group root folded in the sidebar: its worktree children are hidden
    /// (worktrees.md §3). Only meaningful on a root; persisted (worktrees.md §5).
    collapsed: bool,
    /// Project hidden from the sidebar by the user (root + its worktrees). Only
    /// meaningful on a root; persisted.
    hidden: bool,
    tabs: Vec<Tab>,
    active_tab: usize,
}

/// Result of a `sync_group`: `mapping[old index] = Some(new index)`, `None` =
/// entry removed (PTY to kill on the app side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSync {
    pub mapping: Vec<Option<usize>>,
}

#[derive(Default)]
pub struct Workspace {
    entries: Vec<Entry>,
    active: Option<usize>,
    next_tab_id: u64,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    fn mint_tab(&mut self) -> Tab {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        Tab {
            id,
            layout: Layout::new(),
            name: None,
            auto_name: None,
        }
    }

    fn new_entry(&mut self, repo: Repo) -> Entry {
        Entry {
            repo,
            parent: None,
            collapsed: false,
            hidden: false,
            tabs: vec![self.mint_tab()],
            active_tab: 0,
        }
    }

    fn new_child_entry(&mut self, repo: Repo, root: PathBuf) -> Entry {
        Entry {
            parent: Some(root),
            ..self.new_entry(repo)
        }
    }

    /// Stable id of tab `tab` of entry `index` — the pane-cache key side that
    /// survives closes and reorders (M17-11).
    pub fn tab_id(&self, index: usize, tab: usize) -> Option<TabId> {
        Some(self.entries.get(index)?.tabs.get(tab)?.id)
    }

    /// Every live `(entry index, tab id)` pair, for cache reconciliation.
    pub fn all_tab_ids(&self) -> impl Iterator<Item = (usize, TabId)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .flat_map(|(i, e)| e.tabs.iter().map(move |t| (i, t.id)))
    }

    /// Display title of any tab (whichever entry owns it), with the same
    /// precedence as [`tab_titles`] — rename, else auto name, else "Tab N". Used
    /// by the cross-repo agents dashboard, which addresses tabs across entries.
    pub fn tab_label(&self, tab_id: TabId) -> Option<String> {
        self.entries.iter().find_map(|e| {
            e.tabs.iter().position(|t| t.id == tab_id).map(|pos| {
                let t = &e.tabs[pos];
                t.name
                    .clone()
                    .or_else(|| t.auto_name.clone())
                    .unwrap_or_else(|| format!("Tab {}", pos + 1))
            })
        })
    }

    /// Position of tab `tab_id` within entry `index` — the index `set_active_tab`
    /// expects, when focusing an agent's tab from the dashboard (its key carries
    /// the stable `TabId`, not the volatile position).
    pub fn tab_index(&self, index: usize, tab_id: TabId) -> Option<usize> {
        self.entries
            .get(index)?
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn repos(&self) -> impl Iterator<Item = &Repo> + '_ {
        self.entries.iter().map(|e| &e.repo)
    }

    pub fn repo(&self, index: usize) -> Option<&Repo> {
        self.entries.get(index).map(|e| &e.repo)
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn active_repo(&self) -> Option<&Repo> {
        let i = self.active?;
        Some(&self.entries[i].repo)
    }

    pub fn active_layout(&self) -> Option<&Layout> {
        let e = self.active_entry()?;
        Some(&e.tabs[e.active_tab].layout)
    }

    pub fn active_layout_mut(&mut self) -> Option<&mut Layout> {
        let e = self.active_entry_mut()?;
        Some(&mut e.tabs[e.active_tab].layout)
    }

    pub fn tab_count(&self) -> Option<usize> {
        Some(self.active_entry()?.tabs.len())
    }

    /// Titles shown by the active repo's tab bar: user rename (`rename_tab`),
    /// else the activity-derived auto name (`refresh_auto_name`), else a "Tab N"
    /// fallback based on the current position (terminal.md §4).
    pub fn tab_titles(&self) -> Option<Vec<String>> {
        let e = self.active_entry()?;
        Some(
            e.tabs
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    t.name
                        .clone()
                        .or_else(|| t.auto_name.clone())
                        .unwrap_or_else(|| format!("Tab {}", i + 1))
                })
                .collect(),
        )
    }

    /// Focused pane of tab `tab_id` (whichever entry owns it) — the pane whose
    /// activity names the tab (terminal.md §4). `None` if the id is stale.
    pub fn tab_focus(&self, tab_id: TabId) -> Option<PaneId> {
        self.entries
            .iter()
            .flat_map(|e| &e.tabs)
            .find(|t| t.id == tab_id)
            .map(|t| t.layout.focus())
    }

    /// Updates a tab's activity-derived auto name. Sticky: a `None` candidate
    /// (idle prompt) keeps the last name; only a new, different activity replaces
    /// it. The user rename still wins at display (terminal.md §4).
    pub fn refresh_auto_name(&mut self, tab_id: TabId, candidate: Option<&str>) {
        let Some(cand) = candidate else { return };
        if let Some(tab) = self
            .entries
            .iter_mut()
            .flat_map(|e| &mut e.tabs)
            .find(|t| t.id == tab_id)
        {
            if tab.auto_name.as_deref() != Some(cand) {
                tab.auto_name = Some(cand.to_owned());
            }
        }
    }

    /// Renames tab `tab` of the active repo. An empty name (after trim) clears the
    /// custom name and reverts to the default "Tab N" title.
    pub fn rename_tab(&mut self, tab: usize, name: &str) -> bool {
        let Some(e) = self.active_entry_mut() else {
            return false;
        };
        let Some(t) = e.tabs.get_mut(tab) else {
            return false;
        };
        let trimmed = name.trim();
        t.name = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        true
    }

    pub fn active_tab(&self) -> Option<usize> {
        Some(self.active_entry()?.active_tab)
    }

    pub fn add_tab(&mut self) -> Option<usize> {
        self.active_entry()?;
        let fresh = self.mint_tab();
        let e = self.active_entry_mut()?;
        e.tabs.push(fresh);
        e.active_tab = e.tabs.len() - 1;
        Some(e.active_tab)
    }

    pub fn close_tab(&mut self, tab: usize) -> bool {
        let Some(len) = self.active_entry().map(|e| e.tabs.len()) else {
            return false;
        };
        if tab >= len {
            return false;
        }
        // Closing the last tab restarts on a fresh id: the closed tab's PTY set is
        // dropped by the cache sync, exactly like a non-last close (terminal.md §11).
        let fresh = (len == 1).then(|| self.mint_tab());
        let Some(e) = self.active_entry_mut() else {
            return false;
        };
        match fresh {
            Some(fresh) => {
                e.tabs[0] = fresh;
                e.active_tab = 0;
            }
            None => {
                e.tabs.remove(tab);
                if e.active_tab > tab || e.active_tab == e.tabs.len() {
                    e.active_tab -= 1;
                }
            }
        }
        true
    }

    pub fn set_active_tab(&mut self, tab: usize) -> bool {
        let Some(e) = self.active_entry_mut() else {
            return false;
        };
        if tab < e.tabs.len() {
            e.active_tab = tab;
            true
        } else {
            false
        }
    }

    /// Reorders the active repo's tabs: tab `from` is dropped just `after` (or
    /// before) tab `anchor`. `active_tab` follows its tab by identity. Returns
    /// whether the tabs actually moved (a drop onto its own edge is a no-op).
    pub fn reorder_tab(&mut self, from: usize, anchor: usize, after: bool) -> bool {
        let Some(e) = self.active_entry_mut() else {
            return false;
        };
        let len = e.tabs.len();
        if from >= len || anchor >= len {
            return false;
        }
        let insert_at = if after { anchor + 1 } else { anchor };
        if insert_at == from || insert_at == from + 1 {
            return false;
        }
        let active_id = e.tabs[e.active_tab].id;
        let tab = e.tabs.remove(from);
        let dest = if insert_at > from {
            insert_at - 1
        } else {
            insert_at
        };
        e.tabs.insert(dest, tab);
        e.active_tab = e
            .tabs
            .iter()
            .position(|t| t.id == active_id)
            .unwrap_or(e.active_tab);
        true
    }

    fn active_entry(&self) -> Option<&Entry> {
        self.entries.get(self.active?)
    }

    fn active_entry_mut(&mut self) -> Option<&mut Entry> {
        let i = self.active?;
        self.entries.get_mut(i)
    }

    pub fn add(&mut self, repo: Repo) -> usize {
        let entry = self.new_entry(repo);
        self.entries.push(entry);
        let index = self.entries.len() - 1;
        if self.active.is_none() {
            self.active = Some(index);
        }
        index
    }

    /// Adds a group: the root then its children, kept in the **given order**
    /// (the manual order persisted across sessions, worktrees.md §3 — a fresh
    /// import sorts them alpha at the call site). Returns the root's index.
    pub fn add_group(&mut self, root: Repo, children: Vec<Repo>) -> usize {
        let root_path = root.path.clone();
        let root_bare = root.bare;
        let child_count = children.len();
        let index = self.entries.len();
        let root_entry = self.new_entry(root);
        self.entries.push(root_entry);
        for repo in children {
            let child = self.new_child_entry(repo, root_path.clone());
            self.entries.push(child);
        }
        if self.active.is_none() {
            // Non-selectable bare root (worktrees.md §8): auto-activation falls
            // back to the first child.
            if !root_bare {
                self.active = Some(index);
            } else if child_count > 0 {
                self.active = Some(index + 1);
            }
        }
        index
    }

    /// Follows a worktree renamed on disk (`git worktree move`, worktrees.md §6):
    /// the entry keeps its slot, its tabs and the selection — only its path and
    /// name move, so the next disk sync sees a survivor instead of a
    /// vanished-plus-discovered pair.
    pub fn set_repo_path(&mut self, index: usize, path: PathBuf) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.repo = Repo {
            bare: entry.repo.bare,
            ..Repo::new(path)
        };
        true
    }

    /// Reconciles a root's bare flag (sync, worktrees.md §8).
    pub fn set_bare(&mut self, index: usize, bare: bool) {
        if let Some(e) = self.entries.get_mut(index) {
            e.repo.bare = bare;
        }
    }

    /// Path of the group root if the entry is a linked worktree.
    pub fn parent_root(&self, index: usize) -> Option<&Path> {
        self.entries.get(index)?.parent.as_deref()
    }

    /// Stable per-worktree port offset within its group (git.md §3): the group
    /// root is `0`; each worktree is `1` plus its rank among the group's worktrees
    /// ordered by path. Keyed on the worktree's own path rather than its sidebar
    /// row, so a drag-reorder never reshuffles the assigned ports. `0` when
    /// `member` isn't found.
    pub fn group_offset(&self, root: &Path, member: &Path) -> usize {
        if member == root {
            return 0;
        }
        let mut worktrees: Vec<&Path> = self
            .entries
            .iter()
            .filter(|e| e.parent.as_deref() == Some(root))
            .map(|e| e.repo.path.as_path())
            .collect();
        worktrees.sort_unstable();
        worktrees
            .iter()
            .position(|p| *p == member)
            .map_or(0, |rank| rank + 1)
    }

    /// Display name of the entry's *project*: its group root's name when it is a
    /// linked worktree, otherwise its own name. Groups a root and its worktrees
    /// under one heading (specs/agents.md §5).
    pub fn project_name(&self, index: usize) -> Option<String> {
        let own = self.repo(index)?.name.clone();
        Some(match self.parent_root(index).and_then(|p| p.file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => own,
        })
    }

    /// Folded state of a group root (worktrees.md §3); `false` for a child or a
    /// standalone entry, which never collapse.
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.entries.get(index).is_some_and(|e| e.collapsed)
    }

    pub fn set_collapsed(&mut self, index: usize, collapsed: bool) {
        if let Some(e) = self.entries.get_mut(index) {
            e.collapsed = collapsed;
        }
    }

    pub fn toggle_collapsed(&mut self, index: usize) {
        if let Some(e) = self.entries.get_mut(index) {
            e.collapsed = !e.collapsed;
        }
    }

    /// User-hidden state of a group root; `false` for a child or a standalone
    /// entry read directly (the project-level flag lives on the root).
    pub fn is_user_hidden(&self, index: usize) -> bool {
        self.entries.get(index).is_some_and(|e| e.hidden)
    }

    pub fn set_user_hidden(&mut self, index: usize, hidden: bool) {
        if let Some(e) = self.entries.get_mut(index) {
            e.hidden = hidden;
        }
    }

    pub fn toggle_user_hidden(&mut self, index: usize) {
        if let Some(e) = self.entries.get_mut(index) {
            e.hidden = !e.hidden;
        }
    }

    /// `index` belongs to a project the user hid: the root itself, or a linked
    /// worktree whose root is hidden. Whole project (root + worktrees) drops out
    /// of the sidebar and the `⌘1..9` numbering together.
    pub fn is_in_hidden_project(&self, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        match entry.parent.as_deref() {
            Some(parent) => self
                .entries
                .iter()
                .any(|e| e.parent.is_none() && e.repo.path == parent && e.hidden),
            None => entry.hidden,
        }
    }

    /// A sidebar row hidden by a folded group (worktrees.md §3): a linked
    /// worktree whose root is collapsed, or a collapsed root's own main row —
    /// folding leaves only the project header. Skipped by the `⌘1..9` numbering
    /// (§7).
    pub fn is_hidden(&self, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        match entry.parent.as_deref() {
            Some(parent) => self
                .entries
                .iter()
                .any(|e| e.parent.is_none() && e.repo.path == parent && e.collapsed),
            None => entry.collapsed,
        }
    }

    /// A top-level entry that owns only a header, never a selectable row: a bare
    /// root has no main working tree (worktrees.md §8).
    fn is_header_only(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|e| e.parent.is_none() && e.repo.bare)
    }

    /// Entry indices of the selectable rows, in sidebar order — the flattened
    /// order minus the rows folded under a collapsed root (worktrees.md §7) and
    /// the bare roots that own only a header (§8).
    fn selectable_rows(&self) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&i| {
                !self.is_header_only(i) && !self.is_hidden(i) && !self.is_in_hidden_project(i)
            })
            .collect()
    }

    /// `index` of the `n`-th selectable row.
    pub fn nth_visible(&self, n: usize) -> Option<usize> {
        self.selectable_rows().get(n).copied()
    }

    /// Cycle the active selection to the next/previous selectable row, wrapping
    /// (keybindings.md §1): worktrees count as their own row, folded and bare-root
    /// rows are skipped. From no selection, lands on the first (next) or last
    /// (previous) row. No-op when nothing is selectable.
    pub fn cycle_active(&mut self, forward: bool) -> bool {
        let rows = self.selectable_rows();
        if rows.is_empty() {
            return false;
        }
        let pos = self.active.and_then(|a| rows.iter().position(|&i| i == a));
        let next = match pos {
            Some(p) if forward => (p + 1) % rows.len(),
            Some(p) => (p + rows.len() - 1) % rows.len(),
            None if forward => 0,
            None => rows.len() - 1,
        };
        self.set_active(rows[next])
    }

    /// Persistence shape (architecture §4): one `Project` per root, worktrees
    /// nested under the root their `parent` points to. Single encoding of the
    /// grouping outside this module (M17-7) — a worktree whose root is absent
    /// from the workspace falls back to a root of its own.
    pub fn to_projects(&self) -> Vec<Project> {
        let mut projects: Vec<Project> = Vec::new();
        for (i, repo) in self.repos().enumerate() {
            match self
                .parent_root(i)
                .and_then(|root| projects.iter_mut().find(|p| p.root == root))
            {
                Some(project) => project.worktrees.push(repo.path.clone()),
                None => projects.push(Project {
                    root: repo.path.clone(),
                    worktrees: Vec::new(),
                    collapsed: self.entries[i].collapsed,
                    hidden: self.entries[i].hidden,
                }),
            }
        }
        projects
    }

    /// Group root = an entry with no parent that has at least one child.
    pub fn is_group_root(&self, index: usize) -> bool {
        let Some(e) = self.entries.get(index) else {
            return false;
        };
        e.parent.is_none()
            && self
                .entries
                .iter()
                .any(|c| c.parent.as_deref() == Some(e.repo.path.as_path()))
    }

    /// Reconciles group `root_path`'s children with `children` (discovery/purge):
    /// surviving entries keep their **manual order** (worktrees.md §3), their tabs
    /// and trees; vanished ones are removed; worktrees discovered on disk are
    /// **appended** after the survivors (sorted alpha among themselves for a
    /// deterministic first placement). If the active child vanishes, the active
    /// selection falls back to the root. `None` if the root is unknown.
    pub fn sync_group(&mut self, root_path: &Path, children: Vec<Repo>) -> Option<GroupSync> {
        let root_idx = self
            .entries
            .iter()
            .position(|e| e.parent.is_none() && e.repo.path == root_path)?;

        let old = std::mem::take(&mut self.entries);
        let mut mapping = vec![None; old.len()];
        let mut old_children: Vec<(usize, Entry)> = Vec::new();
        let mut rest: Vec<(usize, Entry)> = Vec::new();
        for (i, e) in old.into_iter().enumerate() {
            if e.parent.as_deref() == Some(root_path) {
                old_children.push((i, e));
            } else {
                rest.push((i, e));
            }
        }

        let on_disk: Vec<PathBuf> = children.iter().map(|r| r.path.clone()).collect();
        let survives = |entry: &Entry| on_disk.contains(&entry.repo.path);
        let mut discovered: Vec<Repo> = children
            .into_iter()
            .filter(|r| !old_children.iter().any(|(_, c)| c.repo.path == r.path))
            .collect();
        discovered.sort_by_key(|r| r.name.to_lowercase());

        let mut new_entries: Vec<Entry> = Vec::new();
        for (i, e) in rest {
            mapping[i] = Some(new_entries.len());
            new_entries.push(e);
            if i == root_idx {
                // Survivors first, in their persisted manual order.
                for (old_i, entry) in std::mem::take(&mut old_children) {
                    if survives(&entry) {
                        mapping[old_i] = Some(new_entries.len());
                        new_entries.push(entry);
                    }
                }
                // Then the worktrees created outside the app, appended.
                for repo in discovered.drain(..) {
                    let child = self.new_child_entry(repo, root_path.to_path_buf());
                    new_entries.push(child);
                }
            }
        }

        self.entries = new_entries;
        self.active = self.active.and_then(|a| {
            mapping.get(a).copied().flatten().or_else(|| {
                // Fall back to the root — except bare (worktrees.md §8): no selection.
                mapping[root_idx].filter(|&r| !self.entries[r].repo.bare)
            })
        });
        Some(GroupSync { mapping })
    }

    pub fn set_active(&mut self, index: usize) -> bool {
        // Non-selectable bare root (worktrees.md §8): refused whatever the path
        // (click, ⌘N, import activation).
        if index < self.entries.len() && !self.entries[index].repo.bare {
            self.active = Some(index);
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<Repo> {
        if index >= self.entries.len() {
            return None;
        }
        let removed = self.entries.remove(index);
        if let Some(active) = self.active {
            if self.entries.is_empty() {
                self.active = None;
            } else if index < active {
                self.active = Some(active - 1);
            } else if index == active {
                // Skip bare roots (worktrees.md §8): never land the selection on one.
                let start = active.min(self.entries.len() - 1);
                self.active = (start..self.entries.len())
                    .chain((0..start).rev())
                    .find(|&i| !self.entries[i].repo.bare);
            }
        }
        Some(removed.repo)
    }

    /// Commits a sidebar drag-reorder (worktrees.md §3): the dragged entry `from`
    /// is dropped just `after` (or before) the row `anchor`. A root drags its
    /// whole project block (root + its worktree children) among the top-level
    /// projects; a worktree moves only among its siblings — see
    /// [`resolve_reorder`]. `active` follows its repo to the new index. Returns
    /// whether the entries actually moved, so the caller persists only on a real
    /// change.
    pub fn reorder(&mut self, from: usize, anchor: usize, after: bool) -> bool {
        let child: Vec<bool> = self.entries.iter().map(|e| e.parent.is_some()).collect();
        let Some((start, end, insert_at)) = resolve_reorder(&child, from, anchor, after) else {
            return false;
        };
        let active_path = self.active.map(|a| self.entries[a].repo.path.clone());
        let block: Vec<Entry> = self.entries.drain(start..end).collect();
        // Indices past the removed block shift left; an insertion before it is
        // unchanged (the resolver never targets inside the block).
        let at = if insert_at >= end {
            insert_at - block.len()
        } else {
            insert_at
        };
        self.entries.splice(at..at, block);
        self.active = active_path.and_then(|p| self.entries.iter().position(|e| e.repo.path == p));
        true
    }
}

/// Pure reorder resolution shared by [`Workspace::reorder`] and the sidebar drop
/// indicator. `child[i]` flags entry `i` as an indented worktree; a project is a
/// maximal run `[root, child…]`. Resolves a drag of entry `from` dropped just
/// `after` (or before) `anchor` into `(start, end, insert_at)` — the block
/// `[start, end)` moves to sit before `insert_at` — or `None` when the drop is
/// rejected (a worktree leaving its group) or a no-op. A root carries its whole
/// project block; a worktree stays within its group's sibling span.
pub fn resolve_reorder(
    child: &[bool],
    from: usize,
    anchor: usize,
    after: bool,
) -> Option<(usize, usize, usize)> {
    let len = child.len();
    if from >= len || anchor >= len {
        return None;
    }
    let root_of = |i: usize| (0..=i).rev().find(|&j| !child[j]).unwrap_or(0);
    let block_end = |root: usize| (root + 1..len).find(|&j| !child[j]).unwrap_or(len);

    let (start, end, insert_at) = if !child[from] {
        // Project block: snap the drop to the anchor's top-level boundary.
        let anchor_root = root_of(anchor);
        let insert_at = if after {
            block_end(anchor_root)
        } else {
            anchor_root
        };
        (from, block_end(from), insert_at)
    } else {
        // Worktree: rejected outside its own group, clamped to the sibling span.
        let root = root_of(from);
        if root != root_of(anchor) {
            return None;
        }
        let raw = if after { anchor + 1 } else { anchor };
        (from, from + 1, raw.clamp(root + 1, block_end(root)))
    };

    // Dropped onto its own edge (no move) or, defensively, inside its own block.
    if insert_at == start || insert_at == end || (start < insert_at && insert_at < end) {
        return None;
    }
    Some((start, end, insert_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::layout::Orient;

    fn repo(name: &str) -> Repo {
        Repo::new(PathBuf::from(format!("/tmp/{name}")))
    }

    fn bare_repo(name: &str) -> Repo {
        Repo {
            bare: true,
            ..repo(name)
        }
    }

    #[test]
    fn to_projects_nests_worktrees_under_their_root() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_group(repo("proj"), vec![repo("wt-b"), repo("wt-a")]);

        assert_eq!(
            ws.to_projects(),
            vec![
                Project {
                    root: PathBuf::from("/tmp/a"),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
                Project {
                    root: PathBuf::from("/tmp/proj"),
                    worktrees: vec![PathBuf::from("/tmp/wt-b"), PathBuf::from("/tmp/wt-a")],
                    collapsed: false,
                    hidden: false,
                },
            ]
        );
    }

    #[test]
    fn collapsing_a_root_hides_its_rows_and_carries_into_projects() {
        let mut ws = Workspace::new();
        ws.add(repo("standalone"));
        let root = ws.add_group(repo("proj"), vec![repo("wt-a"), repo("wt-b")]);

        assert!(
            !ws.is_hidden(2) && !ws.is_hidden(3),
            "expanded: children shown"
        );
        ws.toggle_collapsed(root);
        assert!(ws.is_collapsed(root));
        assert!(
            ws.is_hidden(2) && ws.is_hidden(3),
            "the two worktrees are hidden"
        );
        assert!(
            ws.is_hidden(root),
            "the folded root's main row is hidden too — only the header shows"
        );
        assert!(!ws.is_hidden(0), "an unrelated standalone is untouched");

        let projects = ws.to_projects();
        assert!(!projects[0].collapsed, "standalone projects carry no fold");
        assert!(projects[1].collapsed, "the folded group persists its state");
    }

    #[test]
    fn nth_visible_skips_the_rows_folded_under_a_collapsed_root() {
        let mut ws = Workspace::new();
        let root = ws.add_group(repo("proj"), vec![repo("wt")]);
        ws.add(repo("next"));
        // Expanded: root (main), wt, next.
        assert_eq!(
            (ws.nth_visible(0), ws.nth_visible(1), ws.nth_visible(2)),
            (Some(0), Some(1), Some(2))
        );

        ws.set_collapsed(root, true);
        // Folded: main row and worktree both drop out — only the header remains,
        // so "next" takes the first slot.
        assert_eq!(
            ws.nth_visible(0),
            Some(2),
            "the folded group's rows are gone"
        );
        assert_eq!(ws.nth_visible(1), None);
    }

    #[test]
    fn cycle_active_wraps_through_the_visible_rows() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        ws.add(repo("c"));
        ws.set_active(0);

        assert!(ws.cycle_active(true));
        assert_eq!(ws.active(), Some(1));
        assert!(ws.cycle_active(true));
        assert_eq!(ws.active(), Some(2));
        assert!(ws.cycle_active(true), "forward wraps to the first");
        assert_eq!(ws.active(), Some(0));

        assert!(ws.cycle_active(false), "backward wraps to the last");
        assert_eq!(ws.active(), Some(2));
    }

    #[test]
    fn cycle_active_skips_folded_rows() {
        let mut ws = Workspace::new();
        let root = ws.add_group(repo("proj"), vec![repo("wt")]);
        ws.add(repo("next"));
        ws.set_collapsed(root, true);
        // Folded: the root main row and the worktree drop out — only "next" (index
        // 2) stays selectable.
        assert!(ws.cycle_active(true));
        assert_eq!(
            ws.active(),
            Some(2),
            "from no selection, next lands on the only row"
        );
        assert!(ws.cycle_active(true), "a single row wraps onto itself");
        assert_eq!(ws.active(), Some(2));
    }

    #[test]
    fn cycle_active_from_no_selection_picks_an_edge() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        assert!(ws.cycle_active(false));
        assert_eq!(
            ws.active(),
            Some(1),
            "previous from nothing lands on the last row"
        );
    }

    #[test]
    fn cycle_active_is_a_noop_without_selectable_rows() {
        let mut ws = Workspace::new();
        assert!(!ws.cycle_active(true));
        assert_eq!(ws.active(), None);
    }

    #[test]
    fn project_name_shares_the_root_for_worktrees() {
        let mut ws = Workspace::new();
        ws.add(repo("solo"));
        ws.add_group(repo("proj"), vec![repo("wt")]);
        assert_eq!(ws.project_name(0).as_deref(), Some("solo"));
        assert_eq!(
            ws.project_name(1).as_deref(),
            Some("proj"),
            "the root keeps its own name"
        );
        assert_eq!(
            ws.project_name(2).as_deref(),
            Some("proj"),
            "a worktree reports its group root's name"
        );
        assert_eq!(
            ws.project_name(9),
            None,
            "an out-of-range index has no project"
        );
    }

    #[test]
    fn collapse_flag_survives_a_group_sync() {
        let mut ws = Workspace::new();
        let root = ws.add_group(repo("proj"), vec![repo("alpha")]);
        ws.set_collapsed(root, true);

        ws.sync_group(Path::new("/tmp/proj"), vec![repo("alpha"), repo("beta")])
            .unwrap();

        assert!(ws.is_collapsed(0), "the root keeps its fold across a sync");
        assert!(
            ws.is_hidden(1) && ws.is_hidden(2),
            "discovered children stay folded"
        );
    }

    #[test]
    fn to_projects_stray_worktree_falls_back_to_its_own_root() {
        let mut ws = Workspace::new();
        ws.add_group(repo("proj"), vec![repo("wt")]);
        ws.entries[1].parent = Some(PathBuf::from("/tmp/gone"));

        assert_eq!(
            ws.to_projects(),
            vec![
                Project {
                    root: PathBuf::from("/tmp/proj"),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
                Project {
                    root: PathBuf::from("/tmp/wt"),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
            ]
        );
    }

    #[test]
    fn set_active_refuses_a_bare_root() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_group(bare_repo("proj.git"), vec![repo("checkout")]);

        assert!(!ws.set_active(1), "a bare root is not selectable");
        assert_eq!(ws.active(), Some(0));
        assert!(ws.set_active(2), "its worktrees stay selectable");
    }

    #[test]
    fn add_group_with_a_bare_root_activates_the_first_child() {
        let mut ws = Workspace::new();
        ws.add_group(bare_repo("proj.git"), vec![repo("checkout")]);
        assert_eq!(ws.active(), Some(1));

        let mut empty = Workspace::new();
        empty.add_group(bare_repo("proj.git"), Vec::new());
        assert_eq!(empty.active(), None, "no selectable entry at all");
    }

    #[test]
    fn sync_group_active_fallback_skips_a_bare_root() {
        let mut ws = Workspace::new();
        ws.add_group(bare_repo("proj.git"), vec![repo("checkout")]);
        assert_eq!(ws.active(), Some(1));

        let sync = ws
            .sync_group(&PathBuf::from("/tmp/proj.git"), Vec::new())
            .unwrap();

        assert_eq!(sync.mapping, vec![Some(0), None]);
        assert_eq!(
            ws.active(),
            None,
            "the removed active child cannot fall back on a bare root"
        );
    }

    #[test]
    fn repo_name_is_derived_from_path() {
        let r = Repo::new(PathBuf::from("/home/dev/my-project"));
        assert_eq!(r.name, "my-project");
    }

    #[test]
    fn add_appends_in_order_and_activates_only_the_first_repo() {
        let mut ws = Workspace::new();
        assert!(ws.is_empty());
        assert_eq!(ws.active(), None);

        assert_eq!(ws.add(repo("a")), 0);
        assert_eq!(ws.active(), Some(0));

        assert_eq!(ws.add(repo("b")), 1);
        assert_eq!(ws.active(), Some(0));

        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn each_repo_has_its_own_split_tree() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));

        ws.set_active(0);
        ws.active_layout_mut().unwrap().split(Orient::Vertical);
        assert_eq!(ws.active_layout().unwrap().pane_ids().len(), 2);

        ws.set_active(1);
        assert_eq!(ws.active_layout().unwrap().pane_ids().len(), 1);
    }

    #[test]
    fn switching_repos_restores_each_tree_left_as_it_was() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));

        ws.set_active(0);
        ws.active_layout_mut().unwrap().split(Orient::Vertical);
        ws.active_layout_mut().unwrap().split(Orient::Horizontal);
        let a_root = ws.active_layout().unwrap().root().clone();
        let a_focus = ws.active_layout().unwrap().focus();
        assert_eq!(ws.active_layout().unwrap().pane_ids().len(), 3);

        assert!(ws.set_active(1));
        assert_eq!(
            ws.active_layout().unwrap().pane_ids().len(),
            1,
            "the other repo keeps its pristine single-pane tree"
        );

        assert!(ws.set_active(0));
        assert_eq!(
            ws.active_layout().unwrap().root(),
            &a_root,
            "switching back restores the tree identically"
        );
        assert_eq!(
            ws.active_layout().unwrap().focus(),
            a_focus,
            "switching back restores the focused pane"
        );
    }

    #[test]
    fn set_active_rejects_out_of_bounds() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        assert!(ws.set_active(0));
        assert!(!ws.set_active(5));
        assert_eq!(ws.active(), Some(0));
    }

    #[test]
    fn remove_before_active_shifts_active_index() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        ws.add(repo("c"));
        ws.set_active(2);

        let removed = ws.remove(0).unwrap();
        assert_eq!(removed.name, "a");
        assert_eq!(ws.active(), Some(1));
        assert_eq!(ws.active_repo().unwrap().name, "c");
    }

    #[test]
    fn remove_after_active_keeps_active_index() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        ws.set_active(0);

        ws.remove(1);
        assert_eq!(ws.active(), Some(0));
        assert_eq!(ws.active_repo().unwrap().name, "a");
    }

    #[test]
    fn remove_active_falls_back_to_neighbor() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        ws.add(repo("c"));
        ws.set_active(1);

        ws.remove(1);
        assert_eq!(ws.active(), Some(1));
        assert_eq!(ws.active_repo().unwrap().name, "c");

        ws.set_active(1);
        ws.remove(1);
        assert_eq!(ws.active(), Some(0));
        assert_eq!(ws.active_repo().unwrap().name, "a");
    }

    #[test]
    fn remove_last_repo_clears_active() {
        let mut ws = Workspace::new();
        ws.add(repo("only"));
        ws.remove(0);
        assert!(ws.is_empty());
        assert_eq!(ws.active(), None);
        assert!(ws.active_layout().is_none());
    }

    #[test]
    fn remove_out_of_bounds_is_none() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        assert!(ws.remove(9).is_none());
        assert_eq!(ws.len(), 1);
    }

    #[test]
    fn a_fresh_repo_starts_with_one_active_tab() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        assert_eq!(ws.tab_count(), Some(1));
        assert_eq!(ws.active_tab(), Some(0));
    }

    #[test]
    fn tab_ops_without_active_repo_are_no_ops() {
        let mut ws = Workspace::new();
        assert_eq!(ws.tab_count(), None);
        assert_eq!(ws.active_tab(), None);
        assert_eq!(ws.add_tab(), None);
        assert!(!ws.close_tab(0));
        assert!(!ws.set_active_tab(0));
        assert!(!ws.reorder_tab(0, 0, true));
    }

    #[test]
    fn tab_ids_are_stable_across_a_close() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();
        let survivors = (ws.tab_id(0, 1).unwrap(), ws.tab_id(0, 2).unwrap());

        assert!(ws.close_tab(0));

        assert_eq!(
            (ws.tab_id(0, 0).unwrap(), ws.tab_id(0, 1).unwrap()),
            survivors,
            "surviving tabs keep their ids when positions shift"
        );
    }

    #[test]
    fn closing_the_last_tab_mints_a_fresh_id() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        let old = ws.tab_id(0, 0).unwrap();

        assert!(ws.close_tab(0));

        assert_ne!(
            ws.tab_id(0, 0).unwrap(),
            old,
            "the replacement tab is a new identity — its PTY set must not be inherited"
        );
    }

    #[test]
    fn tab_ids_are_unique_across_entries() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        let ids: Vec<TabId> = ws.all_tab_ids().map(|(_, id)| id).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn add_tab_appends_a_fresh_tab_and_activates_it() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));

        assert_eq!(ws.add_tab(), Some(1));
        assert_eq!(ws.tab_count(), Some(2));
        assert_eq!(ws.active_tab(), Some(1));
        assert_eq!(
            ws.active_layout().unwrap().pane_ids().len(),
            1,
            "a new tab is a fresh single-pane tree"
        );
    }

    #[test]
    fn each_tab_keeps_its_own_split_tree_restored_on_switch() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));

        ws.active_layout_mut().unwrap().split(Orient::Vertical);
        assert_eq!(ws.active_layout().unwrap().pane_ids().len(), 2);

        ws.add_tab();
        assert_eq!(
            ws.active_layout().unwrap().pane_ids().len(),
            1,
            "the new tab does not inherit the first tab's splits"
        );

        assert!(ws.set_active_tab(0));
        assert_eq!(
            ws.active_layout().unwrap().pane_ids().len(),
            2,
            "switching back restores the first tab's tree"
        );
    }

    #[test]
    fn set_active_tab_rejects_out_of_bounds() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        assert!(ws.set_active_tab(0));
        assert!(!ws.set_active_tab(2));
        assert_eq!(ws.active_tab(), Some(0));
    }

    #[test]
    fn auto_name_falls_back_then_sticks_across_idle() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        let tab = ws.tab_id(0, 0).unwrap();
        assert_eq!(ws.tab_titles(), Some(vec!["Tab 1".to_string()]));

        ws.refresh_auto_name(tab, Some("claude"));
        assert_eq!(ws.tab_titles(), Some(vec!["claude".to_string()]));

        // Back at an idle prompt (no candidate): the last activity name sticks.
        ws.refresh_auto_name(tab, None);
        assert_eq!(ws.tab_titles(), Some(vec!["claude".to_string()]));

        // A new activity replaces it.
        ws.refresh_auto_name(tab, Some("cargo"));
        assert_eq!(ws.tab_titles(), Some(vec!["cargo".to_string()]));
    }

    #[test]
    fn user_rename_wins_over_auto_name_and_clearing_reverts_to_it() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        let tab = ws.tab_id(0, 0).unwrap();
        ws.refresh_auto_name(tab, Some("vim"));

        assert!(ws.rename_tab(0, "notes"));
        assert_eq!(ws.tab_titles(), Some(vec!["notes".to_string()]));

        // Clearing the rename falls back to the activity-derived name, not "Tab N".
        assert!(ws.rename_tab(0, "   "));
        assert_eq!(ws.tab_titles(), Some(vec!["vim".to_string()]));
    }

    #[test]
    fn tab_focus_resolves_by_id_and_rejects_stale() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        let tab = ws.tab_id(0, 0).unwrap();
        assert!(ws.tab_focus(tab).is_some());
        assert!(ws.tab_focus(TabId(9_999)).is_none());
    }

    #[test]
    fn close_tab_before_active_shifts_active_tab() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();
        assert_eq!(ws.active_tab(), Some(2));

        assert!(ws.close_tab(0));
        assert_eq!(ws.tab_count(), Some(2));
        assert_eq!(ws.active_tab(), Some(1));
    }

    #[test]
    fn close_tab_after_active_keeps_active_tab() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();
        assert!(ws.set_active_tab(0));

        assert!(ws.close_tab(2));
        assert_eq!(ws.tab_count(), Some(2));
        assert_eq!(ws.active_tab(), Some(0));
    }

    #[test]
    fn reorder_tab_moves_a_tab_and_keeps_the_active_one() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();
        let id0 = ws.tab_id(0, 0).unwrap();
        let id2 = ws.tab_id(0, 2).unwrap();
        assert_eq!(ws.active_tab(), Some(2));

        // Drop tab 0 after tab 2 (the end): order becomes [1, 2, 0].
        assert!(ws.reorder_tab(0, 2, true));
        assert_eq!(ws.tab_id(0, 2).unwrap(), id0, "tab 0 moved to the end");
        assert_eq!(ws.tab_id(0, 1).unwrap(), id2);
        assert_eq!(
            ws.active_tab(),
            Some(1),
            "the active tab follows its identity to its new slot"
        );
    }

    #[test]
    fn reorder_tab_onto_its_own_edge_is_a_no_op() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();

        assert!(!ws.reorder_tab(1, 1, false), "dropped on its own left edge");
        assert!(!ws.reorder_tab(1, 1, true), "dropped on its own right edge");
        assert!(!ws.reorder_tab(0, 3, true), "anchor out of bounds");
        assert_eq!(ws.active_tab(), Some(2));
    }

    #[test]
    fn close_active_tab_falls_back_to_neighbor() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();
        assert!(ws.set_active_tab(1));

        assert!(ws.close_tab(1));
        assert_eq!(ws.tab_count(), Some(2));
        assert_eq!(
            ws.active_tab(),
            Some(1),
            "closing a middle tab keeps the index pointing at the next tab"
        );

        assert!(ws.set_active_tab(1));
        assert!(ws.close_tab(1));
        assert_eq!(ws.tab_count(), Some(1));
        assert_eq!(
            ws.active_tab(),
            Some(0),
            "closing the last tab in the list shifts the active index back"
        );
    }

    #[test]
    fn closing_the_last_tab_leaves_a_fresh_tab() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.active_layout_mut().unwrap().split(Orient::Vertical);
        assert_eq!(ws.active_layout().unwrap().pane_ids().len(), 2);

        assert!(ws.close_tab(0));
        assert_eq!(ws.tab_count(), Some(1), "the repo never has zero tabs");
        assert_eq!(ws.active_tab(), Some(0));
        assert_eq!(
            ws.active_layout().unwrap().pane_ids().len(),
            1,
            "the remaining tab is a fresh single-pane tree"
        );
    }

    #[test]
    fn close_tab_out_of_bounds_is_false() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        assert!(!ws.close_tab(9));
        assert_eq!(ws.tab_count(), Some(1));
    }

    #[test]
    fn tab_titles_default_to_tab_n() {
        let mut ws = Workspace::new();
        assert_eq!(ws.tab_titles(), None, "no active repo, no titles");
        ws.add(repo("a"));
        ws.add_tab();
        assert_eq!(ws.tab_titles().unwrap(), vec!["Tab 1", "Tab 2"]);
    }

    #[test]
    fn rename_tab_overrides_the_default_title() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();

        assert!(ws.rename_tab(0, "  build  "));
        assert_eq!(
            ws.tab_titles().unwrap(),
            vec!["build", "Tab 2"],
            "the custom name is trimmed, the other tab keeps its default"
        );
    }

    #[test]
    fn renaming_to_empty_restores_the_default_title() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        assert!(ws.rename_tab(0, "build"));

        assert!(ws.rename_tab(0, "   "));
        assert_eq!(ws.tab_titles().unwrap(), vec!["Tab 1"]);
    }

    #[test]
    fn rename_tab_out_of_bounds_is_false() {
        let mut ws = Workspace::new();
        assert!(!ws.rename_tab(0, "x"), "no active repo");
        ws.add(repo("a"));
        assert!(!ws.rename_tab(9, "x"));
        assert_eq!(ws.tab_titles().unwrap(), vec!["Tab 1"]);
    }

    #[test]
    fn closing_a_tab_shifts_the_default_titles_but_keeps_custom_names() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add_tab();
        ws.add_tab();
        ws.rename_tab(1, "build");

        assert!(ws.close_tab(0));
        assert_eq!(
            ws.tab_titles().unwrap(),
            vec!["build", "Tab 2"],
            "the custom name follows its tab; defaults renumber by position"
        );
    }

    #[test]
    fn closing_the_sole_tab_drops_its_custom_name() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.rename_tab(0, "build");

        assert!(ws.close_tab(0));
        assert_eq!(
            ws.tab_titles().unwrap(),
            vec!["Tab 1"],
            "the replacement tab is fresh, like its pane tree"
        );
    }

    #[test]
    fn tabs_are_scoped_per_repo() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));

        ws.set_active(0);
        ws.add_tab();
        ws.add_tab();
        assert_eq!(ws.tab_count(), Some(3));

        ws.set_active(1);
        assert_eq!(
            ws.tab_count(),
            Some(1),
            "the other repo keeps its single tab"
        );

        ws.set_active(0);
        assert_eq!(ws.tab_count(), Some(3));
        assert_eq!(ws.active_tab(), Some(2));
    }

    #[test]
    fn switch_a_to_b_to_a_restores_the_whole_tab_set_of_a() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));

        ws.set_active(0);
        ws.add_tab();
        ws.add_tab();
        ws.active_layout_mut().unwrap().split(Orient::Vertical);
        ws.set_active_tab(1);
        let a_tab_count = ws.tab_count().unwrap();
        let a_active_tab = ws.active_tab().unwrap();
        let a_tab2_panes = {
            ws.set_active_tab(2);
            let n = ws.active_layout().unwrap().pane_ids().len();
            ws.set_active_tab(1);
            n
        };

        assert!(ws.set_active(1));
        assert_eq!(ws.tab_count(), Some(1), "b keeps its lone tab");

        assert!(ws.set_active(0));
        assert_eq!(
            ws.tab_count(),
            Some(a_tab_count),
            "switching back restores a's tab count"
        );
        assert_eq!(
            ws.active_tab(),
            Some(a_active_tab),
            "switching back restores a's active tab"
        );
        ws.set_active_tab(2);
        assert_eq!(
            ws.active_layout().unwrap().pane_ids().len(),
            a_tab2_panes,
            "the split tree of a's third tab survives the round trip"
        );
    }

    #[test]
    fn add_group_appends_root_then_children_in_given_order() {
        let mut ws = Workspace::new();
        ws.add(repo("standalone"));
        let root_idx = ws.add_group(repo("proj"), vec![repo("fix-bug"), repo("Feature-x")]);

        assert_eq!(root_idx, 1);
        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["standalone", "proj", "fix-bug", "Feature-x"],
            "children keep the given (persisted) order, no alpha re-sort"
        );

        assert_eq!(ws.parent_root(0), None);
        assert_eq!(ws.parent_root(1), None);
        assert_eq!(ws.parent_root(2), Some(Path::new("/tmp/proj")));
        assert_eq!(ws.parent_root(3), Some(Path::new("/tmp/proj")));
        assert!(ws.is_group_root(1));
        assert!(!ws.is_group_root(0), "a standalone repo has no children");
        assert!(!ws.is_group_root(2), "a child is not a root");
    }

    #[test]
    fn sync_group_appends_a_discovered_child_after_the_survivors() {
        let mut ws = Workspace::new();
        ws.add_group(repo("proj"), vec![repo("fix-bug")]);
        ws.add(repo("standalone"));
        ws.set_active(1);
        ws.add_tab();
        assert_eq!(ws.tab_count(), Some(2));

        let sync = ws
            .sync_group(Path::new("/tmp/proj"), vec![repo("fix-bug"), repo("alpha")])
            .unwrap();

        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["proj", "fix-bug", "alpha", "standalone"],
            "the survivor keeps its place; the discovered worktree is appended"
        );
        assert_eq!(sync.mapping, vec![Some(0), Some(1), Some(3)]);
        assert_eq!(
            ws.active(),
            Some(1),
            "the active child keeps following its entry"
        );
        assert_eq!(
            ws.tab_count(),
            Some(2),
            "the surviving child keeps its tab set"
        );
    }

    #[test]
    fn sync_group_preserves_a_manual_child_order_across_a_sync() {
        let mut ws = Workspace::new();
        // A manual order that is *not* alphabetical.
        ws.add_group(repo("proj"), vec![repo("zeta"), repo("alpha")]);

        ws.sync_group(Path::new("/tmp/proj"), vec![repo("alpha"), repo("zeta")])
            .unwrap();

        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["proj", "zeta", "alpha"],
            "a disk re-enumeration never re-sorts the user's manual order"
        );
    }

    #[test]
    fn reorder_moves_a_standalone_project_down() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        ws.add(repo("c"));
        ws.set_active(0);

        // Drop "a" after "b".
        assert!(ws.reorder(0, 1, true));
        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a", "c"]);
        assert_eq!(
            ws.active_repo().unwrap().name,
            "a",
            "active follows its repo"
        );
    }

    #[test]
    fn reorder_moves_a_whole_group_as_a_block() {
        let mut ws = Workspace::new();
        ws.add(repo("standalone"));
        ws.add_group(repo("proj"), vec![repo("wt1"), repo("wt2")]);
        ws.set_active(3); // wt2

        // Drag the group root "proj" before "standalone".
        assert!(ws.reorder(1, 0, false));
        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["proj", "wt1", "wt2", "standalone"],
            "the root carries its worktree children"
        );
        assert_eq!(ws.parent_root(1), Some(Path::new("/tmp/proj")));
        assert_eq!(ws.parent_root(2), Some(Path::new("/tmp/proj")));
        assert_eq!(ws.active_repo().unwrap().name, "wt2");
    }

    #[test]
    fn reorder_moves_a_worktree_within_its_group() {
        let mut ws = Workspace::new();
        ws.add_group(repo("proj"), vec![repo("wt1"), repo("wt2")]);
        ws.set_active(1); // wt1

        // Drop wt2 (index 2) before wt1 (index 1).
        assert!(ws.reorder(2, 1, false));
        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["proj", "wt2", "wt1"]);
        assert_eq!(ws.active_repo().unwrap().name, "wt1", "active follows wt1");
        assert!(ws.parent_root(1).is_some() && ws.parent_root(2).is_some());
    }

    #[test]
    fn group_offset_is_stable_across_a_worktree_reorder() {
        let mut ws = Workspace::new();
        ws.add_group(repo("proj"), vec![repo("wt1"), repo("wt2")]);
        let root = Path::new("/tmp/proj");
        let wt1 = Path::new("/tmp/wt1");
        let wt2 = Path::new("/tmp/wt2");
        assert_eq!(ws.group_offset(root, root), 0);
        assert_eq!(ws.group_offset(root, wt1), 1);
        assert_eq!(ws.group_offset(root, wt2), 2);

        // Drag wt2 ahead of wt1: the sidebar row order flips, the ports must not.
        assert!(ws.reorder(2, 1, false));
        assert_eq!(ws.group_offset(root, root), 0);
        assert_eq!(
            ws.group_offset(root, wt1),
            1,
            "wt1 keeps its port after reorder"
        );
        assert_eq!(
            ws.group_offset(root, wt2),
            2,
            "wt2 keeps its port after reorder"
        );
    }

    #[test]
    fn reorder_rejects_moving_a_worktree_into_another_group() {
        let mut ws = Workspace::new();
        ws.add_group(repo("p1"), vec![repo("w1")]);
        ws.add_group(repo("p2"), vec![repo("w2")]);
        // w1 (idx1) dropped onto w2 (idx3) of the other group: rejected.
        assert!(!ws.reorder(1, 3, false));
        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["p1", "w1", "p2", "w2"], "nothing moved");
    }

    #[test]
    fn reorder_is_a_noop_when_dropped_on_its_own_edge() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        // "a" dropped before "a" (itself) — no move.
        assert!(!ws.reorder(0, 0, false));
        // "a" dropped after "a" — still its own slot.
        assert!(!ws.reorder(0, 0, true));
    }

    #[test]
    fn resolve_reorder_snaps_a_project_drop_to_the_group_boundary() {
        // [standalone0(0), root(1), child(2), child(3), standalone4(4)]
        let child = [false, false, true, true, false];
        // Drag standalone0 onto a child of the group, after it: the project lands
        // after the *whole* group (index 4), not between its worktrees.
        assert_eq!(resolve_reorder(&child, 0, 2, true), Some((0, 1, 4)));
        // Drag standalone4 onto a child, before it: snaps to the group's top
        // boundary (index 1), so it lands just before the root.
        assert_eq!(resolve_reorder(&child, 4, 2, false), Some((4, 5, 1)));
    }

    #[test]
    fn sync_group_removes_gone_child_without_touching_others() {
        let mut ws = Workspace::new();
        ws.add_group(repo("proj"), vec![repo("alpha"), repo("fix-bug")]);
        ws.add(repo("standalone"));
        ws.set_active(3);
        ws.add_tab();

        let sync = ws
            .sync_group(Path::new("/tmp/proj"), vec![repo("fix-bug")])
            .unwrap();

        let names: Vec<&str> = ws.repos().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["proj", "fix-bug", "standalone"]);
        assert_eq!(sync.mapping, vec![Some(0), None, Some(1), Some(2)]);
        assert_eq!(ws.active(), Some(2), "the active standalone repo follows");
        assert_eq!(ws.tab_count(), Some(2), "its tab set is untouched");
    }

    #[test]
    fn sync_group_active_removed_child_falls_back_to_root() {
        let mut ws = Workspace::new();
        ws.add(repo("standalone"));
        ws.add_group(repo("proj"), vec![repo("alpha")]);
        ws.set_active(2);

        ws.sync_group(Path::new("/tmp/proj"), vec![]).unwrap();

        assert_eq!(ws.active(), Some(1), "active falls back to the group root");
        assert_eq!(ws.active_repo().unwrap().name, "proj");
        assert!(!ws.is_group_root(1), "the root has no children left");
    }

    #[test]
    fn sync_group_unknown_root_is_none() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        assert!(ws
            .sync_group(Path::new("/tmp/nope"), vec![repo("x")])
            .is_none());
        assert_eq!(ws.len(), 1);
    }

    #[test]
    fn sync_group_does_not_match_a_child_as_root() {
        let mut ws = Workspace::new();
        ws.add_group(repo("proj"), vec![repo("alpha")]);

        assert!(
            ws.sync_group(Path::new("/tmp/alpha"), vec![]).is_none(),
            "a worktree child is not a group root"
        );
    }

    #[test]
    fn remove_drops_the_whole_tab_set_of_the_repo() {
        let mut ws = Workspace::new();
        ws.add(repo("a"));
        ws.add(repo("b"));
        ws.set_active(1);
        ws.add_tab();
        ws.add_tab();
        assert_eq!(ws.tab_count(), Some(3));

        ws.remove(0);
        assert_eq!(ws.active_repo().unwrap().name, "b");
        assert_eq!(
            ws.tab_count(),
            Some(3),
            "b's tab set is untouched by removing a"
        );

        ws.remove(0);
        assert!(ws.is_empty(), "removing the last repo drops its tabs too");
        assert_eq!(ws.tab_count(), None);
    }
}
