use crate::git::branch::valid_branch_name;
use crate::git::graph::{Edge, Graph, GraphCommit, GraphRef, GraphRow, LaneCache, RefKind};
use crate::git::tag::valid_tag_name;
use crate::theme::{medium_family, Palette, BODY_SIZE, RADIUS_PILL};
use crate::ui::repo_sidebar::DeleteModalAction;
use crate::ui::spinner::Spinner;
use crate::ui::{arrow_nav_pressed, paint_icon, with_alpha, ArrowNav};

const ROW_HEIGHT: f32 = 28.0;
const LANE_WIDTH: f32 = 22.0;
const NODE_RADIUS: f32 = 11.0;
const MERGE_NODE_RADIUS: f32 = 5.0;
const INITIALS_SIZE: f32 = 9.0;
/// Archive icon for the stash node (same Lucide glyph as the Stash button).
const STASH_ICON_SIZE: f32 = 13.0;
/// Author bubble border, in the lane's full color; the fill is the same color
/// darkened (`NODE_FILL_DARKEN`).
const NODE_BORDER_WIDTH: f32 = 2.0;
const NODE_FILL_DARKEN: f32 = 0.45;
const RING_GAP: f32 = 2.0;
const RING_WIDTH: f32 = 1.5;
const EDGE_WIDTH: f32 = 2.0;
/// Rounded 90° corner of a lane transition (orthogonal routing):
/// clamped to the available room when tighter.
const EDGE_CORNER_RADIUS: f32 = 8.0;
/// Quarter-arc sampling of a transition corner — smooth at 8 px.
const CORNER_ARC_STEPS: usize = 8;
/// Dash pattern of the WIP node, shared between its circle and its link to HEAD.
const DASH_LEN: f32 = 2.5;
const DASH_GAP: f32 = 2.0;
/// Padding before the center of the first lane: must cover the author bubble
/// and its selection ring, otherwise lane 0 is clipped on the left of the graph
/// zone (`lane_clip_rect`).
const LANE_LEFT_PAD: f32 = NODE_RADIUS + RING_GAP + RING_WIDTH;
const TEXT_GAP: f32 = 10.0;
const HASH_SIZE: f32 = 12.0;
const TEXT_SIZE: f32 = 13.0;
const COL_GAP: f32 = 12.0;
/// Lane-colored accent bar at the head of the message column (M10-6).
const ACCENT_BAR_WIDTH: f32 = 3.0;
const ACCENT_BAR_MARGIN_Y: f32 = 5.0;
const ACCENT_BAR_GAP: f32 = 7.0;
const BODY_GAP: f32 = 8.0;
/// Minimum width to show the dimmed message body after the summary.
const BODY_MIN_WIDTH: f32 = 40.0;
const LOAD_MORE_GAP: f32 = 8.0;
/// BRANCH / TAG column dedicated to ref chips (M10-5).
const REFS_COL_WIDTH: f32 = 245.0;
const REFS_LEFT_PAD: f32 = 8.0;
/// Horizontal leader linking a ref chip to its commit node; thicker on the
/// checked-out HEAD chip to single it out (in place of the HEAD chip's white ring).
const LABEL_LINK_WIDTH: f32 = 1.5;
const LABEL_LINK_HEAD_WIDTH: f32 = 3.0;
/// Minimum width reserved for the graph zone (stabilizes the message column and
/// leaves room for the GRAPH header even on a linear history).
const MIN_GRAPH_ZONE: f32 = 80.0;
/// Default cap on the graph zone (~16 lanes): on a wide history, the excess lanes
/// are clipped so the message column stays readable — the resize handle lets you
/// show more.
const GRAPH_ZONE_DEFAULT_MAX: f32 = LANE_LEFT_PAD + 16.0 * LANE_WIDTH;
/// Drag handle for the graph ⇄ message boundary (same sizing as terminal splits).
const RESIZE_HANDLE: f32 = 6.0;
const HEADER_HEIGHT: f32 = 24.0;
const HEADER_SIZE: f32 = 10.0;
/// Ref chips: a single one per row, the rest folded into `+N` (expanded on hover).
const CHIP_MAX: usize = 1;
const CHIP_HEIGHT: f32 = 22.0;
const CHIP_TEXT_SIZE: f32 = 12.0;
const CHIP_PAD_X: f32 = 6.0;
const CHIP_GAP: f32 = 4.0;
const GRAPH_MENU_MIN_WIDTH: f32 = 220.0;
const GRAPH_MENU_MAX_WIDTH: f32 = 360.0;
const CHIP_GLYPH: f32 = 12.0;
const CHIP_GLYPH_GAP: f32 = 4.0;
/// Rectangular with slightly rounded corners, not a pill.
const CHIP_RADIUS: u8 = 4;
/// Ink (text + glyphs) of non-checked-out chips: dimmed `lane_ink` — the current
/// branch chip (✓) keeps crisp full-opacity ink so it clearly stands out, the
/// others recede. Kept readable on every lane fill (git.md §9: "slightly dimmed").
const CHIP_DIM_ALPHA: u8 = 180;
/// Graph loading spinner (a11y label, no visible text).
const SPINNER_SIZE: f32 = 22.0;
const LOADING_LABEL: &str = "Loading graph";
/// Inline error of the Branch editor (git.md §10), named after what the field
/// creates — the two names follow different git rules.
const INVALID_NAME: &str = "Invalid branch name";
const INVALID_TAG_NAME: &str = "Invalid tag name";
const ERROR_SIZE: f32 = 12.0;

/// Graph search box (⌘F, git.md §9): floating field at the top-right of the
/// graph, cycling through the matching commits.
const SEARCH_BOX_WIDTH: f32 = 320.0;
const SEARCH_BOX_MARGIN: f32 = 8.0;
const SEARCH_FIELD_WIDTH: f32 = 170.0;
const SEARCH_BTN: f32 = 22.0;
const SEARCH_GLYPH: f32 = 14.0;
const SEARCH_COUNTER_SIZE: f32 = 11.0;
const SEARCH_HINT: &str = "Search commits";
/// Match highlight (amber, distinct from the blue selection): a faint fill on
/// every match, a stronger fill + outline on the current one (the one scrolled
/// into view by the cycle).
const SEARCH_MATCH_COLOR: egui::Color32 = egui::Color32::from_rgb(224, 168, 38);
const SEARCH_MATCH_ALPHA: u8 = 34;
const SEARCH_CURRENT_ALPHA: u8 = 72;
const SEARCH_CURRENT_STROKE: f32 = 1.5;

/// Signals emitted by the graph within a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct GraphAction {
    /// Clicked commit (selection intent arbitrated by the caller).
    pub selected: Option<git2::Oid>,
    /// Click on **Load more**: the caller grows the page and reloads (explicit
    /// pagination, never silent truncation — git.md §9, M9-8).
    pub load_more: bool,
    /// Click on the WIP row (M10-7): the caller switches the right sidebar back
    /// to the status sections (Unstaged/Staged/Commit) instead of the commit detail.
    pub wip_selected: bool,
    /// Double-click on a **branch** chip — non-checked-out local, or remote (DWIM
    /// on the domain side): checkout intent (automatic stash if the tree is dirty),
    /// arbitrated by the caller.
    pub checkout: Option<String>,
    /// The auto-scroll-to-HEAD request (`scroll_to_head`) was consumed (rows
    /// rendered): the caller clears its one-shot flag.
    pub scrolled_to_head: bool,
    /// `Enter` in the Branch editor with a valid name (git.md §10): creation
    /// **on HEAD** + checkout, arbitrated by the caller.
    pub create_branch: Option<String>,
    /// **Create worktree** entry of a branch context menu: already filtered by
    /// the graph's domain data, and revalidated by the caller before writing.
    pub create_worktree: Option<String>,
    /// **Create branch** entry of a chip's context menu (git.md §9): the caller
    /// opens the inline editor on the targeted ref's row (carrying the source
    /// committish), the same field as the toolbar Branch button.
    pub open_branch_editor: Option<CreateBranchRequest>,
    /// `Enter` in a **chip-targeted** Branch editor with a valid name (git.md §9):
    /// creation of a local branch **at the editor's source** (held in
    /// [`BranchEditor::target`]) **without** checkout, arbitrated by the caller.
    pub create_branch_at: Option<String>,
    /// **Create tag** entry of a commit row's menu (git.md §9): the caller opens
    /// the inline editor (tag mode) anchored on that commit's row — its `Enter`
    /// creates a lightweight tag there. Carries the commit to tag.
    pub open_tag_editor: Option<git2::Oid>,
    /// `Enter` in a **tag** editor with a valid name (git.md §9): a lightweight
    /// tag is created on the editor's commit ([`BranchEditor::target`]), no
    /// checkout and no push, arbitrated by the caller.
    pub create_tag_at: Option<String>,
    /// **Rebase onto …** entry of a branch context menu: rebase of the current
    /// branch onto the targeted ref, run by the caller on the sync runner (one
    /// op at a time, spinner + toasts — git.md §9).
    pub rebase_onto: Option<String>,
    /// **Interactive rebase onto …** entry: the caller opens the rebase page
    /// (plan per commit) targeting this ref — nothing runs before its Start
    /// (git.md §9).
    pub interactive_rebase_onto: Option<String>,
    /// **AI rebase onto …** entry: the caller opens the recap modal (commits +
    /// extra AI instructions) targeting this ref — nothing runs before its
    /// Start (git.md §9).
    pub ai_rebase_onto: Option<String>,
    /// **Merge … into …** entry of a branch context menu: merge of the targeted
    /// ref into the current branch, run by the caller on the sync runner (one
    /// op at a time, spinner + toasts — git.md §9).
    pub merge: Option<String>,
    /// **Create pull request into …** entry of a branch context menu (git.md §9):
    /// the targeted ref's branch name = the PR destination, the current branch =
    /// its source; the caller opens the forge's prefilled create-PR page in the
    /// browser. Offered only when `origin` is a recognized cloud forge.
    pub create_pull_request: Option<String>,
    /// **Delete …** entry of the context menu: deletion requested (local, remote,
    /// or both), to be confirmed (modal) by the caller before execution.
    pub delete: Option<DeleteBranchTarget>,
    /// **Checkout** entry of a tag's menu (git.md §9): detached checkout on the
    /// tag's commit (automatic stash if the tree is dirty), arbitrated by the
    /// caller. Menu-only — a double-click on a tag stays ignored.
    pub checkout_tag: Option<String>,
    /// **Push tag** entry of a tag's menu (git.md §9): `git push origin <tag>` on
    /// the sync runner (one op at a time, spinner + toasts), run by the caller.
    pub push_tag: Option<String>,
    /// **Delete tag** entry of a tag's menu (git.md §9): deletion requested, to be
    /// confirmed (modal, with an optional "Also delete on origin") by the caller
    /// before execution.
    pub delete_tag: Option<String>,
    /// **Apply stash** entry of a stash row's context menu: apply without drop,
    /// executed immediately (the stash stays either way — the no-drop twin of
    /// [`Self::stash_pop`]). Only the status is refreshed (the stash list is
    /// unchanged), no graph reload.
    pub stash_apply: Option<git2::Oid>,
    /// **Pop stash** entry of a stash row's context menu: apply then drop,
    /// executed immediately (a conflict keeps the stash on the domain side).
    pub stash_pop: Option<git2::Oid>,
    /// **Delete stash** entry of a stash row's context menu: to be confirmed
    /// (modal) by the caller before execution — a dropped stash is unrecoverable.
    pub stash_drop: Option<StashTarget>,
    /// **Cherry-pick** entry of a row's commit menu (git.md §9): replay the commit
    /// on the current branch (`git cherry-pick <sha>`, sync runner), run by the
    /// caller. Absent on a merge commit or a detached HEAD.
    pub cherry_pick: Option<git2::Oid>,
    /// **Revert** entry of a row's commit menu (git.md §9): commit the inverse
    /// (`git revert --no-edit <sha>`, sync runner), run by the caller. Same
    /// eligibility as [`Self::cherry_pick`].
    pub revert: Option<git2::Oid>,
    /// **Reset `<current>` to here** entry of a row's commit menu (git.md §9):
    /// the target commit and the chosen git mode. Soft/Mixed are run directly by
    /// the caller (`git2` reset on the worker); Hard is gated behind a modal.
    /// Absent on a detached HEAD (no branch to move).
    pub reset: Option<(git2::Oid, git2::ResetType)>,
    /// **Rename** entry of a chip's menu (git.md §9): the caller opens the inline
    /// editor on the branch's row **pre-filled** with the current name (held in
    /// [`RenameRequest`]) — the field renames on `Enter`. Local branches only.
    pub open_rename_editor: Option<RenameRequest>,
    /// `Enter` in a **rename** editor with a valid name (git.md §9): the current
    /// branch name and the new one (`from`, `to`), renamed on the worker by the
    /// caller (`git branch -m` semantics); duplicate/invalid ⇒ inline error.
    pub rename_branch: Option<(String, String)>,
}

/// Frame inputs of [`graph_view`] (read-only data computed by the caller, same
/// convention as `ToolbarState`); the two mutable collaborators — lane cache,
/// Branch editor — stay separate parameters.
pub struct GraphViewState<'a> {
    pub graph: Option<&'a Graph>,
    pub wip: Option<WipRow>,
    pub selected: Option<git2::Oid>,
    pub scroll_to_head: bool,
    pub keyboard_nav: bool,
    /// `origin` is a recognized cloud forge (git.md §9): enables the **Create
    /// pull request** entry on the branch chips' context menu.
    pub can_pull_request: bool,
}

/// Stash targeted from its row's context menu, identified by its **stash
/// commit** (indices shift, the worker re-resolves at execution); the summary
/// names the stash in the confirmation modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashTarget {
    pub oid: git2::Oid,
    pub summary: String,
}

/// Target of a branch deletion requested from the context menu, held by the
/// caller for the duration of the confirmation modal. `Local` carries the local
/// branch name, `Remote` the full remote name (`origin/<name>`, resolved on the
/// domain side), `Both` carries both (combined menu entry when the branch exists
/// on both sides).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteBranchTarget {
    Local(String),
    Remote(String),
    Both { local: String, remote: String },
}

/// Source of a **Create branch** request (chip context menu, git.md §9), carried
/// from the clicked ref to the inline editor it opens. `oid` is the ref's commit
/// (the row the field anchors on); `source` is the fully-qualified committish the
/// branch is created at (`refs/heads|remotes|tags/…`). The field opens **empty**
/// (a pre-filled ref name would read like a rename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateBranchRequest {
    pub oid: git2::Oid,
    pub source: String,
}

/// Source a chip-opened Branch editor creates from (git.md §9): `oid` is the
/// ref's commit (the row the field anchors on), `source` the fully-qualified
/// committish the branch is created at — **without** checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEditorTarget {
    pub oid: git2::Oid,
    pub source: String,
}

/// **Rename branch** request (chip context menu, git.md §9): `oid` is the
/// branch's commit (the row the inline editor anchors on), `name` the current
/// branch name the field opens **pre-filled** with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameRequest {
    pub oid: git2::Oid,
    pub name: String,
}

/// Inline Branch editor (git.md §9–10), owned by the caller. Without a `target`
/// (toolbar's **Branch** button) it is rendered on the **HEAD row** in the
/// BRANCH / TAG column — where the new branch's chip will appear — and creates on
/// HEAD then checks out. With a `target` (a chip's **Create branch** entry) it is
/// rendered on that ref's row and creates the branch **at the ref's commit
/// without** checkout. Either way it stays open while awaiting the worker's
/// response (`pending`), which writes the inline error there (duplicate) or
/// closes it on success.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BranchEditor {
    pub open: bool,
    pub name: String,
    pub error: Option<String>,
    /// `Enter` emitted: creation in progress on the worker, response awaited.
    pub pending: bool,
    /// One-shot focus placed on the field at opening (the toolbar builds the open
    /// state via `..Default::default()`).
    pub(crate) focused: bool,
    /// Set when opened from a chip's **Create branch** entry: the ref's commit
    /// (anchor row) + source committish (created at, no checkout). `None` ⇒ the
    /// toolbar flow (created on HEAD + checkout).
    pub target: Option<BranchEditorTarget>,
    /// **Tag mode** (git.md §9): the field creates a lightweight tag on
    /// `target.oid` instead of a branch — same inline field, "Tag name" hint.
    pub tag: bool,
    /// **Rename mode** (git.md §9): the current branch name the field opened
    /// **pre-filled** with. `Enter` renames that branch to the edited name
    /// instead of creating; `target.oid` still anchors the field on its row.
    pub rename: Option<String>,
}

/// In-graph commit search (⌘F, git.md §9), owned by the caller: a floating box
/// at the top-right of the graph filters the **loaded** commits (summary, hash,
/// author, body, ref names) and cycles through the matches. `current` is the
/// cursor into the match list (wraps); `focused` is the one-shot focus placed on
/// the field when it opens.
#[derive(Debug, Clone, Default)]
pub struct GraphSearch {
    pub open: bool,
    pub query: String,
    pub(crate) current: usize,
    pub(crate) focused: bool,
}

/// Outcome of one frame of the search box, consumed by [`graph_view`] to scroll
/// and highlight the current match.
#[derive(Default)]
struct SearchOut {
    /// Commit index of the current match (highlighted, kept visible).
    focus: Option<usize>,
    /// Scroll the current match into view this frame: the box just opened, the
    /// query was edited, or a cycle (Enter / chevrons) moved the cursor.
    scroll: bool,
}

/// Intent emitted by a row's chips: immediate checkout (double-click) or opening
/// the context menu (right-click).
#[derive(Default)]
struct ChipIntent {
    checkout: Option<String>,
    menu: Option<ChipMenu>,
    /// Area covered by the expanded overlay when it was painted this frame — kept
    /// so that hovering it keeps the expansion (the stacked chips overflow below
    /// the row, outside the refs zone).
    expanded: Option<egui::Rect>,
    /// Right edge of the inline chips/`+N` badge: anchor for the leader linking the
    /// label to its node (`None` when no inline chip is painted).
    content_right: Option<f32>,
}

/// Context menu of the graph (right-click on a **branch** chip — tags have none
/// — or anywhere on a row): ready-to-render sections and anchor rect. Held in
/// egui memory for the duration it is open.
#[derive(Clone)]
struct ChipMenu {
    /// Built once at opening: [`branch_sections`] (chip or row),
    /// [`stash_sections`] for a stash row.
    sections: Vec<MenuSection>,
    /// Clicked chip (the menu anchors **below** it, never on top), or a
    /// zero-size rect at the pointer for a row menu.
    anchor: egui::Rect,
    /// Menu opened from the expanded overlay: (row oid, index of the targeted
    /// chip) — the row keeps its chips expanded **up to this one** while the menu
    /// is open (the label does not vanish under the fold); the following chips
    /// fold back, the menu takes their place.
    expanded: Option<(git2::Oid, usize)>,
}

