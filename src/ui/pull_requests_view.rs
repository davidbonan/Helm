//! Rendering for the Pull Requests cockpit (pull-requests.md §5/§11). Two states:
//! a **browse** list of the workspace PRs grouped **To review** then **Mine**, and
//! — once a row is opened — a **review** surface: the center area holds the open
//! file's read-only diff, or the PR detail (compact header, author, body, checks,
//! conversation and PR-level Open / Checkout actions) when no file is selected; a
//! changed-files rail on the right carries the file list and the composer (so
//! collapsing the rail from the title bar hides the review apparatus). Pure
//! `fn(&mut egui::Ui, …)`: the app owns the cache, the selection, the fetched
//! detail/diff and the persisted rail width, and consumes the returned intents.

use lucide_icons::Icon;
use std::collections::{HashMap, HashSet};

use crate::git::commit_detail::CommitFile;
use crate::git::diff::{FileDiff, LineOrigin};
use crate::git::file_tree::{self, TreeRow};
use crate::pull_requests::model::{
    hunk_snippet, ActionGroup, Checks, PrComment, PrCommit, PrDetail, PrState, PullRequest, Review,
    ReviewVerdict, SnippetKind, SnippetLine, StackRow,
};
use crate::review::{FileComments, ForgeThreads, ReviewIntent};
use crate::theme::{Palette, PILL_SIZE, RADIUS_BUTTON, SECTION_TITLE_SIZE};
use crate::ui::detail::{author_avatar, author_avatar_small, code_snippet, count_chip};
use crate::ui::diff_view::{
    reply_editor, reply_pill, resolve_pill, ConversationEdit, DiffReview, DiffViewState, ReplyEdit,
    REPLY_LABELS,
};
use crate::ui::file_list::{self, file_row, row_separator, FileRow, FileViewMode};
use crate::ui::spinner::Spinner;
use crate::ui::{clickable, paint_icon, with_alpha, SECTION_TOP_MARGIN, TITLEBAR_HEIGHT};

/// Review-surface split bounds: the changed-files rail and the diff each keep a
/// floor; the persisted rail width is clamped between them.
const RAIL_MIN_WIDTH: f32 = 260.0;
const DIFF_MIN_WIDTH: f32 = 420.0;

/// Markdown reading tweaks for the in-house renderer (`markdown`): the body reads
/// smaller than the egui_commonmark default but with looser line-height and a touch
/// of letter-spacing, so long prose blocks don't read as a dense wall.
const MD_TEXT_SIZE: f32 = 14.0;
const MD_CODE_SIZE: f32 = 13.0;
const MD_LINE_HEIGHT: f32 = MD_TEXT_SIZE * 1.55;
const MD_LETTER_SPACING: f32 = 0.4;
const MD_PARAGRAPH_GAP: f32 = 7.0;
const MD_LIST_INDENT: f32 = 18.0;
const MD_QUOTE_INDENT: f32 = 12.0;
const DETAIL_HEADER_TITLE_SIZE: f32 = 16.0;
const DETAIL_HEADER_SUBTITLE_SIZE: f32 = 12.5;
const DETAIL_ACTION_HEIGHT: f32 = 30.0;

/// The review surface's full-width header (pull-requests.md §11): identity + actions,
/// the branch/author line, then the tab bar.
const REVIEW_HEADER_ROW1: f32 = 44.0;
const REVIEW_HEADER_ROW2: f32 = 28.0;
const REVIEW_HEADER_ROW3: f32 = 36.0;
const REVIEW_HEADER_HEIGHT: f32 = REVIEW_HEADER_ROW1 + REVIEW_HEADER_ROW2 + REVIEW_HEADER_ROW3;
const MERGE_HEADER_W: f32 = 98.0;
const FINISH_BTN_W: f32 = 116.0;
const VERDICT_ICON_W: f32 = 34.0;
const SUB_AVATAR: f32 = 18.0;
/// Width the Finish-review popover asks for, so the summary field has room.
const COMPOSER_POPOVER_W: f32 = 300.0;

/// Files-tab geometry (pull-requests.md §11): the toolbar strip over the diff and
/// the widths of its two controls.
const DIFF_TOOLBAR_HEIGHT: f32 = 42.0;
const COMMIT_SCOPE_W: f32 = 240.0;
const THREAD_NAV_W: f32 = 168.0;

const ROW_HEIGHT: f32 = 62.0;
const GROUP_HEADER_HEIGHT: f32 = 46.0;
const PAD_X: f32 = 16.0;
const PANEL_PAD_X: f32 = 18.0;
const PANEL_PAD_Y: f32 = 14.0;
/// Reading measure of the Conversation tab's **prose** (pull-requests.md §11). The
/// cards themselves span the area's full width — only the text inside them wraps at
/// this measure, so a wide window leaves breathing room *inside* a surface rather than
/// a hole between two floating blocks.
const CONV_PROSE_MEASURE: f32 = 900.0;
/// Bound of the Conversation tab's **cards** (§11) — a step and a half past the prose
/// measure, so a surface never dwarfs the text it holds.
const CONV_COLUMN_MAX: f32 = 1200.0;
/// The metadata rail of the Conversation tab (§11): it trails the conversation column
/// across the gutter rather than clinging to the pane's right edge, so the two stay one
/// block instead of drifting into two islands on a wide window. It stands down when the
/// conversation would be left below `CONV_COLUMN_MIN`.
const CONV_RAIL_WIDTH: f32 = 300.0;
const CONV_RAIL_GUTTER: f32 = 32.0;
const CONV_COLUMN_MIN: f32 = 520.0;
/// How far into the gutter the conversation's scroll area reaches, so the bar egui
/// floats against its right edge sits clear of the cards — wide enough that the bar
/// expanded on hover (10px, 4px off the edge) still clears them.
const SCROLL_LANE: f32 = 16.0;

const TITLE_SIZE: f32 = 14.0;
const META_SIZE: f32 = 12.0;
const CHIP_SIZE: f32 = 11.5;
const HEADER_SIZE: f32 = 11.0;
const STATE_ICON: f32 = 16.0;
const STATUS_ICON: f32 = 15.0;
const COUNT_BADGE_SIZE: f32 = 11.0;

/// Browse-list page header (pull-requests.md §5): the title + search + controls
/// strip, then the tab bar beneath it.
const PAGE_HEADER_HEIGHT: f32 = 52.0;
const LIST_TABS_HEIGHT: f32 = 38.0;
const PAGE_TITLE_SIZE: f32 = 20.0;
/// Height of the header's outlined controls (search, Filters, Priority, Refresh).
const CTRL_HEIGHT: f32 = 30.0;
const FILTER_BTN_W: f32 = 104.0;
const SORT_BTN_W: f32 = 154.0;
const REFRESH_BTN_W: f32 = 96.0;
const SEARCH_MAX_W: f32 = 340.0;
const TAB_PAD_X: f32 = 12.0;

/// Row geometry for the browse list (pull-requests.md §5). The right-hand data
/// cluster is laid out from the right edge; the title column takes what is left.
const ROW_AVATAR: f32 = 26.0;
const COL_COMMENTS_W: f32 = 50.0;
const MERGE_BTN_W: f32 = 90.0;
const MERGE_BTN_H: f32 = 28.0;
const CARD_RADIUS: u8 = 10;
/// Pitch of the row's reviewer cluster. Its own, not the review detail's tighter
/// `REVIEWER_OVERLAP`: here each avatar also carries a verdict badge on the slice its
/// neighbour laps, so the discs need to stand further apart to stay legible.
const ROW_REVIEWER_STEP: f32 = 16.0;
/// Room the row reserves for the cluster: `REVIEWER_MAX` avatars plus the overflow
/// disc. Fixed, so the clusters line up down the list however many reviewers a row has.
const COL_REVIEWERS_W: f32 = REVIEWER_AVATAR + 3.0 * ROW_REVIEWER_STEP;
/// Radius of the verdict badge riding a reviewer avatar's top-right corner.
const VERDICT_BADGE_R: f32 = 5.0;
/// Reading measure of the list column: past this the rows stop being scannable, the
/// eye having to travel from a title on the far left to its avatar on the far right.
const LIST_MAX_WIDTH: f32 = 1280.0;
/// A band's rows sit in bordered blocks — one per stack, one for everything loose
/// (pull-requests.md §5) — so a stack reads as a unit rather than as neighbours.
const LIST_BLOCK_RADIUS: u8 = 8;
const LIST_BLOCK_GAP: f32 = 14.0;
const STACK_HEADER_HEIGHT: f32 = 42.0;
/// The row's leading column: the state glyph for a loose PR, the spine and its
/// numbered badge for a stacked one.
const ROW_GUTTER: f32 = 34.0;
const STACK_BADGE: f32 = 19.0;
const ROW_COL_GAP: f32 = 12.0;
const ROW_PAD_R: f32 = 16.0;
const LIST_TITLE_SIZE: f32 = 14.5;
const KEY_SIZE: f32 = 12.0;
/// Half the pitch between a row's title and meta lines.
const ROW_LINE_STEP: f32 = 9.5;
/// Text scale of a row's second line and of the list's chips — its own step, half a
/// point over the `CHIP_SIZE` / `COUNT_BADGE_SIZE` the review surface uses: the browse
/// list is scanned at arm's length, not leant into like a diff.
const LIST_META_SIZE: f32 = 12.5;
const LIST_MONO_SIZE: f32 = 12.0;
const REVIEWER_AVATAR: f32 = 22.0;
const REVIEWER_OVERLAP: f32 = 8.0;
const REVIEWER_MAX: usize = 3;

/// Lines of code previewed atop an inline-comment card (pull-requests.md §5).
const INLINE_SNIPPET_LINES: usize = 8;
/// Extra indent a reply nests under its thread root in the center cards — sized so the
/// thread rail falls on the root avatar's centre, reading as a spine descending from it (§11).
const INLINE_REPLY_INDENT: f32 = 26.0;
/// Conversation-card geometry (pull-requests.md §11, drawn to the `PR Conversation`
/// canvas): the card's own padding, its title, and the composer's action bar.
const CONV_CARD_PAD: i8 = 18;
const COMPOSER_BAR_HEIGHT: f32 = 40.0;
const COMPOSER_PAD_X: f32 = 12.0;
const COMPOSER_PAD_Y: f32 = 10.0;
/// Radius of a block nested inside the conversation card. Concentric with it rather than
/// equal to it: the card is `CARD_RADIUS` with `CONV_CARD_PAD` of padding, so a surface
/// sitting in that padding has to read a step tighter, and an input nested in *that*
/// tighter again (`diff_view::EDITOR_RADIUS`).
const BLOCK_RADIUS: u8 = 8;
/// Text scale of the conversation card: the canvas's own 15 / 13.5 / 12.5 steps, a
/// notch above the browse list's metrics — a thread is prose to read, not a table to
/// scan, and the list's 11–12pt ink was not legible at arm's length.
const CONV_TITLE_SIZE: f32 = 15.0;
const CONV_AUTHOR_SIZE: f32 = 13.5;
const CONV_META_SIZE: f32 = 12.5;
const CONV_MONO_SIZE: f32 = 12.5;
/// The resolved block: its summary header, one folded row per thread, the indent those
/// rows and their opened bodies share, and the participants' avatar stack.
const RESOLVED_HEADER_HEIGHT: f32 = 42.0;
const RESOLVED_ROW_HEIGHT: f32 = 62.0;
const RESOLVED_ROW_INDENT: f32 = 36.0;
const RESOLVED_AVATAR: f32 = 20.0;
const RESOLVED_AVATAR_MAX: usize = 3;

/// Vertical rhythm for the comment cards (pull-requests.md §11): one small scale so the
/// conversation and inline threads breathe on the same beat instead of ad-hoc gaps.
const GAP_XS: f32 = 4.0;
const GAP_SM: f32 = 8.0;
/// Air between two file bands in the Files column. The header strip's own rules do
/// the separating, so this is breathing room, not a boundary (pull-requests.md §11).
const BAND_GAP: f32 = 10.0;
const GAP_MD: f32 = 12.0;
const GAP_LG: f32 = 18.0;
/// Gap between a comment's avatar gutter and its text column (§11).
const AVATAR_GUTTER_GAP: f32 = 10.0;

const AVATAR_GAP: f32 = 9.0;

/// Which commit band row a click in the review rail targeted (per-commit diff: T5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitSelection {
    /// The "All commits" row — the cumulative three-dot diff.
    All,
    /// A single commit by its full sha — its `commit^..commit` delta.
    Commit(String),
}

/// What a click on the cockpit targeted. Collected into one struct and returned
/// each frame; every field is an independent `Option`, mirroring
/// `AgentsPageAction`.
#[derive(Default)]
pub struct PullRequestsPageAction {
    /// A list row was clicked: the app opens it in the review surface.
    pub select: Option<usize>,
    /// **Open in browser** was clicked: the app opens this PR's URL.
    pub open_url: Option<String>,
    /// **Checkout** was clicked in the review surface: the app brings the open
    /// PR's branch up as a worktree (pull-requests.md §7), resolved from the open
    /// review (not a list index, which a concurrent refresh could have shifted).
    pub checkout: bool,
    /// The review surface's **Back** (or `Esc` with no file open) was hit: return
    /// to the list.
    pub back: bool,
    /// The open file's **Close** (or `Esc` over its diff) was hit: drop the file
    /// selection, returning the diff area to its "select a file" placeholder
    /// without leaving the review surface.
    pub close_file: bool,
    /// A changed-file row was clicked in the review rail: load its diff.
    pub select_file: Option<usize>,
    /// A commit band row was clicked: switch the diff range to that commit (or back
    /// to "All commits").
    pub select_commit: Option<CommitSelection>,
    /// An inline-comment card in the center was clicked: open its file (the changed-
    /// file index) and, when known, scroll the diff to its new-side line (§5).
    pub open_inline_comment: Option<(usize, Option<u32>)>,
    /// The rail/diff split was dragged: the app stores and persists the new width.
    pub set_detail_width: Option<f32>,
    /// Flat ⇄ tree view for the changed-files rail. Shared with the Git sidebar
    /// and commit detail via `Prefs.git_file_view`.
    pub set_file_view: Option<FileViewMode>,
    /// Draft-review actions the embedded diff raised (save / delete a line note,
    /// send to agent) — the app applies them to the PR's draft store (§11).
    pub review_intents: Vec<ReviewIntent>,
    /// **Submit review** in the composer was clicked: the app posts the draft
    /// comments + verdict + summary to the forge (§11).
    pub submit_review: bool,
    /// The list header's **Refresh** button was clicked: re-fetch the workspace PRs.
    pub refresh: bool,
    /// **Merge** was clicked on a ready-to-merge list row: the app merges that PR on
    /// the forge after confirmation (pull-requests.md §5).
    pub merge: Option<usize>,
    /// **Merge** was clicked in the review surface header — the open PR (§11).
    pub merge_open: bool,
}

/// Per-source banners for the browse list (pull-requests.md §5): each forge's
/// one-line unavailability hint (`None` when usable, so the other source still
/// lists), whether the workspace has no recognized-forge repo at all, and whether
/// a fetch is in flight — the latter drives the loader instead of the empty state
/// (the cold cache reads `Absent`/`Absent`, indistinguishable from "no repos").
#[derive(Default)]
pub struct PrSourceHints<'a> {
    pub github: Option<&'a str>,
    pub bitbucket: Option<&'a str>,
    pub no_repos: bool,
    pub loading: bool,
    /// Unix seconds of the last landed fetch, for the header's age note beside
    /// **Refresh**; `None` before the first one.
    pub refreshed_at: Option<i64>,
}

/// Everything the review surface renders for the open PR. The app owns the state;
/// this is the per-frame borrow it hands the view (diff scroll state is `&mut` so
/// the view can record it). Loading/error flags drive the placeholders.
pub struct PrReviewView<'a> {
    pub pr: &'a PullRequest,
    pub detail: Option<&'a PrDetail>,
    /// The forge detail (body/comments/checks/commits) is still in flight — the
    /// center shows a loader rather than the (empty, misleading) detail sections.
    pub detail_loading: bool,
    pub detail_error: Option<&'a str>,
    pub files: &'a [CommitFile],
    pub files_loading: bool,
    pub files_error: Option<&'a str>,
    pub selected_file: Option<usize>,
    /// The PR's commits (oldest-first) for the rail's commit band; empty until the
    /// detail loads. `selected_commit` is the chosen commit's sha, or `None` for the
    /// cumulative "All commits" range (per-commit diff: T5).
    pub commits: &'a [PrCommit],
    pub selected_commit: Option<&'a str>,
    /// The diff of each changed file, in `files` order — `None` while it is still in
    /// flight. The Files tab stacks them all in one column (pull-requests.md §11).
    pub diffs: Vec<Option<&'a FileDiff>>,
    /// Per-file fetch failures, in `files` order: one band reports its own error
    /// while the rest of the column renders.
    pub diff_errors: Vec<Option<&'a str>>,
    /// Local diffs for the current range's files that carry an inline comment, so the
    /// center inline cards render a code preview even when the file isn't open — the
    /// Bitbucket case, which has no forge `diff_hunk` (pull-requests.md §5).
    pub comment_diffs: Vec<&'a FileDiff>,
    /// One-shot file the column must bring into view (a rail click, or opening an
    /// inline comment). Cleared by the view once it has scrolled to that band.
    pub scroll_to_file: &'a mut Option<usize>,
    /// Render state **per file** — a `DiffViewState` caches one file's highlighting
    /// and holds its open editors, and the column draws every file at once.
    pub file_views: &'a mut HashMap<String, DiffViewState>,
    /// The conversation tab's own state (its reply / comment composers).
    pub diff_view: &'a mut DiffViewState,
    /// Comments already posted on the PR, anchored per line (read-only).
    pub existing: &'a ForgeThreads,
    /// The user's in-progress forge review comments for this PR — the `Submit
    /// review` pool, posted to GitHub / Bitbucket (editable in the diff).
    pub draft: &'a FileComments,
    /// The user's in-progress agent notes for this PR — the `Send to …` pool, kept
    /// apart from `draft` so forge comments are never forced through the agent.
    pub agent_notes: &'a FileComments,
    /// The agent CLI label shown on the diff's **Send to …** review pill.
    pub agent: &'a str,
    /// Composer state (pull-requests.md §11): the chosen verdict and the summary
    /// the view edits in place, plus the in-flight flag and the last post error.
    pub verdict: &'a mut ReviewVerdict,
    pub summary: &'a mut String,
    pub posting: bool,
    pub post_error: Option<&'a str>,
    /// Current user's display name for the conversation composer avatar (§11), or
    /// `None` before the forge identity resolves — then the avatar is a plain dot.
    pub current_user: Option<&'a str>,
}

/// The cockpit page. `review` switches the surface: `None` ⇒ the browse list;
/// `Some` ⇒ the full-width review surface for the open PR (its diff/detail are
/// loaded lazily by the app, shown via placeholders until they land).
#[allow(clippy::too_many_arguments)]
pub fn pull_requests_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    prs: &[PullRequest],
    selected: Option<usize>,
    hints: &PrSourceHints<'_>,
    review: Option<&mut PrReviewView<'_>>,
    rail_width: f32,
    rail_collapsed: bool,
    file_view: FileViewMode,
) -> PullRequestsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let mut action = PullRequestsPageAction::default();

    match review {
        // The review surface spans to the window top so the rail reads as a
        // full-height side panel (its divider reaches the title strip, like the git
        // sidebar); `render_review` insets the diff and rail *content* past the strip.
        Some(review) => {
            let slide = back_slide(ui);
            // Mid-slide both surfaces are on their way somewhere: neither answers a
            // click, and the review must not start a second slide out of the first.
            let mut sink = PullRequestsPageAction::default();
            if slide.is_some() {
                // The list is already underneath, at rest: the review uncovers the page
                // it is going home to rather than cutting to it.
                render_list(
                    ui,
                    palette,
                    prs,
                    selected,
                    hints,
                    list_body_rect(rect),
                    &mut PullRequestsPageAction::default(),
                );
            }
            let shift = slide.map_or(0.0, |eased| eased * rect.width());
            let surface = rect.translate(egui::vec2(shift, 0.0));
            if shift > 0.0 {
                // Both surfaces sit on `bg_canvas`, so without an edge of its own the
                // review would slide off as a seam nobody can see.
                slide_edge(ui, palette, surface);
            }
            render_review(
                ui,
                palette,
                review,
                surface,
                rail_width,
                rail_collapsed,
                file_view,
                if slide.is_some() {
                    &mut sink
                } else {
                    &mut action
                },
            );
            let requested = slide.is_none() && std::mem::take(&mut action.back);
            action.back = advance_back_slide(ui, slide, requested);
        }
        // The browse list owns the central area like the Agents dashboard: the
        // background already reached the window top, so the body is inset past the
        // macOS title strip to align with the side panels (which inset the same way).
        None => {
            // Home: nothing is sliding, and a slide left behind by a surface that went
            // away some other way must not fire the next time a PR opens.
            ui.data_mut(|d| d.remove_temp::<f64>(back_slide_id()));
            ui.add_space(f32::from(TITLEBAR_HEIGHT));
            let body = ui.available_rect_before_wrap();
            render_list(ui, palette, prs, selected, hints, body, &mut action);
        }
    }
    action
}

/// How long the review takes to slide off to the right on its way back to the list.
const BACK_SLIDE_SECS: f64 = 0.22;

fn back_slide_id() -> egui::Id {
    egui::Id::new("pr_back_slide")
}

/// The browse list's area: the same inset past the macOS title strip the `None` arm
/// takes, computed from a rect rather than from the cursor.
fn list_body_rect(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top() + f32::from(TITLEBAR_HEIGHT), rect.bottom()),
    )
}

/// Progress of a running back-slide, eased out — `None` when the review is at rest.
fn back_slide(ui: &egui::Ui) -> Option<f32> {
    let started: f64 = ui.data(|d| d.get_temp(back_slide_id()))?;
    let now = ui.input(|i| i.time);
    let t = (((now - started) / BACK_SLIDE_SECS) as f32).clamp(0.0, 1.0);
    Some(1.0 - (1.0 - t).powi(3))
}

/// Start, keep or land the back-slide. Returns whether the app should leave the review
/// **now** — only once the surface has finished travelling, so the list it lands on is
/// the one that was already showing underneath.
fn advance_back_slide(ui: &egui::Ui, slide: Option<f32>, requested: bool) -> bool {
    match slide {
        Some(eased) if eased >= 1.0 => {
            ui.data_mut(|d| d.remove_temp::<f64>(back_slide_id()));
            true
        }
        Some(_) => {
            ui.ctx().request_repaint();
            false
        }
        None => {
            if requested {
                let now = ui.input(|i| i.time);
                ui.data_mut(|d| d.insert_temp(back_slide_id(), now));
                ui.ctx().request_repaint();
            }
            false
        }
    }
}

/// The leaving surface's left edge: a hairline over a short shadow, so it reads as a
/// panel travelling over the list rather than as a redraw.
fn slide_edge(ui: &egui::Ui, palette: &Palette, surface: egui::Rect) {
    const DEPTH: f32 = 18.0;
    const STEPS: usize = 6;
    let painter = ui.painter();
    let band_w = DEPTH / STEPS as f32;
    for step in 0..STEPS {
        let t = (step as f32 + 1.0) / STEPS as f32;
        let x = surface.left() - DEPTH + band_w * step as f32;
        let band = egui::Rect::from_x_y_ranges(egui::Rangef::new(x, x + band_w), surface.y_range());
        let alpha = (26.0 * t) as u8;
        painter.rect_filled(band, 0, crate::ui::with_alpha(egui::Color32::BLACK, alpha));
    }
    painter.vline(
        surface.left(),
        surface.y_range(),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

/// The review surface (pull-requests.md §11): the center area shows the open
/// file's read-only diff, or the PR detail (compact Back/title/actions header +
/// the author/body/checks/conversation) when no file is selected; a changed-files
/// rail on the **right** — the commit-detail sidebar's place — carries the file
/// list and the composer. The PR-level Open in browser / Checkout actions live in
/// the center detail and disappear with it when a file diff is open. The title-bar
/// toggle collapses the rail, leaving the center full-width.
#[allow(clippy::too_many_arguments)]
fn render_review(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    rail_width: f32,
    rail_collapsed: bool,
    file_view: FileViewMode,
    action: &mut PullRequestsPageAction,
) {
    // An open editor in the column owns `Esc` itself — it rolls that editor back
    // (`diff_view_band`). Read *before* the body renders: by the end of the frame the
    // editor it cancelled is gone, and the same press must not escalate.
    let editing = review.file_views.values().any(|s| s.has_open_editor());

    // A non-finite persisted width (hand-edited prefs) would poison the layout math.
    let rail_width = if rail_width.is_finite() {
        rail_width
    } else {
        RAIL_MIN_WIDTH
    };

    // The surface header spans the full width above both panes, so Back, the PR
    // identity, the review verdict cluster and the tabs stay put whatever the tab
    // shows underneath (pull-requests.md §11).
    let content_top = rect.top() + f32::from(TITLEBAR_HEIGHT);
    let header_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(content_top, content_top + REVIEW_HEADER_HEIGHT),
    );
    let tab = review_header(ui, palette, review, header_rect, action);
    let body = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(header_rect.bottom(), rect.bottom()),
    );

    // The rail *is* the Files tab's list — Conversation gets the full width rather
    // than a file list it has no use for (pull-requests.md §11).
    if rail_collapsed || tab != PrTab::Files {
        review_body(ui, palette, review, body, tab, action);
    } else {
        let rail_w = rail_width.clamp(
            RAIL_MIN_WIDTH,
            (rect.width() - DIFF_MIN_WIDTH).max(RAIL_MIN_WIDTH),
        );
        let split_x = rect.left() + rail_w;
        let rail_rect =
            egui::Rect::from_x_y_ranges(egui::Rangef::new(rect.left(), split_x), body.y_range());
        review_rail(ui, palette, review, rail_rect, file_view, action);

        let center =
            egui::Rect::from_x_y_ranges(egui::Rangef::new(split_x, rect.right()), body.y_range());
        review_body(ui, palette, review, center, tab, action);

        rail_resize_handle(ui, palette, split_x, body, rail_width, action);
    }

    // Over everything the surface just drew, and *before* the `Esc` cascade below: the
    // viewer owns that key while it is up (§11).
    md_image_viewer(ui, palette, rect);

    // Read *after* the body: a composer takes `Esc` for itself and consumes the key
    // (`reply_editor`), so the press that closes a comment field never also drops the
    // file or leaves the review. With nothing to close, `Esc` clears the file
    // selection, then returns to the list (pull-requests.md §11).
    if !editing && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if review.selected_file.is_some() {
            action.close_file = true;
        } else {
            action.back = true;
        }
    }

    // Same place, same reason: the bands have had their say on whether a rightward
    // swipe was theirs to scroll with.
    if swipe_back(ui) {
        action.back = true;
    }
}

/// How far a trackpad swipe must travel, and how much straighter than tall, before it
/// reads as **go back** rather than as a scroll (pull-requests.md §11).
const SWIPE_BACK_MIN_X: f32 = 64.0;
const SWIPE_BACK_RATIO: f32 = 2.0;
/// Gap that tells two gestures apart. macOS restarts the phase cycle for the momentum
/// that trails a flick (`winit`'s macOS view maps momentum `Began` to `TouchPhase::
/// Started`), so a run beginning within this of the last one ending **continues** it:
/// the coast inherits the run's distance and its verdict, instead of reading as a fresh
/// swipe of its own.
const SWIPE_BACK_GAP: f64 = 0.25;