fn chip_menu_id() -> egui::Id {
    egui::Id::new("graph_chip_menu")
}

/// Closes the chip menu from outside the graph: its sections name the refs of the
/// repo it was opened on, while the state itself lives in egui memory — a repo
/// switch would leave it open, aiming its entries at the new repo (git.md §9).
pub fn close_chip_menu(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<ChipMenu>(chip_menu_id()));
}

/// Ordering bucket of a context-menu section: [`chip_menu`] renders the sections
/// grouped by bucket, in this declaration order, with a separator between
/// buckets — related actions stay together and the destructive ones land last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenuGroup {
    /// Checkout / Create worktree / Create branch — and a stash's Apply / Pop.
    Refs,
    /// Cherry-pick / Revert / Reset / Rebase (×3) / Merge — history rewrites.
    History,
    /// Create tag / Push tag.
    Tag,
    /// Rename.
    Rename,
    /// Copy commit SHA / branch name / tag name.
    Copy,
    /// Delete branch / tag / stash — destructive, always last.
    Delete,
}

/// Entry group for one action of the context menu: untitled ⇒ entries rendered
/// inline (lone branch, stash row); titled ⇒ one submenu (several branches on
/// the row).
#[derive(Clone, Debug, PartialEq)]
struct MenuSection {
    /// Ordering/separator bucket (see [`MenuGroup`]).
    group: MenuGroup,
    /// Submenu label when `Some` (e.g. the per-action branch submenus, or
    /// `Reset <current> to here`); flat inline entries when `None`. Owned because
    /// some titles embed a runtime name (the checked-out branch).
    title: Option<String>,
    entries: Vec<MenuEntry>,
}

/// One activatable entry of the context menu: a click applies the intent and
/// closes the menu.
#[derive(Clone, Debug, PartialEq)]
struct MenuEntry {
    label: String,
    intent: MenuIntent,
}

/// Intent carried by a menu entry, applied to the frame's [`GraphAction`] when
/// activated. A new menu action plugs in here: one variant, its arm in
/// [`MenuIntent::apply`], its entries in [`commit_sections`] / [`branch_sections`] /
/// [`stash_sections`] — the rendering ([`chip_menu`]) never changes.
#[derive(Clone, Debug, PartialEq)]
enum MenuIntent {
    Checkout(String),
    CopyCommitSha(git2::Oid),
    CreateTag(git2::Oid),
    CherryPick(git2::Oid),
    Revert(git2::Oid),
    Reset(git2::Oid, git2::ResetType),
    CopyBranchName(String),
    CreateWorktree(String),
    CreateBranch(CreateBranchRequest),
    Rename(RenameRequest),
    RebaseOnto(String),
    InteractiveRebaseOnto(String),
    AiRebaseOnto(String),
    Merge(String),
    CreatePullRequest(String),
    Delete(DeleteBranchTarget),
    CheckoutTag(String),
    CopyTagName(String),
    PushTag(String),
    DeleteTag(String),
    StashApply(git2::Oid),
    StashPop(git2::Oid),
    StashDrop(StashTarget),
}

impl MenuIntent {
    fn apply(&self, ctx: &egui::Context, action: &mut GraphAction) {
        match self {
            MenuIntent::Checkout(branch) => action.checkout = Some(branch.clone()),
            MenuIntent::CopyCommitSha(oid) => ctx.copy_text(oid.to_string()),
            MenuIntent::CreateTag(oid) => action.open_tag_editor = Some(*oid),
            MenuIntent::CherryPick(oid) => action.cherry_pick = Some(*oid),
            MenuIntent::Revert(oid) => action.revert = Some(*oid),
            MenuIntent::Reset(oid, mode) => action.reset = Some((*oid, *mode)),
            MenuIntent::CopyBranchName(branch) => ctx.copy_text(branch.clone()),
            MenuIntent::CreateWorktree(branch) => action.create_worktree = Some(branch.clone()),
            MenuIntent::CreateBranch(req) => action.open_branch_editor = Some(req.clone()),
            MenuIntent::Rename(req) => action.open_rename_editor = Some(req.clone()),
            MenuIntent::RebaseOnto(branch) => action.rebase_onto = Some(branch.clone()),
            MenuIntent::InteractiveRebaseOnto(branch) => {
                action.interactive_rebase_onto = Some(branch.clone())
            }
            MenuIntent::AiRebaseOnto(branch) => action.ai_rebase_onto = Some(branch.clone()),
            MenuIntent::Merge(branch) => action.merge = Some(branch.clone()),
            MenuIntent::CreatePullRequest(dest) => action.create_pull_request = Some(dest.clone()),
            MenuIntent::Delete(target) => action.delete = Some(target.clone()),
            MenuIntent::CheckoutTag(tag) => action.checkout_tag = Some(tag.clone()),
            MenuIntent::CopyTagName(tag) => ctx.copy_text(tag.clone()),
            MenuIntent::PushTag(tag) => action.push_tag = Some(tag.clone()),
            MenuIntent::DeleteTag(tag) => action.delete_tag = Some(tag.clone()),
            MenuIntent::StashApply(oid) => action.stash_apply = Some(*oid),
            MenuIntent::StashPop(oid) => action.stash_pop = Some(*oid),
            MenuIntent::StashDrop(target) => action.stash_drop = Some(target.clone()),
        }
    }
}

/// Entries offered for one ref of the context menu (a branch, or a tag — which
/// keeps only **Create branch**).
#[derive(Clone)]
struct MenuBranch {
    branch: String,
    /// This ref is a **tag** (git.md §9): drives the tag-only entries — Checkout
    /// (detached, intent `CheckoutTag`), Copy tag name, Push tag, Delete tag —
    /// and selects the Checkout intent (a branch checks out, a tag detaches).
    is_tag: bool,
    checkout: bool,
    create_worktree: bool,
    /// Rebase target eligibility — [`rebase_onto_target`]: any branch ref but
    /// the checked-out one (rebasing a branch onto itself is a no-op). Drives
    /// the three rebase entries **and** the Merge entry (same exclusions:
    /// merging a branch into itself is a no-op too).
    rebase_onto: bool,
    /// **Create branch** source — [`create_branch_target`]: any ref (branch or
    /// tag), `None` only for `origin/HEAD` (remote symref). Creates a local
    /// branch at the ref's commit without checkout.
    create_branch: Option<CreateBranchRequest>,
    /// **Rename** source — [`rename_target`]: local branches only, current
    /// included; `None` for remotes, tags and the detached `HEAD` marker. Opens
    /// the inline editor pre-filled on the branch's row.
    rename: Option<RenameRequest>,
    /// Whether **Copy branch name** applies — branches only (a tag has no
    /// branch name to copy).
    copy: bool,
    /// **Create pull request** destination — [`pull_request_target`]: the branch
    /// name on the remote (remote chip's `<remote>/` prefix stripped), `None` for
    /// the current branch, tags and `origin/HEAD`. The whole section is also
    /// gated on `origin` being a recognized forge (`can_pr`, caller-side).
    pull_request: Option<String>,
    /// Name of the deletable **local** branch — [`delete_local_target`]. Shown
    /// as-is in the `Delete <name>` entry.
    delete_local: Option<String>,
    /// Deletable **full remote** name (`origin/<name>`) — [`delete_remote_target`].
    /// Shown as-is in `Delete <name>`; both present ⇒ third combined entry
    /// `Delete <a> and <b>`.
    delete_remote: Option<String>,
}

/// Menu entries of one ref: checkout/rebase eligibility, Create branch source,
/// named deletions. `oid` is the ref's commit — the row the Create branch editor
/// will anchor on.
fn menu_branch(gref: &GraphRef, oid: git2::Oid) -> MenuBranch {
    let is_tag = gref.kind == RefKind::Tag;
    MenuBranch {
        branch: gref.name.clone(),
        is_tag,
        // A tag is always checkout-eligible (detached, menu-only); a branch only
        // when it is not the checked-out one (`checkout_target`).
        checkout: is_tag || checkout_target(gref).is_some(),
        create_worktree: gref.worktree_available,
        rebase_onto: rebase_onto_target(gref).is_some(),
        create_branch: create_branch_target(gref, oid),
        rename: rename_target(gref).map(|name| RenameRequest {
            oid,
            name: name.to_owned(),
        }),
        copy: gref.kind != RefKind::Tag,
        delete_local: delete_local_target(gref),
        delete_remote: delete_remote_target(gref),
        pull_request: pull_request_target(gref),
    }
}

/// **Create pull request** destination of a chip (git.md §9): any branch ref but
/// the checked-out one and `origin/HEAD` — the same eligibility as Rebase/Merge
/// (a PR into the current branch makes no sense, a tag is not a branch). The
/// value is the branch name **on the remote**: a remote chip's `<remote>/`
/// prefix is stripped (Bitbucket/GitHub name the destination by its branch).
fn pull_request_target(gref: &GraphRef) -> Option<String> {
    rebase_onto_target(gref).map(|name| match gref.kind {
        RefKind::Remote => remote_branch_part(name).to_owned(),
        _ => name.to_owned(),
    })
}

/// Leading WIP row of the graph (M10-7): dirty working tree ⇒ `Some` with the
/// number of touched files; clean tree ⇒ `None` (no row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WipRow {
    pub files: usize,
    pub selected: bool,
}

/// Graph row targeted by ↑/↓ keyboard navigation (keybindings §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavRow {
    Wip,
    Commit(usize),
}

/// Next row for ↑/↓ in the graph — **without wrapping**: the history is paginated
/// (Load more), wrapping "at the bottom" would jump onto an incomplete page.
/// Without a selection, both ↑ and ↓ take the first row (WIP if present).
fn next_row(
    current: Option<NavRow>,
    nav: ArrowNav,
    has_wip: bool,
    commits: usize,
) -> Option<NavRow> {
    match (current, nav) {
        (None, _) => (commits > 0 || has_wip).then_some(if has_wip {
            NavRow::Wip
        } else {
            NavRow::Commit(0)
        }),
        (Some(NavRow::Wip), ArrowNav::Down) => (commits > 0).then_some(NavRow::Commit(0)),
        (Some(NavRow::Wip), ArrowNav::Up) => None,
        (Some(NavRow::Commit(0)), ArrowNav::Up) => has_wip.then_some(NavRow::Wip),
        (Some(NavRow::Commit(i)), ArrowNav::Up) => Some(NavRow::Commit(i - 1)),
        (Some(NavRow::Commit(i)), ArrowNav::Down) => {
            (i + 1 < commits).then_some(NavRow::Commit(i + 1))
        }
    }
}

/// Renders the commit graph in the central zone (design-system §4, git.md §9,
/// M9-5, M10-5). Read-only, three columns under a header row
/// **BRANCH / TAG · GRAPH · COMMIT MESSAGE**: typed ref chips (per-type glyph, ✓
/// checked-out, `+N` fold), lanes/edges/node, then short hash (mono) + summary
/// (no author/date column: initials in the node, detail in the sidebar). The row
/// of the `selected` commit is highlighted (`accent.subtle`). Pure rendering: a
/// click on a row returns the `Oid` of the clicked commit (selection intent
/// arbitrated by the caller).
///
/// The graph column is **capped by default** (`GRAPH_ZONE_DEFAULT_MAX`, excess
/// lanes clipped — the message column stays readable on wide histories) and
/// **resizable** by dragging the graph ⇄ message boundary (setting kept in egui
/// memory for the session).
///
/// Edge cases (M9-8, git.md §8–9): graph **not yet received** (`graph` is
/// `None`, repo switch or first entry into Graph mode) ⇒ minimalist spinner —
/// **No commits** is reserved for a genuinely empty repo (unborn `HEAD`); beyond
/// the first page (`graph.has_more`) ⇒ **Load more** button (explicit
/// pagination). Scrollable for long histories.
///
/// Dirty working tree (`wip` set, M10-7) ⇒ leading row `// WIP · N file(s)`
/// (dashed node, no hash and no author), linked to the HEAD commit by a dashed
/// line on a **dedicated lane** (`assign_lanes_with_wip`: the WIP row enters the
/// lane computation, the other branches shift over — the link is never covered);
/// a click emits `wip_selected` (the sidebar switches back to the status
/// sections).
///
/// `scroll_to_head` (one-shot, armed by the caller when Graph mode opens):
/// scrolls to the `HEAD` commit's row (centered) as soon as the rows are
/// rendered, then signals consumption via `scrolled_to_head`.
///
/// `keyboard_nav` (keybindings §3): unmodified ↑/↓ move the selection row by row
/// (WIP included, without wrapping) and scroll the target into the viewport —
/// inactive as soon as a widget holds keyboard focus (commit field in WIP mode)
/// or when the caller arbitrates the arrows elsewhere (commit diff open, status
/// sidebar file nav armed).
///
/// Context menus (git.md §9): right-click on a chip targets that ref; right-click
/// anywhere else on a row targets **all** the row's refs — flat entries for a
/// single ref, one submenu per action (Checkout, Create branch, Rebase onto, …)
/// when several share the row (a tag carries only **Create branch**; no ref ⇒ no
/// menu); a **stash row** offers Apply stash / Pop stash / Delete stash instead
/// (deletion confirmed by a modal on the caller side). A hovered expanded-chips overlay
/// owns the right-click: the rows it covers and their inline chips stay inert.
///
/// Branch editor (`editor`, git.md §9–10): without a `target`, opened by the
/// toolbar's **Branch** button on the HEAD row **in place of its chips** — where
/// the new branch's chip will appear — `Enter` (valid name) emits `create_branch`
/// (created on HEAD + checkout). With a `target` (a chip's **Create branch**
/// entry), the field is placed on that ref's row and `Enter` emits
/// `create_branch_at` (created at the ref's commit, no checkout). A missing anchor
/// row — possible only on a **stale** graph (cache from a switch, reload in
/// flight) — leaves no anchor and the editor closes.
///
/// Search (`search`, git.md §9): ⌘F opens a floating box at the top-right of the
/// graph. It filters the **loaded** commits (summary, hash, author, body, ref
/// names) and cycles through the matches — Enter / chevrons move the cursor
/// (`Shift+Enter` backward), each match scrolled into view (centered) and
/// highlighted (amber, distinct from the blue selection); `Esc` / ✕ closes.
pub fn graph_view(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &GraphViewState<'_>,
    lanes: &mut LaneCache,
    editor: &mut BranchEditor,
    search: &mut GraphSearch,
) -> GraphAction {
    let GraphViewState {
        graph,
        wip,
        selected,
        scroll_to_head,
        keyboard_nav,
        can_pull_request,
    } = *state;
    let Some(graph) = graph else {
        return loading_placeholder(ui, palette);
    };
    if graph.commits.is_empty() {
        return placeholder(ui, palette, "No commits");
    }
    // Branch editor with no anchor row in the page: stale graph (cache from a
    // switch, reload in flight — a fresh page always contains HEAD, git.md §9) or
    // a targeted ref whose commit fell off the loaded page. No anchor for the
    // field, so it closes instead of staying open and invisible.
    let anchor_present = match &editor.target {
        Some(target) => graph.commits.iter().any(|c| c.oid == target.oid),
        None => graph
            .commits
            .iter()
            .any(|c| c.refs.iter().any(|r| r.is_head)),
    };
    if editor.open && !anchor_present {
        *editor = BranchEditor::default();
    }

    // Checked-out branch, named ("into …") by the menu's Merge entries: the
    // ref carrying `is_head` — except the synthetic "HEAD" entry of a detached
    // head (not a merge target; git refuses "HEAD" as a branch name, no clash).
    let head_branch = graph
        .commits
        .iter()
        .flat_map(|c| &c.refs)
        .find(|r| r.is_head)
        .filter(|r| r.name != "HEAD")
        .map(|r| r.name.as_str());

    // WIP → HEAD link: with a dirty tree and the checked-out commit in the page,
    // a virtual row enters the lane computation and reserves a dedicated lane —
    // the other branches shift over, the dashed line is never covered. HEAD beyond
    // the page ⇒ no link (lone node).
    let wip_parent = wip.and_then(|_| {
        graph
            .commits
            .iter()
            .find(|c| c.refs.iter().any(|r| r.is_head))
            .map(|c| c.oid)
    });
    // Memoized lanes (M10-8): recomputed only when the topology (or the WIP link)
    // changes.
    let (wip_row, rows) = lanes.rows(&graph.commits, wip_parent);
    let lane_count = rows.iter().map(|r| r.lane + 1).max().unwrap_or(1).max(
        rows.iter()
            .flat_map(|r| r.edges.iter().map(|e| e.to_lane.max(e.from_lane) + 1))
            .max()
            .unwrap_or(1),
    );
    // Graph zone width: capped by default, adjustable via the handle — the setting
    // lives in egui memory (session) and is re-clamped every frame.
    let user_zone_id = egui::Id::new("graph_zone_user_width");
    let user_zone: Option<f32> = ui.ctx().data(|d| d.get_temp(user_zone_id));
    let graph_zone = graph_zone_width(lane_count, user_zone);

    let view_rect = ui.available_rect_before_wrap();
    column_headers(ui, palette, graph_zone);

    // ⌘F (git.md §9): open the search box (re-focus + jump if already open). The
    // key is consumed so the field, shown after, does not also receive an 'f'.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
        search.open = true;
        search.focused = false;
    }
    let matches = if search.open {
        matching_commits(&graph.commits, &search.query)
    } else {
        Vec::new()
    };
    let search_out = if search.open {
        graph_search_box(ui, palette, view_rect, search, &matches)
    } else {
        SearchOut::default()
    };
    // The box may have closed itself this frame (Esc / ✕): a closed search
    // highlights nothing and forces no scroll.
    let search_active = search.open;
    let search_focus = search_active.then_some(search_out.focus).flatten();
    let search_scroll = search_active && search_out.scroll;

    let mut out = RowsOut::default();
    let menu_id = chip_menu_id();
    // Hover frozen while the context menu is open: the expanded-chips overlay
    // (Tooltip layer, painted after the Areas of the same tier) would otherwise
    // pass back over the menu as soon as the pointer descends to reach it. The
    // only exception (`menu_pin`): the row of a menu opened from the overlay keeps
    // its chips expanded up to the targeted label — it does not vanish, and the
    // chips that would overlap the menu (below it) are folded back.
    let open_menu: Option<ChipMenu> = ui.ctx().data(|d| d.get_temp(menu_id));
    let menu_open = open_menu.is_some();
    let menu_pin = open_menu.and_then(|m| m.expanded);
    // Area of the expanded overlay from the previous frame: the chips folded
    // behind `+N` stack **below** the row, outside the refs zone — as long as the
    // pointer stays in this area, the row stays expanded (and it alone), otherwise
    // the overlay would fold back before the 2nd chip becomes reachable.
    let chips_zone_id = egui::Id::new("graph_chips_expanded_zone");
    let hover_lock = ui
        .ctx()
        .data(|d| d.get_temp::<(git2::Oid, egui::Rect)>(chips_zone_id))
        .filter(|(_, zone)| ui.rect_contains_pointer(*zone))
        .map(|(oid, _)| oid);
    let nav_target = resolve_nav_target(ui, keyboard_nav, wip, selected, graph);
    match nav_target {
        Some(NavRow::Wip) => out.action.wip_selected = true,
        Some(NavRow::Commit(i)) => out.action.selected = Some(graph.commits[i].oid),
        None => {}
    }
    // Signaled on every frame rendered with the request armed; the caller
    // consumes the one-shot only if the rendered graph was **fresh** — on a stale
    // graph (cache from a switch), the scroll targeted the old HEAD's row and must
    // replay on the fresh graph.
    out.action.scrolled_to_head = scroll_to_head;
    // Virtualized rows: a long history (Load more pages stack up) must not allocate
    // every row each frame — hit-testing thousands of click widgets is what makes
    // the scroll stutter. Rows are fixed-height (`ROW_HEIGHT`); the WIP row, when
    // present, is virtual row 0 and commit `i` is virtual row `lead + i`.
    let lead = wip.is_some() as usize;
    let total_rows = lead + graph.commits.len();
    // Rows force-rendered even off-screen: the scroll target (so `scroll_to_rect`
    // can latch onto it) and the editor's anchor row while it is open (the field
    // keeps its widget and focus during a scroll, git.md §9–10). The anchor is the
    // HEAD row (scroll target / toolbar editor) or the editor's targeted ref row.
    let head_pos = || {
        graph
            .commits
            .iter()
            .position(|c| c.refs.iter().any(|r| r.is_head))
    };
    let scroll_vrow = scroll_to_head.then(head_pos).flatten().map(|i| lead + i);
    let editor_vrow = editor
        .open
        .then(|| match &editor.target {
            Some(target) => graph.commits.iter().position(|c| c.oid == target.oid),
            None => head_pos(),
        })
        .flatten()
        .map(|i| lead + i);
    let nav_vrow = match nav_target {
        Some(NavRow::Wip) => Some(0),
        Some(NavRow::Commit(i)) => Some(lead + i),
        None => None,
    };
    // The current search match, only while a scroll to it is pending (otherwise
    // it is rendered lazily like any row, highlighted when on-screen).
    let search_vrow = search_scroll
        .then(|| search_focus.map(|i| lead + i))
        .flatten();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            // Full virtual height: the scrollbar spans the whole history, with a
            // row's worth of room below for the Load more button when present.
            let extra = if graph.has_more {
                LOAD_MORE_GAP + ROW_HEIGHT
            } else {
                0.0
            };
            ui.set_height(total_rows as f32 * ROW_HEIGHT + extra);

            let max_row = ((viewport.max.y / ROW_HEIGHT).ceil() as usize + 1).min(total_rows);
            let first = (viewport.min.y / ROW_HEIGHT).floor() as usize;
            let mut range = first.saturating_sub(1).min(max_row)..max_row;
            for forced in [scroll_vrow, editor_vrow, nav_vrow, search_vrow]
                .into_iter()
                .flatten()
            {
                range.start = range.start.min(forced);
                range.end = range.end.max((forced + 1).min(total_rows));
            }
            let at_tail = range.end == total_rows;

            let width = ui.available_width();
            let left = ui.max_rect().left();
            let top = ui.max_rect().top();
            let rect = egui::Rect::from_min_max(
                egui::pos2(left, top + range.start as f32 * ROW_HEIGHT),
                egui::pos2(
                    left + width,
                    top + range.end as f32 * ROW_HEIGHT + if at_tail { extra } else { 0.0 },
                ),
            );
            let ctx = RowsCtx {
                rows,
                wip_row,
                head: head_branch,
                can_pr: can_pull_request,
                graph_zone,
                scroll_to_head,
                nav_target,
                selected,
                menu_open,
                menu_pin,
                hover_lock,
                width,
                search_matches: if search_active {
                    matches.as_slice()
                } else {
                    &[]
                },
                search_focus,
                search_scroll,
            };
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                // Contiguous rows: lane continuity is painted row by row (top →
                // bottom of the rect); the theme's vertical item_spacing would open
                // an unpainted gap between two rows (dashed lines).
                ui.spacing_mut().item_spacing.y = 0.0;
                // Stable auto-ids per virtual row regardless of the scroll offset:
                // each row allocates exactly one click widget, so row `n` keeps
                // id `n` (hover/click stay latched while scrolling).
                ui.skip_ahead_auto_ids(range.start);
                for vrow in range.clone() {
                    match wip.filter(|_| vrow == 0) {
                        Some(wip) => wip_row_ui(
                            ui,
                            palette,
                            wip,
                            wip_row.map(|r| r.lane),
                            graph_zone,
                            nav_target == Some(NavRow::Wip),
                            &mut out.action,
                        ),
                        None => commit_row_ui(
                            ui,
                            palette,
                            &ctx,
                            vrow - lead,
                            &graph.commits[vrow - lead],
                            editor,
                            &mut out,
                        ),
                    }
                }
                // Explicit pagination: beyond the loaded page, **Load more** rather
                // than silent truncation (git.md §9, M9-8) — only when the tail row
                // is within the rendered range.
                if graph.has_more && at_tail {
                    ui.add_space(LOAD_MORE_GAP);
                    ui.vertical_centered(|ui| {
                        if ui
                            .button(egui::RichText::new("Load more").color(palette.text_secondary))
                            .clicked()
                        {
                            out.action.load_more = true;
                        }
                    });
                }
            });
        });
    // This frame's expanded overlay becomes the next frame's hover zone; without
    // an expansion, the retained zone is purged (a later pass of the pointer must
    // not re-expand a row without hovering its refs zone).
    ui.ctx().data_mut(|d| match out.expanded_now {
        Some(state) => {
            d.insert_temp(chips_zone_id, state);
        }
        None => d.remove::<(git2::Oid, egui::Rect)>(chips_zone_id),
    });
    resize_handle_ui(ui, palette, view_rect, lane_count, graph_zone, user_zone_id);
    // Chip context menu: the state (branch + chip rect) lives in egui memory for
    // the duration it is open — closed on an activated entry, a click elsewhere or
    // Esc.
    let opened_now = out.menu_request.is_some();
    if let Some(request) = out.menu_request {
        ui.ctx().data_mut(|d| d.insert_temp(menu_id, request));
    }
    if let Some(menu) = ui.ctx().data(|d| d.get_temp::<ChipMenu>(menu_id)) {
        if chip_menu(ui, &menu, opened_now, &mut out.action) {
            ui.ctx().data_mut(|d| d.remove::<ChipMenu>(menu_id));
        }
    }
    out.action
}

/// ↑/↓ target resolved before rendering the rows (keybindings §3): inactive as
/// soon as a widget holds keyboard focus; emission goes through the same
/// signals as a click, the scroll latches onto the rect of the targeted row.
fn resolve_nav_target(
    ui: &egui::Ui,
    keyboard_nav: bool,
    wip: Option<WipRow>,
    selected: Option<git2::Oid>,
    graph: &Graph,
) -> Option<NavRow> {
    (keyboard_nav && ui.memory(|m| m.focused().is_none()))
        .then(|| arrow_nav_pressed(ui))
        .flatten()
        .and_then(|nav| {
            let current = if wip.is_some_and(|w| w.selected) {
                Some(NavRow::Wip)
            } else {
                selected
                    .and_then(|oid| graph.commits.iter().position(|c| c.oid == oid))
                    .map(NavRow::Commit)
            };
            next_row(current, nav, wip.is_some(), graph.commits.len())
        })
}

/// Loop-invariant context of the commit rows: lanes, layout, and the frame's
/// nav/menu/hover state, read by every row.
struct RowsCtx<'a> {
    rows: &'a [GraphRow],
    wip_row: Option<&'a GraphRow>,
    /// Checked-out branch ("into …" of the Merge entries); `None` on a
    /// detached HEAD.
    head: Option<&'a str>,
    /// `origin` is a recognized cloud forge: enables the Create pull request entry.
    can_pr: bool,
    graph_zone: f32,
    scroll_to_head: bool,
    nav_target: Option<NavRow>,
    selected: Option<git2::Oid>,
    menu_open: bool,
    menu_pin: Option<(git2::Oid, usize)>,
    hover_lock: Option<git2::Oid>,
    width: f32,
    /// Commit indices matching the active search (sorted), for the row highlight.
    search_matches: &'a [usize],
    /// Commit index of the current match: stronger highlight + scrolled into view.
    search_focus: Option<usize>,
    /// Scroll the current match into view this frame (cycle / open / edited query).
    search_scroll: bool,
}

/// Signals accumulated by the rows ([`GraphAction`] plus the frame's menu
/// request and expanded-overlay zone, resolved after the scroll area).
#[derive(Default)]
struct RowsOut {
    action: GraphAction,
    menu_request: Option<ChipMenu>,
    expanded_now: Option<(git2::Oid, egui::Rect)>,
}

/// Leading WIP row (M10-7): allocation, click and nav scroll, culled paint,
/// a11y info. Rows deliberately outside `ui::clickable`: no pointer cursor on
/// graph rows, only the chips show it
/// (D-2026-06-04-curseur-pointeur-cliquables, revised).
fn wip_row_ui(
    ui: &mut egui::Ui,
    palette: &Palette,
    wip: WipRow,
    lane: Option<usize>,
    graph_zone: f32,
    nav_selected: bool,
    action: &mut GraphAction,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    let hovered = response.hovered();
    if nav_selected {
        ui.scroll_to_rect(rect, None);
    }
    if response.clicked() {
        action.wip_selected = true;
    }
    if ui.is_rect_visible(rect) {
        paint_wip_row(ui, palette, rect, hovered, wip, lane, graph_zone);
    }
    response.widget_info(move || {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            wip.selected,
            wip_label(wip.files),
        )
    });
}

/// One commit row of the graph: allocation, selection/nav/menu interactions,
/// culled paint, Branch editor anchor and a11y info.
fn commit_row_ui(
    ui: &mut egui::Ui,
    palette: &Palette,
    ctx: &RowsCtx<'_>,
    index: usize,
    commit: &GraphCommit,
    editor: &mut BranchEditor,
    out: &mut RowsOut,
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ctx.width, ROW_HEIGHT), egui::Sense::click());
    let hovered = response.hovered();
    if ctx.scroll_to_head && commit.refs.iter().any(|r| r.is_head) {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
    if ctx.nav_target == Some(NavRow::Commit(index)) {
        ui.scroll_to_rect(rect, None);
    }
    let is_current_match = ctx.search_focus == Some(index);
    if ctx.search_scroll && is_current_match {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
    let is_selected = ctx.selected == Some(commit.oid);
    // Same guard as the right-click below: while the pointer is over an expanded
    // chip overlay the click belongs to the overlay, never to the row it covers.
    if response.clicked() && ctx.hover_lock.is_none() {
        out.action.selected = Some(commit.oid);
    }
    // Branch editor open: the field is placed in place of the chips on its
    // anchor row — the targeted ref's commit (chip "Create branch"), else the
    // HEAD row (toolbar Branch button) (git.md §9–10).
    let editing = editor.open
        && match &editor.target {
            Some(target) => commit.oid == target.oid,
            None => commit.refs.iter().any(|r| r.is_head),
        };
    // Culling: each row paints strictly within its rect, so we skip
    // those outside the viewport (layout/galleys saved on long
    // histories); allocation and widget_info remain for a11y.
    if ui.is_rect_visible(rect) {
        // Number of expanded chips: pinned menu ⇒ up to the targeted
        // chip; otherwise hover (the row's refs zone, or a lock on the
        // already-expanded overlay) ⇒ all — frozen while the menu is
        // open.
        let refs_zone = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.left() + REFS_COL_WIDTH, rect.bottom()),
        );
        let expand_chips = if editing {
            None
        } else if ctx.menu_open {
            ctx.menu_pin
                .filter(|(oid, _)| *oid == commit.oid)
                .map(|(_, upto)| upto + 1)
        } else if !commit.refs.is_empty()
            && match ctx.hover_lock {
                Some(locked) => locked == commit.oid,
                None => ui.rect_contains_pointer(refs_zone),
            }
        {
            Some(commit.refs.len())
        } else {
            None
        };
        let chips = paint_row(
            ui,
            palette,
            &RowPaint {
                rect,
                hovered,
                selected: is_selected,
                commit,
                head: ctx.head,
                can_pr: ctx.can_pr,
                row: &ctx.rows[index],
                // The 1st row receives the continuity of the WIP row
                // (dashed link), the following ones that of the previous
                // row.
                prev_edges: index
                    .checked_sub(1)
                    .map(|i| ctx.rows[i].edges.as_slice())
                    .or_else(|| ctx.wip_row.map(|r| r.edges.as_slice())),
                next_lane: ctx.rows.get(index + 1).map(|r| r.lane),
                graph_zone: ctx.graph_zone,
                expand_chips,
                // Another row's expanded overlay covers this one: its
                // inline chips are painted but inert — the click belongs
                // to the overlay (chips hit-test by hand, without it the
                // last row processed would steal the menu).
                chips_occluded: ctx.hover_lock.is_some_and(|locked| locked != commit.oid),
                hide_chips: editing,
                search_match: ctx.search_matches.contains(&index),
                search_current: is_current_match,
            },
        );
        if let Some(zone) = chips.expanded {
            // Union with the refs zone: descending straight down from
            // the `+N` (to the right of narrower expanded chips) also
            // keeps the expansion.
            out.expanded_now = Some((commit.oid, zone.union(refs_zone)));
        }
        if chips.checkout.is_some() {
            out.action.checkout = chips.checkout;
        }
        if chips.menu.is_some() {
            out.menu_request = chips.menu;
        } else if response.secondary_clicked() && !editing && ctx.hover_lock.is_none() {
            // Right-click on the row outside the chips: same menu,
            // for every branch of the row — never while the pointer
            // is over an expanded overlay (the click is the
            // overlay's, even when it covers this row).
            if let Some(menu) = row_menu(
                commit,
                response.interact_pointer_pos(),
                ctx.head,
                ctx.can_pr,
            ) {
                out.menu_request = Some(menu);
            }
        }
    }
    // Outside the culling: the field keeps its widget (and focus)
    // even if the row leaves the viewport during a scroll.
    if editing {
        branch_editor_field(ui, palette, rect, editor, &mut out.action);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            is_selected,
            format!("{} {}", commit.short_id, commit.summary),
        )
    });
}

/// Graph column resize handle: horizontal drag on the graph ⇄ message
/// boundary, over the full height of the view. Registered after the rows to
/// win pointer arbitration on the boundary.
fn resize_handle_ui(
    ui: &mut egui::Ui,
    palette: &Palette,
    view_rect: egui::Rect,
    lane_count: usize,
    graph_zone: f32,
    user_zone_id: egui::Id,
) {
    let boundary_x = view_rect.left() + REFS_COL_WIDTH + graph_zone;
    let handle = egui::Rect::from_min_max(
        egui::pos2(boundary_x - RESIZE_HANDLE / 2.0, view_rect.top()),
        egui::pos2(boundary_x + RESIZE_HANDLE / 2.0, view_rect.bottom()),
    );
    let response = ui
        .interact(handle, user_zone_id.with("drag"), egui::Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged() {
        let width = graph_zone_width(lane_count, Some(graph_zone + response.drag_delta().x));
        ui.ctx().data_mut(|d| d.insert_temp(user_zone_id, width));
    }
    if response.hovered() || response.dragged() {
        ui.painter().line_segment(
            [
                egui::pos2(boundary_x, view_rect.top()),
                egui::pos2(boundary_x, view_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, palette.border_subtle),
        );
    }
}

fn configure_menu_ui(ui: &mut egui::Ui) {
    ui.set_width_range(GRAPH_MENU_MIN_WIDTH..=GRAPH_MENU_MAX_WIDTH);
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
}

/// Sections reordered into their [`MenuGroup`] buckets for rendering — stable
/// within a bucket, so the builders' intra-group order is kept. [`chip_menu`]
/// draws a separator wherever the bucket changes between two consecutive
/// sections.
fn grouped_sections(sections: &[MenuSection]) -> Vec<&MenuSection> {
    let mut ordered: Vec<&MenuSection> = sections.iter().collect();
    ordered.sort_by_key(|s| s.group);
    ordered
}

/// Context menu of the graph (chip or row): renders the prebuilt sections —
/// entries inline (untitled section: lone branch, stash row) or one submenu
/// per action (titled sections, several branches on the row). Anchored
/// **below** the clicked chip (`CHIP_GAP`, never overlapping the label; the
/// alignment flips above near the bottom edge) — or at the pointer for a row
/// menu. Returns `true` when the menu closes — activated entry, click outside
/// the menu (open submenus included) or Esc.
fn chip_menu(ui: &egui::Ui, menu: &ChipMenu, opened_now: bool, action: &mut GraphAction) -> bool {
    use egui::containers::menu::SubMenuButton;
    let mut acted = false;
    // Surface of the open submenu: a press there must not count as a click
    // outside the menu — it would close it before the entry's release.
    let mut sub_zone = egui::Rect::NOTHING;
    let response = egui::Popup::new(
        egui::Id::new("graph_chip_menu_popup"),
        ui.ctx().clone(),
        egui::PopupAnchor::ParentRect(menu.anchor),
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .gap(CHIP_GAP)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .style(crate::theme::menu_style)
    .show(|ui| {
        configure_menu_ui(ui);
        let mut prev: Option<MenuGroup> = None;
        for section in grouped_sections(&menu.sections) {
            if prev.is_some_and(|g| g != section.group) {
                ui.separator();
            }
            prev = Some(section.group);
            match &section.title {
                None => {
                    for entry in &section.entries {
                        entry_button(ui, entry, action, &mut acted);
                    }
                }
                Some(title) => {
                    let (_, sub) = SubMenuButton::new(title.as_str()).ui(ui, |ui| {
                        configure_menu_ui(ui);
                        for entry in &section.entries {
                            entry_button(ui, entry, action, &mut acted);
                        }
                    });
                    if let Some(sub) = sub {
                        sub_zone = sub_zone.union(sub.response.rect);
                    }
                }
            }
        }
    });
    let Some(inner) = response else {
        return true;
    };
    if acted || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return true;
    }
    let zone = inner.response.rect.union(sub_zone);
    !opened_now
        && ui.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !zone.contains(p))
        })
}