/// A trackpad swipe accumulated across one `TouchPhase` run. macOS has no navigation-
/// swipe event winit surfaces — its `PanGesture` is iOS-only — but a two-finger swipe
/// arrives as a `MouseWheel` in points carrying a real phase, so a run has a start and
/// an end to measure between.
#[derive(Clone, Copy, Default)]
struct SwipeBack {
    delta: egui::Vec2,
    tracking: bool,
    /// The run is disqualified: it ran under a surface that owns the horizontal axis, or
    /// it has already fired and what is left of it is spent.
    void: bool,
    /// When the last run ended, against `InputState::time`; `None` before the first, so
    /// the very first swipe of a session is not measured against the epoch.
    ended_at: Option<f64>,
}

impl SwipeBack {
    /// Fold one wheel event in. `armed` is false while a scrollable surface owns the
    /// horizontal axis under the pointer. Returns true on the event that carries the run
    /// past the threshold — mid-swipe, not on release: macOS trails a flick with a
    /// momentum run whose end lands a second or more later, and a surface that waits for
    /// it reacts long after the fingers have left the trackpad.
    fn feed(&mut self, phase: egui::TouchPhase, delta: egui::Vec2, armed: bool, now: f64) -> bool {
        match phase {
            egui::TouchPhase::Start => {
                let resumed = self
                    .ended_at
                    .is_some_and(|then| now - then < SWIPE_BACK_GAP);
                if !resumed {
                    *self = SwipeBack {
                        ended_at: self.ended_at,
                        ..Default::default()
                    };
                }
                self.tracking = true;
                self.void |= !armed;
                false
            }
            egui::TouchPhase::Move => {
                if !self.tracking {
                    return false;
                }
                self.delta += delta;
                // A run that wanders onto a scrollable line stops being a gesture.
                self.void |= !armed;
                if self.void || !self.completed() {
                    return false;
                }
                // Spent: the rest of the run, and the momentum that continues it, must
                // not fire a second time.
                self.void = true;
                true
            }
            egui::TouchPhase::End => {
                self.tracking = false;
                self.ended_at = Some(now);
                false
            }
            egui::TouchPhase::Cancel => {
                *self = SwipeBack {
                    tracking: false,
                    void: true,
                    ended_at: self.ended_at,
                    ..Default::default()
                };
                false
            }
        }
    }

    /// A push to the right — egui's positive X reveals content to the left, which is
    /// the direction the list is in — far enough and straight enough.
    fn completed(self) -> bool {
        self.delta.x >= SWIPE_BACK_MIN_X && self.delta.x > self.delta.y.abs() * SWIPE_BACK_RATIO
    }
}

/// Drive the back-swipe recognizer off this frame's wheel events. Only **point** deltas
/// count: a line delta is a mouse wheel, which has no gesture to recognize.
fn swipe_back(ui: &egui::Ui) -> bool {
    let armed = !crate::ui::h_scroll_owns_swipe(ui.ctx());
    let id = egui::Id::new("pr_swipe_back");
    let mut swipe: SwipeBack = ui.data(|d| d.get_temp(id).unwrap_or_default());
    let fired = ui.input(|i| {
        let mut fired = false;
        for event in &i.events {
            if let egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                phase,
                modifiers,
            } = event
            {
                if modifiers.is_none() {
                    fired |= swipe.feed(*phase, *delta, armed, i.time);
                }
            }
        }
        fired
    });
    ui.data_mut(|d| d.insert_temp(id, swipe));
    fired
}

/// Which face of the review surface the center shows (pull-requests.md §11).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PrTab {
    /// The PR detail: description, checks, conversation and inline comments.
    #[default]
    Conversation,
    /// The changed files: the rail lists them, the center diffs the selected one.
    Files,
}

impl PrTab {
    const ALL: [PrTab; 2] = [PrTab::Conversation, PrTab::Files];

    fn label(self) -> &'static str {
        match self {
            PrTab::Conversation => "Conversation",
            PrTab::Files => "Files",
        }
    }
}

/// The center pane, dispatched on the open tab.
fn review_body(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    tab: PrTab,
    action: &mut PullRequestsPageAction,
) {
    match tab {
        PrTab::Conversation => {
            ui.painter().rect_filled(rect, 0, palette.bg_canvas);
            // Conversation and rail form one block, **centered in the pane** the way a
            // forge centers its review page: the cards stop a little past the prose
            // measure (a surface three times wider than the text it holds reads as a
            // rendering slip), the rail follows one gutter later, and what a wide window
            // has left over is split between the two sides instead of pooling into a
            // hole between them (§11).
            let column = (rect.width() - CONV_RAIL_GUTTER - CONV_RAIL_WIDTH).min(CONV_COLUMN_MAX);
            let rail = column >= CONV_COLUMN_MIN;
            let block = if rail {
                column + CONV_RAIL_GUTTER + CONV_RAIL_WIDTH
            } else {
                rect.width()
            };
            let left = rect.left() + (rect.width() - block) / 2.0;
            let right = left + if rail { column } else { rect.width() };
            let content =
                egui::Rect::from_x_y_ranges(egui::Rangef::new(left, right), rect.y_range());
            let mut area = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            review_detail(&mut area, palette, review, content, rail, action);
            if rail {
                let split_x = right + CONV_RAIL_GUTTER;
                let rail_rect = egui::Rect::from_x_y_ranges(
                    egui::Rangef::new(split_x, split_x + CONV_RAIL_WIDTH),
                    rect.y_range(),
                );
                conversation_rail(ui, palette, review, rail_rect);
            }
        }
        PrTab::Files => review_diff(ui, palette, review, rect, action),
    }
}

/// Left rail of the **Files** tab (git.md §9 visual language): a *Files changed*
/// band (count chip, filters, ±totals, ratio bar) over the file list. It is the
/// review's only list of changed files — the center diffs whichever row is
/// selected, and the other tabs don't draw the rail at all.
fn review_rail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    file_view: FileViewMode,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let inner = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(rect.left() + PANEL_PAD_X, rect.right() - 6.0),
        rect.y_range(),
    );
    let mut panel = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("pr_review_rail")
        .show(&mut panel, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(PANEL_PAD_Y);
            let collapse_id = egui::Id::new(("pr_review_dirs", review.pr.url.as_str()));
            let mut collapsed: HashSet<String> =
                ui.data(|d| d.get_temp(collapse_id).unwrap_or_default());
            let viewed_id = egui::Id::new(("pr_review_viewed_files", review.pr.url.as_str()));
            let mut viewed: HashSet<String> =
                ui.data(|d| d.get_temp(viewed_id).unwrap_or_default());
            if let Some(file) = review
                .selected_file
                .and_then(|idx| review.files.get(idx))
                .map(|f| f.path.clone())
            {
                viewed.insert(file);
            }
            let unread_only_id = egui::Id::new(("pr_review_unread_only", review.pr.url.as_str()));
            let mut unread_only: bool = ui.data(|d| d.get_temp(unread_only_id).unwrap_or(false));
            let unread_count = review
                .files
                .iter()
                .filter(|file| !viewed.contains(&file.path))
                .count();
            let hide_tests_id = egui::Id::new(("pr_review_hide_tests", review.pr.url.as_str()));
            let mut hide_tests: bool = ui.data(|d| d.get_temp(hide_tests_id).unwrap_or(false));
            let test_count = review
                .files
                .iter()
                .filter(|file| is_test_path(&file.path))
                .count();
            if let Some(target) = files_band(
                ui,
                palette,
                review.files,
                file_view,
                FileFilters {
                    unread_count,
                    unread_only: &mut unread_only,
                    test_count,
                    hide_tests: &mut hide_tests,
                },
            ) {
                action.set_file_view = Some(target);
            }
            ui.add_space(6.0);
            review_file_list(
                ui,
                palette,
                review,
                file_view,
                &viewed,
                unread_only,
                hide_tests,
                &mut collapsed,
                action,
            );
            ui.data_mut(|d| d.insert_temp(collapse_id, collapsed));
            ui.data_mut(|d| d.insert_temp(viewed_id, viewed));
            ui.data_mut(|d| d.insert_temp(unread_only_id, unread_only));
            ui.data_mut(|d| d.insert_temp(hide_tests_id, hide_tests));
            ui.add_space(PANEL_PAD_Y);
        });
}

/// The PR detail in the **center** area when no file is open (pull-requests.md
/// §11): a compact PR header heads the author block + branch flow + body, then
/// Checks and the conversation-level comments. The PR-level actions live here so
/// a selected file swaps the center to its diff and leaves the rail focused on
/// changed files + review submission.
/// `rail` says the metadata rail is up beside it, so the reviewers, the labels and the
/// checks are named there instead of inside the conversation.
fn review_detail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    rail: bool,
    action: &mut PullRequestsPageAction,
) {
    // The scroll area reaches *past* the column, into the gutter before the rail: egui
    // floats the bar against its right edge, so a scroll area that stops where the cards
    // stop puts the bar on their border — and expanding it on hover then lands it on the
    // card itself. Carried into the gutter, the bar clears the cards entirely and they
    // keep an even margin on both sides (§11).
    let lane = if rail { SCROLL_LANE } else { 0.0 };
    let inner = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(rect.left() + PANEL_PAD_X, rect.right() + lane),
        rect.y_range(),
    );
    let mut panel = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("pr_review_detail")
        // Without this the area shrinks to the width of its content, which drags the
        // floating bar back onto the cards' border however far the rect reaches.
        .auto_shrink([false, true])
        .show(&mut panel, |ui| {
            // `markdown` is what holds the prose to its measure (§11).
            ui.set_width(ui.available_width() - PANEL_PAD_X - lane);
            ui.add_space(PANEL_PAD_Y);
            review_meta(ui, palette, review, rail);

            // While the forge detail is in flight, a spinner stands in for the
            // body/checks/conversation rather than their empty shells, which read as
            // "nothing here" instead of "loading".
            if review.detail_loading {
                detail_loading(ui, palette);
                ui.add_space(PANEL_PAD_Y);
                return;
            }

            if let Some(error) = review.detail_error {
                ui.add_space(SECTION_TOP_MARGIN);
                ui.label(muted(palette, error));
            }

            if !rail {
                checks_section(ui, palette, review);
            }

            conversation_card(ui, palette, review, action);
            ui.add_space(PANEL_PAD_Y);
        });
}

/// The Conversation tab's metadata rail (pull-requests.md §11): everything *about* the
/// PR — who owes a review, what CI says, its labels — beside the conversation column,
/// so the conversation itself stays one column of cards. No rule between the two: the
/// gutter and the cards' own edges already say where one ends and the other starts.
fn conversation_rail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    rect: egui::Rect,
) {
    let inner = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(rect.left() + PANEL_PAD_X, rect.right() - PANEL_PAD_X),
        rect.y_range(),
    );
    let mut panel = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("pr_conversation_rail")
        .show(&mut panel, |ui| {
            ui.set_width(ui.available_width());
            reviewers_section(ui, palette, review.pr);
            checks_section(ui, palette, review);
            if !review.pr.labels.is_empty() {
                band_title(ui, palette, "Labels");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for label in &review.pr.labels {
                        neutral_pill(ui, palette, label);
                    }
                });
            }
            ui.add_space(PANEL_PAD_Y);
        });
}

/// The **Checks** section — the forge's check runs, or the PR's rolled-up status while
/// the detail carries no runs. Drawn in the rail, or inline when there is no rail.
fn checks_section(ui: &mut egui::Ui, palette: &Palette, review: &PrReviewView<'_>) {
    let pr = review.pr;
    let checks = review
        .detail
        .map(|d| d.check_runs.as_slice())
        .unwrap_or(&[]);
    if checks.is_empty() && pr.checks == Checks::None {
        return;
    }
    band_title(ui, palette, "Checks");
    if checks.is_empty() {
        status_line(
            ui,
            palette,
            checks_status(palette, pr.checks),
            checks_label(pr.checks),
        );
    } else {
        for run in checks {
            status_line(ui, palette, checks_status(palette, run.status), &run.name);
        }
    }
}

/// The rail's **Reviewers** section (pull-requests.md §11): who was asked to review and
/// where each one stands — a tick once they approved, a cross when they asked for
/// changes, and *Awaiting review* while the review is still owed.
fn reviewers_section(ui: &mut egui::Ui, palette: &Palette, pr: &PullRequest) {
    if pr.reviewers.is_empty() {
        return;
    }
    band_title(ui, palette, "Reviewers");
    for reviewer in &pr.reviewers {
        ui.add_space(GAP_SM);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            author_avatar_small(ui, palette, &reviewer.name);
            ui.add_space(AVATAR_GAP);
            ui.label(
                egui::RichText::new(&reviewer.name)
                    .size(CONV_AUTHOR_SIZE)
                    .color(palette.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match reviewer_verdict(palette, reviewer.state) {
                    Some((icon, color, label)) => {
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(STATUS_ICON, STATUS_ICON),
                            egui::Sense::hover(),
                        );
                        paint_icon(ui.painter(), rect.center(), STATUS_ICON, icon, color);
                        // The verdict is a glyph: name it, so it is readable to
                        // the accessibility tree and not only on hover.
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label)
                        });
                        response.on_hover_text(label);
                    }
                    // Nobody has ruled yet: say so, rather than leave the row
                    // reading as a bare name with an unexplained gap.
                    None => {
                        ui.label(
                            egui::RichText::new("Awaiting review")
                                .size(CONV_META_SIZE)
                                .color(palette.text_muted),
                        );
                    }
                }
            });
        });
    }
}

/// The glyph a rail row carries for a reviewer's verdict — `None` while nothing has
/// been given yet, where the row falls back to naming the wait.
fn reviewer_verdict(
    palette: &Palette,
    state: Review,
) -> Option<(Icon, egui::Color32, &'static str)> {
    match state {
        Review::Approved => Some((Icon::CheckCircle, palette.git_added, "Approved")),
        Review::ChangesRequested => Some((Icon::CircleX, palette.git_deleted, "Changes requested")),
        Review::Pending | Review::None => None,
    }
}

/// Center placeholder while the forge detail is in flight (pull-requests.md §5): a
/// spinner over a muted label, mirroring the rail's "Loading changed files…" line.
fn detail_loading(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(SECTION_TOP_MARGIN);
    ui.horizontal(|ui| {
        ui.add(Spinner::new().size(16.0).color(palette.text_muted));
        ui.add_space(8.0);
        ui.label(muted(palette, "Loading pull request…"));
    });
}

/// One conversation thread: the root comment first, then its replies. Either
/// **PR-level** (no diff anchor) or **line-anchored** (`path` + the new-side line).
/// `index` is the root's position in the fetched comment list — the opaque key the
/// PR-level reply editor is stored under, stable across the Oldest/Newest reversal.
struct ConvThread<'a> {
    index: usize,
    path: Option<&'a str>,
    line: Option<u32>,
    comments: Vec<&'a PrComment>,
}

impl<'a> ConvThread<'a> {
    /// Whether the forge marks the thread resolved — carried on every comment, read
    /// off the root (pull-requests.md §11).
    fn resolved(&self) -> bool {
        self.comments.first().is_some_and(|c| c.resolved)
    }

    /// The thread's forge comment id: the reply target and Bitbucket's resolve handle.
    fn root_id(&self) -> Option<u64> {
        self.comments.iter().find_map(|c| c.id)
    }

    /// GitHub's review-thread node id, the handle `resolveReviewThread` needs.
    fn thread_id(&self) -> Option<String> {
        self.comments.iter().find_map(|c| c.thread_id.clone())
    }

    /// `path:line` for an anchored thread, `None` for a PR-level one.
    fn anchor_label(&self) -> Option<String> {
        let path = self.path?;
        Some(match self.line {
            Some(line) => format!("{path}:{line}"),
            None => path.to_owned(),
        })
    }

    /// Distinct participants, in the order they first spoke — the avatar stack of a
    /// folded row.
    fn participants(&self) -> Vec<&'a str> {
        let mut out: Vec<&str> = Vec::new();
        for c in &self.comments {
            if !out.contains(&c.author.as_str()) {
                out.push(c.author.as_str());
            }
        }
        out
    }

    /// The first non-blank line of the root's body: the folded row's excerpt.
    fn excerpt(&self) -> &'a str {
        self.comments.first().map_or("", |c| {
            c.body
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("")
        })
    }

    fn last_age(&self, now: i64) -> String {
        self.comments
            .last()
            .map(|c| crate::pull_requests::model::relative_age(&c.created_at, now))
            .unwrap_or_default()
    }
}

/// Groups a PR's fetched comments into threads, oldest-first: first the **PR-level**
/// comments nested by `parent_id` (Bitbucket threads them; GitHub issue comments are
/// flat — id and parent both `None` — so each stays its own single-comment thread),
/// then the **line-anchored** threads, one per (file, anchor) in the order they were
/// first commented on.
fn conversation_threads(detail: Option<&PrDetail>) -> Vec<ConvThread<'_>> {
    let Some(detail) = detail else {
        return Vec::new();
    };
    let mut threads: Vec<ConvThread> = Vec::new();
    let level: Vec<(usize, &PrComment)> = detail
        .comments
        .iter()
        .enumerate()
        .filter(|(_, c)| c.path.is_none())
        .collect();
    let by_id: HashMap<u64, (usize, &PrComment)> = level
        .iter()
        .filter_map(|&(i, c)| c.id.map(|id| (id, (i, c))))
        .collect();
    let root_of = |start: (usize, &'_ PrComment)| -> usize {
        let mut cur = start;
        for _ in 0..level.len() {
            let Some(pid) = cur.1.parent_id else {
                break;
            };
            match by_id.get(&pid) {
                Some(&parent) if parent.0 != cur.0 => cur = parent,
                _ => break,
            }
        }
        cur.0
    };
    let mut roots: Vec<(usize, Vec<(usize, &PrComment)>)> = Vec::new();
    for &(i, c) in &level {
        let root = root_of((i, c));
        match roots.iter_mut().find(|(r, _)| *r == root) {
            Some((_, members)) => members.push((i, c)),
            None => roots.push((root, vec![(i, c)])),
        }
    }
    for (root, members) in &mut roots {
        members.sort_by_key(|(i, _)| (*i != *root, *i));
    }
    threads.extend(roots.into_iter().map(|(root, members)| ConvThread {
        index: root,
        path: None,
        line: None,
        comments: members.into_iter().map(|(_, c)| c).collect(),
    }));

    let inline: Vec<(usize, &PrComment)> = detail
        .comments
        .iter()
        .enumerate()
        .filter(|(_, c)| c.path.is_some() && (c.old_lineno.is_some() || c.new_lineno.is_some()))
        .collect();
    let mut anchors: Vec<(&str, Option<u32>, Option<u32>)> = Vec::new();
    for (_, c) in &inline {
        let anchor = (
            c.path.as_deref().unwrap_or_default(),
            c.old_lineno,
            c.new_lineno,
        );
        if !anchors.contains(&anchor) {
            anchors.push(anchor);
        }
    }
    for (path, old, new) in anchors {
        let members: Vec<(usize, &PrComment)> = inline
            .iter()
            .copied()
            .filter(|(_, c)| {
                c.path.as_deref() == Some(path) && (c.old_lineno, c.new_lineno) == (old, new)
            })
            .collect();
        threads.push(ConvThread {
            index: members.first().map_or(0, |(i, _)| *i),
            path: Some(path),
            line: new,
            comments: members.into_iter().map(|(_, c)| c).collect(),
        });
    }
    threads
}

/// The **Conversation** card in the center detail (pull-requests.md §11, drawn to the
/// `PR Conversation.dc.html` canvas): **one** object carrying the header (comment
/// tally + Oldest|Newest order), the threads that still **need attention**, the
/// resolved ones **folded into a single block**, and the composer at its foot.
/// PR-level comments and line-anchored threads share the card — the review reads
/// "what is still open" once, instead of twice across a Conversation and an Inline
/// comments band.
fn conversation_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    // `detail` is a copied-out `'a` reference, so the threads built from it no longer
    // borrow `review` — the reply/composer state under `review.diff_view` can then lend
    // mutably alongside the `comment_diffs` / `files` field borrows.
    let pr = review.pr;
    let current_user = review.current_user;
    let threads = conversation_threads(review.detail);
    let comment_diffs: &[&FileDiff] = &review.comment_diffs;
    let files = review.files;
    let diff_view = &mut *review.diff_view;
    let total: usize = threads.iter().map(|t| t.comments.len()).sum();
    let now = now_epoch_secs();
    ui.add_space(SECTION_TOP_MARGIN);
    comment_frame(palette)
        .inner_margin(egui::Margin::same(CONV_CARD_PAD))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let newest_first = conversation_header(ui, palette, pr.url.as_str(), total);
            let (mut open, mut resolved): (Vec<&ConvThread>, Vec<&ConvThread>) =
                threads.iter().partition(|t| !t.resolved());
            if newest_first {
                open.reverse();
                resolved.reverse();
            }
            if !open.is_empty() {
                section_label(ui, palette, &format!("Needs attention · {}", open.len()));
                for thread in &open {
                    open_thread_block(
                        ui,
                        palette,
                        diff_view,
                        comment_diffs,
                        files,
                        pr,
                        thread,
                        now,
                        action,
                    );
                }
            }
            if !resolved.is_empty() {
                resolved_group(
                    ui,
                    palette,
                    diff_view,
                    comment_diffs,
                    files,
                    pr,
                    &resolved,
                    now,
                    action,
                );
            }
            conversation_add_block(ui, palette, diff_view, current_user, action);
        });
}

/// A section head inside the conversation card (*NEEDS ATTENTION · 1*): the list
/// band grammar of the browse list — uppercase, letter-spaced, quiet.
fn section_label(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.add_space(GAP_MD);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(egui::FontId::new(
                HEADER_SIZE,
                crate::theme::medium_family(ui.ctx()),
            ))
            .extra_letter_spacing(0.8)
            .color(palette.text_muted),
    );
    ui.add_space(GAP_SM);
}

/// The shared comment-card surface for the conversation and inline center sections
/// (design-system "Detail card"): a `bg.surface` fill over the `bg.canvas` detail, a
/// subtle border, a 10pt radius and even padding — so a comment reads the same
/// wherever it appears (pull-requests.md §11).
fn comment_frame(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(12))
}

/// One comment's header line: the author, an optional Author/Reviewer tag, and the
/// relative age pushed to the right edge — rendered in the text column beside the avatar
/// gutter, the shared face for the conversation and inline center cards (§11).
fn comment_meta_line(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    author: &str,
    created_at: &str,
    now: i64,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP_SM;
        ui.label(
            egui::RichText::new(author)
                .size(CONV_AUTHOR_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        if let Some(role) = comment_role(pr, author) {
            neutral_pill(ui, palette, role);
        }
        let age = crate::pull_requests::model::relative_age(created_at, now);
        if !age.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(age)
                        .size(CONV_META_SIZE)
                        .color(palette.text_secondary),
                );
            });
        }
    });
}

/// One comment laid out as an avatar gutter plus a text column: the avatar sits in a
/// fixed left gutter (lighter for a reply) while the author line and body share the
/// column to its right, so the body aligns under the author rather than sliding back
/// under the avatar (§11).
fn comment_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    c: &PrComment,
    now: i64,
    reply: bool,
) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        if reply {
            author_avatar_small(ui, palette, &c.author);
        } else {
            author_avatar(ui, palette, &c.author);
        }
        ui.add_space(AVATAR_GUTTER_GAP);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            comment_meta_line(ui, palette, pr, &c.author, &c.created_at, now);
            ui.add_space(GAP_XS);
            markdown(ui, palette, &c.body);
        });
    });
}

/// Renders a thread's comments inside an already-opened comment card: the root at full
/// weight, then each reply nested under a left thread-rail with a lighter avatar, so a
/// thread reads as one block instead of a stack of drifting cards (§11).
fn thread_members(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    members: &[&PrComment],
    now: i64,
) {
    let Some((&root, replies)) = members.split_first() else {
        return;
    };
    comment_block(ui, palette, pr, root, now, false);
    if replies.is_empty() {
        return;
    }
    let block = ui.horizontal_top(|ui| {
        ui.add_space(INLINE_REPLY_INDENT);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            for &c in replies {
                ui.add_space(GAP_MD);
                comment_block(ui, palette, pr, c, now, true);
            }
        });
    });
    let rect = block.response.rect;
    ui.painter().vline(
        rect.left() + INLINE_REPLY_INDENT * 0.5,
        egui::Rangef::new(rect.top() + GAP_SM, rect.bottom() - GAP_XS),
        egui::Stroke::new(2.0_f32, palette.border_input),
    );
}

/// The conversation card's head: the title, the comment tally as plain text, and the
/// Oldest|Newest order toggle boxed as one segmented control (persisted per PR, shown
/// only with more than one comment), closed by a hairline. Returns whether to render
/// newest-first.
fn conversation_header(ui: &mut egui::Ui, palette: &Palette, pr_url: &str, count: usize) -> bool {
    let id = egui::Id::new(("pr_conversation_newest", pr_url));
    let mut newest_first: bool = ui.data(|d| d.get_temp(id).unwrap_or(false));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP_SM;
        ui.label(
            egui::RichText::new("Conversation")
                .size(CONV_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.label(conv_muted(palette, &plural(count, "comment")));
        if count > 1 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
                    .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON + 1))
                    .inner_margin(egui::Margin::same(2))
                    .show(ui, |ui| {
                        // Right-to-left inside the frame: Newest is added first so that
                        // Oldest reads on the left, as everywhere else.
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if order_segment(ui, palette, "Newest", newest_first) {
                            newest_first = true;
                        }
                        if order_segment(ui, palette, "Oldest", !newest_first) {
                            newest_first = false;
                        }
                    });
            });
        }
    });
    ui.add_space(GAP_MD);
    row_separator(ui, palette);
    ui.data_mut(|d| d.insert_temp(id, newest_first));
    newest_first
}

/// One segment of the Oldest|Newest order control: a pill that fills `accent.subtle`
/// with `accent` ink when active, so the live order reads as a selected segment rather
/// than a faint colour shift (design-system segmented control).
fn order_segment(ui: &mut egui::Ui, palette: &Palette, label: &str, active: bool) -> bool {
    let font = egui::FontId::proportional(CONV_META_SIZE);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let size = galley.size() + egui::vec2(18.0, 8.0);
    let (rect, response, hovered) = clickable(ui, size, true);
    let fill = if active {
        palette.accent_subtle
    } else if hovered {
        palette.bg_surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    }
    let color = if active {
        palette.accent
    } else {
        palette.text_muted
    };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.clicked()
}

/// The role tag for a conversation comment: "Author" for the PR author, "Reviewer"
/// when the commenter is on the reviewer roster, else none.
fn comment_role(pr: &PullRequest, author: &str) -> Option<&'static str> {
    if author == pr.author {
        Some("Author")
    } else if pr.reviewers.iter().any(|r| r.name == author) {
        Some("Reviewer")
    } else {
        None
    }
}

/// The Reply affordance under every top-level conversation card: a "Reply" pill that
/// swaps to the shared composer. A threaded forge passes the card's id as `parent` so
/// the reply nests under it (Bitbucket); a flat forge passes `None` and the reply
/// posts a new top-level comment (GitHub issue comments don't thread) (§11).
fn conversation_reply_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    index: usize,
    parent: Option<u64>,
    action: &mut PullRequestsPageAction,
) {
    ui.add_space(4.0);
    if diff_view.conversation_edit() == Some(ConversationEdit::Reply(index)) {
        let width = (ui.available_width() - 8.0).max(160.0);
        let (buffer, focus) = diff_view.conversation_fields();
        match reply_editor(ui, palette, buffer, focus, width, &REPLY_LABELS) {
            ReplyEdit::Send => {
                let body = diff_view.conversation_buffer_mut().trim().to_owned();
                if !body.is_empty() {
                    action
                        .review_intents
                        .push(ReviewIntent::PostConversationComment { parent, body });
                }
                diff_view.cancel_conversation();
            }
            ReplyEdit::Cancel => diff_view.cancel_conversation(),
            ReplyEdit::Idle => {}
        }
    } else if reply_pill(ui, palette) {
        diff_view.open_conversation_reply(index);
    }
}

/// The conversation composer at the card's foot (pull-requests.md §11): **one object**
/// — the field and its action bar inside a single frame beside the user's avatar —
/// raising `PostConversationComment` with no parent, a new top-level comment on either
/// forge. The **Comment** button only fills with accent once the draft holds non-blank
/// text; blank, it reads as the inert control it is.
fn conversation_add_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    current_user: Option<&str>,
    action: &mut PullRequestsPageAction,
) {
    ui.add_space(GAP_LG);
    row_separator(ui, palette);
    ui.add_space(GAP_LG);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        author_avatar(ui, palette, current_user.unwrap_or(""));
        ui.add_space(AVATAR_GUTTER_GAP);
        raised_frame(palette)
            .inner_margin(egui::Margin::ZERO)
            .show(ui, |ui| {
                // The frame inherits the surrounding horizontal layout, in which the field
                // and its action bar would sit side by side: stack them explicitly.
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let field_id = ui.id().with("pr_conversation_add");
                    // This composer is always on screen, so its `Esc` can't read as
                    // "close the editor": egui drops the field's focus, and the key is
                    // swallowed there rather than falling through to the surface's
                    // cascade and closing the review (pull-requests.md §11).
                    if ui.memory(|m| m.had_focus_last_frame(field_id)) {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                    }
                    ui.add(
                        egui::TextEdit::multiline(diff_view.conversation_add_buffer_mut())
                            .id(field_id)
                            // The box around field *and* action bar is the outer frame, so the
                            // field brings none of its own — but `TextEdit::margin` is only
                            // honoured on a frame egui builds itself, so the padding has to
                            // ride on this one or the text lands flush against the border.
                            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
                                COMPOSER_PAD_X as i8,
                                COMPOSER_PAD_Y as i8,
                            )))
                            .desired_rows(3)
                            .desired_width(ui.available_width())
                            .font(egui::FontId::proportional(MD_TEXT_SIZE))
                            .hint_text("Add a comment…"),
                    );
                    hairline(ui, palette);
                    // A fixed-height bar so the right-to-left layout can't claim the scroll
                    // area's full remaining height and strand the button at the panel foot.
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), COMPOSER_BAR_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add_space(COMPOSER_PAD_X);
                            ui.label(conv_muted(palette, "Markdown supported"));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(COMPOSER_PAD_X);
                                    if comment_button(ui, palette, diff_view) {
                                        let body = diff_view
                                            .conversation_add_buffer_mut()
                                            .trim()
                                            .to_owned();
                                        if !body.is_empty() {
                                            action.review_intents.push(
                                                ReviewIntent::PostConversationComment {
                                                    parent: None,
                                                    body,
                                                },
                                            );
                                            diff_view.conversation_add_buffer_mut().clear();
                                        }
                                    }
                                },
                            );
                        },
                    );
                });
            });
    });
}

/// The composer's submit button: solid accent while the draft holds text, a quiet
/// disabled chip while it is blank. Returns whether it was clicked.
fn comment_button(ui: &mut egui::Ui, palette: &Palette, diff_view: &mut DiffViewState) -> bool {
    const HEIGHT: f32 = 30.0;
    let enabled = !diff_view.conversation_add_buffer_mut().trim().is_empty();
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(
        "Comment".to_owned(),
        font.clone(),
        egui::Color32::PLACEHOLDER,
    );
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(galley.size().x + 28.0, HEIGHT), enabled);
    let (fill, ink) = match (enabled, hovered) {
        (false, _) => (palette.bg_surface, palette.text_muted),
        (true, true) => (palette.accent_hover, palette.lane_node_text),
        (true, false) => (palette.accent, palette.lane_node_text),
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Comment",
        font,
        ink,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "Comment"));
    response.clicked()
}

/// A full-strength 1px rule across the available width. The file list's `row_separator`
/// is alpha'd down for dense rows, which makes it vanish inside a framed input — the bar
/// under a field has to read as a real edge.
fn hairline(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

/// A block raised one step above the card it sits in (the canvas's `surf2`/`border2`):
/// the open-thread blocks and the composer inside the conversation card.
fn raised_frame(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.bg_surface_hover)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_input))
        .corner_radius(egui::CornerRadius::same(BLOCK_RADIUS))
        .inner_margin(egui::Margin::same(14))
}

/// `1 comment` / `3 comments` — the tally grammar of the conversation card.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// A thread that still needs attention (pull-requests.md §11): a raised block inside the
/// conversation card — its `path:line` and the code it hangs on when it is line-anchored,
/// the comments, then the Reply / Resolve controls.
#[allow(clippy::too_many_arguments)]
fn open_thread_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    diffs: &[&FileDiff],
    files: &[CommitFile],
    pr: &PullRequest,
    thread: &ConvThread<'_>,
    now: i64,
    action: &mut PullRequestsPageAction,
) {
    raised_frame(palette).show(ui, |ui| {
        ui.set_width(ui.available_width());
        thread_anchor_label(ui, palette, thread);
        thread_snippet(ui, palette, diffs, files, thread, action);
        thread_members(ui, palette, pr, &thread.comments, now);
        ui.add_space(GAP_SM);
        match thread.root_id() {
            // Line-anchored and posted: the shared Reply + Resolve pair, whose handles are
            // the root's comment id and (GitHub) its review-thread node id.
            Some(root_id) if thread.path.is_some() => center_reply_block(
                ui,
                palette,
                diff_view,
                root_id,
                thread.resolved(),
                thread.thread_id(),
                action,
            ),
            // PR-level: Reply only — a conversation comment carries no resolve handle.
            _ => conversation_reply_block(
                ui,
                palette,
                diff_view,
                thread.index,
                thread.root_id(),
                action,
            ),
        }
    });
    ui.add_space(GAP_MD);
}

/// The `path:line` a thread hangs on, in the monospace file grammar. Nothing for a
/// PR-level thread, which hangs on the PR itself.
fn thread_anchor_label(ui: &mut egui::Ui, palette: &Palette, thread: &ConvThread<'_>) {
    let Some(label) = thread.anchor_label() else {
        return;
    };
    ui.label(
        egui::RichText::new(label)
            .size(CONV_MONO_SIZE)
            .monospace()
            .color(palette.text_secondary),
    );
    ui.add_space(GAP_SM);
}

/// The few lines of code a line-anchored thread was left on, and the click that opens
/// the file there (GitHub's own hunk, else a window of the loaded diff). With no snippet
/// to click — Bitbucket, file not loaded — the affordance degrades to a text link.
fn thread_snippet(
    ui: &mut egui::Ui,
    palette: &Palette,
    diffs: &[&FileDiff],
    files: &[CommitFile],
    thread: &ConvThread<'_>,
    action: &mut PullRequestsPageAction,
) {
    let Some(path) = thread.path else {
        return;
    };
    let open_label = match thread.line {
        Some(line) => format!("Open {path} line {line}"),
        None => format!("Open {path}"),
    };
    let snippet = inline_snippet(diffs, path, thread.line, &thread.comments);
    let open_clicked = if snippet.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(&open_label)
                    .size(CONV_META_SIZE)
                    .color(palette.accent),
            )
            .sense(egui::Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    } else {
        // The code preview is the "open in diff" target, so the comment text below stays
        // selectable (not buried under a whole-block click sense).
        let response = code_snippet(ui, palette, path, &snippet)
            .interact(egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &open_label));
        response.clicked()
    };
    ui.add_space(GAP_SM);
    if open_clicked {
        if let Some(idx) = files.iter().position(|f| f.path == path) {
            action.open_inline_comment = Some((idx, thread.line));
        }
    }
}

/// The resolved threads folded into **one** block (pull-requests.md §11): a summary
/// header (*N threads · M comments · K files*) over one row per thread — its file and
/// line, an excerpt, its participants and its last activity — each expanding in place to
/// the code, the comments and the Reply / Reopen controls. Resolved work is history: it
/// stays reachable without being what the review reads first.
#[allow(clippy::too_many_arguments)]
fn resolved_group(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    diffs: &[&FileDiff],
    files: &[CommitFile],
    pr: &PullRequest,
    threads: &[&ConvThread<'_>],
    now: i64,
    action: &mut PullRequestsPageAction,
) {
    ui.add_space(GAP_MD);
    let comments: usize = threads.iter().map(|t| t.comments.len()).sum();
    let mut paths: Vec<&str> = Vec::new();
    for thread in threads {
        if let Some(path) = thread.path {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    let mut summary = format!(
        "{} · {}",
        plural(threads.len(), "thread"),
        plural(comments, "comment")
    );
    if !paths.is_empty() {
        summary.push_str(&format!(" · {}", plural(paths.len(), "file")));
    }
    let open_id = egui::Id::new(("pr_resolved_group", pr.url.as_str()));
    let mut open: bool = ui.data(|d| d.get_temp(open_id).unwrap_or(true));
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(BLOCK_RADIUS))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            if resolved_group_header(ui, palette, &summary, open) {
                open = !open;
            }
            if !open {
                return;
            }
            let last = threads.len().saturating_sub(1);
            for (i, thread) in threads.iter().enumerate() {
                row_separator(ui, palette);
                let key = thread.root_id();
                let expanded = key.is_some_and(|id| diff_view.is_resolved_expanded(id));
                if resolved_thread_row(ui, palette, thread, now, expanded, i == last) {
                    if let Some(id) = key {
                        diff_view.toggle_resolved(id);
                    }
                }
                if expanded {
                    resolved_thread_body(
                        ui, palette, diff_view, diffs, files, pr, thread, now, action,
                    );
                }
            }
        });
    ui.data_mut(|d| d.insert_temp(open_id, open));
}

/// The resolved block's head: a tick, *Resolved*, the tally of what is folded away, and
/// a Show / Hide chevron. The whole row is the click target; returns whether it was
/// clicked (the caller toggles).
fn resolved_group_header(ui: &mut egui::Ui, palette: &Palette, summary: &str, open: bool) -> bool {
    let (rect, response, hovered) = clickable(
        ui,
        egui::vec2(ui.available_width(), RESOLVED_HEADER_HEIGHT),
        true,
    );
    // Rounded to match the block it heads: all four corners while it *is* the block
    // (folded), only the top two once the rows show below it.
    let radius = if open {
        egui::CornerRadius {
            nw: BLOCK_RADIUS,
            ne: BLOCK_RADIUS,
            sw: 0,
            se: 0,
        }
    } else {
        egui::CornerRadius::same(BLOCK_RADIUS)
    };
    let title_font = egui::FontId::new(CONV_AUTHOR_SIZE, crate::theme::medium_family(ui.ctx()));
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, radius, palette.bg_surface_hover);
    }
    let cy = rect.center().y;
    let mut x = rect.left() + PAD_X;
    paint_icon(
        painter,
        egui::pos2(x + STATUS_ICON / 2.0, cy),
        STATUS_ICON,
        Icon::CheckCircle,
        palette.git_added,
    );
    x += STATUS_ICON + GAP_MD;
    let title = painter.layout_no_wrap("Resolved".to_owned(), title_font, palette.text_primary);
    painter.galley(
        egui::pos2(x, cy - title.size().y / 2.0),
        title.clone(),
        palette.text_primary,
    );
    x += title.size().x + GAP_MD;
    painter.text(
        egui::pos2(x, cy),
        egui::Align2::LEFT_CENTER,
        summary,
        egui::FontId::proportional(CONV_META_SIZE),
        palette.text_muted,
    );
    let chevron_x = rect.right() - PAD_X - STATUS_ICON / 2.0;
    paint_icon(
        painter,
        egui::pos2(chevron_x, cy),
        STATUS_ICON,
        if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        palette.text_muted,
    );
    painter.text(
        egui::pos2(chevron_x - STATUS_ICON / 2.0 - GAP_SM, cy),
        egui::Align2::RIGHT_CENTER,
        if open { "Hide" } else { "Show" },
        egui::FontId::proportional(CONV_META_SIZE),
        palette.text_muted,
    );
    let label = format!("Resolved · {summary}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

/// One folded thread inside the resolved block: `path:line` and its comment tally over a
/// one-line excerpt, with the participants' avatars, the last activity and a chevron on
/// the right. The whole row toggles it open; returns whether it was clicked.
fn resolved_thread_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    thread: &ConvThread<'_>,
    now: i64,
    expanded: bool,
    last: bool,
) -> bool {
    let (rect, response, hovered) = clickable(
        ui,
        egui::vec2(ui.available_width(), RESOLVED_ROW_HEIGHT),
        true,
    );
    // The bottom corners are the block's own, so a hovered last row must not paint over
    // them — unless its body follows below.
    let radius = if last && !expanded {
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: BLOCK_RADIUS,
            se: BLOCK_RADIUS,
        }
    } else {
        egui::CornerRadius::ZERO
    };
    let anchor = thread
        .anchor_label()
        .unwrap_or_else(|| "Conversation".to_owned());
    let count = plural(thread.comments.len(), "comment");
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, radius, palette.bg_surface_hover);
    }
    // The right-hand cluster is laid out from the right edge; the text column takes what
    // is left, its excerpt elided rather than run under the avatars.
    let chevron_x = rect.right() - PAD_X - STATUS_ICON / 2.0;
    paint_icon(
        painter,
        egui::pos2(chevron_x, rect.center().y),
        STATUS_ICON,
        if expanded {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        palette.text_muted,
    );
    let mut right = chevron_x - STATUS_ICON / 2.0 - GAP_SM;
    let age = thread.last_age(now);
    if !age.is_empty() {
        let galley = painter.layout_no_wrap(
            age,
            egui::FontId::proportional(CONV_META_SIZE),
            palette.text_muted,
        );
        painter.galley(
            egui::pos2(
                right - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley.clone(),
            palette.text_muted,
        );
        right -= galley.size().x + GAP_MD;
    }
    for who in thread.participants().iter().take(RESOLVED_AVATAR_MAX).rev() {
        right -= RESOLVED_AVATAR / 2.0;
        crate::ui::detail::paint_author_avatar(
            painter,
            palette,
            who,
            egui::pos2(right, rect.center().y),
            RESOLVED_AVATAR,
        );
        right -= RESOLVED_AVATAR / 2.0 + GAP_XS;
    }
    let left = rect.left() + RESOLVED_ROW_INDENT;
    let anchor_galley = painter.layout_no_wrap(
        anchor.clone(),
        egui::FontId::monospace(CONV_MONO_SIZE),
        palette.text_primary,
    );
    let top_y = rect.top() + 11.0 + anchor_galley.size().y / 2.0;
    painter.galley(
        egui::pos2(left, top_y - anchor_galley.size().y / 2.0),
        anchor_galley.clone(),
        palette.text_primary,
    );
    painter.text(
        egui::pos2(left + anchor_galley.size().x + GAP_SM, top_y),
        egui::Align2::LEFT_CENTER,
        &count,
        egui::FontId::proportional(CONV_META_SIZE),
        palette.text_muted,
    );
    let excerpt = thread.excerpt();
    if !excerpt.is_empty() {
        let job = elided_job(
            excerpt,
            egui::FontId::proportional(MD_CODE_SIZE),
            palette.text_secondary,
            (right - GAP_MD - left).max(40.0),
        );
        let galley = painter.layout_job(job);
        painter.galley(
            egui::pos2(left, rect.bottom() - 11.0 - galley.size().y),
            galley,
            palette.text_secondary,
        );
    }
    let label = format!("{anchor} · {count}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

/// A single line of text elided with an ellipsis rather than wrapped — the excerpt of a
/// folded thread, whose whole point is to stay one row tall.
fn elided_job(
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

/// A folded thread opened in place: the code it hangs on, its comments, the note that it
/// is resolved, and the Reply / Reopen controls — indented under its row so the block
/// still reads as one list (pull-requests.md §11).
#[allow(clippy::too_many_arguments)]
fn resolved_thread_body(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    diffs: &[&FileDiff],
    files: &[CommitFile],
    pr: &PullRequest,
    thread: &ConvThread<'_>,
    now: i64,
    action: &mut PullRequestsPageAction,
) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: RESOLVED_ROW_INDENT as i8,
            right: PAD_X as i8,
            top: 0,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            thread_snippet(ui, palette, diffs, files, thread, action);
            thread_members(ui, palette, pr, &thread.comments, now);
            ui.add_space(GAP_SM);
            resolved_note(ui, palette, thread, now);
            if let Some(root_id) = thread.root_id() {
                center_reply_block(
                    ui,
                    palette,
                    diff_view,
                    root_id,
                    true,
                    thread.thread_id(),
                    action,
                );
            }
        });
}

/// The note closing an opened resolved thread. The forges do not tell us **who**
/// resolved it (neither `reviewThread.isResolved` nor Bitbucket's `resolution` reaches
/// the model), so the row states what is known: that it is resolved, and when it was
/// last spoken on.
fn resolved_note(ui: &mut egui::Ui, palette: &Palette, thread: &ConvThread<'_>, now: i64) {
    let age = thread.last_age(now);
    let text = if age.is_empty() {
        "Resolved".to_owned()
    } else {
        format!("Resolved · last reply {age}")
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (icon, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
        paint_icon(
            ui.painter(),
            icon.center(),
            13.0,
            Icon::Check,
            palette.git_added,
        );
        ui.label(conv_muted(palette, &text));
    });
}

/// The center card's Reply affordance: a "Reply" pill that swaps to the shared
/// reply editor, raising `ReplyToThread` on send (pull-requests.md §11).
#[allow(clippy::too_many_arguments)]
fn center_reply_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    reply_id: u64,
    resolved: bool,
    thread_id: Option<String>,
    action: &mut PullRequestsPageAction,
) {
    if diff_view.reply_target() == Some(reply_id) {
        ui.add_space(4.0);
        let width = (ui.available_width() - 8.0).max(160.0);
        let (buffer, focus) = diff_view.reply_fields();
        let edit = reply_editor(ui, palette, buffer, focus, width, &REPLY_LABELS);
        match edit {
            ReplyEdit::Send => {
                let body = diff_view.reply_buffer_mut().trim().to_owned();
                if !body.is_empty() {
                    action.review_intents.push(ReviewIntent::ReplyToThread {
                        comment_id: reply_id,
                        body,
                    });
                }
                diff_view.cancel_reply();
            }
            ReplyEdit::Cancel => diff_view.cancel_reply(),
            ReplyEdit::Idle => {}
        }
    } else {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if reply_pill(ui, palette) {
                diff_view.open_reply(reply_id);
            }
            if resolve_pill(ui, palette, resolved) {
                action.review_intents.push(ReviewIntent::ResolveThread {
                    thread_id,
                    comment_id: reply_id,
                    resolved: !resolved,
                });
            }
        });
    }
}

/// The few lines of code shown atop an inline-comment card. GitHub carries the hunk
/// on the comment (`diff_hunk`); for Bitbucket (no hunk) fall back to a window of the
/// loaded diff when the comment is on the open file, else nothing (pull-requests.md §5).
fn inline_snippet(
    diffs: &[&FileDiff],
    path: &str,
    new: Option<u32>,
    thread: &[&PrComment],
) -> Vec<SnippetLine> {
    if let Some(hunk) = thread.iter().find_map(|c| c.context.as_deref()) {
        return hunk_snippet(hunk, INLINE_SNIPPET_LINES);
    }
    let Some(new) = new else {
        return Vec::new();
    };
    let Some(diff) = diffs.iter().find(|d| d.path == path) else {
        return Vec::new();
    };
    diff_window_snippet(diff, new).unwrap_or_else(|| source_window_snippet(diff, new))
}

/// A window of the loaded diff ending at new-side line `anchor`, with the add/delete
/// grammar carried over from the hunk (Bitbucket comments hold no hunk of their own).
/// `None` when no hunk covers the anchor — the comment sits on an unchanged line.
fn diff_window_snippet(diff: &FileDiff, anchor: u32) -> Option<Vec<SnippetLine>> {
    let hunk = diff
        .hunks
        .iter()
        .find(|h| anchor >= h.new_start && anchor < h.new_start + h.new_lines)?;
    let end = hunk
        .lines
        .iter()
        .position(|l| l.new_lineno == Some(anchor))?;
    let start = (end + 1).saturating_sub(INLINE_SNIPPET_LINES);
    Some(
        hunk.lines[start..=end]
            .iter()
            .map(|l| SnippetLine {
                old_no: l.old_lineno,
                new_no: l.new_lineno,
                kind: match l.origin {
                    LineOrigin::Addition => SnippetKind::Added,
                    LineOrigin::Deletion => SnippetKind::Deleted,
                    LineOrigin::Context => SnippetKind::Context,
                },
                text: l.content.trim_end_matches('\n').to_owned(),
            })
            .collect(),
    )
}

/// Neutral context window from the file's new-side source, used when the comment is
/// anchored outside any hunk (no add/delete colour to carry).
fn source_window_snippet(diff: &FileDiff, anchor: u32) -> Vec<SnippetLine> {
    if diff.source_lines.is_empty() {
        return Vec::new();
    }
    let end = (anchor as usize).min(diff.source_lines.len());
    let start = end.saturating_sub(INLINE_SNIPPET_LINES);
    diff.source_lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, text)| SnippetLine {
            old_no: None,
            new_no: Some((start + i + 1) as u32),
            kind: SnippetKind::Context,
            text: text.clone(),
        })
        .collect()
}

/// Full-width header of the review surface (pull-requests.md §11), three stacked
/// rows: Back + title + `#number` + the health cluster + the verdict icon group +
/// **Merge**; then the identity line (state chip, author, branch flow, age); then
/// the Conversation / Files / Commits tabs. Returns the open tab.
fn review_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) -> PrTab {
    let pr = review.pr;
    ui.painter().rect_filled(rect, 0, palette.bg_surface);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );

    // ── row 1: identity + actions ────────────────────────────────────────────
    let row1 = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top(), rect.top() + REVIEW_HEADER_ROW1),
    );
    let center_y = row1.center().y;
    let back_rect = egui::Rect::from_center_size(
        egui::pos2(
            row1.left() + PANEL_PAD_X + DETAIL_ACTION_HEIGHT / 2.0,
            center_y,
        ),
        egui::vec2(DETAIL_ACTION_HEIGHT, DETAIL_ACTION_HEIGHT),
    );
    if detail_back_button(ui, palette, back_rect) {
        action.back = true;
    }

    // Right-hand cluster, laid out from the right edge.
    let mut x = row1.right() - PANEL_PAD_X;
    x -= MERGE_HEADER_W;
    let merge_rect = egui::Rect::from_min_size(
        egui::pos2(x, center_y - DETAIL_ACTION_HEIGHT / 2.0),
        egui::vec2(MERGE_HEADER_W, DETAIL_ACTION_HEIGHT),
    );
    if merge_button(ui, palette, merge_rect, "pr_header_merge") {
        action.merge_open = true;
    }
    x -= GAP_SM;

    // Finish review: verdict, summary and Submit together in one popover, so the
    // choice and the button that acts on it are never on opposite screen edges.
    x -= FINISH_BTN_W;
    let finish_rect = egui::Rect::from_min_size(
        egui::pos2(x, center_y - DETAIL_ACTION_HEIGHT / 2.0),
        egui::vec2(FINISH_BTN_W, DETAIL_ACTION_HEIGHT),
    );
    finish_review_button(ui, palette, review, finish_rect, action);
    // A wider gap keeps the two action buttons apart from the navigation icons.
    x -= GAP_MD + GAP_SM;

    // PR-level actions, icon-only at this size.
    for (icon, label, id) in [
        (Icon::GitBranch, "Checkout", "pr_detail_checkout"),
        (
            Icon::ExternalLink,
            "Open in browser",
            "pr_detail_open_browser",
        ),
    ] {
        x -= DETAIL_ACTION_HEIGHT;
        let r = egui::Rect::from_min_size(
            egui::pos2(x, center_y - DETAIL_ACTION_HEIGHT / 2.0),
            egui::vec2(DETAIL_ACTION_HEIGHT, DETAIL_ACTION_HEIGHT),
        );
        if icon_action(ui, palette, r, icon, label, id) {
            match label {
                "Checkout" => action.checkout = true,
                _ => action.open_url = Some(pr.url.clone()),
            }
        }
        x -= GAP_XS;
    }

    // Health cluster: passing checks, unresolved threads, mergeability. A hairline
    // marks it off from the actions — it is read-only, and its icons would otherwise
    // read as three more buttons.
    x -= GAP_MD;
    ui.painter().vline(
        x,
        egui::Rangef::new(center_y - 9.0, center_y + 9.0),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    x -= GAP_MD;
    x = health_cluster(ui, palette, review, x, center_y);
    x -= GAP_MD;

    let number = format!("#{}", pr.number);
    let number_galley = ui.painter().layout_no_wrap(
        number.clone(),
        egui::FontId::monospace(CHIP_SIZE),
        palette.text_muted,
    );
    x -= number_galley.size().x;
    let number_rect = egui::Rect::from_min_size(
        egui::pos2(x, center_y - number_galley.size().y / 2.0),
        number_galley.size(),
    );
    ui.painter()
        .galley(number_rect.min, number_galley, palette.text_muted);
    detail_label_accessibility(ui, number_rect, "pr_detail_number", number);
    x -= GAP_MD;

    let title_left = back_rect.right() + GAP_MD;
    let title_clip = egui::Rect::from_min_max(
        egui::pos2(title_left, row1.top()),
        egui::pos2(x.max(title_left + 40.0), row1.bottom()),
    );
    cell_text(
        ui,
        &pr.title,
        egui::FontId::new(
            DETAIL_HEADER_TITLE_SIZE,
            crate::theme::medium_family(ui.ctx()),
        ),
        palette.text_primary,
        title_left,
        center_y,
        title_clip.width(),
    );
    detail_label_accessibility(ui, title_clip, "pr_detail_header_title", pr.title.clone());

    // ── row 2: state chip, author, branch flow, age ──────────────────────────
    let row2 = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(row1.bottom(), row1.bottom() + REVIEW_HEADER_ROW2),
    );
    let sub_y = row2.center().y;
    let (state_label, state_color) = match pr.state {
        PrState::Open => ("Open", palette.git_added),
        PrState::Draft => ("Draft", palette.text_muted),
    };
    let mut sx = state_chip(ui, state_label, state_color, title_left, sub_y);
    sx += GAP_MD;
    paint_avatar(
        ui.painter(),
        palette,
        &pr.author,
        egui::pos2(sx + SUB_AVATAR / 2.0, sub_y),
        SUB_AVATAR,
        None,
    );
    sx += SUB_AVATAR + GAP_XS + 2.0;
    let created = review
        .detail
        .map(|d| crate::pull_requests::model::relative_age(&d.created_at, now_epoch_secs()))
        .filter(|age| !age.is_empty())
        .map(|age| format!(" · {age}"))
        .unwrap_or_default();
    let subtitle = format!(
        "{} · {} → {}{}",
        pr.author, pr.source_branch, pr.dest_branch, created
    );
    cell_text(
        ui,
        &subtitle,
        egui::FontId::proportional(DETAIL_HEADER_SUBTITLE_SIZE),
        palette.text_secondary,
        sx,
        sub_y,
        (row2.right() - PANEL_PAD_X - sx).max(0.0),
    );
    detail_label_accessibility(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(sx, row2.top()),
            egui::pos2(row2.right(), row2.bottom()),
        ),
        "pr_detail_header_subtitle",
        subtitle,
    );

    // ── row 3: tabs ──────────────────────────────────────────────────────────
    let row3 = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(row2.bottom(), rect.bottom()),
    );
    review_tabs(ui, palette, review, row3, action)
}