/// One activatable entry: a click applies the intent and closes the menu.
fn entry_button(ui: &mut egui::Ui, entry: &MenuEntry, action: &mut GraphAction, acted: &mut bool) {
    if ui
        .add(
            egui::Button::new(entry.label.as_str())
                .truncate()
                .min_size(egui::vec2(GRAPH_MENU_MIN_WIDTH, 0.0)),
        )
        .clicked()
    {
        entry.intent.apply(ui.ctx(), action);
        *acted = true;
    }
}

/// Sections of a branch menu, one per action — Checkout / Create worktree /
/// Rebase onto / Interactive rebase onto / AI rebase onto / Merge / Copy branch
/// name / Delete — the empty ones dropped (git.md §9).
/// A lone branch
/// keeps flat entries (untitled sections, labeled by the action); several nest
/// each action into a titled submenu, one entry per branch — the deletions and
/// the merges stay explicitly named either way. `head` is the checked-out
/// branch (the merge target named by the entry); `None` (detached HEAD, every
/// page contains HEAD) ⇒ no Merge entries.
/// **Commit actions** of a row's context menu (git.md §9): present on every
/// commit row even ref-less, before the ref actions. **Copy commit SHA** puts
/// the full hash on the clipboard, **Create tag** opens the tag editor.
/// **Cherry-pick** / **Revert** replay or invert the commit on the current
/// branch — offered only when on a branch (`head` is `Some`, absent on a
/// detached HEAD) and never on a merge commit (more than one parent, ambiguous
/// mainline). **Reset `<head>` to here** nests Soft/Mixed/Hard in its own
/// submenu — offered on a branch for any commit (a merge target is legitimate).
fn commit_sections(commit: &GraphCommit, head: Option<&str>) -> Vec<MenuSection> {
    // One entry per section: each commit action carries its own bucket, so the
    // copy lands with the other copies and the replays with the history rewrites
    // ([`grouped_sections`]) instead of all bundled at the top.
    let single = |group, label: &str, intent| MenuSection {
        group,
        title: None,
        entries: vec![MenuEntry {
            label: label.to_owned(),
            intent,
        }],
    };
    let mut sections = vec![
        single(
            MenuGroup::Copy,
            "Copy commit SHA",
            MenuIntent::CopyCommitSha(commit.oid),
        ),
        single(
            MenuGroup::Tag,
            "Create tag",
            MenuIntent::CreateTag(commit.oid),
        ),
    ];
    if head.is_some() && commit.parents.len() <= 1 {
        sections.push(single(
            MenuGroup::History,
            "Cherry-pick",
            MenuIntent::CherryPick(commit.oid),
        ));
        sections.push(single(
            MenuGroup::History,
            "Revert",
            MenuIntent::Revert(commit.oid),
        ));
    }
    if let Some(branch) = head {
        sections.push(MenuSection {
            group: MenuGroup::History,
            title: Some(format!("Reset {branch} to here")),
            entries: vec![
                MenuEntry {
                    label: "Soft".to_owned(),
                    intent: MenuIntent::Reset(commit.oid, git2::ResetType::Soft),
                },
                MenuEntry {
                    label: "Mixed".to_owned(),
                    intent: MenuIntent::Reset(commit.oid, git2::ResetType::Mixed),
                },
                MenuEntry {
                    label: "Hard".to_owned(),
                    intent: MenuIntent::Reset(commit.oid, git2::ResetType::Hard),
                },
            ],
        });
    }
    sections
}

fn branch_sections(branches: &[MenuBranch], head: Option<&str>, can_pr: bool) -> Vec<MenuSection> {
    let nested = branches.len() > 1;
    let title = |t: &str| nested.then(|| t.to_owned());
    let label = |action: &str, branch: &MenuBranch| {
        if nested {
            branch.branch.clone()
        } else {
            action.to_owned()
        }
    };
    let rebase_section = |action: &'static str, intent: fn(String) -> MenuIntent| MenuSection {
        group: MenuGroup::History,
        title: title(action),
        entries: branches
            .iter()
            .filter(|b| b.rebase_onto)
            .map(|b| MenuEntry {
                label: if nested {
                    b.branch.clone()
                } else {
                    format!("{action} {}", b.branch)
                },
                intent: intent(b.branch.clone()),
            })
            .collect(),
    };
    // Tag-only entry: `action` flat (the lone tag is obvious), the tag's name
    // nested (its own submenu, like the per-action submenus above).
    let tag_section = |group, action: &'static str, intent: fn(String) -> MenuIntent| MenuSection {
        group,
        title: title(action),
        entries: branches
            .iter()
            .filter(|b| b.is_tag)
            .map(|b| MenuEntry {
                label: label(action, b),
                intent: intent(b.branch.clone()),
            })
            .collect(),
    };
    let sections = [
        MenuSection {
            group: MenuGroup::Refs,
            title: title("Checkout"),
            entries: branches
                .iter()
                .filter(|b| b.checkout)
                .map(|b| MenuEntry {
                    label: label("Checkout", b),
                    // A tag detaches HEAD on its commit (`CheckoutTag`); a branch
                    // checks out normally — same Checkout submenu, distinct intent.
                    intent: if b.is_tag {
                        MenuIntent::CheckoutTag(b.branch.clone())
                    } else {
                        MenuIntent::Checkout(b.branch.clone())
                    },
                })
                .collect(),
        },
        MenuSection {
            group: MenuGroup::Refs,
            title: title("Create worktree"),
            entries: branches
                .iter()
                .filter(|b| b.create_worktree)
                .map(|b| MenuEntry {
                    label: label("Create worktree", b),
                    intent: MenuIntent::CreateWorktree(b.branch.clone()),
                })
                .collect(),
        },
        MenuSection {
            group: MenuGroup::Refs,
            title: title("Create branch"),
            entries: branches
                .iter()
                .filter_map(|b| {
                    b.create_branch.clone().map(|req| MenuEntry {
                        label: label("Create branch", b),
                        intent: MenuIntent::CreateBranch(req),
                    })
                })
                .collect(),
        },
        // The three rebase flavors share their eligibility (`rebase_onto`) and,
        // unlike `label`, their flat entries name the target (like Delete):
        // "Rebase onto" alone would not say which way the rebase goes.
        rebase_section("Rebase onto", MenuIntent::RebaseOnto),
        rebase_section("Interactive rebase onto", MenuIntent::InteractiveRebaseOnto),
        rebase_section("AI rebase onto", MenuIntent::AiRebaseOnto),
        // Same eligibility as the rebase flavors; the entries name both sides
        // (like Delete): "Merge" alone would not say which way the merge goes.
        MenuSection {
            group: MenuGroup::History,
            title: title("Merge"),
            entries: head
                .map(|head| {
                    branches
                        .iter()
                        .filter(|b| b.rebase_onto)
                        .map(|b| MenuEntry {
                            label: format!("Merge {} into {head}", b.branch),
                            intent: MenuIntent::Merge(b.branch.clone()),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        // Create pull request: the clicked ref is the destination, the current
        // branch the source — needs a forge (`can_pr`) and a current branch
        // (`head`, no source on a detached HEAD). The intent carries the
        // destination's remote branch name (`pull_request`); the flat label names
        // the clicked ref like Merge does.
        MenuSection {
            group: MenuGroup::Refs,
            title: title("Create pull request"),
            entries: if can_pr && head.is_some() {
                branches
                    .iter()
                    .filter_map(|b| {
                        b.pull_request.clone().map(|dest| MenuEntry {
                            label: if nested {
                                b.branch.clone()
                            } else {
                                format!("Create pull request into {}", b.branch)
                            },
                            intent: MenuIntent::CreatePullRequest(dest),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            },
        },
        MenuSection {
            group: MenuGroup::Rename,
            title: title("Rename"),
            entries: branches
                .iter()
                .filter_map(|b| {
                    b.rename.clone().map(|req| MenuEntry {
                        label: label("Rename", b),
                        intent: MenuIntent::Rename(req),
                    })
                })
                .collect(),
        },
        MenuSection {
            group: MenuGroup::Copy,
            title: title("Copy branch name"),
            entries: branches
                .iter()
                .filter(|b| b.copy)
                .map(|b| MenuEntry {
                    label: label("Copy branch name", b),
                    intent: MenuIntent::CopyBranchName(b.branch.clone()),
                })
                .collect(),
        },
        // Tag-only actions (git.md §9): the tag entries the chips' menu carries
        // besides Checkout / Create branch — flat for a lone tag, one entry per
        // tag in their own submenus when several refs share the row.
        tag_section(MenuGroup::Copy, "Copy tag name", MenuIntent::CopyTagName),
        tag_section(MenuGroup::Tag, "Push tag", MenuIntent::PushTag),
        MenuSection {
            group: MenuGroup::Delete,
            title: title("Delete"),
            entries: branches.iter().flat_map(delete_entries).collect(),
        },
        tag_section(MenuGroup::Delete, "Delete tag", MenuIntent::DeleteTag),
    ];
    sections
        .into_iter()
        .filter(|s| !s.entries.is_empty())
        .collect()
}

/// Deletions named explicitly (git.md §9): local, remote, then the combined
/// entry when the branch exists on both sides.
fn delete_entries(branch: &MenuBranch) -> Vec<MenuEntry> {
    let delete = |label: String, target: DeleteBranchTarget| MenuEntry {
        label,
        intent: MenuIntent::Delete(target),
    };
    let mut entries = Vec::new();
    if let Some(local) = &branch.delete_local {
        entries.push(delete(
            format!("Delete {local}"),
            DeleteBranchTarget::Local(local.clone()),
        ));
    }
    if let Some(remote) = &branch.delete_remote {
        entries.push(delete(
            format!("Delete {remote}"),
            DeleteBranchTarget::Remote(remote.clone()),
        ));
    }
    if let (Some(local), Some(remote)) = (&branch.delete_local, &branch.delete_remote) {
        entries.push(delete(
            format!("Delete {local} and {remote}"),
            DeleteBranchTarget::Both {
                local: local.clone(),
                remote: remote.clone(),
            },
        ));
    }
    entries
}

/// Menu of a stash row: **Apply stash** (no drop) and **Pop stash** (apply then
/// drop — a conflict keeps the stash, domain side) emitted immediately, then —
/// past a separator — the destructive **Delete stash**, which goes through the
/// caller's confirmation modal before anything is sent.
fn stash_sections(stash: &StashTarget) -> Vec<MenuSection> {
    vec![
        MenuSection {
            group: MenuGroup::Refs,
            title: None,
            entries: vec![
                MenuEntry {
                    label: "Apply stash".to_owned(),
                    intent: MenuIntent::StashApply(stash.oid),
                },
                MenuEntry {
                    label: "Pop stash".to_owned(),
                    intent: MenuIntent::StashPop(stash.oid),
                },
            ],
        },
        MenuSection {
            group: MenuGroup::Delete,
            title: None,
            entries: vec![MenuEntry {
                label: "Delete stash".to_owned(),
                intent: MenuIntent::StashDrop(stash.clone()),
            }],
        },
    ]
}

/// Context menu of a whole row (right-click outside the chips), anchored at the
/// pointer: the commit actions ([`commit_sections`], every commit row even
/// ref-less) then every ref of the row (each branch or tag with its own
/// actions). A **stash row** (never a commit) gets its own entries instead:
/// Pop / Delete.
fn row_menu(
    commit: &crate::git::graph::GraphCommit,
    pos: Option<egui::Pos2>,
    head: Option<&str>,
    can_pr: bool,
) -> Option<ChipMenu> {
    let pos = pos?;
    let anchor = egui::Rect::from_min_size(pos, egui::Vec2::ZERO);
    if commit.stash {
        return Some(ChipMenu {
            sections: stash_sections(&StashTarget {
                oid: commit.oid,
                summary: commit.summary.clone(),
            }),
            anchor,
            expanded: None,
        });
    }
    // Commit actions first (every commit row, even ref-less), then the ref
    // actions for whatever refs the row carries.
    let mut sections = commit_sections(commit, head);
    let branches: Vec<MenuBranch> = commit
        .refs
        .iter()
        .map(|r| menu_branch(r, commit.oid))
        .collect();
    sections.extend(branch_sections(&branches, head, can_pr));
    Some(ChipMenu {
        sections,
        anchor,
        expanded: None,
    })
}

/// Confirmation modal for a branch deletion (Delete entries of the context menu,
/// git.md §9): local — recoverable via the reflog — or remote (public action,
/// removed for everyone). Outcome in `out` (red button ⇒ confirm, Cancel/Esc ⇒
/// dismiss), arbitrated by the caller.
pub fn delete_branch_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    target: &DeleteBranchTarget,
    out: &mut DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("delete_branch_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            let (title, detail) = match target {
                DeleteBranchTarget::Local(name) => (
                    format!("Delete branch “{name}”?"),
                    "The local branch is deleted; its commits stay in the reflog.",
                ),
                DeleteBranchTarget::Remote(name) => (
                    format!("Delete “{name}” on the remote?"),
                    "The branch is removed from the remote for everyone.",
                ),
                DeleteBranchTarget::Both { local, remote } => (
                    format!("Delete “{local}” and “{remote}”?"),
                    "The local branch is deleted; the remote branch is removed for everyone.",
                ),
            };
            ui.label(egui::RichText::new(title).strong());
            ui.add_space(4.0);
            ui.label(egui::RichText::new(detail).color(palette.text_secondary));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(crate::ui::danger_button(palette, "Delete"))
                        .clicked()
                    {
                        out.confirm = true;
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                out.confirm = true;
            }
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Confirmation modal for a stash deletion (Delete stash entry of the stash
/// row's context menu): unlike a branch, a dropped stash leaves no ref behind —
/// unrecoverable from the UI. Same outcome contract as [`delete_branch_modal`].
pub fn delete_stash_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    target: &StashTarget,
    out: &mut DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("delete_stash_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            ui.label(egui::RichText::new(format!("Delete stash “{}”?", target.summary)).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("The stashed changes are discarded.")
                    .color(palette.text_secondary),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(crate::ui::danger_button(palette, "Delete"))
                        .clicked()
                    {
                        out.confirm = true;
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                out.confirm = true;
            }
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Confirmation modal for a tag deletion (Delete tag entry of the context menu,
/// git.md §9): names the tag, red **Delete** to confirm. When a remote exists, an
/// **"Also delete on origin"** checkbox (`also_remote`) lets the deletion reach
/// the remote too — the caller runs the network deletion first, then the local
/// one (busy ⇒ nothing happens). Same outcome contract as [`delete_branch_modal`].
pub fn delete_tag_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    tag: &str,
    has_remote: bool,
    also_remote: &mut bool,
    out: &mut DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("delete_tag_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(280.0);
            ui.label(egui::RichText::new(format!("Delete tag “{tag}”?")).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("The local tag is deleted.").color(palette.text_secondary),
            );
            // `refs/tags` is a local namespace: the graph cannot know whether the
            // tag also lives on origin, so the option is offered whenever a remote
            // exists — a remote-side miss simply surfaces git's error as a toast.
            if has_remote {
                ui.add_space(8.0);
                ui.checkbox(also_remote, "Also delete on origin");
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(crate::ui::danger_button(palette, "Delete"))
                        .clicked()
                    {
                        out.confirm = true;
                    }
                });
            });
            if crate::ui::modal_confirm_pressed(ui) {
                out.confirm = true;
            }
        });
    if modal.should_close() {
        out.dismiss = true;
    }
}

/// Field of the Branch editor (git.md §9–10), placed in place of the chips on its
/// anchor row — the HEAD row (toolbar) or the targeted ref's row (chip "Create
/// branch"). Focus on opening; `Enter` validates the name (`git2`) then emits the
/// intent — `create_branch` (HEAD + checkout) or `create_branch_at` (the editor's
/// source, no checkout). The editor stays open while awaiting the worker
/// (duplicate ⇒ inline error written by the caller, success ⇒ closed by the
/// caller); `Esc` or a click elsewhere cancels. The error is shown below the
/// field, on the tooltip layer (it overlaps the next row).
fn branch_editor_field(
    ui: &mut egui::Ui,
    palette: &Palette,
    row: egui::Rect,
    editor: &mut BranchEditor,
    action: &mut GraphAction,
) {
    let field = egui::Rect::from_min_size(
        egui::pos2(
            row.left() + REFS_LEFT_PAD,
            row.center().y - CHIP_HEIGHT / 2.0,
        ),
        egui::vec2(REFS_COL_WIDTH - REFS_LEFT_PAD - COL_GAP, CHIP_HEIGHT),
    );
    // `new_child`, never `put`: `put` reallocates the rect in the parent layout
    // and would push back the cursor of the rows column (cf. toolbar spinner).
    let mut field_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    // Same rounding as the chips it replaces (the theme sets RADIUS_PILL).
    let w = &mut field_ui.style_mut().visuals.widgets;
    for ws in [&mut w.inactive, &mut w.hovered, &mut w.active] {
        ws.corner_radius = egui::CornerRadius::same(CHIP_RADIUS);
    }
    let response = egui::TextEdit::singleline(&mut editor.name)
        .hint_text(if editor.tag {
            "Tag name"
        } else {
            "Branch name"
        })
        .font(egui::FontId::proportional(CHIP_TEXT_SIZE))
        .desired_width(f32::INFINITY)
        .show(&mut field_ui)
        .response;
    let opened_now = !editor.focused;
    if opened_now {
        editor.focused = true;
        response.request_focus();
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let name = editor.name.trim().to_owned();
        // Rename to the unchanged name is a no-op, not a duplicate: close the
        // field silently rather than surfacing git's "already exists".
        if editor.rename.as_deref() == Some(name.as_str()) {
            *editor = BranchEditor::default();
        } else if (editor.tag && valid_tag_name(&name)) || (!editor.tag && valid_branch_name(&name))
        {
            editor.error = None;
            editor.pending = true;
            // A tag editor tags its commit; a rename editor renames its branch; a
            // chip-targeted editor creates a branch at its source (no checkout);
            // the toolbar one creates on HEAD + checkout. The commit/source/old
            // name come from `editor.target` / `editor.rename`.
            if editor.tag {
                action.create_tag_at = Some(name);
            } else if let Some(from) = &editor.rename {
                action.rename_branch = Some((from.clone(), name));
            } else if editor.target.is_some() {
                action.create_branch_at = Some(name);
            } else {
                action.create_branch = Some(name);
            }
        } else {
            editor.error = Some(
                if editor.tag {
                    INVALID_TAG_NAME
                } else {
                    INVALID_NAME
                }
                .to_owned(),
            );
            response.request_focus();
        }
    }
    let mut zone = field;
    if let Some(error) = &editor.error {
        let area = egui::Area::new(egui::Id::new("graph_branch_editor_error"))
            .order(egui::Order::Tooltip)
            .fixed_pos(egui::pos2(field.left(), field.bottom() + CHIP_GAP))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(error)
                            .size(ERROR_SIZE)
                            .color(palette.git_deleted),
                    );
                });
            });
        zone = zone.union(area.response.rect);
    }
    // Never on the opening frame: the click on the Branch button (toolbar,
    // outside the field) just opened the editor, it must not close it.
    let dismissed = ui.input(|i| i.key_pressed(egui::Key::Escape))
        || (!opened_now
            && ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !zone.contains(p))
            }));
    if dismissed {
        *editor = BranchEditor::default();
    }
}

/// Commit indices (ascending) of the **loaded** commits matching `query`
/// (case-insensitive substring). Empty/blank query ⇒ no match. Pure: unit-tested
/// and reused for the row highlight and the box counter.
fn matching_commits(commits: &[GraphCommit], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    commits
        .iter()
        .enumerate()
        .filter(|(_, c)| commit_matches(c, &needle))
        .map(|(i, _)| i)
        .collect()
}

/// Whether a commit matches the (already lowercased) search needle: summary,
/// short hash, author, body or any ref name.
fn commit_matches(commit: &GraphCommit, needle: &str) -> bool {
    commit.summary.to_lowercase().contains(needle)
        || commit.short_id.to_lowercase().contains(needle)
        || commit.author.to_lowercase().contains(needle)
        || commit.body.to_lowercase().contains(needle)
        || commit
            .refs
            .iter()
            .any(|r| r.name.to_lowercase().contains(needle))
}

/// Wrapping cursor step over `count` matches (`delta` +1 next / -1 previous);
/// `count == 0` parks the cursor at 0.
fn cycle(current: usize, count: usize, delta: i64) -> usize {
    if count == 0 {
        return 0;
    }
    let n = count as i64;
    (((current as i64 + delta) % n + n) % n) as usize
}

/// Floating search box (⌘F, git.md §9), anchored at the top-right of the graph:
/// a text field, a `current/total` counter and prev/next chevrons. `Enter` /
/// next chevron cycles forward, `Shift+Enter` / prev chevron backward, `Esc` / ✕
/// closes. Returns the current match (to scroll to and highlight) and whether a
/// scroll is pending this frame.
fn graph_search_box(
    ui: &mut egui::Ui,
    palette: &Palette,
    view_rect: egui::Rect,
    search: &mut GraphSearch,
    matches: &[usize],
) -> SearchOut {
    let count = matches.len();
    // Cursor kept in range as the match set shrinks (graph reload, edited query).
    search.current = if count == 0 {
        0
    } else {
        search.current.min(count - 1)
    };
    let mut scroll = false;
    let pos = egui::pos2(
        (view_rect.right() - SEARCH_BOX_WIDTH - SEARCH_BOX_MARGIN).max(view_rect.left()),
        view_rect.top() + SEARCH_BOX_MARGIN,
    );
    egui::Area::new(egui::Id::new("graph_search_box"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(lucide_icons::Icon::Search.unicode().to_string())
                                .size(SEARCH_GLYPH)
                                .color(palette.text_muted),
                        )
                        .selectable(false),
                    );
                    let response = egui::TextEdit::singleline(&mut search.query)
                        .hint_text(SEARCH_HINT)
                        .desired_width(SEARCH_FIELD_WIDTH)
                        .show(ui)
                        .response;
                    if response.changed() {
                        // Incremental search jumps back to the first match.
                        search.current = 0;
                        scroll = true;
                    }
                    if !search.focused {
                        search.focused = true;
                        response.request_focus();
                        scroll = true;
                    }
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let delta = if ui.input(|i| i.modifiers.shift) {
                            -1
                        } else {
                            1
                        };
                        search.current = cycle(search.current, count, delta);
                        scroll = true;
                        // Enter surrenders a singleline's focus: keep it so the
                        // user can chain Enter to walk the matches.
                        response.request_focus();
                    }
                    let counter = if search.query.trim().is_empty() {
                        String::new()
                    } else {
                        let cur = if count == 0 { 0 } else { search.current + 1 };
                        format!("{cur}/{count}")
                    };
                    let counter_color = if count == 0 && !search.query.trim().is_empty() {
                        palette.git_deleted
                    } else {
                        palette.text_muted
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(counter)
                                .size(SEARCH_COUNTER_SIZE)
                                .color(counter_color),
                        )
                        .selectable(false),
                    );
                    if search_icon_button(
                        ui,
                        palette,
                        lucide_icons::Icon::ChevronUp,
                        count > 0,
                        "Previous match",
                    ) {
                        search.current = cycle(search.current, count, -1);
                        scroll = true;
                        response.request_focus();
                    }
                    if search_icon_button(
                        ui,
                        palette,
                        lucide_icons::Icon::ChevronDown,
                        count > 0,
                        "Next match",
                    ) {
                        search.current = cycle(search.current, count, 1);
                        scroll = true;
                        response.request_focus();
                    }
                    if search_icon_button(ui, palette, lucide_icons::Icon::X, true, "Close search")
                    {
                        search.open = false;
                    }
                });
            });
        });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        search.open = false;
    }
    SearchOut {
        focus: matches.get(search.current).copied(),
        scroll,
    }
}

/// Small icon button of the search box (chevrons, close): hover background +
/// Lucide glyph, dimmed when disabled. Returns `true` on click.
fn search_icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: lucide_icons::Icon,
    enabled: bool,
    label: &'static str,
) -> bool {
    let (rect, response, hovered) =
        crate::ui::clickable(ui, egui::vec2(SEARCH_BTN, SEARCH_BTN), enabled);
    let clicked = response.clicked();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, egui::CornerRadius::same(4), palette.bg_surface_hover);
    }
    let color = if enabled {
        palette.text_secondary
    } else {
        palette.state_disabled
    };
    paint_icon(painter, rect.center(), SEARCH_GLYPH, icon, color);
    clicked
}

/// Graph zone width: the natural width of the lanes, capped by default at
/// [`GRAPH_ZONE_DEFAULT_MAX`] to preserve the message column on wide histories.
/// `user` (handle drag) replaces the cap, clamped between [`MIN_GRAPH_ZONE`] and
/// the natural width (never any useless empty space).
fn graph_zone_width(lane_count: usize, user: Option<f32>) -> f32 {
    let natural = (LANE_LEFT_PAD + lane_count as f32 * LANE_WIDTH).max(MIN_GRAPH_ZONE);
    user.unwrap_or(GRAPH_ZONE_DEFAULT_MAX)
        .clamp(MIN_GRAPH_ZONE, natural)
}

fn placeholder(ui: &mut egui::Ui, palette: &Palette, text: &str) -> GraphAction {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 2.0 - ROW_HEIGHT);
        ui.label(egui::RichText::new(text).color(palette.text_muted));
    });
    GraphAction::default()
}

/// Minimalist loader while the first graph has not arrived (large repo, switch in
/// Graph mode): a centered spinner, same position as the placeholders.
fn loading_placeholder(ui: &mut egui::Ui, palette: &Palette) -> GraphAction {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 2.0 - ROW_HEIGHT);
        ui.add(Spinner::new().size(SPINNER_SIZE).color(palette.text_muted))
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, LOADING_LABEL)
            });
    });
    GraphAction::default()
}

/// Column header row (uppercase `text_muted`, design-system §2), fixed above the
/// scroll, underlined with a separator.
fn column_headers(ui: &mut egui::Ui, palette: &Palette, graph_zone: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, HEADER_HEIGHT), egui::Sense::hover());
    header_label(
        ui,
        palette,
        rect,
        "BRANCH / TAG",
        rect.left() + REFS_LEFT_PAD,
    );
    header_label(ui, palette, rect, "GRAPH", rect.left() + REFS_COL_WIDTH);
    header_label(
        ui,
        palette,
        rect,
        "COMMIT MESSAGE",
        rect.left() + REFS_COL_WIDTH + graph_zone + TEXT_GAP,
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.bottom() - 1.0),
            egui::pos2(rect.right(), rect.bottom() - 1.0),
        ],
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

fn header_label(ui: &mut egui::Ui, palette: &Palette, strip: egui::Rect, text: &str, x: f32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(HEADER_SIZE),
        palette.text_muted,
    );
    let rect = egui::Rect::from_min_size(
        egui::pos2(x, strip.center().y - galley.size().y / 2.0),
        galley.size(),
    );
    ui.put(
        rect,
        egui::Label::new(
            egui::RichText::new(text)
                .size(HEADER_SIZE)
                .color(palette.text_muted),
        )
        .selectable(false),
    );
}

/// Paint inputs of one commit row, bundled by [`commit_row_ui`].
/// `hide_chips` (Branch editor placed on the row) suppresses the chips —
/// painted with manual hit-testing, they would click through the field.
/// `chips_occluded` (another row's expanded overlay covers this one) keeps the
/// inline chips painted but inert — the manual hit-test would otherwise fire
/// through the overlay.
struct RowPaint<'a> {
    rect: egui::Rect,
    hovered: bool,
    selected: bool,
    commit: &'a GraphCommit,
    /// Checked-out branch ("into …" of the Merge entries); `None` on a
    /// detached HEAD.
    head: Option<&'a str>,
    /// `origin` is a recognized cloud forge: enables the Create pull request entry.
    can_pr: bool,
    row: &'a GraphRow,
    prev_edges: Option<&'a [Edge]>,
    next_lane: Option<usize>,
    graph_zone: f32,
    expand_chips: Option<usize>,
    chips_occluded: bool,
    hide_chips: bool,
    /// Commit matches the active search (⌘F): amber row highlight.
    search_match: bool,
    /// Commit is the current match: stronger fill + amber outline.
    search_current: bool,
}

/// Returns the intent emitted by the refs column chips (double-clicked checkout,
/// right-click context menu), if any.
fn paint_row(ui: &egui::Ui, palette: &Palette, paint: &RowPaint<'_>) -> ChipIntent {
    let RowPaint {
        rect,
        hovered,
        selected,
        commit,
        head,
        can_pr,
        row,
        prev_edges,
        next_lane,
        graph_zone,
        expand_chips,
        chips_occluded,
        hide_chips,
        search_match,
        search_current,
    } = *paint;
    let painter = ui.painter();
    // Rows on the bare canvas: only selection and hover lay a background — a
    // per-lane veil (alpha 10, ex-M10-8) detached the block of rows from the app
    // background, in light as in dark.
    if selected {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.accent_subtle,
        );
    } else if hovered {
        // Hover skips the BRANCH/TAG column: the highlight covers only the graph
        // and message zone, and lights up only while the pointer is over it.
        let hl = egui::Rect::from_min_max(
            egui::pos2(rect.left() + REFS_COL_WIDTH, rect.top()),
            rect.max,
        );
        if ui.rect_contains_pointer(hl) {
            painter.rect_filled(hl, egui::CornerRadius::same(4), palette.bg_surface_hover);
        }
    }
    // Search highlight (⌘F): amber fill over any backgrounds (translucent, so a
    // selected match keeps its blue tint), the current match stronger + outlined
    // — distinct from the blue selection ring.
    if search_match || search_current {
        let alpha = if search_current {
            SEARCH_CURRENT_ALPHA
        } else {
            SEARCH_MATCH_ALPHA
        };
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            with_alpha(SEARCH_MATCH_COLOR, alpha),
        );
        if search_current {
            painter.rect_stroke(
                rect.shrink(SEARCH_CURRENT_STROKE / 2.0),
                egui::CornerRadius::same(RADIUS_PILL),
                egui::Stroke::new(SEARCH_CURRENT_STROKE, SEARCH_MATCH_COLOR),
                egui::StrokeKind::Inside,
            );
        }
    }
    // BRANCH / TAG column (M10-5): a single chip + `+N` fold. On expansion
    // (`expand_chips`, chip count arbitrated by the caller: hover of the refs zone
    // or of the overlay itself ⇒ all, pinned menu ⇒ up to the targeted chip), the
    // overlay (full-width chips, stacked on the tooltip layer) **replaces** the
    // inline chips — never both at once, otherwise the inline ones show through
    // under the overlay.
    let chips_anchor = egui::pos2(rect.left() + REFS_LEFT_PAD, rect.center().y);
    let intent = if hide_chips {
        ChipIntent::default()
    } else if let Some(count) = expand_chips {
        paint_ref_chips_expanded(
            ui,
            palette,
            commit,
            head,
            can_pr,
            palette.lane_color(row.lane),
            chips_anchor,
            count,
        )
    } else {
        paint_ref_chips(
            ui,
            palette,
            commit,
            head,
            can_pr,
            palette.lane_color(row.lane),
            chips_anchor,
            rect.left() + REFS_COL_WIDTH - COL_GAP,
            chips_occluded,
        )
    };

    let lane_x =
        |lane: usize| rect.left() + REFS_COL_WIDTH + LANE_LEFT_PAD + lane as f32 * LANE_WIDTH;
    // Lanes, edges and node clipped to the graph zone: capped by default (and
    // resizable), the excess lanes never spill over onto the message column.
    let lane_painter = painter.with_clip_rect(lane_clip_rect(rect, graph_zone));
    // Top half: continuity of the previous row's edges (each row paints its own
    // rect, so hover/selection backgrounds never cover an edge). One vertical per
    // arrival lane, in that lane's color (after a merge-in, the line belongs to
    // the joined lane) — except a first-parent transition landing on **this
    // node**: it finishes here with its rounded approach (drop from the source
    // lane, corner, horizontal into the node), in the source lane's color. A
    // `dashed` edge (WIP → HEAD link, stash) stays dashed.
    if let Some(prev) = prev_edges {
        let mut done: Vec<usize> = Vec::with_capacity(prev.len());
        for edge in prev {
            if edge.from_lane != edge.to_lane && edge.to_lane == row.lane && !edge.merge {
                let from = egui::pos2(lane_x(edge.from_lane), rect.top());
                let to = egui::pos2(lane_x(edge.to_lane), rect.center().y);
                stroke_path(
                    &lane_painter,
                    corner_into(from, to),
                    palette.lane_color(edge.from_lane),
                    edge.dashed,
                );
                continue;
            }
            if done.contains(&edge.to_lane) {
                continue;
            }
            done.push(edge.to_lane);
            let x = lane_x(edge.to_lane);
            let color = palette.lane_color(edge.to_lane);
            if edge.dashed {
                dashed_segment(&lane_painter, x, rect.top(), rect.center().y, color);
            } else {
                lane_painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.center().y)],
                    egui::Stroke::new(EDGE_WIDTH, color),
                );
            }
        }
    }
    // Bottom half: this row's edges. Pass-through = vertical (dashed for the
    // WIP → HEAD link); lane transition = orthogonal route with one rounded
    // corner. A merge link (2nd+ parent) bends at this row:
    // horizontal out of the node, then down the joined lane (which carries the
    // line's color). A first-parent transition keeps its node's color and drops
    // straight, bending at the other end — into the next row's node when that
    // is the target (rounded approach painted by that row), at the row boundary
    // otherwise (collapse onto a passing lane).
    for edge in &row.edges {
        let from = egui::pos2(lane_x(edge.from_lane), rect.center().y);
        let to = egui::pos2(lane_x(edge.to_lane), rect.bottom());
        if edge.from_lane == edge.to_lane {
            let color = palette.lane_color(edge.to_lane);
            if edge.dashed {
                dashed_segment(&lane_painter, from.x, from.y, to.y, color);
            } else {
                lane_painter.line_segment([from, to], egui::Stroke::new(EDGE_WIDTH, color));
            }
        } else if edge.merge {
            stroke_path(
                &lane_painter,
                corner_out_of(from, to),
                palette.lane_color(edge.to_lane),
                edge.dashed,
            );
        } else {
            let color = palette.lane_color(edge.from_lane);
            if next_lane == Some(edge.to_lane) {
                if edge.dashed {
                    dashed_segment(&lane_painter, from.x, from.y, to.y, color);
                } else {
                    lane_painter.line_segment(
                        [from, egui::pos2(from.x, to.y)],
                        egui::Stroke::new(EDGE_WIDTH, color),
                    );
                }
            } else {
                stroke_path(&lane_painter, corner_into(from, to), color, edge.dashed);
            }
        }
    }
    // Node: merge = small dot; stash = dashed square + archive icon;
    // regular commit = bubble with the author's initials; selection = `accent`
    // ring around the node.
    let node_center = egui::pos2(lane_x(row.lane), rect.center().y);
    // Leader linking the row's label to its node (thicker on the checked-out HEAD),
    // drawn before the node so it tucks under it.
    if let Some(chip_right) = intent.content_right {
        let lane_color = palette.lane_color(row.lane);
        // Checked-out HEAD: thicker and in the full lane color (matching the author
        // bubble border) to single it out; the others stay on the dimmer chip fill.
        let (width, color) = if commit.refs.iter().any(|r| r.is_head) {
            (LABEL_LINK_HEAD_WIDTH, lane_color)
        } else {
            (LABEL_LINK_WIDTH, chip_fill(palette, lane_color))
        };
        painter.line_segment(
            [
                egui::pos2(chip_right, node_center.y),
                egui::pos2(node_center.x, node_center.y),
            ],
            egui::Stroke::new(width, color),
        );
    }
    let node_color = palette.lane_color(row.lane);
    let node_radius = if commit.parents.len() >= 2 {
        MERGE_NODE_RADIUS
    } else {
        NODE_RADIUS
    };
    if commit.stash {
        let node_rect =
            egui::Rect::from_center_size(node_center, egui::Vec2::splat(node_radius * 2.0));
        lane_painter.rect_filled(node_rect, 0, darkened(node_color));
        dashed_rect(
            &lane_painter,
            node_rect.shrink(NODE_BORDER_WIDTH / 2.0),
            egui::Stroke::new(NODE_BORDER_WIDTH, node_color),
        );
        paint_icon(
            &lane_painter,
            node_center,
            STASH_ICON_SIZE,
            lucide_icons::Icon::Archive,
            lane_ink(palette),
        );
    } else if commit.parents.len() >= 2 {
        lane_painter.circle_filled(node_center, node_radius, node_color);
    } else {
        lane_painter.circle_filled(node_center, node_radius, darkened(node_color));
        lane_painter.circle_stroke(
            node_center,
            node_radius - NODE_BORDER_WIDTH / 2.0,
            egui::Stroke::new(NODE_BORDER_WIDTH, node_color),
        );
        let text = initials(&commit.author);
        if !text.is_empty() {
            lane_painter.text(
                node_center,
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(INITIALS_SIZE),
                lane_ink(palette),
            );
        }
    }
    let mut cursor = rect.left() + REFS_COL_WIDTH + graph_zone + TEXT_GAP;
    // Lane-colored accent bar at the head of the message column (M10-6).
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(cursor, rect.top() + ACCENT_BAR_MARGIN_Y),
            egui::pos2(
                cursor + ACCENT_BAR_WIDTH,
                rect.bottom() - ACCENT_BAR_MARGIN_Y,
            ),
        ),
        egui::CornerRadius::same(1),
        palette.lane_color(row.lane),
    );
    cursor += ACCENT_BAR_WIDTH + ACCENT_BAR_GAP;
    let hash = painter.layout_no_wrap(
        commit.short_id.clone(),
        egui::FontId::monospace(HASH_SIZE),
        palette.text_muted,
    );
    let hash_size = hash.size();
    painter.galley(
        egui::pos2(cursor, rect.center().y - hash_size.y / 2.0),
        hash,
        palette.text_muted,
    );
    cursor += hash_size.x + COL_GAP;

    // The summary fills all the remaining space, on a single elided line — no
    // author/date column: initials in the node, detail in the sidebar.
    let right_limit = rect.right() - COL_GAP;
    let summary_max = (right_limit - cursor).max(0.0);
    let summary = painter.layout_job(single_line(
        &commit.summary,
        egui::FontId::proportional(TEXT_SIZE),
        palette.text_primary,
        summary_max,
    ));
    let summary_width = summary.size().x;
    painter.galley(
        egui::pos2(cursor, rect.center().y - summary.size().y / 2.0),
        summary,
        palette.text_primary,
    );
    // Message body, dimmed, following the summary (M10-6), flattened onto a single
    // line and elided in the remaining space.
    if !commit.body.is_empty() {
        let body_x = cursor + summary_width + BODY_GAP;
        let body_max = right_limit - body_x;
        if body_max >= BODY_MIN_WIDTH {
            let flat = commit.body.split_whitespace().collect::<Vec<_>>().join(" ");
            let body = painter.layout_job(single_line(
                &flat,
                egui::FontId::proportional(BODY_SIZE),
                palette.text_muted,
                body_max,
            ));
            painter.galley(
                egui::pos2(body_x, rect.center().y - body.size().y / 2.0),
                body,
                palette.text_muted,
            );
        }
    }
    intent
}