/// Small filled state chip ("Open" / "Draft"). Returns its right edge.
fn state_chip(ui: &egui::Ui, label: &str, color: egui::Color32, left: f32, center_y: f32) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(CHIP_SIZE, crate::theme::medium_family(ui.ctx())),
        color,
    );
    let h = CHIP_SIZE + 9.0;
    let chip = egui::Rect::from_min_size(
        egui::pos2(left, center_y - h / 2.0),
        egui::vec2(galley.size().x + 16.0, h),
    );
    ui.painter()
        .rect_filled(chip, h / 2.0, with_alpha(color, 36));
    ui.painter()
        .galley(chip.center() - galley.size() / 2.0, galley, color);
    chip.right()
}

/// Passing checks · unresolved threads · mergeability, right-to-left from `right`.
/// Returns the left edge it reached.
fn health_cluster(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    right: f32,
    center_y: f32,
) -> f32 {
    let passing = review
        .detail
        .map(|d| {
            d.check_runs
                .iter()
                .filter(|r| r.status == Checks::Passing)
                .count()
        })
        .unwrap_or(0);
    let unresolved = review
        .existing
        .values()
        .flat_map(|anchors| anchors.values())
        .filter(|thread| thread.first().is_some_and(|root| !root.resolved))
        .count();

    let mut items: Vec<(Icon, String, egui::Color32, &'static str)> = Vec::new();
    if passing > 0 {
        items.push((
            Icon::CheckCircle2,
            passing.to_string(),
            palette.git_added,
            "checks passing",
        ));
    }
    if unresolved > 0 {
        items.push((
            Icon::MessageSquare,
            unresolved.to_string(),
            palette.git_modified,
            "unresolved threads",
        ));
    }
    items.push(match review.pr.checks {
        Checks::Failing => (
            Icon::GitMerge,
            String::new(),
            palette.git_deleted,
            "not mergeable",
        ),
        _ => (
            Icon::GitMerge,
            String::new(),
            palette.git_added,
            "mergeable",
        ),
    });

    let mut x = right;
    for (icon, text, color, hint) in items.into_iter().rev() {
        let galley = (!text.is_empty()).then(|| {
            ui.painter()
                .layout_no_wrap(text.clone(), egui::FontId::proportional(META_SIZE), color)
        });
        let text_w = galley.as_ref().map_or(0.0, |g| g.size().x + 4.0);
        x -= STATUS_ICON + text_w;
        paint_icon(
            ui.painter(),
            egui::pos2(x + STATUS_ICON / 2.0, center_y),
            STATUS_ICON,
            icon,
            color,
        );
        if let Some(galley) = galley {
            ui.painter().galley(
                egui::pos2(x + STATUS_ICON + 4.0, center_y - galley.size().y / 2.0),
                galley,
                color,
            );
        }
        let hit = egui::Rect::from_min_size(
            egui::pos2(x, center_y - STATUS_ICON / 2.0),
            egui::vec2(STATUS_ICON + text_w, STATUS_ICON),
        );
        let label = if text.is_empty() {
            hint.to_owned()
        } else {
            format!("{text} {hint}")
        };
        ui.interact(hit, ui.id().with(("pr_health", hint)), egui::Sense::hover())
            .on_hover_text(label.clone())
            .widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone())
            });
        x -= GAP_SM;
    }
    x
}

/// **Finish review** in the header: an outlined button badged with the pending draft
/// count, opening the composer as a popover (pull-requests.md §11). Verdict, summary
/// and Submit sit together there — the header no longer carries a verdict selector a
/// screen away from the button that acts on it.
fn finish_review_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    let drafts = crate::review::count(review.draft);
    let response = ui.interact(rect, ui.id().with("pr_finish_review"), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, RADIUS_BUTTON, palette.bg_surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().rect_stroke(
        rect,
        RADIUS_BUTTON,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let center_y = rect.center().y;
    let badge = (drafts > 0).then(|| drafts.to_string());
    let badge_w = badge.as_ref().map_or(0.0, |t| 6.0 + t.len() as f32 * 7.0);
    cell_text(
        ui,
        "Finish review",
        egui::FontId::new(CHIP_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_primary,
        rect.left() + 10.0,
        center_y,
        (rect.width() - 20.0 - badge_w).max(0.0),
    );
    if let Some(text) = &badge {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, center_y),
            egui::Align2::RIGHT_CENTER,
            text,
            egui::FontId::proportional(META_SIZE),
            palette.accent,
        );
    }
    let label = match drafts {
        0 => "Finish review".to_owned(),
        n => format!("Finish review, {n} pending"),
    };
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
    });

    let shown = egui::Popup::from_toggle_button_response(&response)
        .align(egui::RectAlign::BOTTOM_END)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(COMPOSER_POPOVER_W);
            review_composer(ui, palette, review, action);
        });
    // The popover reads `Esc` itself (egui closes it): swallow the key so the press
    // that folded the composer away doesn't also step out of the review (§11).
    if shown.is_some() {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
}

/// The composer's verdict group (Comment · Request changes · Approve). It selects
/// what Submit will send, right above the button itself.
fn verdict_icon_group(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    verdict: &mut ReviewVerdict,
) {
    ui.painter().rect_stroke(
        rect,
        RADIUS_BUTTON,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let options = [
        (
            ReviewVerdict::Comment,
            Icon::MessageSquarePlus,
            "Comment-only review",
        ),
        (
            ReviewVerdict::RequestChanges,
            Icon::FileX,
            "Request changes",
        ),
        (ReviewVerdict::Approve, Icon::Check, "Approve"),
    ];
    for (i, (option, icon, label)) in options.into_iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * VERDICT_ICON_W, rect.top()),
            egui::vec2(VERDICT_ICON_W, rect.height()),
        );
        let selected = *verdict == option;
        let response = ui.interact(
            cell,
            ui.id().with(("pr_verdict", label)),
            egui::Sense::click(),
        );
        let tint = match option {
            ReviewVerdict::Approve => palette.git_added,
            ReviewVerdict::RequestChanges => palette.git_deleted,
            ReviewVerdict::Comment => palette.text_secondary,
        };
        if selected {
            ui.painter()
                .rect_filled(cell, RADIUS_BUTTON, with_alpha(tint, 36));
        } else if response.hovered() {
            ui.painter()
                .rect_filled(cell, RADIUS_BUTTON, palette.bg_surface_hover);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if i > 0 {
            ui.painter().vline(
                cell.left(),
                egui::Rangef::new(cell.top() + 4.0, cell.bottom() - 4.0),
                egui::Stroke::new(1.0_f32, palette.border_subtle),
            );
        }
        paint_icon(
            ui.painter(),
            cell.center(),
            STATUS_ICON,
            icon,
            if selected {
                tint
            } else {
                palette.text_secondary
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
        });
        if response.clicked() {
            *verdict = option;
        }
    }
}

/// A square icon-only header action.
fn icon_action(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    icon: Icon,
    label: &str,
    id: &str,
) -> bool {
    let response = ui.interact(rect, ui.id().with(id), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, RADIUS_BUTTON, palette.bg_surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    paint_icon(
        ui.painter(),
        rect.center(),
        STATUS_ICON,
        icon,
        palette.text_secondary,
    );
    let owned = label.to_owned();
    response
        .clone()
        .on_hover_text(owned.clone())
        .widget_info(move || {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, owned.clone())
        });
    response.clicked()
}

/// Conversation / Files / Commits, each with its count. Persisted per PR url in
/// `ui.data`, so switching PRs starts on the conversation again.
fn review_tabs(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) -> PrTab {
    let id = egui::Id::new(("pr_review_tab", review.pr.url.as_str()));
    let mut tab: PrTab = ui.data(|d| d.get_temp(id).unwrap_or_default());
    // An open file *is* the Files tab — a rail row click drives the center directly,
    // so the two can never disagree about what the center shows.
    if review.selected_file.is_some() {
        tab = PrTab::Files;
    }
    let counts = |t: PrTab| match t {
        PrTab::Conversation => review
            .detail
            .map(|d| d.comments.iter().filter(|c| c.path.is_none()).count())
            .unwrap_or(0),
        PrTab::Files => review.files.len(),
    };

    let mut x = rect.left() + PANEL_PAD_X;
    for candidate in PrTab::ALL {
        let label = candidate.label();
        let active = tab == candidate;
        let galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            egui::FontId::new(
                SECTION_TITLE_SIZE,
                if active {
                    crate::theme::medium_family(ui.ctx())
                } else {
                    egui::FontFamily::Proportional
                },
            ),
            if active {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
        let count = counts(candidate);
        let count_w = if count > 0 { 22.0 } else { 0.0 };
        let w = galley.size().x + count_w + 2.0 * TAB_PAD_X;
        let cell =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, rect.height()));
        let response = ui.interact(
            cell,
            ui.id().with(("pr_review_tab", label)),
            egui::Sense::click(),
        );
        if response.hovered() && !active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let center_y = cell.center().y;
        ui.painter().galley(
            egui::pos2(cell.left() + TAB_PAD_X, center_y - galley.size().y / 2.0),
            galley,
            palette.text_primary,
        );
        if count > 0 {
            ui.painter().text(
                egui::pos2(cell.right() - TAB_PAD_X, center_y),
                egui::Align2::RIGHT_CENTER,
                count.to_string(),
                egui::FontId::proportional(META_SIZE),
                palette.text_muted,
            );
        }
        if active {
            ui.painter().hline(
                egui::Rangef::new(cell.left() + 4.0, cell.right() - 4.0),
                cell.bottom() - 1.5,
                egui::Stroke::new(2.0_f32, palette.accent),
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label.to_owned())
        });
        if response.clicked() {
            tab = candidate;
            // Leaving Files drops the open file, so coming back to the diff is an
            // explicit choice rather than a stale selection (pull-requests.md §11).
            if candidate != PrTab::Files && review.selected_file.is_some() {
                action.close_file = true;
            }
        }
        x += w;
    }
    ui.data_mut(|d| d.insert_temp(id, tab));
    tab
}

fn detail_back_button(ui: &mut egui::Ui, palette: &Palette, rect: egui::Rect) -> bool {
    let response = ui.interact(rect, ui.id().with("pr_detail_back"), egui::Sense::click());
    let fill = if response.hovered() {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    paint_icon(
        ui.painter(),
        rect.center(),
        STATUS_ICON,
        Icon::ArrowLeft,
        palette.text_secondary,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back".to_owned())
    });
    response.clicked()
}

fn detail_label_accessibility(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: &'static str,
    label: String,
) {
    let response = ui.interact(rect, ui.id().with(id), egui::Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone()));
}

#[allow(clippy::too_many_arguments)]
fn review_file_list(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    view: FileViewMode,
    viewed: &HashSet<String>,
    unread_only: bool,
    hide_tests: bool,
    collapsed: &mut HashSet<String>,
    action: &mut PullRequestsPageAction,
) {
    if review.files_loading && review.files.is_empty() {
        ui.label(muted(palette, "Loading changed files…"));
        return;
    }
    if let Some(error) = review.files_error {
        ui.label(muted(palette, error));
        return;
    }
    if review.files.is_empty() {
        ui.label(muted(palette, "No file changes"));
        return;
    }
    let visible: Vec<usize> = review
        .files
        .iter()
        .enumerate()
        .filter(|(_, file)| !unread_only || !viewed.contains(&file.path))
        .filter(|(_, file)| !hide_tests || !is_test_path(&file.path))
        .map(|(idx, _)| idx)
        .collect();
    if visible.is_empty() {
        let message = if hide_tests && !unread_only {
            "Only test files changed"
        } else {
            "All files viewed"
        };
        ui.label(muted(palette, message));
        return;
    }
    // Rows abut with hairline separators, matching the commit-detail file list;
    // scoped so the zeroed spacing doesn't bleed into the sections below.
    // `ordered` records the file indices in display order so ↑/↓ can step the
    // selection through exactly what is on screen (flat order, or tree-leaf order).
    let mut ordered: Vec<usize> = Vec::new();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        match view {
            FileViewMode::Flat => {
                for (row_idx, idx) in visible.iter().copied().enumerate() {
                    if row_idx > 0 {
                        row_separator(ui, palette);
                    }
                    ordered.push(idx);
                    let file = &review.files[idx];
                    review_file_row(ui, palette, review, idx, file, &file.path, 0.0, action);
                }
            }
            FileViewMode::Tree => {
                let paths: Vec<&str> = visible
                    .iter()
                    .map(|idx| review.files[*idx].path.as_str())
                    .collect();
                let rows = file_tree::tree_rows(&paths, collapsed);
                let mut toggle: Option<String> = None;
                for row in rows {
                    match row {
                        TreeRow::Dir {
                            name,
                            full_path,
                            depth,
                            collapsed: is_collapsed,
                        } => {
                            let indent = depth as f32 * file_list::TREE_INDENT_STEP;
                            if file_list::dir_row(ui, palette, &name, indent, is_collapsed)
                                .clicked()
                            {
                                toggle = Some(full_path);
                            }
                        }
                        TreeRow::File { index, depth } => {
                            let idx = visible[index];
                            ordered.push(idx);
                            let file = &review.files[idx];
                            let indent = depth as f32 * file_list::TREE_INDENT_STEP;
                            review_file_row(
                                ui,
                                palette,
                                review,
                                idx,
                                file,
                                leaf_name(&file.path),
                                indent,
                                action,
                            );
                        }
                    }
                }
                if let Some(dir) = toggle {
                    if !collapsed.remove(&dir) {
                        collapsed.insert(dir);
                    }
                }
            }
        }
    });

    if let Some(target) = file_nav_target(ui, review.selected_file, &ordered) {
        // The keyboard-picked row follows the selection into the viewport; a click
        // never requests a scroll (its row is already visible).
        file_list::request_row_scroll(ui, pr_file_scroll_id(), target);
        action.select_file = Some(target);
    }
}

/// `pr_review_rail`-scoped id under which keyboard navigation parks the row to
/// scroll into view, consumed by the matching row on the next frame.
fn pr_file_scroll_id() -> egui::Id {
    egui::Id::new("pr_review_file_scroll")
}

/// ↑/↓ over the changed-files rail (mirrors the git sidebar): with a file open
/// and no text field capturing keys, step the selection to the previous/next file
/// in display order, wrapping at both ends. Returns the file index to select.
fn file_nav_target(ui: &egui::Ui, selected: Option<usize>, ordered: &[usize]) -> Option<usize> {
    if selected.is_none() || ordered.is_empty() || ui.ctx().egui_wants_keyboard_input() {
        return None;
    }
    let forward = ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if no_modifiers(*modifiers) => match key {
                egui::Key::ArrowDown => Some(true),
                egui::Key::ArrowUp => Some(false),
                _ => None,
            },
            _ => None,
        })
    })?;
    let len = ordered.len();
    let current = selected.and_then(|sel| ordered.iter().position(|&idx| idx == sel));
    let target = match (current, forward) {
        (Some(index), true) => (index + 1) % len,
        (None, true) => 0,
        (Some(0) | None, false) => len - 1,
        (Some(index), false) => index - 1,
    };
    Some(ordered[target])
}

fn no_modifiers(modifiers: egui::Modifiers) -> bool {
    !modifiers.command
        && !modifiers.mac_cmd
        && !modifiers.alt
        && !modifiers.ctrl
        && !modifiers.shift
}

#[allow(clippy::too_many_arguments)]
fn review_file_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    idx: usize,
    file: &CommitFile,
    display: &str,
    indent: f32,
    action: &mut PullRequestsPageAction,
) {
    let indicators = file_indicators_for(review, &file.path);
    let out = file_row(
        ui,
        palette,
        egui::Sense::click(),
        &FileRow {
            path: display,
            kind: file.kind,
            additions: file.additions,
            deletions: file.deletions,
            selected: review.selected_file == Some(idx),
            stats_hidden_on_hover: false,
            indent,
            trailing_reserved: indicators.reserved_width(),
        },
    );
    paint_file_indicators(
        ui,
        palette,
        out.trailing_rect,
        &file.path,
        indicators,
        review.selected_file == Some(idx),
        out.hovered,
    );
    let response = out.response.on_hover_text(&file.path);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            review.selected_file == Some(idx),
            &file.path,
        )
    });
    file_list::consume_row_scroll(ui, &response, pr_file_scroll_id(), &idx);
    if response.clicked() {
        action.select_file = Some(idx);
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct FileIndicators {
    forge: bool,
    agent: bool,
}

impl FileIndicators {
    fn reserved_width(self) -> f32 {
        let mut width = 0.0;
        if self.forge {
            width += 16.0;
        }
        if self.agent {
            width += 16.0;
        }
        if width > 0.0 {
            width + 4.0
        } else {
            0.0
        }
    }
}

fn file_indicators_for(review: &PrReviewView<'_>, path: &str) -> FileIndicators {
    FileIndicators {
        forge: review
            .draft
            .get(path)
            .is_some_and(|comments| !comments.is_empty()),
        agent: review
            .agent_notes
            .get(path)
            .is_some_and(|comments| !comments.is_empty()),
    }
}

fn paint_file_indicators(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    path: &str,
    indicators: FileIndicators,
    selected: bool,
    hovered: bool,
) {
    if indicators.reserved_width() <= 0.0 {
        return;
    }
    let mut x = rect.left();
    let y = rect.center().y;
    let active = selected || hovered;
    if indicators.forge {
        x += file_indicator_icon(
            ui,
            palette,
            egui::pos2(x, y),
            Icon::MessageSquarePlus,
            palette.accent,
            active,
            format!("{path}: has review comments"),
        );
    }
    if indicators.agent {
        file_indicator_icon(
            ui,
            palette,
            egui::pos2(x, y),
            Icon::Sparkles,
            palette.accent_ai,
            active,
            format!("{path}: has agent notes"),
        );
    }
}

fn file_indicator_icon(
    ui: &mut egui::Ui,
    palette: &Palette,
    pos: egui::Pos2,
    icon: Icon,
    color: egui::Color32,
    active: bool,
    label: String,
) -> f32 {
    let width = 16.0;
    let rect = egui::Rect::from_center_size(
        egui::pos2(pos.x + width / 2.0, pos.y),
        egui::vec2(width, 18.0),
    );
    let tint = if active { color } else { palette.text_muted };
    paint_icon(ui.painter(), rect.center(), 12.0, icon, tint);
    let response = ui.interact(
        rect,
        ui.id().with(("pr_file_indicator", label.as_str())),
        egui::Sense::hover(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone()));
    width
}

/// The inline style flags carried while walking the markdown events, so a run can be
/// rendered with the right weight/shape/family.
#[derive(Clone, Copy, Default)]
struct MdInline {
    strong: bool,
    emphasis: bool,
    code: bool,
    strike: bool,
    link: bool,
}

/// Renders a PR body / comment as markdown. Unlike `egui_commonmark` (which emits
/// plain `RichText`, so line-height and letter-spacing are fixed at the font row
/// height), this builds the `LayoutJob`s itself — the prose carries a looser
/// line-height and a little letter-spacing, the levers that make a long description
/// or reply readable rather than a dense wall (pull-requests.md §11).
pub(crate) fn markdown(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    // The card around the prose spans its area; the *text* stops at its measure, so a
    // wide window leaves margin inside the surface instead of 150-character lines (§11).
    let max_width = ui.available_width().min(CONV_PROSE_MEASURE);
    let new_job = |indent: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = (max_width - indent).max(80.0);
        job
    };
    // Emphasis is carried by the medium face rather than by a brighter ink: prose that
    // only reaches full contrast where it is bold leaves the rest reading as disabled,
    // which is what greys a whole description in dark mode (§11).
    let medium = crate::theme::medium_family(ui.ctx());
    let append = |job: &mut egui::text::LayoutJob,
                  run: &str,
                  s: MdInline,
                  heading: Option<HeadingLevel>,
                  quote: bool| {
        let size = match heading {
            Some(level) => md_heading_size(level),
            None if s.code => MD_CODE_SIZE,
            None => MD_TEXT_SIZE,
        };
        let family = if s.code {
            egui::FontFamily::Monospace
        } else if s.strong || heading.is_some() {
            medium.clone()
        } else {
            egui::FontFamily::Proportional
        };
        let mut color = palette.text_primary;
        if quote {
            color = palette.text_muted;
        }
        if s.link {
            color = palette.accent;
        }
        let format = egui::text::TextFormat {
            font_id: egui::FontId::new(size, family),
            color,
            background: if s.code {
                with_alpha(palette.text_muted, 28)
            } else {
                egui::Color32::TRANSPARENT
            },
            italics: s.emphasis,
            extra_letter_spacing: if s.code { 0.0 } else { MD_LETTER_SPACING },
            line_height: Some(if heading.is_some() {
                size * 1.3
            } else {
                MD_LINE_HEIGHT
            }),
            underline: if s.link {
                egui::Stroke::new(1.0_f32, palette.accent)
            } else {
                egui::Stroke::NONE
            },
            strikethrough: if s.strike {
                egui::Stroke::new(1.0_f32, color)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        };
        job.append(run, 0.0, format);
    };
    let flush = |ui: &mut egui::Ui,
                 job: &mut egui::text::LayoutJob,
                 links: &mut Vec<MdLink>,
                 indent: f32| {
        if job.text.trim().is_empty() {
            *job = new_job(indent);
            links.clear();
            return;
        }
        let done = std::mem::replace(job, new_job(indent));
        let runs = std::mem::take(links);
        if indent > 0.0 {
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                prose(ui, done, &runs);
            });
        } else {
            prose(ui, done, &runs);
        }
        ui.add_space(MD_PARAGRAPH_GAP);
    };

    let mut job = new_job(0.0);
    let mut style = MdInline::default();
    let mut heading: Option<HeadingLevel> = None;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut quote_depth: usize = 0;
    let mut in_code_block = false;
    let mut code_block = String::new();
    let mut table: Option<MdTable> = None;
    let mut cell: Option<MdCell> = None;
    let mut in_head = false;
    // The image being described: its alt text arrives as ordinary text events between
    // the tags, and belongs to the image, not to the paragraph around it.
    let mut image: Option<(String, String)> = None;
    // The link runs of the block being built: where each one sits in the text, so the
    // laid-out galley can be hit-tested (a `LayoutJob` carries no link of its own).
    let mut links: Vec<MdLink> = Vec::new();
    let mut link: Option<(usize, String)> = None;
    // Bitbucket writes a smart link as `[text](url){: data-inline-card='' }`: without
    // the attribute-list extension the brace run lands in the text right after the
    // link, and reads as garbage in the middle of a sentence.
    let mut after_link = false;

    let indent_now = |list: &[Option<u64>], quote: usize| {
        list.len() as f32 * MD_LIST_INDENT + quote as f32 * MD_QUOTE_INDENT
    };

    // GFM tables and task lists: a PR description that carries a result table is a
    // table, not a paragraph of pipes.
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level),
            Event::Start(Tag::Strong) => style.strong = true,
            Event::Start(Tag::Emphasis) => style.emphasis = true,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::Start(Tag::Link { dest_url, .. }) => {
                style.link = true;
                let target = cell.as_ref().map_or(&job, |c| &c.job);
                link = Some((target.text.chars().count(), dest_url.to_string()));
            }
            Event::Start(Tag::List(start)) => list_stack.push(start),
            Event::Start(Tag::Item) => {
                let prefix = match list_stack.last_mut() {
                    Some(Some(n)) => {
                        let p = format!("{n}. ");
                        *n += 1;
                        p
                    }
                    _ => "•  ".to_owned(),
                };
                job = new_job(indent_now(&list_stack, quote_depth));
                append(&mut job, &prefix, MdInline::default(), None, false);
            }
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_block.clear();
            }
            Event::Start(Tag::Table(aligns)) => {
                flush(
                    ui,
                    &mut job,
                    &mut links,
                    indent_now(&list_stack, quote_depth),
                );
                table = Some(MdTable {
                    aligns,
                    head: Vec::new(),
                    rows: Vec::new(),
                });
            }
            Event::Start(Tag::TableHead) => in_head = true,
            Event::Start(Tag::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => cell = Some(MdCell::new(new_job(0.0))),
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some((dest_url.to_string(), String::new()));
            }
            Event::End(TagEnd::Image) => {
                if let Some((url, alt)) = image.take() {
                    // A cell hangs its pictures under its text — a smoke-test table
                    // carries its evidence in the Evidence column, and naming the file
                    // there is not the evidence (§11).
                    if let Some(cell) = cell.as_mut() {
                        cell.images.push((url, alt));
                    } else {
                        flush(
                            ui,
                            &mut job,
                            &mut links,
                            indent_now(&list_stack, quote_depth),
                        );
                        md_image(ui, palette, &url, &alt);
                    }
                }
            }
            // A pasted screenshot often arrives as raw `<img src="…">` rather than
            // markdown; the tag names an image all the same.
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some((url, alt)) = html_img(&html) {
                    if let Some(cell) = cell.as_mut() {
                        cell.images.push((url, alt));
                    } else {
                        flush(
                            ui,
                            &mut job,
                            &mut links,
                            indent_now(&list_stack, quote_depth),
                        );
                        md_image(ui, palette, &url, &alt);
                    }
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let (Some(t), Some(done)) = (table.as_mut(), cell.take()) {
                    if in_head {
                        t.head.push(done);
                    } else if let Some(row) = t.rows.last_mut() {
                        row.push(done);
                    }
                }
            }
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    md_table(ui, palette, t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(
                    ui,
                    &mut job,
                    &mut links,
                    indent_now(&list_stack, quote_depth),
                );
                heading = None;
            }
            Event::End(TagEnd::Paragraph) => {
                if list_stack.is_empty() {
                    flush(
                        ui,
                        &mut job,
                        &mut links,
                        quote_depth as f32 * MD_QUOTE_INDENT,
                    );
                }
            }
            Event::End(TagEnd::Item) => flush(
                ui,
                &mut job,
                &mut links,
                indent_now(&list_stack, quote_depth),
            ),
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
            Event::End(TagEnd::CodeBlock) => {
                md_code_block(ui, palette, code_block.trim_end());
                in_code_block = false;
            }
            Event::End(TagEnd::Strong) => style.strong = false,
            Event::End(TagEnd::Emphasis) => style.emphasis = false,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::End(TagEnd::Link) => {
                style.link = false;
                after_link = true;
                if let Some((start, url)) = link.take() {
                    let target = cell.as_ref().map_or(&job, |c| &c.job);
                    let end = target.text.chars().count();
                    let run = MdLink {
                        range: start..end,
                        url,
                    };
                    match cell.as_mut() {
                        Some(cell) => cell.links.push(run),
                        None => links.push(run),
                    }
                }
            }
            Event::Text(t) => {
                let attribute_run = after_link;
                after_link = false;
                if in_code_block {
                    code_block.push_str(&t);
                } else if let Some((_, alt)) = image.as_mut() {
                    alt.push_str(&t);
                } else {
                    // Header cells carry no `Strong` of their own — the grammar makes
                    // them headers, so the weight is ours to add.
                    let mut s = style;
                    s.strong |= in_head;
                    let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                    let text = if attribute_run {
                        strip_link_attributes(&t)
                    } else {
                        &t
                    };
                    append(target, text, s, heading, quote_depth > 0);
                }
            }
            Event::Code(t) => {
                let mut s = style;
                s.code = true;
                s.strong |= in_head;
                let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                append(target, &t, s, heading, quote_depth > 0);
            }
            Event::SoftBreak => {
                let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                append(target, " ", style, heading, quote_depth > 0);
            }
            Event::HardBreak => {
                let target = cell.as_mut().map_or(&mut job, |c| &mut c.job);
                append(target, "\n", style, heading, quote_depth > 0);
            }
            _ => {}
        }
    }
    flush(
        ui,
        &mut job,
        &mut links,
        quote_depth as f32 * MD_QUOTE_INDENT,
    );
}