/// WIP row (M10-7): **dashed** node on the HEAD lane + label `// WIP · N file(s)`
/// in the message column — no hash and no author. When the HEAD commit is in the
/// page (`head_lane` is `Some`), a dashed segment starts below the circle and
/// joins its row (relay handled by `paint_row`).
fn paint_wip_row(
    ui: &egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    hovered: bool,
    wip: WipRow,
    head_lane: Option<usize>,
    graph_zone: f32,
) {
    let lane = head_lane.unwrap_or(0);
    let painter = ui.painter();
    if wip.selected {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.accent_subtle,
        );
    } else if hovered {
        // Hover skips the BRANCH/TAG column: the highlight covers only the graph
        // and message zone, and lights up only while the pointer is over it.
        let hl = egui::Rect::from_min_max(
            egui::pos2(rect.left() + REFS_COL_WIDTH, rect.top()),
            rect.max,
        );
        if ui.rect_contains_pointer(hl) {
            painter.rect_filled(hl, egui::CornerRadius::same(4), palette.bg_surface_hover);
        }
    }

    let center = egui::pos2(
        rect.left() + REFS_COL_WIDTH + LANE_LEFT_PAD + lane as f32 * LANE_WIDTH,
        rect.center().y,
    );
    let lane_painter = painter.with_clip_rect(lane_clip_rect(rect, graph_zone));
    if head_lane.is_some() {
        // Start of the WIP → HEAD link: below the circle (hollow, don't cross it).
        dashed_segment(
            &lane_painter,
            center.x,
            center.y + NODE_RADIUS,
            rect.bottom(),
            palette.lane_color(lane),
        );
    }
    dashed_circle(
        &lane_painter,
        center,
        NODE_RADIUS,
        egui::Stroke::new(RING_WIDTH, palette.lane_color(lane)),
    );
    let cursor = rect.left() + REFS_COL_WIDTH + graph_zone + TEXT_GAP;
    let galley = painter.layout_job(single_line(
        &wip_label(wip.files),
        egui::FontId::proportional(TEXT_SIZE),
        palette.text_secondary,
        (rect.right() - COL_GAP - cursor).max(0.0),
    ));
    painter.galley(
        egui::pos2(cursor, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );
}

/// Label of the WIP row: counter of touched files.
fn wip_label(files: usize) -> String {
    let unit = if files == 1 { "file" } else { "files" };
    format!("// WIP · {files} {unit}")
}

/// Graph-zone portion of a row's rect: clips the lanes/edges/nodes when the
/// column is narrower than the history.
fn lane_clip_rect(rect: egui::Rect, graph_zone: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + REFS_COL_WIDTH, rect.top()),
        egui::pos2(rect.left() + REFS_COL_WIDTH + graph_zone, rect.bottom()),
    )
}

/// Dashed circle (WIP node): the circle is sampled into a polyline then drawn via
/// `Shape::dashed_line`.
fn dashed_circle(painter: &egui::Painter, center: egui::Pos2, radius: f32, stroke: egui::Stroke) {
    let points: Vec<egui::Pos2> = (0..=32)
        .map(|i| {
            let angle = std::f32::consts::TAU * i as f32 / 32.0;
            center + radius * egui::vec2(angle.cos(), angle.sin())
        })
        .collect();
    painter.extend(egui::Shape::dashed_line(
        &points, stroke, DASH_LEN, DASH_GAP,
    ));
}

/// Dashed square (stash node), same dash pattern as `dashed_circle`.
fn dashed_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let points = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(egui::Shape::dashed_line(
        &points, stroke, DASH_LEN, DASH_GAP,
    ));
}

/// Dashed vertical (WIP → HEAD link), same pattern as `dashed_circle`.
fn dashed_segment(painter: &egui::Painter, x: f32, top: f32, bottom: f32, color: egui::Color32) {
    painter.extend(egui::Shape::dashed_line(
        &[egui::pos2(x, top), egui::pos2(x, bottom)],
        egui::Stroke::new(EDGE_WIDTH, color),
        DASH_LEN,
        DASH_GAP,
    ));
}

/// Orthogonal transition leaving a node sideways (merge link): horizontal out
/// of `from` at the node's level, one rounded corner, then down the target
/// lane to `to`.
fn corner_out_of(from: egui::Pos2, to: egui::Pos2) -> Vec<egui::Pos2> {
    let radius = corner_radius(from, to);
    let dir = (to.x - from.x).signum();
    let mut points = vec![from, egui::pos2(to.x - dir * radius, from.y)];
    let end = if dir > 0.0 {
        0.0
    } else {
        -std::f32::consts::PI
    };
    arc(
        &mut points,
        egui::pos2(to.x - dir * radius, from.y + radius),
        radius,
        -std::f32::consts::FRAC_PI_2,
        end,
    );
    points.push(to);
    points
}

/// Orthogonal transition landing sideways (first-parent link): straight drop
/// from `from`, one rounded corner, then horizontal at `to`'s level — into the
/// target node (rounded approach) or onto a passing lane (collapse).
fn corner_into(from: egui::Pos2, to: egui::Pos2) -> Vec<egui::Pos2> {
    let radius = corner_radius(from, to);
    let dir = (to.x - from.x).signum();
    let mut points = vec![from, egui::pos2(from.x, to.y - radius)];
    let start = if dir > 0.0 { std::f32::consts::PI } else { 0.0 };
    arc(
        &mut points,
        egui::pos2(from.x + dir * radius, to.y - radius),
        radius,
        start,
        std::f32::consts::FRAC_PI_2,
    );
    points.push(to);
    points
}

/// Corner radius clamped to the room the transition actually has.
fn corner_radius(from: egui::Pos2, to: egui::Pos2) -> f32 {
    EDGE_CORNER_RADIUS
        .min((to.x - from.x).abs())
        .min((to.y - from.y).abs())
}

/// Appends a quarter-arc (`a0` → `a1`, screen coordinates) to a transition path.
fn arc(points: &mut Vec<egui::Pos2>, center: egui::Pos2, radius: f32, a0: f32, a1: f32) {
    for i in 1..=CORNER_ARC_STEPS {
        let angle = a0 + (a1 - a0) * i as f32 / CORNER_ARC_STEPS as f32;
        points.push(center + radius * egui::vec2(angle.cos(), angle.sin()));
    }
}

/// Strokes a transition path, dashed (WIP / stash links) or solid.
fn stroke_path(
    painter: &egui::Painter,
    points: Vec<egui::Pos2>,
    color: egui::Color32,
    dashed: bool,
) {
    let stroke = egui::Stroke::new(EDGE_WIDTH, color);
    if dashed {
        painter.extend(egui::Shape::dashed_line(
            &points, stroke, DASH_LEN, DASH_GAP,
        ));
    } else {
        painter.add(egui::Shape::line(points, stroke));
    }
}

/// Darkened lane color: fill of the author bubbles and, in dark mode, of the ref
/// chips (the border / the lines stay full color).
fn darkened(color: egui::Color32) -> egui::Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let dark = |c: u8| (c as f32 * NODE_FILL_DARKEN) as u8;
    egui::Color32::from_rgb(dark(r), dark(g), dark(b))
}

/// Ink laid on a background derived from the lane (author bubble, ref chip):
/// `lane_node_text` in light mode (designed for the full lane, cf. the commit
/// detail avatar), white in dark mode — there the background is always darkened
/// and `lane_node_text` may be dark (themes with pastel lanes).
fn lane_ink(palette: &Palette) -> egui::Color32 {
    if palette.dark {
        egui::Color32::WHITE
    } else {
        palette.lane_node_text
    }
}

/// Fill of a ref chip: darkened lane in dark mode, full lane in
/// light mode — the darkened one would make a blackish pill that clashes on a
/// light canvas.
fn chip_fill(palette: &Palette, lane_color: egui::Color32) -> egui::Color32 {
    if palette.dark {
        darkened(lane_color)
    } else {
        lane_color
    }
}

/// Ink (text + glyphs) of a ref chip: crisp for the checked-out branch, dimmed
/// (`CHIP_DIM_ALPHA`) for the others — the branch you are working on stands out
/// at a glance.
fn chip_ink(palette: &Palette, is_head: bool) -> egui::Color32 {
    if is_head {
        lane_ink(palette)
    } else {
        with_alpha(lane_ink(palette), CHIP_DIM_ALPHA)
    }
}

/// Author initials for the node bubble (reused by the commit detail avatar): 1st
/// letter of the first and last word (single word ⇒ first 2 characters), in
/// uppercase.
pub(crate) fn initials(author: &str) -> String {
    let mut words = author.split_whitespace();
    let first = words.next();
    match (first, words.last()) {
        (Some(f), Some(l)) => f
            .chars()
            .take(1)
            .chain(l.chars().take(1))
            .flat_map(char::to_uppercase)
            .collect(),
        (Some(f), None) => f.chars().take(2).flat_map(char::to_uppercase).collect(),
        _ => String::new(),
    }
}

/// Text on a **single line**, elided (`…`) beyond `max_width` — avoids word-by-word
/// wrapping in a narrow central zone.
fn single_line(
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::text::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    job
}

/// Display plan for a row's chips: at most `max` visible chips, the rest folded
/// into a `+N` counter (never a silent overflow of the longest name — it is
/// elided, not hidden).
fn chip_plan(refs: &[GraphRef], max: usize) -> (&[GraphRef], usize) {
    let visible = refs.len().min(max);
    (&refs[..visible], refs.len() - visible)
}

/// Returns the intent emitted by the chips (double-click ⇒ checkout, right-click
/// ⇒ context menu), if any. `occluded`: chips covered by another row's expanded
/// overlay — painted, but no interaction (cursor included).
#[allow(clippy::too_many_arguments)]
fn paint_ref_chips(
    ui: &egui::Ui,
    palette: &Palette,
    commit: &crate::git::graph::GraphCommit,
    head: Option<&str>,
    can_pr: bool,
    lane_color: egui::Color32,
    left: egui::Pos2,
    right_limit: f32,
    occluded: bool,
) -> ChipIntent {
    let painter = ui.painter();
    let mut intent = ChipIntent::default();
    let (visible, overflow) = chip_plan(&commit.refs, CHIP_MAX);
    if visible.is_empty() {
        return intent;
    }
    let overflow_galley = (overflow > 0).then(|| {
        painter.layout_no_wrap(
            format!("+{overflow}"),
            egui::FontId::proportional(CHIP_TEXT_SIZE),
            palette.text_secondary,
        )
    });
    let badge_width = overflow_galley
        .as_ref()
        .map_or(0.0, |g| g.size().x + CHIP_PAD_X * 2.0 + CHIP_GAP);
    let avail = right_limit - left.x - badge_width;
    let per_chip =
        ((avail - (visible.len() - 1) as f32 * CHIP_GAP) / visible.len() as f32).max(0.0);

    // An occluded row sits under another row's overlay: its chips are painted but
    // dead — no hover restore either, else the masked chip lights up through the overlay.
    let hover_pos = (!occluded)
        .then(|| ui.input(|i| i.pointer.hover_pos()))
        .flatten();
    let mut x = left.x;
    let mut content_right = left.x;
    for gref in visible {
        let right = paint_chip(
            painter, palette, gref, lane_color, x, left.y, per_chip, hover_pos,
        );
        let chip = egui::Rect::from_min_max(
            egui::pos2(x, left.y - CHIP_HEIGHT / 2.0),
            egui::pos2(right, left.y + CHIP_HEIGHT / 2.0),
        );
        if !occluded {
            chip_interactions(ui, chip, gref, commit.oid, head, can_pr, None, &mut intent);
        }
        content_right = right;
        x = right + CHIP_GAP;
    }
    if let Some(galley) = overflow_galley {
        let size = galley.size();
        let badge = egui::Rect::from_min_size(
            egui::pos2(x, left.y - CHIP_HEIGHT / 2.0),
            egui::vec2(size.x + CHIP_PAD_X * 2.0, CHIP_HEIGHT),
        );
        painter.rect_filled(
            badge,
            egui::CornerRadius::same(CHIP_RADIUS),
            palette.bg_surface_hover,
        );
        painter.galley(
            egui::pos2(badge.left() + CHIP_PAD_X, left.y - size.y / 2.0),
            galley,
            palette.text_secondary,
        );
        content_right = badge.right();
    }
    intent.content_right = Some(content_right);
    intent
}

/// Expansion of the refs zone: the first `count` chips of the row, at full width
/// (whole names), stacked vertically on the tooltip layer — they replace the
/// inline chips and cover the following rows for the duration of the hover (all
/// on hover; truncated to the targeted chip when the context menu is pinned).
/// Returns the intent emitted by the chips, with the covered area (`expanded`):
/// hovering it keeps the expansion on the next frame.
#[allow(clippy::too_many_arguments)]
fn paint_ref_chips_expanded(
    ui: &egui::Ui,
    palette: &Palette,
    commit: &crate::git::graph::GraphCommit,
    head: Option<&str>,
    can_pr: bool,
    lane_color: egui::Color32,
    left: egui::Pos2,
    count: usize,
) -> ChipIntent {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new(("graph_ref_chips", commit.oid)),
    ));
    let mut intent = ChipIntent::default();
    let mut zone = egui::Rect::NOTHING;
    let mut y = left.y;
    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    for (index, gref) in commit.refs.iter().take(count).enumerate() {
        let right = paint_chip(
            &painter,
            palette,
            gref,
            lane_color,
            left.x,
            y,
            f32::INFINITY,
            hover_pos,
        );
        let chip = egui::Rect::from_min_max(
            egui::pos2(left.x, y - CHIP_HEIGHT / 2.0),
            egui::pos2(right, y + CHIP_HEIGHT / 2.0),
        );
        zone = zone.union(chip);
        // The top chip sits on the row center: its right edge anchors the leader to
        // the node, kept while the row's refs are expanded on hover.
        if index == 0 {
            intent.content_right = Some(right);
        }
        chip_interactions(
            ui,
            chip,
            gref,
            commit.oid,
            head,
            can_pr,
            Some((commit.oid, index)),
            &mut intent,
        );
        y += CHIP_HEIGHT + CHIP_GAP;
    }
    intent.expanded = Some(zone);
    intent
}

/// Interactions of a painted chip: double-click ⇒ checkout intent (branches
/// only); right-click on any chip ⇒ opening of the context menu anchored below
/// the chip (a tag's menu carries only **Create branch**). `expanded`: position
/// of the chip in the expanded overlay (`None` for an inline chip) — the menu's
/// row stays expanded up to the targeted chip.
#[allow(clippy::too_many_arguments)]
fn chip_interactions(
    ui: &egui::Ui,
    chip: egui::Rect,
    gref: &GraphRef,
    oid: git2::Oid,
    head: Option<&str>,
    can_pr: bool,
    expanded: Option<(git2::Oid, usize)>,
    intent: &mut ChipIntent,
) {
    // Painted chips (not widgets): pointer cursor set by hand on hover — every
    // chip kind is interactive (a tag has no left-click but carries a right-click
    // menu: Checkout / Push / Delete tag).
    if hovered_at(ui, chip) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if double_clicked_at(ui, chip) {
        intent.checkout = checkout_target(gref).map(str::to_owned);
    }
    if secondary_clicked_at(ui, chip) {
        intent.menu = Some(ChipMenu {
            sections: branch_sections(&[menu_branch(gref, oid)], head, can_pr),
            anchor: chip,
            expanded,
        });
    }
}

/// Checkout target of a chip: non-checked-out **local** branch, or **remote** ref
/// (DWIM on the domain side: same-named local, created and tracked as needed) — a
/// double-click on a tag or the current branch is ignored.
fn checkout_target(gref: &GraphRef) -> Option<&str> {
    (gref.kind != RefKind::Tag && !gref.is_head).then_some(gref.name.as_str())
}