/// An embedded image, as far as the renderer is concerned: fetched off-thread by the
/// app (`PrReviewRequest::Image`) and decoded into a texture it hands back here.
#[derive(Clone)]
pub enum MdImage {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

/// Cache of embedded images, by URL. Lives in egui's temp memory rather than in the
/// view struct: `markdown` is called from three surfaces (body, conversation card,
/// diff overlay), and threading a media borrow through all of them — the diff view
/// included — would buy nothing the URL key does not already give.
pub fn md_image_cache_id() -> egui::Id {
    egui::Id::new("pr_md_images")
}

/// Links clicked in a body this frame: the view has no business opening a browser, so
/// it names the URL here and the app drains it.
pub fn md_link_clicked_id() -> egui::Id {
    egui::Id::new("pr_md_links_clicked")
}

/// Images a body asked for this frame but has no bytes for yet — the app drains it,
/// fetches, and writes the result back into `md_image_cache_id`.
pub fn md_image_wanted_id() -> egui::Id {
    egui::Id::new("pr_md_images_wanted")
}

/// The reading width an embedded image is allowed: wide enough for a screenshot to be
/// legible, capped so a full-page capture does not push everything else off screen.
const MD_IMAGE_MAX_H: f32 = 420.0;

/// The open image viewer: which picture, and how the user has moved it since (§11).
#[derive(Clone)]
struct MdViewer {
    url: String,
    /// Multiplier over the fit-to-screen size — 1.0 is "the whole picture on screen".
    zoom: f32,
    /// Pan, in screen points, from the centred position.
    offset: egui::Vec2,
}

impl MdViewer {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }
}

/// Zoom bounds of the viewer: out to the fit size, in to 8× — past that a screenshot is
/// a grid of texels, not a detail.
const MD_VIEWER_MIN_ZOOM: f32 = 1.0;
const MD_VIEWER_MAX_ZOOM: f32 = 8.0;

fn md_viewer_id() -> egui::Id {
    egui::Id::new("pr_md_image_viewer")
}

/// Whether the image viewer is up — the surfaces around it read this rather than the
/// state itself (which is the viewer's own business).
pub fn md_viewer_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<MdViewer>(md_viewer_id()).is_some())
}

/// The clicked picture, full surface: scroll (or pinch) zooms, drag moves it, double
/// click resets, `Esc` / a click on the backdrop / the ✕ closes. Drawn over the review
/// and owning `Esc` while it is up, so the press that closes it never also drops the
/// file or leaves the review (pull-requests.md §11).
fn md_image_viewer(ui: &mut egui::Ui, palette: &Palette, screen: egui::Rect) {
    let Some(mut state) = ui.data(|d| d.get_temp::<MdViewer>(md_viewer_id())) else {
        return;
    };
    let texture = ui.data(|d| {
        d.get_temp::<HashMap<String, MdImage>>(md_image_cache_id())
            .and_then(|images| match images.get(&state.url) {
                Some(MdImage::Ready(texture)) => Some(texture.clone()),
                _ => None,
            })
    });
    // The bytes went away (a reload dropped the cache): nothing to look at.
    let Some(texture) = texture else {
        ui.data_mut(|d| d.remove::<MdViewer>(md_viewer_id()));
        return;
    };

    let mut close = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    egui::Area::new(md_viewer_id().with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(screen);
            let backdrop = ui.interact(
                screen,
                md_viewer_id().with("backdrop"),
                egui::Sense::click_and_drag(),
            );
            ui.painter()
                .rect_filled(screen, 0, with_alpha(palette.bg_canvas, 235));

            // Fit first, then the user's own zoom on top of it.
            let size = texture.size_vec2();
            let fit = (screen.width() * 0.9 / size.x)
                .min(screen.height() * 0.9 / size.y)
                .min(1.0);
            let pointer = ui.input(|i| i.pointer.hover_pos());
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (1.0 + scroll * 0.002);
            if (factor - 1.0).abs() > f32::EPSILON {
                let before = state.zoom;
                state.zoom = (state.zoom * factor).clamp(MD_VIEWER_MIN_ZOOM, MD_VIEWER_MAX_ZOOM);
                // Zoom about the pointer, so the detail under it stays under it.
                if let Some(pointer) = pointer {
                    let anchor = pointer - screen.center() - state.offset;
                    state.offset -= anchor * (state.zoom / before - 1.0);
                }
            }
            if backdrop.dragged() {
                state.offset += backdrop.drag_delta();
            }
            if backdrop.double_clicked() {
                state = MdViewer::new(&state.url);
            } else if backdrop.clicked() {
                close = true;
            }
            if state.zoom <= MD_VIEWER_MIN_ZOOM {
                state.offset = egui::Vec2::ZERO;
            }
            let shown = size * fit * state.zoom;
            let rect = egui::Rect::from_center_size(screen.center() + state.offset, shown);
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if backdrop.hovered() {
                ui.ctx()
                    .set_cursor_icon(if state.zoom > MD_VIEWER_MIN_ZOOM {
                        egui::CursorIcon::Grab
                    } else {
                        egui::CursorIcon::ZoomIn
                    });
            }
            md_viewer_bar(ui, palette, screen, &state, &mut close);
        });

    if close {
        ui.data_mut(|d| d.remove::<MdViewer>(md_viewer_id()));
    } else {
        ui.data_mut(|d| d.insert_temp(md_viewer_id(), state));
    }
}

/// The viewer's chrome: the zoom level and a ✕, in a strip at the top.
fn md_viewer_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    screen: egui::Rect,
    state: &MdViewer,
    close: &mut bool,
) {
    let bar = egui::Rect::from_min_size(
        egui::pos2(screen.left(), screen.top()),
        egui::vec2(screen.width(), 44.0),
    );
    ui.painter().text(
        egui::pos2(bar.left() + PANEL_PAD_X, bar.center().y),
        egui::Align2::LEFT_CENTER,
        format!(
            "{:.0}%  ·  scroll to zoom, drag to move",
            state.zoom * 100.0
        ),
        egui::FontId::proportional(CONV_META_SIZE),
        palette.text_muted,
    );
    let hit = egui::Rect::from_center_size(
        egui::pos2(bar.right() - PANEL_PAD_X - 12.0, bar.center().y),
        egui::vec2(28.0, 28.0),
    );
    let response = ui.interact(hit, md_viewer_id().with("close"), egui::Sense::click());
    let ink = if response.hovered() {
        palette.text_primary
    } else {
        palette.text_muted
    };
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    paint_icon(ui.painter(), hit.center(), STATUS_ICON, Icon::X, ink);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    *close |= response.clicked();
}

/// One embedded image: the texture once it has landed, else a muted placeholder line
/// carrying its alt text (and, on failure, why).
fn md_image(ui: &mut egui::Ui, palette: &Palette, url: &str, alt: &str) {
    let slot: Option<MdImage> = ui.data(|d| {
        d.get_temp::<HashMap<String, MdImage>>(md_image_cache_id())
            .and_then(|images| images.get(url).cloned())
    });
    match slot {
        Some(MdImage::Ready(texture)) => {
            let size = texture.size_vec2();
            let scale = (ui.available_width() / size.x)
                .min(MD_IMAGE_MAX_H / size.y.max(1.0))
                .min(1.0);
            // In the flow a screenshot is a thumbnail — the detail lives in the viewer,
            // one click away (§11).
            let response = ui
                .add(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(size * scale)
                        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
                        .sense(egui::Sense::click()),
                )
                .on_hover_text(format!("{url}\n\nClick to open"));
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                ui.data_mut(|d| d.insert_temp(md_viewer_id(), MdViewer::new(url)));
            }
            ui.add_space(MD_PARAGRAPH_GAP);
        }
        Some(MdImage::Failed(reason)) => {
            md_image_placeholder(
                ui,
                palette,
                url,
                alt,
                &format!("image unavailable — {reason}"),
            );
        }
        Some(MdImage::Loading) => md_image_placeholder(ui, palette, url, alt, "loading image…"),
        None => {
            // First sight: ask the app for it, and stand in until it lands.
            ui.data_mut(|d| {
                let wanted: &mut Vec<String> = d.get_temp_mut_or_default(md_image_wanted_id());
                if !wanted.iter().any(|u| u == url) {
                    wanted.push(url.to_owned());
                }
            });
            md_image_placeholder(ui, palette, url, alt, "loading image…");
        }
    }
}

fn md_image_placeholder(ui: &mut egui::Ui, palette: &Palette, url: &str, alt: &str, status: &str) {
    ui.horizontal(|ui| {
        paint_icon_at_cursor(ui, palette, Icon::Image);
        let label = format!("{} — {status}", image_label(url, alt));
        ui.label(
            egui::RichText::new(label)
                .size(MD_CODE_SIZE)
                .color(palette.text_muted),
        );
    });
    ui.add_space(MD_PARAGRAPH_GAP);
}

fn paint_icon_at_cursor(ui: &mut egui::Ui, palette: &Palette, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    paint_icon(ui.painter(), rect.center(), 14.0, icon, palette.text_muted);
}

/// How an image names itself while it is not on screen (loading, or unavailable): its
/// alt text, else the file name the URL ends on.
fn image_label(url: &str, alt: &str) -> String {
    if !alt.trim().is_empty() {
        return alt.to_owned();
    }
    url.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(url)
        .to_owned()
}

/// `src` and `alt` of an `<img …>` tag in a raw-HTML chunk, if it is one. A body's
/// HTML is not parsed beyond this: the one tag that carries a picture is worth
/// reading, a general HTML renderer is not.
fn html_img(html: &str) -> Option<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<img")?;
    let end = lower[start..].find('>').map(|i| start + i)?;
    let attr = |name: &str| {
        let at = lower[start..end].find(&format!("{name}="))? + start + name.len() + 1;
        let rest = html.get(at..end)?;
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        rest[1..].split(quote).next().map(str::to_owned)
    };
    let src = attr("src")?;
    Some((src, attr("alt").unwrap_or_default()))
}

/// A GFM table collected while parsing: the header cells, then one job per cell per
/// row, with the column alignments the delimiter row declared.
struct MdTable {
    aligns: Vec<pulldown_cmark::Alignment>,
    head: Vec<MdCell>,
    rows: Vec<Vec<MdCell>>,
}

/// A link run inside a laid-out block: where it sits in the text, and where it points.
struct MdLink {
    range: std::ops::Range<usize>,
    url: String,
}

/// Draws a prose block from its job and makes its link runs clickable.
///
/// Laid out **here** rather than handed to `Label` as a job: `Label` relayouts a bare
/// `LayoutJob` at the width of the `Ui` it lands in (egui `label.rs`), which would drop
/// the reading measure — and the galley is what the link hit-boxes are measured on. A
/// clicked URL goes to the app through `md_link_clicked_id`, which owns the opening.
fn prose(ui: &mut egui::Ui, job: egui::text::LayoutJob, links: &[MdLink]) {
    let galley = ui.painter().layout_job(job);
    let response = ui.add(egui::Label::new(galley.clone()));
    let origin = response.rect.min.to_vec2();
    for (i, link) in links.iter().enumerate() {
        for (j, rect) in link_rects(&galley, &link.range).into_iter().enumerate() {
            let hit = ui.interact(
                rect.translate(origin),
                response.id.with(("md_link", i, j)),
                egui::Sense::click(),
            );
            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if hit.clicked() {
                ui.data_mut(|d| {
                    let clicked: &mut Vec<String> = d.get_temp_mut_or_default(md_link_clicked_id());
                    clicked.push(link.url.clone());
                });
            }
            // A run of a paragraph is a widget of its own here: name it, or the link is
            // invisible to the accessibility tree (and to the tests driving it).
            let url = link.url.clone();
            hit.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Link, true, url.clone())
            });
            hit.on_hover_text(&link.url);
        }
    }
}

/// The boxes a link run occupies in its galley — one per row it spans, since a link
/// that wraps is two boxes on two lines, not one rectangle across the paragraph.
fn link_rects(galley: &egui::Galley, range: &std::ops::Range<usize>) -> Vec<egui::Rect> {
    let start = galley.pos_from_cursor(egui::text::CCursor::new(range.start));
    let end = galley.pos_from_cursor(egui::text::CCursor::new(range.end));
    if (start.top() - end.top()).abs() < 0.5 {
        return vec![egui::Rect::from_min_max(start.min, end.max)];
    }
    galley
        .rows
        .iter()
        .filter(|row| row.max_y() > start.top() && row.min_y() < end.bottom())
        .map(|row| {
            let first = row.min_y() <= start.top() && start.top() < row.max_y();
            let last = row.min_y() <= end.top() && end.top() < row.max_y();
            let left = if first {
                start.left()
            } else {
                row.rect().left()
            };
            let right = if last {
                end.right()
            } else {
                row.rect().right()
            };
            egui::Rect::from_min_max(
                egui::pos2(left, row.min_y()),
                egui::pos2(right, row.max_y()),
            )
        })
        .collect()
}

/// Bitbucket's smart-link attribute run — `{: data-inline-card='' }` right after a
/// link — dropped: without the attribute-list extension it lands in the prose, and it
/// is markup, not something the author wrote to be read.
fn strip_link_attributes(text: &str) -> &str {
    let rest = text.strip_prefix("{:").unwrap_or(text);
    if std::ptr::eq(rest, text) {
        return text;
    }
    match rest.find('}') {
        Some(end) if !rest[..end].contains('\n') => &rest[end + 1..],
        _ => text,
    }
}

/// One table cell: its text, plus the pictures it embeds — drawn under that text
/// inside the column, so a table can carry a screenshot as evidence (§11).
struct MdCell {
    job: egui::text::LayoutJob,
    images: Vec<(String, String)>,
    links: Vec<MdLink>,
}

impl MdCell {
    fn new(job: egui::text::LayoutJob) -> Self {
        Self {
            job,
            images: Vec::new(),
            links: Vec::new(),
        }
    }
}

/// What a cell would take on a single unwrapped line — the width its column asks for.
fn md_cell_natural_width(ui: &egui::Ui, cell: &MdCell) -> f32 {
    let text = if cell.job.text.trim().is_empty() {
        0.0
    } else {
        let mut job = cell.job.clone();
        job.wrap.max_width = f32::INFINITY;
        ui.painter().layout_job(job).rect.width()
    };
    if cell.images.is_empty() {
        text
    } else {
        text.max(MD_CELL_IMAGE_W)
    }
}

const MD_CELL_PAD_X: f32 = 8.0;
const MD_CELL_PAD_Y: f32 = 5.0;
const MD_CELL_MIN_W: f32 = 72.0;
/// Ceiling on what a column may *ask* for: without it a single long line (a URL, a
/// stack trace) would claim the whole table and squeeze its neighbours to the floor.
const MD_CELL_MAX_W: f32 = 420.0;
/// What a cell holding an image asks for, since its text ("same screenshot") says
/// nothing about the thumbnail below it — and the thumbnail scales to the column.
const MD_CELL_IMAGE_W: f32 = 220.0;
/// The frame's horizontal inner margin, subtracted from the width the columns divide.
const MD_TABLE_MARGIN_X: f32 = 10.0;

/// A markdown table: fixed-width columns over the reading width, each cell wrapping
/// inside its own column. Columns are sized on what their content asks for — a *Step*
/// column of single digits next to a sentence should not take the same quarter — then
/// scaled to the width available, so the table fills the card without ever pushing past
/// it and taking the horizontal scroll with it.
fn md_table(ui: &mut egui::Ui, palette: &Palette, mut table: MdTable) {
    let columns = table
        .rows
        .iter()
        .map(Vec::len)
        .chain(std::iter::once(table.head.len()))
        .max()
        .unwrap_or(0);
    if columns == 0 {
        return;
    }
    let natural: Vec<f32> = (0..columns)
        .map(|i| {
            table
                .rows
                .iter()
                .chain(std::iter::once(&table.head))
                .filter_map(|row| row.get(i))
                .map(|cell| md_cell_natural_width(ui, cell))
                .fold(0.0_f32, f32::max)
                .clamp(MD_CELL_MIN_W, MD_CELL_MAX_W)
        })
        .collect();
    let asked: f32 = natural.iter().sum();
    let free =
        (ui.available_width() - 2.0 * MD_TABLE_MARGIN_X - (columns as f32 - 1.0) * MD_CELL_PAD_X)
            .max(MD_CELL_MIN_W);
    let widths: Vec<f32> = natural
        .iter()
        .map(|w| (w / asked * free).max(MD_CELL_MIN_W))
        .collect();
    let align = |i: usize| match table.aligns.get(i) {
        Some(pulldown_cmark::Alignment::Center) => egui::Align::Center,
        Some(pulldown_cmark::Alignment::Right) => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let row_ui = |ui: &mut egui::Ui, cells: &mut Vec<MdCell>| {
        ui.horizontal_top(|ui| {
            let last = cells.len().saturating_sub(1);
            for (i, cell) in cells.iter_mut().enumerate() {
                let col_w = widths[i];
                cell.job.wrap.max_width = col_w;
                let done = std::mem::take(&mut cell.job);
                let images = std::mem::take(&mut cell.images);
                let links = std::mem::take(&mut cell.links);
                ui.allocate_ui_with_layout(
                    egui::vec2(col_w, 0.0),
                    egui::Layout::top_down(align(i)),
                    |ui| {
                        // Without a floor the cell shrinks to its text and the next
                        // column starts wherever this one happened to end — the rows
                        // would stop lining up under their headers.
                        ui.set_min_width(col_w);
                        if !done.text.trim().is_empty() {
                            prose(ui, done, &links);
                        }
                        for (url, alt) in &images {
                            md_image(ui, palette, url, alt);
                        }
                    },
                );
                if i != last {
                    ui.add_space(MD_CELL_PAD_X);
                }
            }
        });
    };

    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
        .inner_margin(egui::Margin::symmetric(MD_TABLE_MARGIN_X as i8, 8))
        .show(ui, |ui| {
            if !table.head.is_empty() {
                row_ui(ui, &mut table.head);
                ui.add_space(MD_CELL_PAD_Y);
                let line = ui.min_rect().x_range();
                ui.painter().hline(
                    line,
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, palette.border_subtle),
                );
                ui.add_space(MD_CELL_PAD_Y);
            }
            let last = table.rows.len().saturating_sub(1);
            for (index, row) in table.rows.iter_mut().enumerate() {
                row_ui(ui, row);
                if index != last {
                    ui.add_space(MD_CELL_PAD_Y);
                    ui.painter().hline(
                        ui.min_rect().x_range(),
                        ui.cursor().top(),
                        egui::Stroke::new(1.0_f32, with_alpha(palette.border_subtle, 120)),
                    );
                    ui.add_space(MD_CELL_PAD_Y);
                }
            }
        });
    ui.add_space(MD_PARAGRAPH_GAP);
}

/// Per-level heading size for `markdown`, scaling down from the body size.
fn md_heading_size(level: pulldown_cmark::HeadingLevel) -> f32 {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => MD_TEXT_SIZE + 6.0,
        H2 => MD_TEXT_SIZE + 4.0,
        H3 => MD_TEXT_SIZE + 2.0,
        H4 => MD_TEXT_SIZE + 1.0,
        H5 | H6 => MD_TEXT_SIZE,
    }
}

/// A fenced/indented code block in `markdown`: monospace lines on a subtle fill.
fn md_code_block(ui: &mut egui::Ui, palette: &Palette, code: &str) {
    egui::Frame::new()
        .fill(with_alpha(palette.text_muted, 24))
        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for line in code.lines() {
                ui.label(
                    egui::RichText::new(line)
                        .size(MD_CODE_SIZE)
                        .monospace()
                        .color(palette.text_secondary),
                );
            }
        });
    ui.add_space(MD_PARAGRAPH_GAP);
}

/// Author block of the detail (pull-requests.md §11): avatar + name, the
/// `source → dest` branch flow and a right-aligned "Created" age, then a reviewers +
/// labels meta-row and the PR body in a card.
/// `aside` tells it the reviewers already have a card of their own beside the column,
/// so the meta-row drops its cluster rather than naming them twice.
fn review_meta(ui: &mut egui::Ui, palette: &Palette, review: &PrReviewView<'_>, aside: bool) {
    let pr = review.pr;
    // No author block over the body: author, branches and age are already the surface
    // header's second line, and repeating them under the tabs reads as a rendering slip
    // rather than as context (§11).
    meta_row(ui, palette, pr, aside);

    let body = review.detail.map(|d| d.body.trim()).unwrap_or("");
    if !body.is_empty() {
        ui.add_space(10.0);
        egui::Frame::new()
            .fill(palette.bg_surface)
            .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
            .corner_radius(egui::CornerRadius::same(CARD_RADIUS))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                markdown(ui, palette, body);
            });
    }
}

/// Reviewers cluster + labels under the author block (pull-requests.md §11). Hidden
/// when the PR carries neither.
fn meta_row(ui: &mut egui::Ui, palette: &Palette, pr: &PullRequest, rail: bool) {
    // With the rail up, both the reviewers and the labels are named there: the row
    // would only repeat it under the author block.
    let (reviewers, labels) = if rail {
        (&[][..], &[][..])
    } else {
        (&pr.reviewers[..], &pr.labels[..])
    };
    if reviewers.is_empty() && labels.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if !reviewers.is_empty() {
            ui.label(
                egui::RichText::new("Reviewers")
                    .size(META_SIZE)
                    .color(palette.text_muted),
            );
            let shown = reviewers.len().min(REVIEWER_MAX);
            let step = REVIEWER_AVATAR - REVIEWER_OVERLAP;
            let mut width = REVIEWER_AVATAR + step * shown.saturating_sub(1) as f32;
            if reviewers.len() > shown {
                width += 22.0;
            }
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, REVIEWER_AVATAR), egui::Sense::hover());
            reviewer_stack(ui, palette, reviewers, rect.x_range(), rect.center().y);
        }
        for label in labels {
            neutral_pill(ui, palette, label);
        }
    });
}

/// A small outlined neutral pill (design-system §4 pill grammar): labels and the
/// conversation role tags. Deliberately monochrome — the `accent.ai` hue stays
/// reserved for AI/agent surfaces (§1).
fn neutral_pill(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    let label = text.to_owned();
    let galley = ui.painter().layout_no_wrap(
        label.clone(),
        egui::FontId::proportional(11.0),
        palette.text_secondary,
    );
    let size = galley.size() + egui::vec2(14.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_PILL),
        palette.bg_surface,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone()));
}

/// Wall-clock Unix seconds for humanizing the PR's "Created" age; the model layer
/// stays pure (it takes `now` as an argument).
pub(crate) fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The Files toolbar's commit scope (per-commit diff: T5). Picking a commit narrows
/// the rail's files and the diff to `commit^..commit`; **All commits** restores the
/// cumulative three-dot range. It is the only place the PR's commits are listed.
fn commits_dropdown(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    let current = review
        .selected_commit
        .and_then(|sha| review.commits.iter().find(|c| c.sha == sha))
        .map(|c| c.subject.as_str())
        .unwrap_or("All commits");
    let response = ui.interact(rect, ui.id().with("pr_commit_scope"), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, RADIUS_BUTTON, palette.bg_surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().rect_stroke(
        rect,
        RADIUS_BUTTON,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let center_y = rect.center().y;
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 10.0 + CHIP_SIZE / 2.0, center_y),
        CHIP_SIZE,
        Icon::GitCommit,
        palette.text_muted,
    );
    let count = review.commits.len().to_string();
    let count_w = 10.0 + count.len() as f32 * 7.0 + STATUS_ICON;
    cell_text(
        ui,
        current,
        egui::FontId::proportional(META_SIZE),
        palette.text_secondary,
        rect.left() + 10.0 + CHIP_SIZE + 6.0,
        center_y,
        (rect.width() - 26.0 - CHIP_SIZE - count_w).max(0.0),
    );
    ui.painter().text(
        egui::pos2(rect.right() - 10.0 - STATUS_ICON, center_y),
        egui::Align2::RIGHT_CENTER,
        count,
        egui::FontId::proportional(META_SIZE),
        palette.text_muted,
    );
    paint_icon(
        ui.painter(),
        egui::pos2(rect.right() - 10.0 - STATUS_ICON / 2.0, center_y),
        STATUS_ICON,
        Icon::ChevronDown,
        palette.text_muted,
    );
    let label = current.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
    });

    egui::Popup::menu(&response)
        .style(crate::theme::menu_style)
        .show(|ui| {
            ui.set_min_width(240.0);
            if ui
                .radio(review.selected_commit.is_none(), "All commits")
                .clicked()
            {
                action.select_commit = Some(CommitSelection::All);
            }
            for commit in review.commits {
                let selected = review.selected_commit == Some(commit.sha.as_str());
                if ui
                    .radio(selected, format!("{}  {}", commit.short, commit.subject))
                    .clicked()
                {
                    action.select_commit = Some(CommitSelection::Commit(commit.sha.clone()));
                }
            }
        });
}

/// The two file-list filters the rail band owns: unread-only and hide-tests.
struct FileFilters<'a> {
    unread_count: usize,
    unread_only: &'a mut bool,
    test_count: usize,
    hide_tests: &'a mut bool,
}

/// "Files changed" band (commit-detail's `files_header`): the title + a count chip
/// and the list filters, with the ±totals and ratio bar pinned right. Filtering the
/// list sits next to the list it filters, not in the diff pane across the split.
fn files_band(
    ui: &mut egui::Ui,
    palette: &Palette,
    files: &[CommitFile],
    view: FileViewMode,
    filters: FileFilters<'_>,
) -> Option<FileViewMode> {
    let mut set_view = None;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Files changed")
                .size(SECTION_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(2.0);
        count_chip(ui, palette, files.len());
        ui.add_space(6.0);
        filter_chip(
            ui,
            palette,
            Icon::EyeOff,
            filters.unread_count,
            filters.unread_only,
            "Unread only",
        );
        ui.add_space(4.0);
        filter_chip(
            ui,
            palette,
            Icon::FlaskConical,
            filters.test_count,
            filters.hide_tests,
            "Hide tests",
        );
        // The ± totals sit in the Files toolbar, which always has the room for them —
        // a rail narrow enough to be useful never does, past the chips.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            set_view = file_list::view_toggle(ui, palette, view);
        });
    });
    set_view
}

/// A toggling icon + count chip filtering the rail's file list.
fn filter_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    count: usize,
    on: &mut bool,
    label: &'static str,
) {
    let count_font = egui::FontId::proportional(11.0);
    let count_text = count.to_string();
    let count_galley =
        ui.painter()
            .layout_no_wrap(count_text.clone(), count_font, egui::Color32::PLACEHOLDER);
    let icon_w = 12.0;
    let gap = 5.0;
    let size = egui::vec2(9.0 + icon_w + gap + count_galley.size().x + 9.0, 24.0);
    let enabled = count > 0 || *on;
    let (rect, response, hovered) = clickable(ui, size, enabled);
    let selected = *on;
    let fill = if selected {
        palette.accent_subtle
    } else if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    let stroke = if selected {
        egui::Stroke::new(1.0_f32, with_alpha(palette.accent, 150))
    } else {
        egui::Stroke::new(1.0_f32, palette.border_subtle)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_BUTTON),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let content = if selected || hovered {
        palette.accent
    } else if enabled {
        palette.text_secondary
    } else {
        palette.text_muted
    };
    let center_y = rect.center().y;
    let icon_center = egui::pos2(rect.left() + 9.0 + icon_w / 2.0, center_y);
    paint_icon(ui.painter(), icon_center, icon_w, icon, content);
    ui.painter().galley(
        egui::pos2(
            icon_center.x + icon_w / 2.0 + gap,
            center_y - count_galley.size().y / 2.0,
        ),
        count_galley,
        content,
    );
    response.clone().on_hover_text(label).widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, label)
    });
    if response.clicked() {
        *on = !*on;
    }
}

/// Section heading in the commit-detail language: bold primary text, preceded by
/// the standard top margin.
fn band_title(ui: &mut egui::Ui, palette: &Palette, title: &str) {
    ui.add_space(SECTION_TOP_MARGIN);
    ui.label(
        egui::RichText::new(title)
            .size(SECTION_TITLE_SIZE)
            .strong()
            .color(palette.text_primary),
    );
    ui.add_space(6.0);
}

/// The review composer, drawn inside the header's **Finish review** popover: the
/// verdict group, a summary field and the Submit button. Posts the accumulated draft
/// line comments together with the chosen verdict + summary.
fn review_composer(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        let (group, _) = ui.allocate_exact_size(
            egui::vec2(VERDICT_ICON_W * 3.0, DETAIL_ACTION_HEIGHT),
            egui::Sense::hover(),
        );
        verdict_icon_group(ui, palette, group, review.verdict);
        ui.add_space(GAP_SM);
        // Caption of the selected segment, painted rather than added: the segments
        // themselves carry the accessible labels, and a second node named after the
        // verdict would only make the tree ambiguous.
        let caption = match *review.verdict {
            ReviewVerdict::Comment => "Comment-only review",
            ReviewVerdict::RequestChanges => "Request changes",
            ReviewVerdict::Approve => "Approve",
        };
        let (caption_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), DETAIL_ACTION_HEIGHT),
            egui::Sense::hover(),
        );
        ui.painter().text(
            egui::pos2(caption_rect.left(), caption_rect.center().y),
            egui::Align2::LEFT_CENTER,
            caption,
            egui::FontId::proportional(META_SIZE),
            palette.text_secondary,
        );
    });
    ui.add_space(GAP_SM);

    ui.add(
        egui::TextEdit::multiline(&mut *review.summary)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("Summary (optional)"),
    );
    ui.add_space(6.0);

    if let Some(error) = review.post_error {
        ui.label(
            egui::RichText::new(error)
                .size(12.0)
                .color(palette.git_deleted),
        );
        ui.add_space(4.0);
    }

    let count = crate::review::count(review.draft);
    let empty_comment_review =
        *review.verdict == ReviewVerdict::Comment && count == 0 && review.summary.trim().is_empty();
    let label = submit_review_label(*review.verdict, count, review.posting, empty_comment_review);
    let enabled = !review.posting && !empty_comment_review;
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, 32.0), enabled);
    let fill = if !enabled {
        palette.state_disabled
    } else if hovered {
        palette.accent_hover
    } else {
        palette.accent
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &label,
        egui::FontId::proportional(13.0),
        palette.lane_node_text,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    if response.clicked() {
        action.submit_review = true;
    }
}

fn submit_review_label(
    verdict: ReviewVerdict,
    count: usize,
    posting: bool,
    empty_comment_review: bool,
) -> String {
    if posting {
        return "Submitting…".to_owned();
    }
    if empty_comment_review {
        return "Nothing to submit".to_owned();
    }
    match verdict {
        ReviewVerdict::Comment => match count {
            0 => "Submit comment".to_owned(),
            1 => "Submit 1 comment".to_owned(),
            n => format!("Submit {n} comments"),
        },
        ReviewVerdict::Approve => match count {
            0 => "Approve".to_owned(),
            1 => "Approve + 1 comment".to_owned(),
            n => format!("Approve + {n} comments"),
        },
        ReviewVerdict::RequestChanges => match count {
            0 => "Request changes".to_owned(),
            1 => "Request changes + 1 comment".to_owned(),
            n => format!("Request changes + {n} comments"),
        },
    }
}

/// The **Files** tab (pull-requests.md §11): every changed file diffed in **one
/// continuous column**, one band per file, in the rail's order. The rail is the
/// column's table of contents — picking a row scrolls to that band. A slim toolbar
/// above it carries the commit scope and the thread navigator.
fn review_diff(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let toolbar_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top(), rect.top() + DIFF_TOOLBAR_HEIGHT),
    );
    diff_toolbar(ui, palette, review, toolbar_rect, action);

    let body = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(toolbar_rect.bottom(), rect.bottom()),
    );
    let mut area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    let shown = column_files(&area, review);
    if shown.is_empty() {
        diff_placeholder(&mut area, palette, review);
        return;
    }
    diff_column(&mut area, palette, review, &shown, action);
}

/// The changed files the column draws, in rail order: the rail's own filters
/// (unread only / hide tests) gate both lists, so the table of contents and the
/// column can never disagree on what the review covers.
fn column_files(ui: &egui::Ui, review: &PrReviewView<'_>) -> Vec<usize> {
    let url = review.pr.url.as_str();
    let unread_only: bool = ui.data(|d| {
        d.get_temp(egui::Id::new(("pr_review_unread_only", url)))
            .unwrap_or(false)
    });
    let hide_tests: bool = ui.data(|d| {
        d.get_temp(egui::Id::new(("pr_review_hide_tests", url)))
            .unwrap_or(false)
    });
    let viewed: HashSet<String> = ui.data(|d| {
        d.get_temp(egui::Id::new(("pr_review_viewed_files", url)))
            .unwrap_or_default()
    });
    review
        .files
        .iter()
        .enumerate()
        .filter(|(_, file)| !(hide_tests && is_test_path(&file.path)))
        .filter(|(_, file)| !(unread_only && viewed.contains(&file.path)))
        .map(|(idx, _)| idx)
        .collect()
}

/// The column itself: one band per file inside a single vertical scroll.
///
/// Bands whose height is already known and that sit well outside the viewport are
/// **not drawn** — their space is reserved instead. A PR is dozens of files and
/// thousands of rows; laying every one of them out on every frame is what made a
/// continuous view unaffordable before. The measured height is kept per (file,
/// width) so a resize re-measures rather than reserving a stale height.
fn diff_column(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    shown: &[usize],
    action: &mut PullRequestsPageAction,
) {
    let url = review.pr.url.to_owned();
    let folded_id = egui::Id::new(("pr_review_folded", url.as_str()));
    let sizes_id = egui::Id::new(("pr_review_band_sizes", url.as_str()));
    let mut folded: HashSet<String> = ui.data(|d| d.get_temp(folded_id).unwrap_or_default());
    let mut sizes: HashMap<String, (f32, f32)> =
        ui.data(|d| d.get_temp(sizes_id).unwrap_or_default());
    let mut review_out: Vec<ReviewIntent> = Vec::new();

    egui::ScrollArea::vertical()
        .id_salt(("pr_review_column", url.as_str()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(PANEL_PAD_Y);
            let width = ui.available_width();
            // Culling margin: a screenful either side, so scrolling never lands on a
            // band that has not been laid out yet.
            let visible = ui
                .clip_rect()
                .expand2(egui::vec2(0.0, ui.clip_rect().height()));
            for (position, &idx) in shown.iter().enumerate() {
                let Some(file) = review.files.get(idx) else {
                    continue;
                };
                let path = file.path.clone();
                let top = ui.cursor().top();
                let known = sizes
                    .get(&path)
                    .filter(|(w, _)| (*w - width).abs() < 0.5)
                    .map(|(_, h)| *h);
                let offscreen =
                    known.is_some_and(|h| top > visible.bottom() || top + h < visible.top());
                let band = if offscreen {
                    let height = known.unwrap_or_default();
                    ui.allocate_space(egui::vec2(width, height));
                    egui::Rect::from_min_size(
                        egui::pos2(ui.min_rect().left(), top),
                        egui::vec2(width, height),
                    )
                } else {
                    let mut reveal = None;
                    match (
                        review.diffs.get(idx).copied().flatten(),
                        review.diff_errors.get(idx).copied().flatten(),
                    ) {
                        (Some(diff), _) => {
                            let state = review.file_views.entry(path.clone()).or_default();
                            let out = crate::ui::diff_view::diff_view_band(
                                ui,
                                palette,
                                diff,
                                state,
                                folded.contains(&path),
                                Some(&mut DiffReview {
                                    comments: review.agent_notes,
                                    forge: Some(review.draft),
                                    existing: review.existing,
                                    agent: review.agent,
                                    intents: &mut review_out,
                                }),
                            );
                            if out.toggled {
                                if !folded.remove(&path) {
                                    folded.insert(path.clone());
                                }
                                sizes.remove(&path);
                            }
                            reveal = out.reveal;
                        }
                        (None, error) => pending_band(ui, palette, file, error),
                    }
                    let height = ui.cursor().top() - top;
                    sizes.insert(path.clone(), (width, height));
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(ui.min_rect().left(), top),
                        egui::vec2(width, height),
                    );
                    // A line the conversation asked to reveal: the band's own scroll
                    // area owns the horizontal axis only, so the scroll is ours.
                    if let Some(row) = reveal {
                        ui.scroll_to_rect(row, Some(egui::Align::Center));
                        *review.scroll_to_file = None;
                    }
                    rect
                };
                if *review.scroll_to_file == Some(idx) {
                    ui.scroll_to_rect(band, Some(egui::Align::TOP));
                    *review.scroll_to_file = None;
                }
                // Each band is an outlined card of its own, so what separates two files
                // is air, not a rule: a hairline against a card's own outline reads as
                // a double border.
                if position + 1 < shown.len() {
                    ui.add_space(BAND_GAP);
                }
            }
            // A file the filters keep out of the column can never be scrolled to:
            // dropping the request beats holding it against a band that will not come.
            if review
                .scroll_to_file
                .is_some_and(|target| !shown.contains(&target))
            {
                *review.scroll_to_file = None;
            }
            ui.add_space(PANEL_PAD_Y);
        });

    action.review_intents.append(&mut review_out);
    ui.data_mut(|d| d.insert_temp(folded_id, folded));
    ui.data_mut(|d| d.insert_temp(sizes_id, sizes));
}

/// A band whose diff has not landed yet (or failed): the file's own header line
/// over the reason, so the column keeps its shape while the fetches trickle in.
fn pending_band(ui: &mut egui::Ui, palette: &Palette, file: &CommitFile, error: Option<&str>) {
    // The same flat header strip a loaded band wears, so a half-loaded column does not
    // read as two kinds of object; only the rows are missing.
    let strip = egui::Frame::new()
        .fill(palette.bg_surface_hover)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                paint_icon(
                    ui.painter(),
                    icon_rect.center(),
                    15.0,
                    Icon::FileText,
                    palette.text_secondary,
                );
                ui.label(
                    egui::RichText::new(&file.path)
                        .size(TITLE_SIZE)
                        .color(palette.text_primary),
                );
                ui.label(
                    egui::RichText::new(format!("+{}", file.additions))
                        .size(PILL_SIZE)
                        .monospace()
                        .color(palette.git_added),
                );
                ui.label(
                    egui::RichText::new(format!("−{}", file.deletions))
                        .size(PILL_SIZE)
                        .monospace()
                        .color(palette.git_deleted),
                );
                // A muted line, not a spinner: a column can hold dozens of pending
                // bands, and dozens of animations would repaint the whole app for
                // as long as the fetches trickle in.
                ui.label(muted(palette, error.unwrap_or("Loading diff…")));
            });
        });
    let rule = egui::Stroke::new(1.0_f32, palette.border_subtle);
    let x = strip.response.rect.x_range();
    ui.painter().hline(x, strip.response.rect.top(), rule);
    ui.painter().hline(x, strip.response.rect.bottom(), rule);
}

/// Centered message where the column would be: no changed files at all, or the
/// list itself still loading / failed.
fn diff_placeholder(ui: &mut egui::Ui, palette: &Palette, review: &PrReviewView<'_>) {
    let (message, spinner) = if review.files_loading && review.files.is_empty() {
        ("Loading changed files…", true)
    } else if let Some(error) = review.files_error {
        (error, false)
    } else if review.files.is_empty() {
        ("No file changes", false)
    } else {
        ("No file matches the filters", false)
    };
    ui.add_space(SECTION_TOP_MARGIN * 2.0);
    ui.vertical_centered(|ui| {
        if spinner {
            ui.add(Spinner::new().size(16.0).color(palette.text_muted));
            ui.add_space(GAP_SM);
        }
        ui.label(muted(palette, message));
    });
}

/// Toolbar above the diff (pull-requests.md §11): the PR's file and ± tally, then
/// the commit scope, with the unresolved-thread navigator pinned right. The list
/// *filters* stay in the rail, next to the list they filter.
fn diff_toolbar(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_surface);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    let center_y = rect.center().y;

    let additions: usize = review.files.iter().map(|f| f.additions).sum();
    let deletions: usize = review.files.iter().map(|f| f.deletions).sum();
    let mut x = rect.left() + PANEL_PAD_X;
    x = cell_text(
        ui,
        &format!("{} files", review.files.len()),
        egui::FontId::proportional(META_SIZE),
        palette.text_secondary,
        x,
        center_y,
        110.0,
    );
    x += GAP_SM;
    x = cell_text(
        ui,
        &format!("+{additions}"),
        egui::FontId::monospace(META_SIZE),
        palette.git_added,
        x,
        center_y,
        70.0,
    );
    x += GAP_SM;
    x = cell_text(
        ui,
        &format!("−{deletions}"),
        egui::FontId::monospace(META_SIZE),
        palette.git_deleted,
        x,
        center_y,
        70.0,
    );

    if !review.commits.is_empty() {
        let scope = egui::Rect::from_min_size(
            egui::pos2(x + GAP_MD, center_y - CTRL_HEIGHT / 2.0),
            egui::vec2(COMMIT_SCOPE_W, CTRL_HEIGHT),
        );
        commits_dropdown(ui, palette, review, scope, action);
    }

    // Thread navigator: steps the selection through the files carrying unresolved
    // threads. One thread needs no navigator — its file row already flags it.
    let threads: Vec<&String> = review.existing.keys().collect();
    if threads.len() < 2 {
        return;
    }
    let nav_id = egui::Id::new(("pr_thread_nav", review.pr.url.as_str()));
    let mut cursor: usize = ui.data(|d| d.get_temp(nav_id).unwrap_or(0));
    cursor = cursor.min(threads.len() - 1);
    let nav = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - PANEL_PAD_X - THREAD_NAV_W,
            center_y - CTRL_HEIGHT / 2.0,
        ),
        egui::vec2(THREAD_NAV_W, CTRL_HEIGHT),
    );
    ui.painter().rect_stroke(
        nav,
        RADIUS_BUTTON,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        egui::pos2(nav.left() + 10.0 + CHIP_SIZE / 2.0, center_y),
        CHIP_SIZE,
        Icon::MessageSquare,
        palette.git_modified,
    );
    let label = format!("Thread {} / {}", cursor + 1, threads.len());
    cell_text(
        ui,
        &label,
        egui::FontId::proportional(META_SIZE),
        palette.text_secondary,
        nav.left() + 10.0 + CHIP_SIZE + 6.0,
        center_y,
        THREAD_NAV_W - 70.0,
    );
    let step = |ui: &mut egui::Ui, right: f32, icon, id, delta: isize| -> bool {
        let r = egui::Rect::from_center_size(
            egui::pos2(right, center_y),
            egui::vec2(20.0, CTRL_HEIGHT),
        );
        let hit = ui.interact(r, ui.id().with(id), egui::Sense::click());
        if hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        paint_icon(
            ui.painter(),
            r.center(),
            STATUS_ICON,
            icon,
            palette.text_secondary,
        );
        let name = if delta < 0 {
            "Previous thread"
        } else {
            "Next thread"
        };
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        hit.clicked()
    };
    let n = threads.len();
    let mut jumped = false;
    if step(
        ui,
        nav.right() - 34.0,
        Icon::ChevronUp,
        "pr_thread_prev",
        -1,
    ) {
        cursor = (cursor + n - 1) % n;
        jumped = true;
    }
    if step(
        ui,
        nav.right() - 14.0,
        Icon::ChevronDown,
        "pr_thread_next",
        1,
    ) {
        cursor = (cursor + 1) % n;
        jumped = true;
    }
    if jumped {
        if let Some(idx) = review
            .files
            .iter()
            .position(|file| &file.path == threads[cursor])
        {
            action.select_file = Some(idx);
        }
    }
    ui.data_mut(|d| d.insert_temp(nav_id, cursor));
}

/// Whether a changed path is test scaffolding — what **Hide tests** filters out.
fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    let name = leaf_name(&lower);
    lower.starts_with("test/")
        || lower.starts_with("tests/")
        || lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("__tests__/")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_spec.rb")
}

fn rail_resize_handle(
    ui: &mut egui::Ui,
    palette: &Palette,
    x: f32,
    rect: egui::Rect,
    rail_width: f32,
    action: &mut PullRequestsPageAction,
) {
    let handle = egui::Rect::from_x_y_ranges(egui::Rangef::new(x - 3.0, x + 3.0), rect.y_range());
    let resp = ui.interact(handle, ui.id().with("pr_rail_resize"), egui::Sense::drag());
    let active = resp.hovered() || resp.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    ui.painter().vline(
        x,
        if active {
            egui::Rangef::new(rect.top() + 10.0, rect.bottom() - 10.0)
        } else {
            rect.y_range()
        },
        egui::Stroke::new(
            if active { 2.0_f32 } else { 1.0_f32 },
            if active {
                palette.accent
            } else {
                palette.border_subtle
            },
        ),
    );
    if resp.dragged() {
        // The rail sits on the left, so dragging the split right (positive delta)
        // widens it.
        let max = (rect.width() - DIFF_MIN_WIDTH).max(RAIL_MIN_WIDTH);
        action.set_detail_width =
            Some((rail_width + resp.drag_delta().x).clamp(RAIL_MIN_WIDTH, max));
    }
}

fn render_list(
    ui: &mut egui::Ui,
    palette: &Palette,
    prs: &[PullRequest],
    selected: Option<usize>,
    hints: &PrSourceHints<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    let state_id = egui::Id::new("pr_list_state");
    let mut state: ListState = ui.data(|d| d.get_temp(state_id).unwrap_or_default());

    let header_h = PAGE_HEADER_HEIGHT + LIST_TABS_HEIGHT;
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_h));
    // The header names how many rows survive the filters, so it needs the count before
    // it draws — and the controls it draws may change the filters again, which is what
    // the second pass below picks up.
    let shown = visible_indices(prs, &state).len();
    list_header(
        ui,
        palette,
        header_rect,
        prs,
        hints,
        shown,
        &mut state,
        action,
    );
    let visible = visible_indices(prs, &state);

    let body_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top() + header_h, rect.bottom()),
    );
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("pr_list")
        .show(&mut content, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(0, 8))
                .show(ui, |ui| {
                    if let Some(text) = hints.github {
                        hint_banner(ui, palette, "GitHub", text);
                    }
                    if let Some(text) = hints.bitbucket {
                        hint_banner(ui, palette, "Bitbucket", text);
                    }
                    if prs.is_empty() {
                        if hints.loading {
                            loading_state(ui, palette);
                        } else {
                            empty_state(ui, palette, hints);
                        }
                        return;
                    }
                    if visible.is_empty() {
                        filtered_out_state(ui, palette);
                        return;
                    }
                    for group in ActionGroup::ALL {
                        let mut indices: Vec<usize> = visible
                            .iter()
                            .copied()
                            .filter(|&i| ActionGroup::of(&prs[i]) == group)
                            .collect();
                        state.sort.apply(prs, &mut indices);
                        band(
                            ui, palette, group, &indices, prs, selected, &mut state, action,
                        );
                    }
                    list_footer(ui, palette, visible.len());
                });
        });

    ui.data_mut(|d| d.insert_temp(state_id, state));
}

/// Which PRs the current tab, project filter and query leave standing, as indices into
/// `prs`. Tab first — it drives the counts the rest of the header reports.
fn visible_indices(prs: &[PullRequest], state: &ListState) -> Vec<usize> {
    prs.iter()
        .enumerate()
        .filter(|(_, pr)| state.tab.accepts(pr))
        .filter(|(_, pr)| !state.hidden_projects.contains(&pr.repo_label))
        .filter(|(_, pr)| crate::pull_requests::model::matches_search(pr, &state.query))
        .map(|(i, _)| i)
        .collect()
}

/// The list runs out rather than trailing off — a quiet line saying so, and how much
/// the filters let through (pull-requests.md §5).
fn list_footer(ui: &mut egui::Ui, palette: &Palette, shown: usize) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::hover());
    let label = format!("End of list · {}", plural(shown, "pull request"));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &label,
        egui::FontId::proportional(LIST_META_SIZE),
        palette.text_muted,
    );
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

/// The reading column the list centers itself in: full width up to `LIST_MAX_WIDTH`,
/// then centered with at least the panel's own gutter on each side.
fn list_column(rect: egui::Rect) -> egui::Rect {
    let width = (rect.width() - 2.0 * PANEL_PAD_X).min(LIST_MAX_WIDTH);
    let left = rect.left() + ((rect.width() - width) / 2.0).max(PANEL_PAD_X);
    egui::Rect::from_min_size(
        egui::pos2(left, rect.top()),
        egui::vec2(width, rect.height()),
    )
}

/// Ordering **within** an actionability band (pull-requests.md §5) — the bands
/// themselves are always in `ActionGroup::ALL` order, which is the real priority.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ListSort {
    /// Oldest touched first: a PR that has been sitting is the more urgent one.
    #[default]
    Priority,
    /// Most recently touched first.
    Recent,
}

impl ListSort {
    fn label(self) -> &'static str {
        match self {
            ListSort::Priority => "Priority",
            ListSort::Recent => "Recently updated",
        }
    }

    fn apply(self, prs: &[PullRequest], indices: &mut [usize]) {
        indices.sort_by(|&a, &b| {
            let ord = prs[a].updated_at.cmp(&prs[b].updated_at);
            match self {
                ListSort::Priority => ord,
                ListSort::Recent => ord.reverse(),
            }
        });
    }
}

/// Session state of the browse list (pull-requests.md §5): the open tab, the search
/// query, the projects hidden through the **Filters** menu and the in-band ordering.
/// Not persisted — a cockpit reopened later starts on the default view.
#[derive(Clone, Default)]
struct ListState {
    tab: crate::pull_requests::model::ListTab,
    query: String,
    hidden_projects: std::collections::BTreeSet<String>,
    sort: ListSort,
    /// Stacks folded to their header, keyed by `repo · base branch of the stack`
    /// — an index would move under the next refresh.
    collapsed_stacks: std::collections::BTreeSet<String>,
}

/// The list header (pull-requests.md §5): a raised band carrying the title, the
/// search field, the **Filters** / **Priority** / **Refresh** controls, and the
/// tab bar beneath them.
#[allow(clippy::too_many_arguments)]
fn list_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    prs: &[PullRequest],
    hints: &PrSourceHints<'_>,
    shown: usize,
    state: &mut ListState,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_surface);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );

    let top = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PAGE_HEADER_HEIGHT));
    let center_y = top.center().y;
    let title = ui.painter().layout_no_wrap(
        "Pull Requests".to_owned(),
        egui::FontId::new(PAGE_TITLE_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_primary,
    );
    let mut left = top.left() + PANEL_PAD_X;
    let title_w = title.size().x;
    ui.painter().galley(
        egui::pos2(left, center_y - title.size().y / 2.0),
        title,
        palette.text_primary,
    );
    left += title_w + GAP_MD;

    // Controls are laid out from the right edge; what they leave is what the title's
    // tally and then the search field get — in that order, since a narrow window has
    // to drop the field before it drops the count.
    let mut x = top.right() - PANEL_PAD_X;
    let control = |x: &mut f32, w: f32| {
        *x -= w;
        let r = egui::Rect::from_min_size(
            egui::pos2(*x, center_y - CTRL_HEIGHT / 2.0),
            egui::vec2(w, CTRL_HEIGHT),
        );
        *x -= GAP_SM;
        r
    };
    // "Refreshed 2 min ago" only makes sense once a fetch has landed.
    let age = hints.refreshed_at.map(|at| {
        format!(
            "· {}",
            crate::pull_requests::model::age_label(now_epoch_secs() - at)
        )
    });
    let age_w = age.as_deref().map_or(0.0, |text| {
        GAP_XS
            + ui.painter()
                .layout_no_wrap(
                    text.to_owned(),
                    egui::FontId::proportional(COUNT_BADGE_SIZE),
                    palette.text_muted,
                )
                .size()
                .x
    });
    let refresh_rect = control(&mut x, REFRESH_BTN_W + age_w);
    let divider_x = x + GAP_SM / 2.0;
    x -= GAP_SM;
    let sort_rect = control(&mut x, SORT_BTN_W);
    let filter_rect = control(&mut x, FILTER_BTN_W);

    // Refresh is the page's own housekeeping, not a view control — it drops the outline
    // and sits past a divider so the two groups don't read as one row of buttons.
    ui.painter().vline(
        divider_x,
        egui::Rangef::new(center_y - 10.0, center_y + 10.0),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    if refresh_button(ui, palette, refresh_rect, hints.loading, age.as_deref()) {
        action.refresh = true;
    }
    sort_menu(ui, palette, sort_rect, state);
    filter_menu(ui, palette, filter_rect, prs, state);

    // What the page is holding, next to its name: the whole cache at rest, and what the
    // filters left when they are narrowing it.
    let tally = if shown == prs.len() {
        let drafts = prs.iter().filter(|pr| pr.state == PrState::Draft).count();
        match drafts {
            0 => plural(prs.len(), "open pull request"),
            n => format!("{} open · {}", prs.len(), plural(n, "draft")),
        }
    } else {
        format!("{shown} of {} shown", prs.len())
    };
    let controls_left = x + GAP_SM;
    left = cell_text(
        ui,
        &tally,
        egui::FontId::proportional(CHIP_SIZE),
        palette.text_muted,
        left,
        center_y,
        (controls_left - GAP_MD - left).clamp(0.0, 220.0),
    );

    let search_left = left + GAP_MD;
    let search_right = (x + GAP_SM).min(search_left + SEARCH_MAX_W);
    if search_right - search_left > 80.0 {
        search_field(
            ui,
            palette,
            egui::Rect::from_x_y_ranges(
                egui::Rangef::new(search_left, search_right),
                egui::Rangef::new(center_y - CTRL_HEIGHT / 2.0, center_y + CTRL_HEIGHT / 2.0),
            ),
            state,
        );
    }

    let tabs = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top() + PAGE_HEADER_HEIGHT, rect.bottom()),
    );
    tab_bar(ui, palette, tabs, prs, state);
}

/// What sits in a header control's leading slot. `Spinner` reserves the same slot
/// as a glyph, so a button doesn't shift when a fetch starts.
enum HeaderIcon {
    Glyph(Icon),
    Spinner,
}

/// A header control: leading slot, label, then whichever of a count pill, a quiet
/// trailing note and a chevron it asked for. Paints itself into `rect` and reports the
/// click.
struct HeaderCtrl<'a> {
    id: &'a str,
    icon: HeaderIcon,
    label: &'a str,
    /// A count pill after the label — how many filters are on.
    badge: Option<usize>,
    /// A quiet run after the label — how long ago the list was refreshed.
    note: Option<&'a str>,
    chevron: bool,
    /// The view controls are outlined so they read as a set; Refresh is not one of
    /// them and stands bare.
    outlined: bool,
}