/// Rebase target of a chip: any **branch** ref (local or remote — a remote chip
/// is a valid committish for `git rebase`) except the checked-out one and
/// `origin/HEAD` (remote symref, not a branch — same exclusion as Delete: the
/// real default-branch chip is the legible target). Detached HEAD is refused on
/// the domain side (`sync::rebase_onto`), like Push.
fn rebase_onto_target(gref: &GraphRef) -> Option<&str> {
    let remote_symref = gref.kind == RefKind::Remote && remote_branch_part(&gref.name) == "HEAD";
    (gref.kind != RefKind::Tag && !gref.is_head && !remote_symref).then_some(gref.name.as_str())
}

/// **Create branch** source of a chip (git.md §9): any ref — local, remote or
/// tag — except `origin/HEAD` (remote symref, not a real ref, same exclusion as
/// Rebase/Delete). The committish is the ref **fully qualified** so a branch and
/// a tag sharing a name never collide.
fn create_branch_target(gref: &GraphRef, oid: git2::Oid) -> Option<CreateBranchRequest> {
    let source = match gref.kind {
        RefKind::Local => format!("refs/heads/{}", gref.name),
        RefKind::Remote => {
            if remote_branch_part(&gref.name) == "HEAD" {
                return None;
            }
            format!("refs/remotes/{}", gref.name)
        }
        RefKind::Tag => format!("refs/tags/{}", gref.name),
    };
    Some(CreateBranchRequest { oid, source })
}

/// **Rename** target of a chip (git.md §9): a **local** branch, the **current
/// branch included** (`git branch -m` moves HEAD with it). Remotes (renamed by
/// push + delete) and tags are not offered; the synthetic detached-`HEAD` marker
/// is excluded (git refuses "HEAD" as a branch name, no real ref behind it).
fn rename_target(gref: &GraphRef) -> Option<&str> {
    (gref.kind == RefKind::Local && gref.name != "HEAD").then_some(gref.name.as_str())
}

/// **Local** branch deletable from a chip: the non-checked-out local itself, or
/// the same-named local of a remote chip (`counterpart` — already excludes the
/// checked-out one). As soon as the branch exists on both sides, the menu names
/// the deletions on both sides whatever the chip (git.md §9).
fn delete_local_target(gref: &GraphRef) -> Option<String> {
    match gref.kind {
        RefKind::Local => (!gref.is_head).then(|| gref.name.clone()),
        RefKind::Remote => gref.counterpart.clone(),
        RefKind::Tag => None,
    }
}

/// **Remote** branch deletable from a chip — full remote name, shown as-is and
/// resolved `<remote>/<name>` on the domain side: the remote ref itself
/// (`origin/HEAD` excluded — symref of the remote, not a branch), or the
/// same-named remote of a local chip (`counterpart`, merged or diverged).
fn delete_remote_target(gref: &GraphRef) -> Option<String> {
    match gref.kind {
        RefKind::Remote => (remote_branch_part(&gref.name) != "HEAD").then(|| gref.name.clone()),
        RefKind::Local => gref.counterpart.clone(),
        RefKind::Tag => None,
    }
}

/// Branch name behind the `<remote>/` prefix of a remote ref.
fn remote_branch_part(name: &str) -> &str {
    name.split_once('/').map_or(name, |(_, branch)| branch)
}

/// Primary double-click in `rect` this frame. Detected by manual hit-testing: the
/// chips are painted (not widgets), and the expanded overlay lives on the tooltip
/// layer where an `interact` would lose arbitration against the rows below. The
/// triple also counts: a click < 0.6 s earlier (row selection, same position)
/// reclassifies the double's 2nd click as a triple in egui's counter — without it
/// the double-click would be silently lost.
fn double_clicked_at(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| {
        (i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
            || i.pointer
                .button_triple_clicked(egui::PointerButton::Primary))
            && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))
    })
}

/// Right-click in `rect` this frame. Same manual hit-testing as
/// [`double_clicked_at`] (painted chips, not widgets).
fn secondary_clicked_at(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| {
        i.pointer.button_clicked(egui::PointerButton::Secondary)
            && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))
    })
}

/// Pointer in `rect` this frame. Same manual hit-testing as [`double_clicked_at`].
fn hovered_at(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p)))
}