fn header_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    ctrl: HeaderCtrl<'_>,
) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(ctrl.id), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, RADIUS_BUTTON, palette.bg_surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if ctrl.outlined {
        ui.painter().rect_stroke(
            rect,
            RADIUS_BUTTON,
            egui::Stroke::new(1.0_f32, palette.border_subtle),
            egui::StrokeKind::Inside,
        );
    }
    let center_y = rect.center().y;
    let mut x = rect.left() + 10.0;
    let icon_center = egui::pos2(x + STATUS_ICON / 2.0, center_y);
    match ctrl.icon {
        HeaderIcon::Glyph(icon) => {
            paint_icon(
                ui.painter(),
                icon_center,
                STATUS_ICON,
                icon,
                palette.text_secondary,
            );
        }
        HeaderIcon::Spinner => {
            Spinner::new()
                .size(STATUS_ICON)
                .color(palette.text_secondary)
                .paint_at(
                    ui,
                    egui::Rect::from_center_size(icon_center, egui::vec2(STATUS_ICON, STATUS_ICON)),
                );
        }
    }
    x += STATUS_ICON + 6.0;
    x = cell_text(
        ui,
        ctrl.label,
        egui::FontId::new(META_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_secondary,
        x,
        center_y,
        rect.right() - x,
    );
    if let Some(count) = ctrl.badge {
        x = count_pill(ui, palette, x + GAP_XS + 2.0, center_y, count, true);
    }
    if let Some(note) = ctrl.note {
        cell_text(
            ui,
            note,
            egui::FontId::proportional(COUNT_BADGE_SIZE),
            palette.text_muted,
            x + GAP_XS,
            center_y,
            rect.right() - x,
        );
    }
    if ctrl.chevron {
        paint_icon(
            ui.painter(),
            egui::pos2(rect.right() - 12.0, center_y),
            STATUS_ICON,
            Icon::ChevronDown,
            palette.text_muted,
        );
    }
    let label = ctrl.label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
    });
    response
}

/// **Refresh** (pull-requests.md §6) — the icon becomes a spinner while a fetch is in
/// flight, and `note` says how long ago the last one landed.
fn refresh_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    loading: bool,
    note: Option<&str>,
) -> bool {
    let icon = if loading {
        HeaderIcon::Spinner
    } else {
        HeaderIcon::Glyph(Icon::RefreshCw)
    };
    header_button(
        ui,
        palette,
        rect,
        HeaderCtrl {
            id: "pr_refresh",
            icon,
            label: "Refresh",
            badge: None,
            note,
            chevron: false,
            outlined: false,
        },
    )
    .clicked()
}

/// A small count pill — the tab bar's per-tab tally, a band's row count, a stack's
/// size. `accent` tints it as a live figure; otherwise it reads neutral. Returns the x
/// it ended at.
fn count_pill(
    ui: &egui::Ui,
    palette: &Palette,
    left: f32,
    center_y: f32,
    count: usize,
    accent: bool,
) -> f32 {
    let (ink, fill) = if accent {
        (palette.accent, with_alpha(palette.accent, 38))
    } else {
        (palette.text_muted, with_alpha(palette.text_muted, 28))
    };
    let galley = ui.painter().layout_no_wrap(
        count.to_string(),
        egui::FontId::new(
            COUNT_BADGE_SIZE - 1.0,
            crate::theme::medium_family(ui.ctx()),
        ),
        ink,
    );
    let h = COUNT_BADGE_SIZE + 5.0;
    let w = (galley.size().x + 11.0).max(h);
    let pill = egui::Rect::from_min_size(egui::pos2(left, center_y - h / 2.0), egui::vec2(w, h));
    ui.painter().rect_filled(pill, h / 2.0, fill);
    ui.painter()
        .galley(pill.center() - galley.size() / 2.0, galley, ink);
    pill.right()
}

/// **Priority** (pull-requests.md §5): picks the ordering applied *inside* each
/// actionability band.
fn sort_menu(ui: &mut egui::Ui, palette: &Palette, rect: egui::Rect, state: &mut ListState) {
    let response = header_button(
        ui,
        palette,
        rect,
        HeaderCtrl {
            id: "pr_sort",
            icon: HeaderIcon::Glyph(Icon::ArrowUpDown),
            label: state.sort.label(),
            badge: None,
            note: None,
            chevron: true,
            outlined: true,
        },
    );
    egui::Popup::menu(&response)
        .style(crate::theme::menu_style)
        .show(|ui| {
            for option in [ListSort::Priority, ListSort::Recent] {
                if ui.radio(state.sort == option, option.label()).clicked() {
                    state.sort = option;
                }
            }
        });
}

/// **Filters** (pull-requests.md §5): one checkbox per workspace project in the
/// list, so a noisy repo can be muted without leaving the cockpit.
fn filter_menu(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    prs: &[PullRequest],
    state: &mut ListState,
) {
    let hidden = state.hidden_projects.len();
    let response = header_button(
        ui,
        palette,
        rect,
        HeaderCtrl {
            id: "pr_filters",
            icon: HeaderIcon::Glyph(Icon::ListFilter),
            label: "Filters",
            badge: (hidden > 0).then_some(hidden),
            note: None,
            chevron: true,
            outlined: true,
        },
    );
    let mut projects: Vec<&str> = prs.iter().map(|p| p.repo_label.as_str()).collect();
    projects.sort_unstable();
    projects.dedup();
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .style(crate::theme::menu_style)
        .show(|ui| {
            ui.set_min_width(200.0);
            ui.label(muted(palette, "Projects"));
            for project in projects {
                let mut shown = !state.hidden_projects.contains(project);
                if ui.checkbox(&mut shown, project).changed() {
                    if shown {
                        state.hidden_projects.remove(project);
                    } else {
                        state.hidden_projects.insert(project.to_owned());
                    }
                }
            }
        });
}

/// The header's search field (pull-requests.md §5) — matches title, number, author,
/// branch, project through `model::matches_search`.
fn search_field(ui: &mut egui::Ui, palette: &Palette, rect: egui::Rect, state: &mut ListState) {
    ui.painter()
        .rect_filled(rect, RADIUS_BUTTON, palette.bg_canvas);
    ui.painter().rect_stroke(
        rect,
        RADIUS_BUTTON,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 10.0 + STATUS_ICON / 2.0, rect.center().y),
        STATUS_ICON,
        Icon::Search,
        palette.text_muted,
    );
    // The clear affordance only appears once there is something to clear, so the field
    // reads as a plain input at rest.
    let mut field_right = rect.right() - 8.0;
    if !state.query.is_empty() {
        let hit = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 8.0 - STATUS_ICON / 2.0, rect.center().y),
            egui::Vec2::splat(CTRL_HEIGHT - 6.0),
        );
        let clear = ui.interact(hit, ui.id().with("pr_search_clear"), egui::Sense::click());
        let ink = if clear.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            palette.text_primary
        } else {
            palette.text_muted
        };
        paint_icon(ui.painter(), hit.center(), STATUS_ICON, Icon::X, ink);
        clear.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear search")
        });
        if clear.clicked() {
            state.query.clear();
        }
        field_right = hit.left() - 4.0;
    }
    let field = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(rect.left() + 10.0 + STATUS_ICON + 6.0, field_right),
        rect.y_range(),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.visuals_mut().extreme_bg_color = egui::Color32::TRANSPARENT;
    child.add(
        egui::TextEdit::singleline(&mut state.query)
            .id_salt("pr_search")
            .frame(egui::Frame::NONE)
            .desired_width(field.width())
            .font(egui::FontId::proportional(META_SIZE))
            .text_color(palette.text_primary)
            .hint_text(
                egui::RichText::new("Search a PR, an author, a branch…")
                    .size(META_SIZE)
                    .color(palette.text_muted),
            ),
    );
}

/// The list's tab bar (pull-requests.md §5): four views over the same cache, each
/// carrying its count.
fn tab_bar(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    prs: &[PullRequest],
    state: &mut ListState,
) {
    let mut x = rect.left() + PANEL_PAD_X;
    for tab in crate::pull_requests::model::ListTab::ALL {
        let count = prs.iter().filter(|pr| tab.accepts(pr)).count();
        let label = tab.label();
        let active = state.tab == tab;
        let font = egui::FontId::new(
            SECTION_TITLE_SIZE,
            if active {
                crate::theme::medium_family(ui.ctx())
            } else {
                egui::FontFamily::Proportional
            },
        );
        let galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            font,
            if active {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
        let label_w = galley.size().x;
        let w = label_w + 34.0 + 2.0 * TAB_PAD_X;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, rect.height()));
        let response = ui.interact(
            tab_rect,
            ui.id().with(("pr_tab", label)),
            egui::Sense::click(),
        );
        if response.hovered() && !active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let center_y = tab_rect.center().y;
        let text_left = tab_rect.left() + TAB_PAD_X;
        ui.painter().galley(
            egui::pos2(text_left, center_y - galley.size().y / 2.0),
            galley,
            palette.text_primary,
        );
        // The count rides in a pill, tinted on the open tab — a tab that reads "0" is
        // as much of an answer as one that reads "14".
        count_pill(
            ui,
            palette,
            text_left + label_w + GAP_SM,
            center_y,
            count,
            active,
        );
        if active {
            ui.painter().hline(
                egui::Rangef::new(tab_rect.left() + 4.0, tab_rect.right() - 4.0),
                tab_rect.bottom() - 1.5,
                egui::Stroke::new(2.0_f32, palette.accent),
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label.to_owned())
        });
        if response.clicked() {
            state.tab = tab;
        }
        x += w;
    }
}

/// Shown when the tab / search / project filters leave nothing — distinct from an
/// empty workspace, which `empty_state` covers.
fn filtered_out_state(ui: &mut egui::Ui, palette: &Palette) {
    blank_slate(
        ui,
        palette,
        Icon::FilterX,
        "No pull request matches these filters",
        Some("Clear the search or switch tab to see the rest."),
    );
}

/// The list's centered nothing-here state: a muted glyph, the headline, and an
/// optional line saying what to do about it.
fn blank_slate(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    headline: &str,
    hint: Option<&str>,
) {
    ui.add_space(56.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(28.0), egui::Sense::hover());
        paint_icon(ui.painter(), rect.center(), 28.0, icon, palette.text_muted);
        ui.add_space(GAP_SM);
        ui.label(
            egui::RichText::new(headline)
                .size(13.0)
                .color(palette.text_secondary),
        );
        if let Some(hint) = hint {
            ui.add_space(GAP_XS);
            ui.label(
                egui::RichText::new(hint)
                    .size(CHIP_SIZE)
                    .color(palette.text_muted),
            );
        }
    });
}

/// A full-width unavailability banner — a forge tag plus its one-line hint (§3).
fn hint_banner(ui: &mut egui::Ui, palette: &Palette, forge: &str, text: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
    let card = egui::Rect::from_min_max(
        egui::pos2(rect.left() + PAD_X, rect.top() + 4.0),
        egui::pos2(rect.right() - PAD_X, rect.bottom() - 4.0),
    );
    ui.painter().rect_filled(card, 8.0, palette.bg_surface);
    let cy = card.center().y;
    paint_icon(
        ui.painter(),
        egui::pos2(card.left() + 14.0, cy),
        STATUS_ICON,
        Icon::AlertTriangle,
        palette.text_muted,
    );
    ui.painter().text(
        egui::pos2(card.left() + 30.0, cy),
        egui::Align2::LEFT_CENTER,
        format!("{forge}: {text}"),
        egui::FontId::new(META_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_secondary,
    );
}

fn empty_state(ui: &mut egui::Ui, palette: &Palette, hints: &PrSourceHints<'_>) {
    if hints.no_repos {
        blank_slate(
            ui,
            palette,
            Icon::FolderGit2,
            "No GitHub or Bitbucket repository in your workspace",
            Some("Add one from the project sidebar to see its pull requests."),
        );
    } else {
        blank_slate(
            ui,
            palette,
            Icon::GitPullRequest,
            "No pull requests",
            Some("Nothing is waiting on you right now."),
        );
    }
}

/// Centered loader for the cold list fetch (pull-requests.md §6): a spinner over a
/// muted label, shown in place of `empty_state` until the first reply lands — the
/// empty cache is otherwise indistinguishable from "no pull requests".
fn loading_state(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        ui.add(Spinner::new().size(22.0).color(palette.text_muted));
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("Loading pull requests…")
                .size(13.0)
                .color(palette.text_muted),
        );
    });
}

/// One actionability band of the list (pull-requests.md §5): a colored section header
/// over the band's blocks. Stacked PRs still stay inside their band, so merging a base
/// stays visibly ahead of what it carries.
#[allow(clippy::too_many_arguments)]
fn band(
    ui: &mut egui::Ui,
    palette: &Palette,
    group: ActionGroup,
    indices: &[usize],
    prs: &[PullRequest],
    selected: Option<usize>,
    state: &mut ListState,
    action: &mut PullRequestsPageAction,
) {
    if indices.is_empty() {
        return;
    }
    section_header(ui, palette, group, indices.len());
    for block in crate::pull_requests::model::list_blocks(prs, indices) {
        list_block(ui, palette, prs, group, &block, selected, state, action);
        ui.add_space(LIST_BLOCK_GAP);
    }
    ui.add_space(GAP_SM);
}

/// One bordered block of a band: a stack under its own header, or the run of PRs that
/// stand alone. Every part of it is a fixed height, so the card is allocated whole and
/// its rows are carved out of it — which is also what lets a row round the corner it
/// sits in.
#[allow(clippy::too_many_arguments)]
fn list_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    prs: &[PullRequest],
    group: ActionGroup,
    block: &crate::pull_requests::model::ListBlock,
    selected: Option<usize>,
    state: &mut ListState,
    action: &mut PullRequestsPageAction,
) {
    use crate::pull_requests::model::ListBlock;

    let stack = match block {
        ListBlock::Stack(stack) => Some(stack),
        ListBlock::Singles(_) => None,
    };
    let key = stack.map(|s| format!("{} · {}", s.repo_label, s.base));
    let collapsed = key
        .as_ref()
        .is_some_and(|key| state.collapsed_stacks.contains(key));
    let rows: Vec<(usize, Option<StackAt<'_>>)> = match block {
        ListBlock::Stack(s) => s
            .rows
            .iter()
            .map(|row| {
                (
                    row.idx,
                    Some(StackAt {
                        row,
                        len: s.rows.len(),
                    }),
                )
            })
            .collect(),
        ListBlock::Singles(idx) => idx.iter().map(|&i| (i, None)).collect(),
    };

    let header_h = if stack.is_some() {
        STACK_HEADER_HEIGHT
    } else {
        0.0
    };
    let shown = if collapsed { 0 } else { rows.len() };
    let height = header_h + ROW_HEIGHT * shown as f32;
    let (strip, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let card = list_column(strip);
    ui.painter()
        .rect_filled(card, LIST_BLOCK_RADIUS, palette.bg_surface);

    let mut y = card.top();
    if let (Some(stack), Some(key)) = (stack, key) {
        let head = egui::Rect::from_min_size(
            egui::pos2(card.left(), y),
            egui::vec2(card.width(), header_h),
        );
        if stack_header(ui, palette, head, stack, collapsed) && !state.collapsed_stacks.remove(&key)
        {
            state.collapsed_stacks.insert(key);
        }
        y += header_h;
    }
    for (n, (idx, row)) in rows.iter().take(shown).enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(card.left(), y),
            egui::vec2(card.width(), ROW_HEIGHT),
        );
        pr_row(
            ui,
            palette,
            prs,
            *idx,
            group,
            selected == Some(*idx),
            *row,
            rect,
            RowCorners {
                top: n == 0 && header_h == 0.0,
                bottom: n + 1 == shown,
            },
            action,
        );
        y += ROW_HEIGHT;
    }
    // The border last, over the row fills, so a hovered row never bleeds through it.
    ui.painter().rect_stroke(
        card,
        LIST_BLOCK_RADIUS,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
}

/// Which of a block's corners a row has to round — the first and last row carry the
/// card's own radius, everything between is square.
#[derive(Clone, Copy)]
struct RowCorners {
    top: bool,
    bottom: bool,
}

impl RowCorners {
    fn radius(self) -> egui::CornerRadius {
        let r = LIST_BLOCK_RADIUS;
        egui::CornerRadius {
            nw: if self.top { r } else { 0 },
            ne: if self.top { r } else { 0 },
            sw: if self.bottom { r } else { 0 },
            se: if self.bottom { r } else { 0 },
        }
    }
}

/// A stack's header (pull-requests.md §5): what it is, how big, where it lands, and
/// the one instruction that matters — merge it bottom-up. Returns whether the fold
/// chevron was hit.
fn stack_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    stack: &crate::pull_requests::model::PrStack,
    collapsed: bool,
) -> bool {
    let response = ui.interact(
        rect,
        ui.id().with(("pr_stack", &stack.repo_label, &stack.base)),
        egui::Sense::click(),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter()
        .rect_filled(rect, 0, with_alpha(palette.accent, 20));
    let center_y = rect.center().y;
    let mut x = rect.left() + PAD_X;
    paint_icon(
        ui.painter(),
        egui::pos2(x + CHIP_SIZE / 2.0, center_y),
        STATUS_ICON,
        Icon::ListTree,
        palette.accent,
    );
    x += STATUS_ICON + GAP_SM;
    x = cell_text(
        ui,
        "STACK",
        egui::FontId::new(HEADER_SIZE + 0.5, crate::theme::medium_family(ui.ctx())),
        palette.accent,
        x,
        center_y,
        rect.width(),
    );
    x = count_pill(ui, palette, x + GAP_SM, center_y, stack.rows.len(), true) + GAP_MD;
    x = cell_text(
        ui,
        &stack.repo_label,
        egui::FontId::monospace(LIST_MONO_SIZE),
        palette.text_muted,
        x,
        center_y,
        220.0,
    ) + GAP_SM;

    let chevron_x = rect.right() - PAD_X - STATUS_ICON / 2.0;
    // The instruction earns its place only on a stack you can actually walk up.
    let hint = if stack.rows.len() > 1 {
        "Merge bottom-up — start at #1"
    } else {
        ""
    };
    let hint_w = ui
        .painter()
        .layout_no_wrap(
            hint.to_owned(),
            egui::FontId::proportional(LIST_META_SIZE),
            palette.text_secondary,
        )
        .size()
        .x;
    let hint_left = chevron_x - STATUS_ICON / 2.0 - GAP_MD - hint_w;
    branch_pill(ui, palette, x, center_y, None, &stack.base, hint_left - x);
    if !hint.is_empty() {
        cell_text(
            ui,
            hint,
            egui::FontId::proportional(LIST_META_SIZE),
            palette.text_secondary,
            hint_left,
            center_y,
            hint_w,
        );
    }
    paint_icon(
        ui.painter(),
        egui::pos2(chevron_x, center_y),
        STATUS_ICON,
        if collapsed {
            Icon::ChevronDown
        } else {
            Icon::ChevronUp
        },
        palette.text_muted,
    );

    let label = format!("Stack · {}", plural(stack.rows.len(), "PR"));
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, !collapsed, label.clone())
    });
    response.clicked()
}

/// Icon + color a band wears in its section header.
fn band_style(palette: &Palette, band: ActionGroup) -> (Icon, egui::Color32) {
    match band {
        ActionGroup::WaitingOnMyReview => (Icon::ClipboardCheck, palette.git_modified),
        ActionGroup::ReadyToMerge => (Icon::CheckCircle2, palette.git_added),
        ActionGroup::WaitingOnAuthor => (Icon::Hourglass, palette.text_muted),
        ActionGroup::InReview => (Icon::MessageSquare, palette.text_muted),
    }
}

/// A band's header: its glyph and uppercase label in the band color, its count, then a
/// rule running out to the edge of the column so the band reads as one span.
fn section_header(ui: &mut egui::Ui, palette: &Palette, band: ActionGroup, count: usize) {
    let (strip, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), GROUP_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    let rect = list_column(strip);
    let (icon, color) = band_style(palette, band);
    let center_y = rect.center().y + 3.0;
    let left = rect.left() + 2.0;
    paint_icon(
        ui.painter(),
        egui::pos2(left + STATUS_ICON / 2.0, center_y),
        STATUS_ICON,
        icon,
        color,
    );
    let label = band.label().to_uppercase();
    let galley = ui.painter().layout_no_wrap(
        label.clone(),
        egui::FontId::new(HEADER_SIZE + 1.0, crate::theme::medium_family(ui.ctx())),
        color,
    );
    let text_left = left + STATUS_ICON + GAP_SM;
    let text_w = galley.size().x;
    ui.painter().galley(
        egui::pos2(text_left, center_y - galley.size().y / 2.0),
        galley,
        color,
    );
    let rule_left = count_pill(
        ui,
        palette,
        text_left + text_w + GAP_SM,
        center_y,
        count,
        false,
    ) + GAP_MD;
    ui.painter().hline(
        egui::Rangef::new(rule_left, rect.right()),
        center_y,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone())
    });
}

/// Where a row's clusters sit. The author's avatar leads on the left, beside the state
/// glyph — whose PR this is belongs with what it is, not across the row from it. The
/// right edge is laid out backwards from there: the reviewers, the comment tally and
/// the **Ready to merge** band's inline Merge button, with whatever is left over for
/// the tag flags. The main column takes the rest.
struct RowCols {
    author: f32,
    main: egui::Rangef,
    tags_right: f32,
    comments: egui::Rangef,
    merge: Option<egui::Rect>,
    reviewers_right: f32,
}

fn row_columns(rect: egui::Rect, band: ActionGroup) -> RowCols {
    let mut x = rect.right() - ROW_PAD_R;
    let center_y = rect.center().y;
    let reviewers_right = x;
    x -= COL_REVIEWERS_W + ROW_COL_GAP;

    x -= COL_COMMENTS_W;
    let comments = egui::Rangef::new(x, x + COL_COMMENTS_W);
    x -= ROW_COL_GAP;

    let merge = (band == ActionGroup::ReadyToMerge).then(|| {
        x -= MERGE_BTN_W;
        let btn = egui::Rect::from_min_size(
            egui::pos2(x, center_y - MERGE_BTN_H / 2.0),
            egui::vec2(MERGE_BTN_W, MERGE_BTN_H),
        );
        x -= ROW_COL_GAP;
        btn
    });

    let author = rect.left() + ROW_GUTTER + ROW_AVATAR / 2.0;
    let main_left = rect.left() + ROW_GUTTER + ROW_AVATAR + ROW_COL_GAP;
    RowCols {
        author,
        // The tag cluster is sized by its own content, so the main column only knows
        // it must not run past where the tags may start.
        main: egui::Rangef::new(main_left, x),
        tags_right: x,
        comments,
        merge,
        reviewers_right,
    }
}

/// A flag a row wears on its right-hand cluster (pull-requests.md §5), in paint order.
/// The mock's row dropped the CI column because Bitbucket never fills it; folding the
/// status in here keeps it for GitHub without giving an always-empty column its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowTag {
    /// The base of a stack: the one PR in it whose review unblocks the rest.
    ReviewFirst,
    ChangesRequested,
    ChecksFailing,
    ChecksRunning,
    Draft,
    /// How many listed PRs are waiting on this one.
    Blocks(usize),
}

/// Which flags a row wears. Pure so the composition is unit-testable — the chips
/// themselves are painted, like every other row ornament.
fn row_tags(prs: &[PullRequest], idx: usize, stack_base: bool) -> Vec<RowTag> {
    let pr = &prs[idx];
    let mut tags = Vec::new();
    if stack_base {
        tags.push(RowTag::ReviewFirst);
    }
    if pr.review == Review::ChangesRequested {
        tags.push(RowTag::ChangesRequested);
    }
    match pr.checks {
        Checks::Failing => tags.push(RowTag::ChecksFailing),
        Checks::Pending => tags.push(RowTag::ChecksRunning),
        // A green build is not news; an absent one is not a fact.
        Checks::Passing | Checks::None => {}
    }
    if pr.state == PrState::Draft {
        tags.push(RowTag::Draft);
    }
    let blocks = crate::pull_requests::model::blocked_count(prs, idx);
    if blocks > 0 {
        tags.push(RowTag::Blocks(blocks));
    }
    tags
}

impl RowTag {
    fn label(self) -> String {
        match self {
            RowTag::ReviewFirst => "Review first".to_owned(),
            RowTag::ChangesRequested => "Changes requested".to_owned(),
            RowTag::ChecksFailing => "Checks failing".to_owned(),
            RowTag::ChecksRunning => "Checks running".to_owned(),
            RowTag::Draft => "Draft".to_owned(),
            RowTag::Blocks(n) => format!("blocks {n}"),
        }
    }

    fn icon(self) -> Option<Icon> {
        match self {
            RowTag::ReviewFirst | RowTag::Draft => None,
            RowTag::ChangesRequested => Some(Icon::AlertCircle),
            RowTag::ChecksFailing => Some(Icon::X),
            RowTag::ChecksRunning => Some(Icon::Clock),
            RowTag::Blocks(_) => Some(Icon::Layers),
        }
    }

    fn color(self, palette: &Palette) -> egui::Color32 {
        match self {
            RowTag::ReviewFirst => palette.accent,
            RowTag::ChangesRequested | RowTag::ChecksFailing => palette.git_deleted,
            RowTag::ChecksRunning | RowTag::Blocks(_) => palette.git_modified,
            RowTag::Draft => palette.text_muted,
        }
    }
}