/// Paints a typed ref chip — `[✓] name [type glyph][globe if also_remote]`:
/// rectangle with slightly rounded corners, lane-colored fill
/// (`chip_fill`), name elided to fit within `max_width` (`f32::INFINITY` ⇒ full
/// width). The `<remote>/` prefix of a Remote ref is replaced by its glyph. Ink
/// `chip_ink`: crisp white if checked-out (plus medium-weight name and
/// `HEAD_CHIP_RING`), dimmed otherwise. Returns the right edge of the chip.
#[allow(clippy::too_many_arguments)]
fn paint_chip(
    painter: &egui::Painter,
    palette: &Palette,
    gref: &GraphRef,
    lane_color: egui::Color32,
    left: f32,
    center_y: f32,
    max_width: f32,
    hover_pos: Option<egui::Pos2>,
) -> f32 {
    // Checked-out chip: medium weight (regular elsewhere) — spotted at a glance
    // when scanning the refs column.
    let font = if gref.is_head {
        egui::FontId::new(CHIP_TEXT_SIZE, medium_family(painter.ctx()))
    } else {
        egui::FontId::proportional(CHIP_TEXT_SIZE)
    };
    let glyphs = 1 + usize::from(gref.is_head) + usize::from(gref.also_remote);
    let glyphs_width = glyphs as f32 * (CHIP_GLYPH + CHIP_GLYPH_GAP);
    let name = match (gref.kind, gref.name.split_once('/')) {
        (RefKind::Remote, Some((_, branch))) => branch,
        _ => gref.name.as_str(),
    };
    let text_max = (max_width - CHIP_PAD_X * 2.0 - glyphs_width).max(0.0);
    let galley = painter.layout_job(single_line(
        name,
        font,
        egui::Color32::PLACEHOLDER,
        text_max,
    ));
    let width = (CHIP_PAD_X * 2.0 + glyphs_width + galley.size().x).min(max_width);
    let chip = egui::Rect::from_min_size(
        egui::pos2(left, center_y - CHIP_HEIGHT / 2.0),
        egui::vec2(width, CHIP_HEIGHT),
    );
    // Hovering a dimmed ref restores full-opacity ink — pointing at it reveals it.
    let ink = chip_ink(
        palette,
        gref.is_head || hover_pos.is_some_and(|p| chip.contains(p)),
    );
    painter.rect_filled(
        chip,
        egui::CornerRadius::same(CHIP_RADIUS),
        chip_fill(palette, lane_color),
    );
    let mut x = chip.left() + CHIP_PAD_X;
    let glyph_rect = |x: &mut f32| {
        let rect = egui::Rect::from_center_size(
            egui::pos2(*x + CHIP_GLYPH / 2.0, center_y),
            egui::vec2(CHIP_GLYPH, CHIP_GLYPH),
        );
        *x += CHIP_GLYPH + CHIP_GLYPH_GAP;
        rect
    };
    if gref.is_head {
        paint_icon(
            painter,
            glyph_rect(&mut x).center(),
            CHIP_GLYPH,
            lucide_icons::Icon::Check,
            ink,
        );
    }
    let text_width = galley.size().x;
    painter.galley(egui::pos2(x, center_y - galley.size().y / 2.0), galley, ink);
    x += text_width + CHIP_GLYPH_GAP;
    let kind_icon = match gref.kind {
        RefKind::Local => lucide_icons::Icon::Laptop,
        RefKind::Remote => lucide_icons::Icon::Globe,
        RefKind::Tag => lucide_icons::Icon::Tag,
    };
    paint_icon(
        painter,
        glyph_rect(&mut x).center(),
        CHIP_GLYPH,
        kind_icon,
        ink,
    );
    if gref.also_remote {
        paint_icon(
            painter,
            glyph_rect(&mut x).center(),
            CHIP_GLYPH,
            lucide_icons::Icon::Globe,
            ink,
        );
    }
    chip.right()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_ref(name: &str) -> GraphRef {
        GraphRef {
            name: name.to_string(),
            kind: RefKind::Local,
            is_head: false,
            also_remote: false,
            counterpart: None,
            worktree_available: false,
        }
    }

    #[test]
    fn next_row_steps_through_commits_without_wrapping() {
        let on = |i| Some(NavRow::Commit(i));
        assert_eq!(next_row(on(1), ArrowNav::Up, false, 3), on(0));
        assert_eq!(next_row(on(1), ArrowNav::Down, false, 3), on(2));
        assert_eq!(next_row(on(0), ArrowNav::Up, false, 3), None);
        assert_eq!(next_row(on(2), ArrowNav::Down, false, 3), None);
    }

    #[test]
    fn next_row_includes_the_wip_row_when_dirty() {
        assert_eq!(
            next_row(Some(NavRow::Commit(0)), ArrowNav::Up, true, 3),
            Some(NavRow::Wip)
        );
        assert_eq!(
            next_row(Some(NavRow::Wip), ArrowNav::Down, true, 3),
            Some(NavRow::Commit(0))
        );
        assert_eq!(next_row(Some(NavRow::Wip), ArrowNav::Up, true, 3), None);
    }

    #[test]
    fn next_row_without_selection_takes_the_first_row() {
        assert_eq!(
            next_row(None, ArrowNav::Down, false, 3),
            Some(NavRow::Commit(0))
        );
        assert_eq!(
            next_row(None, ArrowNav::Up, false, 3),
            Some(NavRow::Commit(0))
        );
        assert_eq!(next_row(None, ArrowNav::Down, true, 3), Some(NavRow::Wip));
    }

    #[test]
    fn graph_zone_follows_lanes_then_caps_at_default() {
        assert_eq!(graph_zone_width(1, None), MIN_GRAPH_ZONE);
        assert_eq!(
            graph_zone_width(3, None),
            LANE_LEFT_PAD + 3.0 * LANE_WIDTH,
            "below the cap, the zone follows the natural lane width"
        );
        assert_eq!(
            graph_zone_width(20, None),
            GRAPH_ZONE_DEFAULT_MAX,
            "wide history: capped by default"
        );
    }

    #[test]
    fn graph_zone_user_width_is_clamped_to_the_lanes() {
        let natural = LANE_LEFT_PAD + 12.0 * LANE_WIDTH;
        assert_eq!(graph_zone_width(12, Some(natural - 30.0)), natural - 30.0);
        assert_eq!(
            graph_zone_width(12, Some(natural + 500.0)),
            natural,
            "no widening beyond the lanes (useless empty space)"
        );
        assert_eq!(graph_zone_width(12, Some(0.0)), MIN_GRAPH_ZONE);
    }

    fn search_commit(
        short_id: &str,
        summary: &str,
        author: &str,
        refs: Vec<GraphRef>,
    ) -> GraphCommit {
        GraphCommit {
            oid: git2::Oid::from_bytes(&[0u8; 20]).unwrap(),
            short_id: short_id.to_string(),
            summary: summary.to_string(),
            body: String::new(),
            author: author.to_string(),
            time: 0,
            parents: Vec::new(),
            refs,
            stash: false,
        }
    }

    #[test]
    fn matching_commits_is_case_insensitive_across_fields() {
        let commits = vec![
            search_commit("abc1234", "Fix the parser", "Ada", vec![graph_ref("main")]),
            search_commit("def5678", "Add tests", "Bob", vec![]),
            search_commit(
                "9990000",
                "Refactor",
                "Carol",
                vec![graph_ref("feature/parser")],
            ),
        ];
        // Summary (case-insensitive), hash prefix, author, and ref name each hit.
        assert_eq!(matching_commits(&commits, "PARSER"), vec![0, 2]);
        assert_eq!(matching_commits(&commits, "def567"), vec![1]);
        assert_eq!(matching_commits(&commits, "carol"), vec![2]);
        assert_eq!(matching_commits(&commits, "main"), vec![0]);
        assert!(matching_commits(&commits, "  ").is_empty(), "blank query");
        assert!(matching_commits(&commits, "zzz").is_empty(), "no match");
    }

    #[test]
    fn cycle_wraps_both_ways_and_parks_when_empty() {
        assert_eq!(cycle(0, 3, 1), 1);
        assert_eq!(cycle(2, 3, 1), 0, "forward wraps to the first");
        assert_eq!(cycle(0, 3, -1), 2, "backward wraps to the last");
        assert_eq!(cycle(5, 0, 1), 0, "no match: cursor parked at 0");
    }

    #[test]
    fn chip_plan_folds_overflow_into_counter() {
        let refs = vec![graph_ref("a"), graph_ref("b"), graph_ref("c")];
        let (visible, overflow) = chip_plan(&refs, 2);
        assert_eq!(visible.len(), 2);
        assert_eq!(overflow, 1);
    }

    #[test]
    fn chip_plan_shows_all_when_under_max() {
        let refs = vec![graph_ref("a")];
        let (visible, overflow) = chip_plan(&refs, 2);
        assert_eq!(visible.len(), 1);
        assert_eq!(overflow, 0);

        let (visible, overflow) = chip_plan(&[], 2);
        assert!(visible.is_empty());
        assert_eq!(overflow, 0);
    }

    #[test]
    fn checkout_target_accepts_branches_but_not_head_nor_tags() {
        assert_eq!(checkout_target(&graph_ref("feat")), Some("feat"));

        let remote = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/feat")
        };
        assert_eq!(checkout_target(&remote), Some("origin/feat"));

        let head = GraphRef {
            is_head: true,
            ..graph_ref("main")
        };
        assert_eq!(checkout_target(&head), None);

        let tag = GraphRef {
            kind: RefKind::Tag,
            ..graph_ref("v1.0")
        };
        assert_eq!(checkout_target(&tag), None);
    }

    #[test]
    fn rebase_onto_target_accepts_branches_but_not_head_nor_tags() {
        assert_eq!(rebase_onto_target(&graph_ref("feat")), Some("feat"));

        // A remote chip is a valid rebase target as-is (committish).
        let remote = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/main")
        };
        assert_eq!(rebase_onto_target(&remote), Some("origin/main"));

        let head = GraphRef {
            is_head: true,
            ..graph_ref("main")
        };
        assert_eq!(rebase_onto_target(&head), None, "never onto itself");

        let tag = GraphRef {
            kind: RefKind::Tag,
            ..graph_ref("v1.0")
        };
        assert_eq!(rebase_onto_target(&tag), None);

        // `origin/HEAD` is the remote's symref, not a branch: same exclusion
        // as Delete — the real default-branch chip is the legible target.
        let symref = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/HEAD")
        };
        assert_eq!(rebase_onto_target(&symref), None);
    }

    #[test]
    fn delete_local_target_takes_the_local_name_even_from_a_remote_chip() {
        assert_eq!(
            delete_local_target(&graph_ref("feat")),
            Some("feat".to_string())
        );

        // Remote chip paired with a same-named local: the **local** name goes
        // into the intent.
        let remote = GraphRef {
            kind: RefKind::Remote,
            counterpart: Some("feat".to_string()),
            ..graph_ref("origin/feat")
        };
        assert_eq!(delete_local_target(&remote), Some("feat".to_string()));

        let lone_remote = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/feat")
        };
        assert_eq!(delete_local_target(&lone_remote), None);

        let head = GraphRef {
            is_head: true,
            ..graph_ref("main")
        };
        assert_eq!(delete_local_target(&head), None);
    }

    #[test]
    fn delete_remote_target_names_the_full_remote_ref() {
        // Local paired with a same-named remote (merged or diverged): the
        // **full** remote name is shown and goes into the intent.
        let twinned = GraphRef {
            counterpart: Some("origin/feat".to_string()),
            ..graph_ref("feat")
        };
        assert_eq!(
            delete_remote_target(&twinned),
            Some("origin/feat".to_string())
        );

        assert_eq!(delete_remote_target(&graph_ref("feat")), None);

        let remote = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/feat")
        };
        assert_eq!(
            delete_remote_target(&remote),
            Some("origin/feat".to_string())
        );

        // `origin/HEAD`: symref of the remote, not a deletable branch.
        let symref = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/HEAD")
        };
        assert_eq!(delete_remote_target(&symref), None);
    }

    fn test_oid() -> git2::Oid {
        git2::Oid::from_bytes(&[7u8; 20]).unwrap()
    }

    // A non-head local branch: the same eligibility `menu_branch` derives for a
    // `Local` chip (checkout, rebase, Create branch, Copy, local deletion).
    fn menu_branch_named(name: &str) -> MenuBranch {
        menu_branch(&graph_ref(name), test_oid())
    }

    #[test]
    fn lone_branch_menu_keeps_flat_entries() {
        let sections = branch_sections(&[menu_branch_named("feat")], Some("main"), false);
        assert!(
            sections.iter().all(|s| s.title.is_none()),
            "a lone branch never nests its actions"
        );
        let labels: Vec<&str> = sections
            .iter()
            .flat_map(|s| &s.entries)
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Checkout",
                "Create branch",
                "Rebase onto feat",
                "Interactive rebase onto feat",
                "AI rebase onto feat",
                "Merge feat into main",
                "Rename",
                "Copy branch name",
                "Delete feat"
            ]
        );
    }

    #[test]
    fn available_branch_menu_offers_create_worktree() {
        let branch = MenuBranch {
            create_worktree: true,
            ..menu_branch_named("feat")
        };
        let sections = branch_sections(&[branch], Some("main"), false);
        let labels: Vec<&str> = sections
            .iter()
            .flat_map(|s| &s.entries)
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Checkout",
                "Create worktree",
                "Create branch",
                "Rebase onto feat",
                "Interactive rebase onto feat",
                "AI rebase onto feat",
                "Merge feat into main",
                "Rename",
                "Copy branch name",
                "Delete feat"
            ]
        );
        assert_eq!(
            sections[1].entries[0].intent,
            MenuIntent::CreateWorktree("feat".into())
        );
        assert_eq!(
            sections[2].entries[0].intent,
            MenuIntent::CreateBranch(CreateBranchRequest {
                oid: test_oid(),
                source: "refs/heads/feat".into(),
            })
        );
        assert_eq!(
            sections[3].entries[0].intent,
            MenuIntent::RebaseOnto("feat".into())
        );
        assert_eq!(
            sections[4].entries[0].intent,
            MenuIntent::InteractiveRebaseOnto("feat".into())
        );
        assert_eq!(
            sections[5].entries[0].intent,
            MenuIntent::AiRebaseOnto("feat".into())
        );
        assert_eq!(
            sections[6].entries[0].intent,
            MenuIntent::Merge("feat".into())
        );
    }

    #[test]
    fn multi_branch_menu_nests_one_submenu_per_action() {
        let head = MenuBranch {
            checkout: false,
            rebase_onto: false,
            delete_local: None,
            ..menu_branch_named("main")
        };
        let sections = branch_sections(&[head, menu_branch_named("feat")], Some("main"), false);
        let titles: Vec<_> = sections.iter().map(|s| s.title.as_deref()).collect();
        assert_eq!(
            titles,
            [
                Some("Checkout"),
                Some("Create branch"),
                Some("Rebase onto"),
                Some("Interactive rebase onto"),
                Some("AI rebase onto"),
                Some("Merge"),
                Some("Rename"),
                Some("Copy branch name"),
                Some("Delete")
            ]
        );
        // Per-branch entries labeled by the branch name; the ineligible HEAD
        // stays out of Checkout, the Rebase entries and Delete — but Create
        // branch and Copy apply to every ref, so both branches appear there.
        assert_eq!(
            sections[0].entries,
            [MenuEntry {
                label: "feat".into(),
                intent: MenuIntent::Checkout("feat".into()),
            }]
        );
        assert_eq!(
            sections[1]
                .entries
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            ["main", "feat"]
        );
        assert_eq!(
            sections[2].entries,
            [MenuEntry {
                label: "feat".into(),
                intent: MenuIntent::RebaseOnto("feat".into()),
            }]
        );
        assert_eq!(
            sections[3].entries,
            [MenuEntry {
                label: "feat".into(),
                intent: MenuIntent::InteractiveRebaseOnto("feat".into()),
            }]
        );
        assert_eq!(
            sections[4].entries,
            [MenuEntry {
                label: "feat".into(),
                intent: MenuIntent::AiRebaseOnto("feat".into()),
            }]
        );
        // Merge entries stay explicitly named (like Delete): the direction
        // would otherwise be ambiguous under the nested title.
        assert_eq!(
            sections[5].entries,
            [MenuEntry {
                label: "Merge feat into main".into(),
                intent: MenuIntent::Merge("feat".into()),
            }]
        );
        // Rename applies to every local ref (the current branch included), so
        // both branches appear under the nested title.
        assert_eq!(
            sections[6]
                .entries
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            ["main", "feat"]
        );
        assert_eq!(sections[7].entries.len(), 2);
        assert_eq!(sections[8].entries[0].label, "Delete feat");
    }

    #[test]
    fn detached_head_offers_no_merge_entry() {
        // No checked-out branch to merge into: the section is dropped, the
        // other actions stay (the domain refuses a detached rebase at run
        // time, but a "Merge feat into ?" entry cannot even be labeled).
        let sections = branch_sections(&[menu_branch_named("feat")], None, false);
        assert!(
            sections
                .iter()
                .flat_map(|s| &s.entries)
                .all(|e| !matches!(e.intent, MenuIntent::Merge(_))),
            "no Merge entry without a checked-out branch"
        );
    }

    #[test]
    fn empty_sections_are_dropped() {
        // HEAD alone: no checkout, rebase nor deletion — only the always-on
        // Create branch, Rename (the current branch renames too) and Copy remain
        // (their empty siblings are dropped).
        let head = MenuBranch {
            checkout: false,
            rebase_onto: false,
            delete_local: None,
            ..menu_branch_named("main")
        };
        let sections = branch_sections(&[head], Some("main"), false);
        assert_eq!(
            sections
                .iter()
                .flat_map(|s| &s.entries)
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            ["Create branch", "Rename", "Copy branch name"]
        );
    }

    #[test]
    fn rename_target_offers_local_branches_including_the_current_one() {
        assert_eq!(rename_target(&graph_ref("feat")), Some("feat"));
        let head = GraphRef {
            is_head: true,
            ..graph_ref("main")
        };
        assert_eq!(rename_target(&head), Some("main"), "current branch renames");
        let remote = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/feat")
        };
        assert_eq!(
            rename_target(&remote),
            None,
            "remote: push + delete, not -m"
        );
        let tag = GraphRef {
            kind: RefKind::Tag,
            ..graph_ref("v1.0")
        };
        assert_eq!(rename_target(&tag), None);
        let detached = GraphRef {
            is_head: true,
            ..graph_ref("HEAD")
        };
        assert_eq!(rename_target(&detached), None, "detached marker, not a ref");
    }

    #[test]
    fn branch_menu_rename_entry_anchors_on_the_row_and_carries_the_name() {
        let sections = branch_sections(&[menu_branch_named("feat")], Some("main"), false);
        let rename = sections
            .iter()
            .flat_map(|s| &s.entries)
            .find(|e| matches!(e.intent, MenuIntent::Rename(_)))
            .expect("a local branch offers Rename");
        assert_eq!(rename.label, "Rename");
        assert_eq!(
            rename.intent,
            MenuIntent::Rename(RenameRequest {
                oid: test_oid(),
                name: "feat".into(),
            })
        );
    }

    #[test]
    fn tag_menu_offers_checkout_create_branch_and_the_tag_actions() {
        let tag = GraphRef {
            kind: RefKind::Tag,
            ..graph_ref("v1.2")
        };
        let sections = branch_sections(&[menu_branch(&tag, test_oid())], Some("main"), false);
        let entries: Vec<(&str, &MenuIntent)> = sections
            .iter()
            .flat_map(|s| &s.entries)
            .map(|e| (e.label.as_str(), &e.intent))
            .collect();
        // A lone tag: no worktree/rebase/merge/branch-delete — Checkout (detached),
        // Create branch, then the three tag-only entries, all flat.
        assert_eq!(
            entries,
            [
                ("Checkout", &MenuIntent::CheckoutTag("v1.2".into())),
                (
                    "Create branch",
                    &MenuIntent::CreateBranch(CreateBranchRequest {
                        oid: test_oid(),
                        source: "refs/tags/v1.2".into(),
                    })
                ),
                ("Copy tag name", &MenuIntent::CopyTagName("v1.2".into())),
                ("Push tag", &MenuIntent::PushTag("v1.2".into())),
                ("Delete tag", &MenuIntent::DeleteTag("v1.2".into())),
            ]
        );
        assert!(
            sections.iter().all(|s| s.title.is_none()),
            "a lone tag never nests its actions"
        );
    }

    #[test]
    fn mixed_branch_and_tag_row_nests_tags_in_their_own_submenus() {
        // A branch and a tag on the same row: the Checkout submenu lists both (the
        // tag detaches), and the tag-only actions get their own titled submenus.
        let tag = GraphRef {
            kind: RefKind::Tag,
            ..graph_ref("v1.2")
        };
        let sections = branch_sections(
            &[menu_branch_named("feat"), menu_branch(&tag, test_oid())],
            Some("main"),
            false,
        );
        let by_title = |title: &str| {
            sections
                .iter()
                .find(|s| s.title.as_deref() == Some(title))
                .unwrap_or_else(|| panic!("missing {title} submenu"))
        };
        // Checkout submenu mixes the branch (normal) and the tag (detached).
        assert_eq!(
            by_title("Checkout").entries,
            [
                MenuEntry {
                    label: "feat".into(),
                    intent: MenuIntent::Checkout("feat".into()),
                },
                MenuEntry {
                    label: "v1.2".into(),
                    intent: MenuIntent::CheckoutTag("v1.2".into()),
                },
            ]
        );
        // The tag-only submenus carry only the tag.
        for (title, intent) in [
            ("Copy tag name", MenuIntent::CopyTagName("v1.2".into())),
            ("Push tag", MenuIntent::PushTag("v1.2".into())),
            ("Delete tag", MenuIntent::DeleteTag("v1.2".into())),
        ] {
            assert_eq!(
                by_title(title).entries,
                [MenuEntry {
                    label: "v1.2".into(),
                    intent,
                }]
            );
        }
    }

    #[test]
    fn create_branch_target_qualifies_the_source_and_skips_origin_head() {
        let remote = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/feat")
        };
        assert_eq!(
            create_branch_target(&remote, test_oid()),
            Some(CreateBranchRequest {
                oid: test_oid(),
                source: "refs/remotes/origin/feat".into(),
            })
        );
        let symref = GraphRef {
            kind: RefKind::Remote,
            ..graph_ref("origin/HEAD")
        };
        assert_eq!(create_branch_target(&symref, test_oid()), None);
    }

    #[test]
    fn delete_entries_name_local_remote_then_combined() {
        let twinned = MenuBranch {
            delete_remote: Some("origin/feat".to_string()),
            ..menu_branch_named("feat")
        };
        let entries = delete_entries(&twinned);
        assert_eq!(
            entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
            [
                "Delete feat",
                "Delete origin/feat",
                "Delete feat and origin/feat"
            ]
        );
        assert_eq!(
            entries[2].intent,
            MenuIntent::Delete(DeleteBranchTarget::Both {
                local: "feat".into(),
                remote: "origin/feat".into(),
            })
        );
    }

    #[test]
    fn stash_sections_offer_apply_pop_then_delete() {
        let stash = StashTarget {
            oid: git2::Oid::ZERO_SHA1,
            summary: "WIP on main".into(),
        };
        let sections = stash_sections(&stash);
        // Apply/Pop, then — past a separator — the destructive Delete: two
        // buckets so [`grouped_sections`] draws a divider between them.
        assert!(sections.iter().all(|s| s.title.is_none()));
        assert_eq!(
            sections
                .iter()
                .flat_map(|s| &s.entries)
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            ["Apply stash", "Pop stash", "Delete stash"]
        );
        assert_eq!(sections[0].group, MenuGroup::Refs);
        assert_eq!(sections[1].group, MenuGroup::Delete);
        assert_eq!(
            sections[0].entries[0].intent,
            MenuIntent::StashApply(stash.oid)
        );
        assert_eq!(
            sections[0].entries[1].intent,
            MenuIntent::StashPop(stash.oid)
        );
        assert_eq!(sections[1].entries[0].intent, MenuIntent::StashDrop(stash));
    }

    fn test_commit(refs: Vec<GraphRef>, stash: bool) -> GraphCommit {
        GraphCommit {
            oid: test_oid(),
            short_id: "0000007".into(),
            summary: "WIP on main: work".into(),
            body: String::new(),
            author: "Ada".into(),
            time: 0,
            parents: vec![],
            refs,
            stash,
        }
    }

    #[test]
    fn commit_sections_offer_copy_sha_create_tag_then_cherry_pick_revert() {
        let commit = test_commit(vec![], false);
        let sections = commit_sections(&commit, Some("main"));
        // One section per action (each joins its own bucket); the flat ones in
        // build order, plus the titled Reset submenu (its own test).
        let flat: Vec<(&str, MenuIntent)> = sections
            .iter()
            .filter(|s| s.title.is_none())
            .flat_map(|s| &s.entries)
            .map(|e| (e.label.as_str(), e.intent.clone()))
            .collect();
        assert_eq!(
            flat,
            [
                ("Copy commit SHA", MenuIntent::CopyCommitSha(test_oid())),
                ("Create tag", MenuIntent::CreateTag(test_oid())),
                ("Cherry-pick", MenuIntent::CherryPick(test_oid())),
                ("Revert", MenuIntent::Revert(test_oid())),
            ],
        );
        assert!(sections
            .iter()
            .any(|s| s.title.as_deref() == Some("Reset main to here")));
    }

    #[test]
    fn commit_sections_reset_submenu_names_the_branch_and_nests_the_three_modes() {
        let commit = test_commit(vec![], false);
        let sections = commit_sections(&commit, Some("main"));
        let reset = sections
            .iter()
            .find(|s| s.title.as_deref() == Some("Reset main to here"))
            .expect("the Reset submenu is present on a branch");
        assert_eq!(
            reset
                .entries
                .iter()
                .map(|e| (e.label.as_str(), e.intent.clone()))
                .collect::<Vec<_>>(),
            [
                ("Soft", MenuIntent::Reset(test_oid(), git2::ResetType::Soft)),
                (
                    "Mixed",
                    MenuIntent::Reset(test_oid(), git2::ResetType::Mixed)
                ),
                ("Hard", MenuIntent::Reset(test_oid(), git2::ResetType::Hard)),
            ],
        );

        // A merge target is a legitimate reset destination: the submenu stays.
        let merge = GraphCommit {
            parents: vec![test_oid(), test_oid()],
            ..test_commit(vec![], false)
        };
        assert!(commit_sections(&merge, Some("main"))
            .iter()
            .any(|s| s.title.as_deref() == Some("Reset main to here")));
    }

    #[test]
    fn commit_sections_off_branch_drop_replay_and_reset_entirely() {
        // Detached HEAD: nothing to replay onto and no branch to move, only the
        // always-present commit actions remain (no Reset submenu).
        let commit = test_commit(vec![], false);
        let sections = commit_sections(&commit, None);
        assert!(sections.iter().all(|s| s.title.is_none()));
        assert_eq!(
            sections
                .iter()
                .flat_map(|s| &s.entries)
                .map(|e| e.label.clone())
                .collect::<Vec<_>>(),
            ["Copy commit SHA", "Create tag"]
        );
    }

    #[test]
    fn commit_sections_drop_cherry_pick_revert_on_a_merge() {
        // Merge commit (two parents): the mainline is ambiguous, replay refused —
        // but Reset still targets it (submenu present, see its own test).
        let merge = GraphCommit {
            parents: vec![test_oid(), test_oid()],
            ..test_commit(vec![], false)
        };
        assert_eq!(
            commit_sections(&merge, Some("main"))
                .iter()
                .filter(|s| s.title.is_none())
                .flat_map(|s| &s.entries)
                .map(|e| e.label.clone())
                .collect::<Vec<_>>(),
            ["Copy commit SHA", "Create tag"]
        );
    }

    #[test]
    fn row_menu_prepends_commit_actions_per_row_kind() {
        let pos = Some(egui::pos2(1.0, 1.0));

        // Ref-less row: the commit sections alone — the menu now opens.
        let refless_commit = test_commit(vec![], false);
        let refless = row_menu(&refless_commit, pos, Some("main"), false).unwrap();
        assert_eq!(
            refless.sections,
            commit_sections(&refless_commit, Some("main"))
        );

        // Ref-bearing row: commit actions first, then the ref actions.
        let with_ref_commit = test_commit(vec![graph_ref("feat")], false);
        let with_ref = row_menu(&with_ref_commit, pos, Some("main"), false).unwrap();
        assert_eq!(
            with_ref.sections[0],
            commit_sections(&with_ref_commit, Some("main"))[0]
        );
        assert!(
            with_ref.sections.len() > 1,
            "the ref actions follow the commit section"
        );

        // Stash row: its own Apply/Pop/Delete entries, never the commit section.
        let stash = row_menu(&test_commit(vec![], true), pos, Some("main"), false).unwrap();
        assert!(stash
            .sections
            .iter()
            .all(|s| !s.entries.iter().any(|e| e.label == "Copy commit SHA")));
        assert_eq!(stash.sections[0].entries[0].label, "Apply stash");
    }

    #[test]
    fn grouped_sections_orders_by_bucket_for_the_separators() {
        // Build order interleaves the buckets (commit copy first, then the refs,
        // the history rewrites…); grouping reorders them into the MenuGroup
        // sequence so chip_menu can drop a separator at every bucket change.
        let commit = test_commit(vec![graph_ref("feat")], false);
        let menu = row_menu(&commit, Some(egui::pos2(1.0, 1.0)), Some("main"), false).unwrap();
        let ordered = grouped_sections(&menu.sections);
        let groups: Vec<MenuGroup> = ordered.iter().map(|s| s.group).collect();
        assert!(
            groups.windows(2).all(|w| w[0] <= w[1]),
            "render order follows the bucket order: {groups:?}"
        );
        // Refs lead (Checkout on top), the copies are grouped lower, Delete last.
        assert_eq!(ordered.first().unwrap().entries[0].label, "Checkout");
        assert_eq!(ordered.last().unwrap().group, MenuGroup::Delete);
        let pos = |label: &str| {
            ordered
                .iter()
                .position(|s| s.entries.iter().any(|e| e.label == label))
                .unwrap()
        };
        assert!(
            pos("Copy commit SHA") < pos("Delete feat"),
            "the copies sit above the destructive Delete"
        );
    }

    #[test]
    fn head_chip_ink_is_pure_white_and_stands_out() {
        for palette in [Palette::light(), Palette::dark()] {
            assert_eq!(chip_ink(&palette, true), egui::Color32::WHITE);
            assert_ne!(chip_ink(&palette, false), chip_ink(&palette, true));
        }
    }

    #[test]
    fn chip_fill_follows_the_theme_mode() {
        let light = Palette::light();
        let dark = Palette::dark();
        let lane = light.lane_color(0);
        assert_eq!(chip_fill(&light, lane), lane);
        assert_eq!(
            chip_fill(&dark, dark.lane_color(0)),
            darkened(dark.lane_color(0))
        );
    }

    /// WCAG relative luminance — used to guarantee a readable ink/background gap.
    fn luminance(color: egui::Color32) -> f32 {
        let [r, g, b, _] = color.to_srgba_unmultiplied();
        let lin = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    #[test]
    fn chip_ink_reads_on_chip_fill_in_every_preset() {
        for preset in &crate::theme::PRESETS {
            let p = &preset.palette;
            for lane in 0..p.lane_colors.len() {
                let fill = chip_fill(p, p.lane_color(lane));
                let gap = (luminance(chip_ink(p, true)) - luminance(fill)).abs();
                assert!(
                    gap >= 0.25,
                    "{} lane {lane}: unreadable chip ink (gap {gap:.2})",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn lane_ink_reads_on_darkened_nodes_in_every_preset() {
        for preset in &crate::theme::PRESETS {
            let p = &preset.palette;
            for lane in 0..p.lane_colors.len() {
                let fill = darkened(p.lane_color(lane));
                let gap = (luminance(lane_ink(p)) - luminance(fill)).abs();
                assert!(
                    gap >= 0.25,
                    "{} lane {lane}: unreadable initials (gap {gap:.2})",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn wip_label_counts_files() {
        assert_eq!(wip_label(1), "// WIP · 1 file");
        assert_eq!(wip_label(3), "// WIP · 3 files");
    }

    #[test]
    fn initials_take_first_and_last_word() {
        assert_eq!(initials("Maxime Gomez-Duret"), "MG");
        assert_eq!(initials("jean dupont"), "JD");
        assert_eq!(initials("Anna Maria van der Berg"), "AB");
    }

    #[test]
    fn initials_single_word_takes_two_chars() {
        assert_eq!(initials("legirard1"), "LE");
        assert_eq!(initials("x"), "X");
    }

    #[test]
    fn initials_empty_author_yields_empty() {
        assert_eq!(initials(""), "");
        assert_eq!(initials("   "), "");
    }
}