/// One list row (pull-requests.md §5), carved out of its block's card. The gutter
/// carries the state glyph — or, in a stack, the spine and this PR's rank; the main
/// column its ticket key, title and meta line; the right-hand cluster its flags, the
/// comment tally (or the inline **Merge**) and the author avatar.
#[allow(clippy::too_many_arguments)]
fn pr_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    prs: &[PullRequest],
    idx: usize,
    band: ActionGroup,
    selected: bool,
    stack: Option<StackAt<'_>>,
    rect: egui::Rect,
    corners: RowCorners,
    action: &mut PullRequestsPageAction,
) {
    let pr = &prs[idx];
    let response = ui.interact(rect, ui.id().with(("pr_row", idx)), egui::Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if selected {
        ui.painter()
            .rect_filled(rect, corners.radius(), with_alpha(palette.accent, 28));
        // A bar in the gutter, so the open row stays findable once the wash is scrolled
        // past the pointer.
        ui.painter().vline(
            rect.left() + 1.0,
            rect.y_range(),
            egui::Stroke::new(2.0_f32, palette.accent),
        );
    } else if hovered {
        ui.painter()
            .rect_filled(rect, corners.radius(), palette.bg_surface_hover);
    }
    if !corners.top {
        ui.painter().hline(
            egui::Rangef::new(rect.left(), rect.right()),
            rect.top() - 0.5,
            egui::Stroke::new(1.0_f32, with_alpha(palette.border_subtle, 120)),
        );
    }

    let cols = row_columns(rect, band);
    let center_y = rect.center().y;
    // The two lines read as one block: they sit a fixed step either side of the row's
    // centre, so the room the row gained lands around the pair, not between them.
    let title_y = center_y - ROW_LINE_STEP;
    let meta_y = center_y + ROW_LINE_STEP;
    // A band waiting on its author is informational — its rows read a notch quieter.
    let quiet = band == ActionGroup::WaitingOnAuthor;
    let title_color = if quiet {
        palette.text_secondary
    } else {
        palette.text_primary
    };

    row_gutter(ui, palette, rect, pr, stack);
    paint_avatar(
        ui.painter(),
        palette,
        &pr.author,
        egui::pos2(cols.author, center_y),
        ROW_AVATAR,
        None,
    );

    // The right-hand cluster first: the flags size themselves, and what they leave is
    // the measure the title has to fit in.
    let tags = row_tags(prs, idx, stack.is_some_and(|s| s.row.n == 1));
    let tags_left = paint_row_tags(ui, palette, &tags, cols.tags_right, center_y);
    let main_right = (tags_left - ROW_COL_GAP).max(cols.main.min);

    // Title line: the ticket the team knows this PR by, then its subject.
    let mut x = cols.main.min;
    if let Some(key) = crate::pull_requests::model::issue_key(pr) {
        x = cell_text(
            ui,
            key,
            egui::FontId::new(KEY_SIZE, crate::theme::medium_family(ui.ctx())),
            palette.accent,
            x,
            title_y,
            main_right - x,
        ) + GAP_SM;
    }
    cell_text(
        ui,
        &pr.title,
        egui::FontId::new(LIST_TITLE_SIZE, crate::theme::medium_family(ui.ctx())),
        title_color,
        x,
        title_y,
        (main_right - x).max(0.0),
    );

    // Meta line: number, author, age — then the project (a stack names it once, in its
    // own header), the branch flow, what this row hangs off, and the ± tally.
    let age = crate::pull_requests::model::relative_age(&pr.updated_at, now_epoch_secs());
    let age = if age.is_empty() {
        pr.updated_at.clone()
    } else {
        age
    };
    let mut meta = MetaLine {
        x: cols.main.min,
        y: meta_y,
        right: main_right,
    };
    let meta_font = egui::FontId::proportional(LIST_META_SIZE);
    let mono = egui::FontId::monospace(LIST_MONO_SIZE);
    meta.run(
        ui,
        &format!("#{}", pr.number),
        palette.text_muted,
        mono.clone(),
    );
    meta.run(ui, &pr.author, palette.text_secondary, meta_font.clone());
    meta.run(ui, &age, palette.text_muted, meta_font.clone());
    if stack.is_none() {
        meta.run(ui, &pr.repo_label, palette.text_muted, mono.clone());
    }
    if meta.x < meta.right {
        meta.x = branch_pill(
            ui,
            palette,
            meta.x,
            meta.y,
            Some(&pr.source_branch),
            &pr.dest_branch,
            meta.right - meta.x,
        ) + GAP_SM;
    }
    if let Some(off) = stack.and_then(|s| s.row.off_parent) {
        meta.run(ui, &format!("↳ off #{off}"), palette.text_muted, meta_font);
    }
    // The ± tally is GitHub-only (model §4); it rides the meta line rather than holding
    // a column that Bitbucket would always leave blank.
    if let Some((added, deleted)) = pr.diffstat {
        meta.run(ui, &format!("+{added}"), palette.git_added, mono.clone());
        meta.run(ui, &format!("−{deleted}"), palette.git_deleted, mono);
    }

    // Comment tally — Bitbucket-only, so the column stays blank rather than lying with
    // a zero when the forge simply did not say (model §4).
    if let Some(count) = pr.comment_count {
        let ink = if count == 0 {
            with_alpha(palette.text_muted, 120)
        } else {
            palette.text_muted
        };
        let text = count.to_string();
        let galley =
            ui.painter()
                .layout_no_wrap(text, egui::FontId::proportional(LIST_MONO_SIZE), ink);
        ui.painter().galley(
            egui::pos2(
                cols.comments.max - galley.size().x,
                center_y - galley.size().y / 2.0,
            ),
            galley.clone(),
            ink,
        );
        paint_icon(
            ui.painter(),
            egui::pos2(
                cols.comments.max - galley.size().x - GAP_XS - LIST_META_SIZE / 2.0,
                center_y,
            ),
            LIST_META_SIZE,
            Icon::MessageSquare,
            ink,
        );
    }

    if let Some(btn) = cols.merge {
        if merge_button(ui, palette, btn, ("pr_row_merge", idx)) {
            action.merge = Some(idx);
        }
    }

    reviewer_cluster(ui, palette, &pr.reviewers, cols.reviewers_right, center_y);

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, pr.title.clone())
    });
    if response.clicked() {
        action.select = Some(idx);
    }
}

/// A row's meta line, laid out left to right: each run is appended where the last one
/// ended and dropped once the line has reached the tag cluster.
struct MetaLine {
    x: f32,
    y: f32,
    right: f32,
}

impl MetaLine {
    fn run(&mut self, ui: &egui::Ui, text: &str, color: egui::Color32, font: egui::FontId) {
        if self.x < self.right {
            self.x = cell_text(ui, text, font, color, self.x, self.y, self.right - self.x) + GAP_SM;
        }
    }
}

/// The row's leading column: a stacked PR gets the spine running through it and a
/// badge with its rank; a loose one gets the open / draft / changes-requested glyph.
fn row_gutter(
    ui: &egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    pr: &PullRequest,
    stack: Option<StackAt<'_>>,
) {
    let center = egui::pos2(rect.left() + ROW_GUTTER / 2.0, rect.center().y);
    let Some(stack) = stack else {
        let (icon, color) = match (pr.state, pr.review) {
            (PrState::Draft, _) => (Icon::GitPullRequestDraft, palette.text_muted),
            (PrState::Open, Review::ChangesRequested) => {
                (Icon::GitPullRequest, palette.git_deleted)
            }
            (PrState::Open, _) => (Icon::GitPullRequest, palette.git_added),
        };
        paint_icon(ui.painter(), center, STATE_ICON, icon, color);
        return;
    };
    // The spine spans the chain, not the block: it starts at the base's badge and dies
    // at the top one's, so the run has visible ends.
    let top = if stack.row.n == 1 {
        center.y
    } else {
        rect.top()
    };
    let bottom = if stack.row.n == stack.len {
        center.y
    } else {
        rect.bottom()
    };
    ui.painter().vline(
        center.x,
        egui::Rangef::new(top, bottom),
        egui::Stroke::new(1.0_f32, palette.border_input),
    );
    ui.painter()
        .circle_filled(center, STACK_BADGE / 2.0, palette.bg_surface);
    ui.painter().circle_stroke(
        center,
        STACK_BADGE / 2.0,
        egui::Stroke::new(1.0_f32, palette.border_input),
    );
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        stack.row.n.to_string(),
        egui::FontId::new(9.5, crate::theme::medium_family(ui.ctx())),
        palette.text_secondary,
    );
}

/// The assigned reviewers, as an overlapping row of avatars each badged with where it
/// stands (pull-requests.md §5). Past `REVIEWER_MAX` the rest collapse into a `+N`
/// disc. Painted right-aligned at `right`, so the clusters line up down the list.
fn reviewer_cluster(
    ui: &egui::Ui,
    palette: &Palette,
    reviewers: &[crate::pull_requests::model::Reviewer],
    right: f32,
    center_y: f32,
) {
    let ordered = reviewers_by_verdict(reviewers);
    if ordered.is_empty() {
        return;
    }
    let shown = ordered.len().min(REVIEWER_MAX);
    let hidden = ordered.len() - shown;
    let discs = shown + usize::from(hidden > 0);
    let width = REVIEWER_AVATAR + (discs - 1) as f32 * ROW_REVIEWER_STEP;
    let left = right - width + REVIEWER_AVATAR / 2.0;
    let at = |i: usize| egui::pos2(left + i as f32 * ROW_REVIEWER_STEP, center_y);
    // Left to right, so each disc laps the one before it the way the canvas draws it.
    for (i, reviewer) in ordered[..shown].iter().enumerate() {
        paint_avatar(
            ui.painter(),
            palette,
            &reviewer.name,
            at(i),
            REVIEWER_AVATAR,
            None,
        );
    }
    if hidden > 0 {
        let center = at(shown);
        let r = REVIEWER_AVATAR / 2.0;
        ui.painter()
            .circle_filled(center, r + 1.5, palette.bg_canvas);
        ui.painter()
            .circle_filled(center, r, with_alpha(palette.text_muted, 46));
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            format!("+{hidden}"),
            egui::FontId::new(
                REVIEWER_AVATAR * 0.42,
                crate::theme::medium_family(ui.ctx()),
            ),
            palette.text_secondary,
        );
    }
    // Badges last, once every disc is down: each sits where its neighbour laps it, and a
    // verdict buried under the next avatar would be a verdict lost.
    for (i, reviewer) in ordered[..shown].iter().enumerate() {
        verdict_badge(ui, palette, reviewer.state, at(i));
    }
}

/// Reviewers with a verdict lead, changes-requested first: past `REVIEWER_MAX` the
/// rest fall behind the `+N` disc, and the one standing in the way is the last thing
/// that may go missing there.
fn reviewers_by_verdict(
    reviewers: &[crate::pull_requests::model::Reviewer],
) -> Vec<&crate::pull_requests::model::Reviewer> {
    let rank = |state: Review| match state {
        Review::ChangesRequested => 0,
        Review::Approved => 1,
        Review::Pending | Review::None => 2,
    };
    let mut ordered: Vec<_> = reviewers.iter().collect();
    ordered.sort_by_key(|reviewer| rank(reviewer.state));
    ordered
}

/// The small disc on a reviewer avatar's top-right saying where they stand. A reviewer
/// who has not ruled yet wears none — an empty badge would read as a verdict.
fn verdict_badge(ui: &egui::Ui, palette: &Palette, state: Review, center: egui::Pos2) {
    let (color, icon) = match state {
        Review::Approved => (palette.git_added, Icon::Check),
        Review::ChangesRequested => (palette.git_deleted, Icon::Minus),
        Review::Pending | Review::None => return,
    };
    let offset = REVIEWER_AVATAR / 2.0 * std::f32::consts::FRAC_1_SQRT_2;
    let at = center + egui::vec2(offset, -offset);
    ui.painter()
        .circle_filled(at, VERDICT_BADGE_R + 1.2, palette.bg_canvas);
    ui.painter().circle_filled(at, VERDICT_BADGE_R, color);
    paint_icon(
        ui.painter(),
        at,
        VERDICT_BADGE_R * 2.0,
        icon,
        egui::Color32::WHITE,
    );
}

/// Where a row sits in its stack — its rank, and how long the chain is, which is what
/// tells the spine where to stop.
#[derive(Clone, Copy)]
struct StackAt<'a> {
    row: &'a StackRow,
    len: usize,
}

/// Paint the row's flags right-aligned at `right`, in `RowTag` order. Returns the x
/// the leftmost one starts at, which is where the title column has to stop.
fn paint_row_tags(
    ui: &egui::Ui,
    palette: &Palette,
    tags: &[RowTag],
    right: f32,
    center_y: f32,
) -> f32 {
    let mut left = right;
    for tag in tags.iter().rev() {
        let w = tag_chip(ui, palette, *tag, left, center_y);
        left -= w + GAP_XS + 2.0;
    }
    left
}

/// One flag chip — a tinted pill in the flag's own color. Painted right-aligned at
/// `right`; returns its width.
fn tag_chip(ui: &egui::Ui, palette: &Palette, tag: RowTag, right: f32, center_y: f32) -> f32 {
    let color = tag.color(palette);
    let galley = ui.painter().layout_no_wrap(
        tag.label(),
        egui::FontId::new(LIST_MONO_SIZE - 1.0, crate::theme::medium_family(ui.ctx())),
        color,
    );
    let icon = tag.icon();
    let icon_w = if icon.is_some() {
        LIST_META_SIZE + 3.0
    } else {
        0.0
    };
    let h = LIST_MONO_SIZE + 8.0;
    let w = galley.size().x + icon_w + 14.0;
    let chip =
        egui::Rect::from_min_size(egui::pos2(right - w, center_y - h / 2.0), egui::vec2(w, h));
    ui.painter()
        .rect_filled(chip, RADIUS_BUTTON, with_alpha(color, 34));
    ui.painter().rect_stroke(
        chip,
        RADIUS_BUTTON,
        egui::Stroke::new(1.0_f32, with_alpha(color, 90)),
        egui::StrokeKind::Inside,
    );
    let mut x = chip.left() + 7.0;
    if let Some(icon) = icon {
        paint_icon(
            ui.painter(),
            egui::pos2(x + LIST_META_SIZE / 2.0, center_y),
            LIST_META_SIZE,
            icon,
            color,
        );
        x += icon_w;
    }
    ui.painter().galley(
        egui::pos2(x, center_y - galley.size().y / 2.0),
        galley,
        color,
    );
    w
}

/// The branch flow as one chip — `head → base`, or just `→ base` when the head is
/// understood (a stack header names where the whole chain lands). The head takes the
/// truncation, since the base is the short, load-bearing half. Returns its right edge.
fn branch_pill(
    ui: &egui::Ui,
    palette: &Palette,
    left: f32,
    center_y: f32,
    head: Option<&str>,
    base: &str,
    max_w: f32,
) -> f32 {
    if max_w < 40.0 {
        return left;
    }
    let font = egui::FontId::monospace(LIST_MONO_SIZE);
    let base_galley =
        ui.painter()
            .layout_no_wrap(base.to_owned(), font.clone(), palette.text_secondary);
    let arrow_w = LIST_META_SIZE + 4.0;
    let fixed = base_galley.size().x + arrow_w + 14.0;
    let head_galley = head.map(|head| {
        let mut job = egui::text::LayoutJob::single_section(
            head.to_owned(),
            egui::TextFormat::simple(font, palette.text_muted),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((max_w - fixed).max(24.0));
        ui.painter().layout_job(job)
    });
    let h = LIST_MONO_SIZE + 8.0;
    let w = fixed + head_galley.as_ref().map_or(0.0, |g| g.size().x);
    let chip = egui::Rect::from_min_size(egui::pos2(left, center_y - h / 2.0), egui::vec2(w, h));
    ui.painter()
        .rect_filled(chip, RADIUS_BUTTON, with_alpha(palette.text_muted, 26));
    let mut x = chip.left() + 7.0;
    if let Some(galley) = head_galley {
        let width = galley.size().x;
        ui.painter().galley(
            egui::pos2(x, center_y - galley.size().y / 2.0),
            galley,
            palette.text_muted,
        );
        x += width;
    }
    paint_icon(
        ui.painter(),
        egui::pos2(x + arrow_w / 2.0, center_y),
        LIST_META_SIZE,
        Icon::ArrowRight,
        palette.text_muted,
    );
    x += arrow_w;
    ui.painter().galley(
        egui::pos2(x, center_y - base_galley.size().y / 2.0),
        base_galley,
        palette.text_secondary,
    );
    chip.right()
}

/// Confirmation before merging on the forge (pull-requests.md §5). Merging is
/// outward-facing and not undoable from helm, so it follows the modal contract:
/// the primary action on the right, Cancel / `Esc` to dismiss.
pub fn merge_modal(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    out: &mut crate::ui::repo_sidebar::DeleteModalAction,
) {
    let modal = egui::Modal::new(egui::Id::new("pr_merge_modal"))
        .frame(crate::ui::modal_frame(ui.style()))
        .show(ui.ctx(), |ui| {
            crate::ui::modal_controls_style(ui);
            ui.set_width(320.0);
            ui.label(egui::RichText::new(format!("Merge #{}?", pr.number)).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} merges {} into {} on {}. The source branch is kept.",
                    pr.repo_label,
                    pr.source_branch,
                    pr.dest_branch,
                    match pr.forge_kind {
                        crate::pull_requests::model::ForgeKind::GitHub => "GitHub",
                        crate::pull_requests::model::ForgeKind::Bitbucket => "Bitbucket",
                    },
                ))
                .color(palette.text_secondary),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    out.dismiss = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Merge").clicked() {
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

/// The filled **Merge** button — inline on a ready-to-merge row, and in the review
/// surface header (pull-requests.md §5/§11).
fn merge_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    id: impl std::hash::Hash,
) -> bool {
    let response = ui.interact(rect, ui.id().with(id), egui::Sense::click());
    let fill = if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        palette.accent_hover
    } else {
        palette.accent
    };
    ui.painter().rect_filled(rect, RADIUS_BUTTON, fill);
    let center_y = rect.center().y;
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 12.0 + CHIP_SIZE / 2.0, center_y),
        CHIP_SIZE,
        Icon::GitMerge,
        egui::Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 12.0 + CHIP_SIZE + 5.0, center_y),
        egui::Align2::LEFT_CENTER,
        "Merge",
        egui::FontId::new(CHIP_SIZE, crate::theme::medium_family(ui.ctx())),
        egui::Color32::WHITE,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Merge"));
    response.clicked()
}

/// Left-aligned, vertically-centered text truncated to a column width. Returns the
/// x the text actually ended at, so a caller can butt the next run against it.
fn cell_text(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    left: f32,
    center_y: f32,
    max_w: f32,
) -> f32 {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(0.0));
    let galley = ui.painter().layout_job(job);
    let width = galley.size().x;
    ui.painter().galley(
        egui::pos2(left, center_y - galley.size().y / 2.0),
        galley,
        color,
    );
    left + width
}

/// A small initials avatar on a name-derived color, with a `bg_canvas` separator
/// halo (so overlapping avatars in a stack stay distinct) and an optional review
/// ring (approved / changes-requested).
fn paint_avatar(
    painter: &egui::Painter,
    palette: &Palette,
    name: &str,
    center: egui::Pos2,
    diameter: f32,
    ring: Option<egui::Color32>,
) {
    let r = diameter / 2.0;
    painter.circle_filled(center, r + 1.5, palette.bg_canvas);
    let hash = name.bytes().fold(0usize, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    painter.circle_filled(center, r, palette.lane_color(hash));
    let text = crate::ui::graph_view::initials(name);
    if !text.is_empty() {
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(diameter * 0.42),
            palette.lane_node_text,
        );
    }
    if let Some(color) = ring {
        painter.circle_stroke(center, r, egui::Stroke::new(1.5_f32, color));
    }
}

/// Overlapping reviewer avatars (capped at `REVIEWER_MAX`, the rest a `+N` tally),
/// each ringed by its review state.
fn reviewer_stack(
    ui: &egui::Ui,
    palette: &Palette,
    reviewers: &[crate::pull_requests::model::Reviewer],
    col: egui::Rangef,
    center_y: f32,
) {
    if reviewers.is_empty() {
        return;
    }
    let step = REVIEWER_AVATAR - REVIEWER_OVERLAP;
    let shown = reviewers.len().min(REVIEWER_MAX);
    let mut cx = col.min + REVIEWER_AVATAR / 2.0;
    for reviewer in &reviewers[..shown] {
        let ring = match reviewer.state {
            Review::Approved => Some(palette.git_added),
            Review::ChangesRequested => Some(palette.git_deleted),
            _ => None,
        };
        paint_avatar(
            ui.painter(),
            palette,
            &reviewer.name,
            egui::pos2(cx, center_y),
            REVIEWER_AVATAR,
            ring,
        );
        cx += step;
    }
    if reviewers.len() > shown {
        ui.painter().text(
            egui::pos2(cx - step + REVIEWER_AVATAR / 2.0 + 5.0, center_y),
            egui::Align2::LEFT_CENTER,
            format!("+{}", reviewers.len() - shown),
            egui::FontId::proportional(11.0),
            palette.text_muted,
        );
    }
}

fn checks_status(palette: &Palette, checks: Checks) -> Option<(Icon, egui::Color32)> {
    match checks {
        Checks::Passing => Some((Icon::Check, palette.git_added)),
        Checks::Failing => Some((Icon::X, palette.git_deleted)),
        Checks::Pending => Some((Icon::Clock, palette.text_muted)),
        Checks::None => None,
    }
}

fn status_line(
    ui: &mut egui::Ui,
    palette: &Palette,
    status: Option<(Icon, egui::Color32)>,
    text: &str,
) {
    ui.horizontal(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(STATUS_ICON, STATUS_ICON), egui::Sense::hover());
        if let Some((icon, color)) = status {
            paint_icon(ui.painter(), rect.center(), STATUS_ICON, icon, color);
        }
        ui.label(
            egui::RichText::new(text)
                .size(12.5)
                .color(palette.text_secondary),
        );
    });
}

fn muted(palette: &Palette, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(12.0)
        .color(palette.text_muted)
}

/// `muted` at the conversation card's own scale — the card reads a notch larger than the
/// browse list around it (§11).
fn conv_muted(palette: &Palette, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(CONV_META_SIZE)
        .color(palette.text_muted)
}

fn leaf_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn checks_label(checks: Checks) -> &'static str {
    match checks {
        Checks::Passing => "All checks passing",
        Checks::Failing => "Some checks failing",
        Checks::Pending => "Checks pending",
        Checks::None => "No checks",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pull_requests::model::{ForgeKind, PrRole};

    fn tagged(state: PrState, review: Review, checks: Checks) -> PullRequest {
        PullRequest {
            forge_kind: ForgeKind::GitHub,
            repo_label: "acme/web".to_owned(),
            number: 1,
            title: "A change".to_owned(),
            role: PrRole::ToReview,
            state,
            author: "mira".to_owned(),
            source_branch: "feature".to_owned(),
            dest_branch: "main".to_owned(),
            url: String::new(),
            updated_at: String::new(),
            checks,
            review,
            reviewers: Vec::new(),
            labels: Vec::new(),
            diffstat: None,
            comment_count: None,
        }
    }

    #[test]
    fn a_settled_row_wears_no_flags() {
        let prs = vec![tagged(PrState::Open, Review::Pending, Checks::Passing)];
        assert!(row_tags(&prs, 0, false).is_empty());
        // Nor does an absent CI invent one.
        let prs = vec![tagged(PrState::Open, Review::Pending, Checks::None)];
        assert!(row_tags(&prs, 0, false).is_empty());
    }

    #[test]
    fn a_row_wears_every_flag_that_applies_in_order() {
        let prs = vec![tagged(
            PrState::Draft,
            Review::ChangesRequested,
            Checks::Failing,
        )];
        assert_eq!(
            row_tags(&prs, 0, true),
            [
                RowTag::ReviewFirst,
                RowTag::ChangesRequested,
                RowTag::ChecksFailing,
                RowTag::Draft,
            ]
        );
    }

    #[test]
    fn the_blocks_flag_counts_the_prs_stacked_on_this_one() {
        let mut base = tagged(PrState::Open, Review::Pending, Checks::None);
        base.source_branch = "a".to_owned();
        let mut child = tagged(PrState::Open, Review::Pending, Checks::None);
        child.number = 2;
        child.dest_branch = "a".to_owned();
        let prs = vec![base, child];
        assert_eq!(row_tags(&prs, 0, false), [RowTag::Blocks(1)]);
        assert!(row_tags(&prs, 1, false).is_empty());
    }

    #[test]
    fn a_running_build_flags_itself_apart_from_a_failing_one() {
        let prs = vec![tagged(PrState::Open, Review::Pending, Checks::Pending)];
        assert_eq!(row_tags(&prs, 0, false), [RowTag::ChecksRunning]);
    }

    /// Play a whole run — start, the moves, release — at `t`, and say whether it fired.
    fn swipe(moves: &[(f32, f32)], armed: bool, t: f64, state: &mut SwipeBack) -> bool {
        let mut fired = state.feed(egui::TouchPhase::Start, egui::Vec2::ZERO, armed, t);
        for &(x, y) in moves {
            fired |= state.feed(egui::TouchPhase::Move, egui::vec2(x, y), armed, t);
        }
        fired | state.feed(egui::TouchPhase::End, egui::Vec2::ZERO, armed, t)
    }

    #[test]
    fn a_swipe_right_goes_back_only_once_it_is_long_enough() {
        let mut state = SwipeBack::default();
        assert!(!swipe(&[(20.0, 0.0), (20.0, 0.0)], true, 1.0, &mut state));
        assert!(swipe(&[(40.0, 2.0), (40.0, -2.0)], true, 2.0, &mut state));
    }

    #[test]
    fn a_swipe_left_or_a_vertical_scroll_is_not_a_back() {
        let mut state = SwipeBack::default();
        // Leftward: that direction reveals content to the right, not the list.
        assert!(!swipe(&[(-90.0, 0.0)], true, 1.0, &mut state));
        // Mostly vertical, even though it drifts 70pt sideways.
        assert!(!swipe(&[(70.0, 200.0)], true, 2.0, &mut state));
    }

    #[test]
    fn a_swipe_a_scrolled_diff_owns_never_goes_back() {
        let mut state = SwipeBack::default();
        assert!(!swipe(&[(90.0, 0.0)], false, 1.0, &mut state));
        // Nor does a run that only wanders onto one halfway through.
        let mut fired = state.feed(egui::TouchPhase::Start, egui::Vec2::ZERO, true, 2.0);
        fired |= state.feed(egui::TouchPhase::Move, egui::vec2(50.0, 0.0), true, 2.0);
        fired |= state.feed(egui::TouchPhase::Move, egui::vec2(50.0, 0.0), false, 2.0);
        fired |= state.feed(egui::TouchPhase::End, egui::Vec2::ZERO, true, 2.0);
        assert!(!fired);
    }

    #[test]
    fn the_momentum_trailing_a_flick_does_not_swipe_again() {
        // macOS replays start/move/end for the coast after the fingers lift; it
        // continues the gesture that is already spent rather than firing a second one.
        let mut state = SwipeBack::default();
        assert!(swipe(&[(90.0, 0.0)], true, 1.0, &mut state));
        assert!(!swipe(
            &[(300.0, 0.0)],
            true,
            1.0 + SWIPE_BACK_GAP / 2.0,
            &mut state
        ));
        // A deliberate second swipe, after a human-sized pause, still works.
        assert!(swipe(&[(90.0, 0.0)], true, 3.0, &mut state));
    }

    #[test]
    fn a_flick_completes_on_the_momentum_that_carries_it() {
        // Fingers barely travel before they lift; the coast is the rest of that same
        // gesture, not a new one that has to clear the threshold on its own.
        let mut state = SwipeBack::default();
        assert!(!swipe(&[(20.0, 0.0)], true, 1.0, &mut state));
        assert!(swipe(
            &[(50.0, 0.0)],
            true,
            1.0 + SWIPE_BACK_GAP / 2.0,
            &mut state
        ));
    }

    #[test]
    fn a_swipe_fires_while_the_fingers_are_still_down() {
        // Waiting for the release would hang the surface on the momentum tail, which
        // ends a second or more after the gesture reads as unmistakable.
        let mut state = SwipeBack::default();
        assert!(!state.feed(egui::TouchPhase::Start, egui::Vec2::ZERO, true, 1.0));
        assert!(state.feed(egui::TouchPhase::Move, egui::vec2(70.0, 0.0), true, 1.0));
    }

    #[test]
    fn a_release_with_no_run_behind_it_fires_nothing() {
        let mut state = SwipeBack::default();
        assert!(!state.feed(egui::TouchPhase::End, egui::vec2(200.0, 0.0), true, 1.0));
    }

    #[test]
    fn the_reviewer_cluster_leads_with_the_verdicts() {
        let reviewer = |name: &str, state| crate::pull_requests::model::Reviewer {
            name: name.to_owned(),
            state,
        };
        let reviewers = vec![
            reviewer("pending", Review::Pending),
            reviewer("approved", Review::Approved),
            reviewer("unasked", Review::None),
            reviewer("blocked", Review::ChangesRequested),
        ];
        // Only three discs fit; the one standing in the way must not be the fourth.
        assert_eq!(
            reviewers_by_verdict(&reviewers)
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["blocked", "approved", "pending", "unasked"]
        );
    }

    #[test]
    fn html_img_reads_src_and_alt() {
        assert_eq!(
            html_img(r#"<img width="600" alt="Step 1" src="https://x.test/a.png" />"#),
            Some(("https://x.test/a.png".to_owned(), "Step 1".to_owned()))
        );
        assert_eq!(
            html_img("<img src='https://x.test/b.png'>"),
            Some(("https://x.test/b.png".to_owned(), String::new()))
        );
        assert_eq!(html_img("<p>no picture here</p>"), None);
    }

    /// In a table cell an image can only read as text: its alt, else the file name —
    /// never the whole URL, which would blow the column open.
    #[test]
    fn image_label_falls_back_to_the_file_name() {
        assert_eq!(image_label("https://x.test/a/step-1.png", ""), "step-1.png");
        assert_eq!(
            image_label("https://x.test/a/step-1.png", "Step 1"),
            "Step 1"
        );
    }
}
