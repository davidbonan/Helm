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
    hunk_snippet, Checks, PrComment, PrCommit, PrDetail, PrRole, PrState, PullRequest, Review,
    ReviewVerdict, SnippetKind, SnippetLine,
};
use crate::review::{FileComments, ForgeThreads, ReviewIntent};
use crate::theme::{Palette, RADIUS_BUTTON, SECTION_TITLE_SIZE};
use crate::ui::detail::{author_avatar, author_avatar_small, code_snippet, count_chip};
use crate::ui::diff_view::{
    reply_editor, reply_pill, resolve_pill, ConversationEdit, DiffReview, DiffSurface,
    DiffViewState, ReplyEdit, REPLY_LABELS,
};
use crate::ui::file_list::{self, file_row, row_separator, FileRow, FileViewMode};
use crate::ui::git_panel::ratio_bar;
use crate::ui::spinner::Spinner;
use crate::ui::{clickable, paint_icon, with_alpha, SECTION_TOP_MARGIN, TITLEBAR_HEIGHT};

/// Review-surface split bounds: the changed-files rail and the diff each keep a
/// floor; the persisted rail width is clamped between them.
const RAIL_MIN_WIDTH: f32 = 260.0;
const DIFF_MIN_WIDTH: f32 = 420.0;

/// Markdown reading tweaks for the in-house renderer (`markdown`): the body reads
/// smaller than the egui_commonmark default but with looser line-height and a touch
/// of letter-spacing, so long prose blocks don't read as a dense wall.
const MD_TEXT_SIZE: f32 = 13.5;
const MD_CODE_SIZE: f32 = 12.5;
const MD_LINE_HEIGHT: f32 = MD_TEXT_SIZE * 1.55;
const MD_LETTER_SPACING: f32 = 0.4;
const MD_PARAGRAPH_GAP: f32 = 7.0;
const MD_LIST_INDENT: f32 = 18.0;
const MD_QUOTE_INDENT: f32 = 12.0;
const DETAIL_HEADER_TITLE_SIZE: f32 = 16.0;
const DETAIL_HEADER_SUBTITLE_SIZE: f32 = 12.5;
const DETAIL_HEADER_BACK_SIZE: f32 = 38.0;
const DETAIL_HEADER_WIDE_HEIGHT: f32 = 58.0;
const DETAIL_HEADER_STACKED_HEIGHT: f32 = 96.0;
const DETAIL_ACTION_HEIGHT: f32 = 30.0;
const DETAIL_ACTION_OPEN_WIDTH: f32 = 152.0;
const DETAIL_ACTION_CHECKOUT_WIDTH: f32 = 112.0;
const DETAIL_HEADER_GAP: f32 = 8.0;

const ROW_HEIGHT: f32 = 60.0;
const GROUP_HEADER_HEIGHT: f32 = 34.0;
const PAD_X: f32 = 16.0;
const PANEL_PAD_X: f32 = 18.0;
const PANEL_PAD_Y: f32 = 14.0;

const TITLE_SIZE: f32 = 14.0;
const META_SIZE: f32 = 12.0;
const CHIP_SIZE: f32 = 11.5;
const HEADER_SIZE: f32 = 11.0;
const STATE_ICON: f32 = 16.0;
const STATUS_ICON: f32 = 15.0;
const COUNT_BADGE_SIZE: f32 = 11.0;

/// Browse-list page header (the `Pull Requests` title + Refresh).
const PAGE_HEADER_HEIGHT: f32 = 48.0;
const PAGE_TITLE_SIZE: f32 = 20.0;

/// Column-table geometry for the browse list (pull-requests.md §5). Fixed columns
/// are laid out from the right edge; the Title column fills the remaining width.
const COL_GAP: f32 = 12.0;
const COL_PROJECT_W: f32 = 172.0;
const COL_AUTHOR_W: f32 = 130.0;
const COL_REVIEWERS_W: f32 = 92.0;
const COL_STATUS_W: f32 = 136.0;
const COL_UPDATED_W: f32 = 96.0;
const COL_CHEVRON_W: f32 = 16.0;
const TITLE_MIN_W: f32 = 240.0;
const TITLE_MAX_W: f32 = 620.0;
const COL_HEADER_HEIGHT: f32 = 26.0;
const CARD_RADIUS: u8 = 10;
const REVIEWER_AVATAR: f32 = 22.0;
const REVIEWER_OVERLAP: f32 = 8.0;
const REVIEWER_MAX: usize = 3;

/// Lines of code previewed atop an inline-comment card (pull-requests.md §5).
const INLINE_SNIPPET_LINES: usize = 8;
/// Extra indent a reply nests under its thread root in the center cards — sized so the
/// thread rail falls on the root avatar's centre, reading as a spine descending from it (§11).
const INLINE_REPLY_INDENT: f32 = 26.0;
/// Height of a resolved thread's collapsed summary row (§11).
const RESOLVED_ROW_HEIGHT: f32 = 34.0;

/// Vertical rhythm for the comment cards (pull-requests.md §11): one small scale so the
/// conversation and inline threads breathe on the same beat instead of ad-hoc gaps.
const GAP_XS: f32 = 4.0;
const GAP_SM: f32 = 8.0;
const GAP_MD: f32 = 12.0;
/// Gap between a comment's avatar gutter and its text column (§11).
const AVATAR_GUTTER_GAP: f32 = 10.0;

const AVATAR_GAP: f32 = 9.0;
const AUTHOR_NAME_SIZE: f32 = 13.0;
const TOTALS_SIZE: f32 = 13.0;

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
    pub diff: Option<&'a FileDiff>,
    /// Local diffs for the current range's files that carry an inline comment, so the
    /// center inline cards render a code preview even when the file isn't open — the
    /// Bitbucket case, which has no forge `diff_hunk` (pull-requests.md §5).
    pub comment_diffs: Vec<&'a FileDiff>,
    pub diff_loading: bool,
    pub diff_error: Option<&'a str>,
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
        Some(review) => render_review(
            ui,
            palette,
            review,
            rect,
            rail_width,
            rail_collapsed,
            file_view,
            &mut action,
        ),
        // The browse list owns the central area like the Agents dashboard: the
        // background already reached the window top, so the body is inset past the
        // macOS title strip to align with the side panels (which inset the same way).
        None => {
            ui.add_space(f32::from(TITLEBAR_HEIGHT));
            let body = ui.available_rect_before_wrap();
            render_list(ui, palette, prs, selected, hints, body, &mut action);
        }
    }
    action
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
    // An open diff owns `Esc` itself — it cancels an in-progress note first, then
    // escalates to closing the file (mapped below). With no diff drawn, `Esc`
    // closes a still-selected (loading) file, else returns to the list.
    if review.diff.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if review.selected_file.is_some() {
            action.close_file = true;
        } else {
            action.back = true;
        }
    }

    // A non-finite persisted width (hand-edited prefs) would poison the layout math.
    let rail_width = if rail_width.is_finite() {
        rail_width
    } else {
        RAIL_MIN_WIDTH
    };

    // The rail frame (its divider + background) spans the full height so it reaches
    // the title strip like the real side panels; the center content insets past it.
    let center = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top() + f32::from(TITLEBAR_HEIGHT), rect.bottom()),
    );

    if rail_collapsed {
        review_diff(ui, palette, review, center, action);
        return;
    }

    let rail_w = rail_width.clamp(
        RAIL_MIN_WIDTH,
        (rect.width() - DIFF_MIN_WIDTH).max(RAIL_MIN_WIDTH),
    );
    let split_x = rect.right() - rail_w;
    let diff_rect =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(rect.left(), split_x), center.y_range());
    review_diff(ui, palette, review, diff_rect, action);

    let rail_rect =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(split_x, rect.right()), rect.y_range());
    review_rail(ui, palette, review, rail_rect, file_view, action);

    rail_resize_handle(ui, palette, split_x, rect, rail_width, action);
}

/// Right rail of the review surface (git.md §9 visual language): a **Files
/// changed** band (count chip, ±totals, ratio bar) and the file list, with the
/// review composer pinned to the foot. The Back control, PR-level actions, title
/// and detail live in the center area when no file is open.
fn review_rail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    file_view: FileViewMode,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let footer_h = composer_height(review);
    // Content starts below the floating title strip (the rail frame itself reaches
    // the window top); the composer stays pinned to the foot.
    let content_top = rect.top() + f32::from(TITLEBAR_HEIGHT);
    let scroll_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(content_top, (rect.bottom() - footer_h).max(content_top)),
    );
    let footer_rect = egui::Rect::from_x_y_ranges(rect.x_range(), {
        let top = (rect.bottom() - footer_h).max(content_top);
        egui::Rangef::new(top, rect.bottom())
    });
    let inner = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(scroll_rect.left() + PANEL_PAD_X, scroll_rect.right() - 6.0),
        scroll_rect.y_range(),
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
            if !review.commits.is_empty() {
                commits_band(ui, palette, review, action);
                ui.add_space(10.0);
            }
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
            if let Some(target) = files_band(
                ui,
                palette,
                review.files,
                file_view,
                unread_count,
                &mut unread_only,
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
                &mut collapsed,
                action,
            );
            ui.data_mut(|d| d.insert_temp(collapse_id, collapsed));
            ui.data_mut(|d| d.insert_temp(viewed_id, viewed));
            ui.data_mut(|d| d.insert_temp(unread_only_id, unread_only));
            ui.add_space(PANEL_PAD_Y);
        });

    review_composer(ui, palette, review, footer_rect, action);
}

/// The PR detail in the **center** area when no file is open (pull-requests.md
/// §11): a compact PR header heads the author block + branch flow + body, then
/// Checks and the conversation-level comments. The PR-level actions live here so
/// a selected file swaps the center to its diff and leaves the rail focused on
/// changed files + review submission.
fn review_detail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
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
        .id_salt("pr_review_detail")
        .show(&mut panel, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(PANEL_PAD_Y);
            review_detail_header(ui, palette, review, action);
            ui.add_space(SECTION_TOP_MARGIN);
            review_meta(ui, palette, review);
            let pr = review.pr;

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

            let checks = review
                .detail
                .map(|d| d.check_runs.as_slice())
                .unwrap_or(&[]);
            if !checks.is_empty() || pr.checks != Checks::None {
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

            conversation_section(ui, palette, review, action);
            inline_comments_section(ui, palette, review, action);
            ui.add_space(PANEL_PAD_Y);
        });
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

/// **Conversation** band in the center detail (pull-requests.md §11): the PR's
/// top-level comments (no diff anchor), each with a Reply affordance when the forge
/// threads them (Bitbucket carries a comment id; GitHub issue comments are flat), and
/// a standalone composer that always lets the user start a new comment.
fn conversation_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    // `detail` is a copied-out `'a` reference, so it no longer borrows `review` — the
    // composer state under `review.diff_view` can then lend mutably (as in the inline
    // section).
    let pr = review.pr;
    let detail = review.detail;
    let current_user = review.current_user;
    let diff_view = &mut *review.diff_view;
    // The index is the comment's position in the oldest-first conversation, stable
    // across the Newest/Oldest display reversal — so an open reply editor stays
    // anchored to its thread when the order toggles.
    let flat: Vec<(usize, &PrComment)> = detail
        .map(|d| {
            d.comments
                .iter()
                .filter(|c| c.path.is_none())
                .enumerate()
                .collect()
        })
        .unwrap_or_default();
    // Nest each reply under the comment it answers: Bitbucket threads conversation
    // comments via `parent_id`; GitHub issue comments are flat (id/parent both None),
    // so each stays its own single-comment thread.
    let index_by_id: HashMap<u64, usize> = flat
        .iter()
        .filter_map(|(i, c)| c.id.map(|id| (id, *i)))
        .collect();
    let root_index = |start: usize| -> usize {
        let mut idx = start;
        for _ in 0..flat.len() {
            let Some(pid) = flat[idx].1.parent_id else {
                break;
            };
            match index_by_id.get(&pid) {
                Some(&p) if p != idx => idx = p,
                _ => break,
            }
        }
        idx
    };
    let mut threads: Vec<(usize, Vec<(usize, &PrComment)>)> = Vec::new();
    for &(i, c) in &flat {
        let root = root_index(i);
        match threads.iter_mut().find(|(r, _)| *r == root) {
            Some((_, members)) => members.push((i, c)),
            None => threads.push((root, vec![(i, c)])),
        }
    }
    for (root, members) in &mut threads {
        members.sort_by_key(|(i, _)| (*i != *root, *i));
    }
    let newest_first = conversation_header(ui, palette, pr.url.as_str(), flat.len());
    if newest_first {
        threads.reverse();
    }
    let now = now_epoch_secs();
    for (root, members) in &threads {
        let collapse_key = members
            .first()
            .filter(|(_, c)| c.resolved)
            .and_then(|(_, c)| c.id);
        if let Some(id) = collapse_key {
            let expanded = diff_view.is_resolved_expanded(id);
            if resolved_header_row(ui, palette, members.len(), expanded) {
                diff_view.toggle_resolved(id);
            }
            ui.add_space(GAP_SM);
            if !expanded {
                continue;
            }
        }
        // One card per thread: the root and its replies stack inside under a left rail,
        // and the reply affordance sits in the card's foot. A threaded forge (Bitbucket)
        // carries the root's id; a flat one (GitHub) has None and posts a new top-level
        // comment.
        let comments: Vec<&PrComment> = members.iter().map(|(_, c)| *c).collect();
        let root_id = members.first().and_then(|(_, c)| c.id);
        comment_frame(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            thread_members(ui, palette, pr, &comments, now);
            ui.add_space(GAP_SM);
            conversation_reply_block(ui, palette, diff_view, *root, root_id, action);
        });
        ui.add_space(GAP_MD);
    }
    conversation_add_block(ui, palette, diff_view, current_user, action);
}

/// The shared comment-card surface for the conversation and inline center sections
/// (design-system "Detail card"): a `bg.surface` fill over the `bg.canvas` detail, a
/// subtle border, a 10pt radius and even padding — so a comment reads the same
/// wherever it appears (pull-requests.md §11).
fn comment_frame(palette: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0, palette.border_subtle))
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
                .size(META_SIZE)
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
                        .size(META_SIZE)
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
        egui::Stroke::new(2.0, palette.border_input),
    );
}

/// The Conversation band header: the title plus an Oldest|Newest order toggle
/// (persisted per PR, shown only with more than one comment). Returns whether to
/// render newest-first.
fn conversation_header(ui: &mut egui::Ui, palette: &Palette, pr_url: &str, count: usize) -> bool {
    let id = egui::Id::new(("pr_conversation_newest", pr_url));
    let mut newest_first: bool = ui.data(|d| d.get_temp(id).unwrap_or(false));
    ui.add_space(SECTION_TOP_MARGIN);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP_SM;
        ui.label(
            egui::RichText::new("Conversation")
                .size(SECTION_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        count_chip(ui, palette, count);
        if count > 1 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if order_segment(ui, palette, "Newest", newest_first) {
                    newest_first = true;
                }
                if order_segment(ui, palette, "Oldest", !newest_first) {
                    newest_first = false;
                }
            });
        }
    });
    ui.add_space(6.0);
    ui.data_mut(|d| d.insert_temp(id, newest_first));
    newest_first
}

/// One segment of the Oldest|Newest order control: a pill that fills `accent.subtle`
/// with `accent` ink when active, so the live order reads as a selected segment rather
/// than a faint colour shift (design-system segmented control).
fn order_segment(ui: &mut egui::Ui, palette: &Palette, label: &str, active: bool) -> bool {
    let font = egui::FontId::proportional(META_SIZE);
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

/// The standalone conversation composer (pull-requests.md §11): an always-visible bar
/// at the foot of the band — avatar, an input field, and a filled-accent **Comment**
/// button — raising `PostConversationComment` with no parent, a new top-level comment
/// on either forge. The button reads solid accent throughout but only submits once the
/// draft holds non-blank text.
fn conversation_add_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    current_user: Option<&str>,
    action: &mut PullRequestsPageAction,
) {
    const BUTTON_HEIGHT: f32 = 32.0;
    ui.add_space(GAP_MD);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        author_avatar(ui, palette, current_user.unwrap_or(""));
        ui.add_space(AVATAR_GUTTER_GAP);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            // The field grows from a single line as the draft fills, so an empty composer
            // reads as a quiet prompt rather than a tall well; its surface matches the
            // comment cards (egui paints a TextEdit from `extreme_bg_color`, not widget fill).
            ui.visuals_mut().extreme_bg_color = palette.bg_surface;
            let radius = egui::CornerRadius::same(CARD_RADIUS);
            let w = &mut ui.visuals_mut().widgets;
            for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                s.corner_radius = radius;
            }
            w.inactive.bg_stroke = egui::Stroke::new(1.0, palette.border_subtle);
            w.hovered.bg_stroke = egui::Stroke::new(1.0, palette.border_input);
            w.active.bg_stroke = egui::Stroke::new(1.5, palette.accent);
            ui.visuals_mut().selection.stroke = egui::Stroke::new(1.5, palette.accent);
            ui.add(
                egui::TextEdit::multiline(diff_view.conversation_add_buffer_mut())
                    .desired_rows(1)
                    .desired_width(ui.available_width())
                    .font(egui::FontId::proportional(13.5))
                    .margin(egui::Margin::symmetric(12, 9))
                    .hint_text("Add a comment…"),
            );
            ui.add_space(GAP_SM);
            // A fixed-height row so the right-to-left layout can't claim the scroll area's
            // full remaining height and strand the button at the panel foot.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), BUTTON_HEIGHT),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let enabled = !diff_view.conversation_add_buffer_mut().trim().is_empty();
                    let font = egui::FontId::proportional(13.0);
                    let galley = ui.painter().layout_no_wrap(
                        "Comment".to_owned(),
                        font.clone(),
                        egui::Color32::PLACEHOLDER,
                    );
                    let button_size = egui::vec2(galley.size().x + 28.0, BUTTON_HEIGHT);
                    let (rect, response, hovered) = clickable(ui, button_size, enabled);
                    let fill = if hovered {
                        palette.accent_hover
                    } else {
                        palette.accent
                    };
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Comment",
                        font,
                        palette.lane_node_text,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "Comment")
                    });
                    if response.clicked() {
                        let body = diff_view.conversation_add_buffer_mut().trim().to_owned();
                        if !body.is_empty() {
                            action
                                .review_intents
                                .push(ReviewIntent::PostConversationComment { parent: None, body });
                            diff_view.conversation_add_buffer_mut().clear();
                        }
                    }
                },
            );
        });
    });
}

/// **Inline comments** band in the center detail (pull-requests.md §5): the PR's
/// inline review threads, grouped per file, each over a small monochrome snippet of
/// the code they were left on. The thread itself still renders anchored on the diff
/// rows (the overlay); this is the navigable summary — clicking a card opens the file
/// and scrolls the diff to the line.
fn inline_comments_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    let Some(detail) = review.detail else {
        return;
    };
    let inline: Vec<&PrComment> = detail
        .comments
        .iter()
        .filter(|c| c.path.is_some() && (c.old_lineno.is_some() || c.new_lineno.is_some()))
        .collect();
    if inline.is_empty() {
        return;
    }
    // `comment_diffs` and `changed_files` are field borrows disjoint from
    // `review.diff_view`, so the latter is still free to lend mutably to each card's
    // reply editor.
    let comment_diffs: &[&FileDiff] = &review.comment_diffs;
    let changed_files = review.files;
    let pr = review.pr;
    let diff_view = &mut *review.diff_view;
    ui.add_space(SECTION_TOP_MARGIN);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP_SM;
        ui.label(
            egui::RichText::new("Inline comments")
                .size(SECTION_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        count_chip(ui, palette, inline.len());
    });
    ui.add_space(6.0);
    let mut files: Vec<&str> = Vec::new();
    for c in &inline {
        if let Some(path) = c.path.as_deref() {
            if !files.contains(&path) {
                files.push(path);
            }
        }
    }
    for path in files {
        ui.add_space(GAP_SM);
        ui.label(
            egui::RichText::new(path)
                .size(META_SIZE)
                .monospace()
                .strong()
                .color(palette.text_secondary),
        );
        let mut anchors: Vec<(Option<u32>, Option<u32>)> = Vec::new();
        for c in inline.iter().filter(|c| c.path.as_deref() == Some(path)) {
            let anchor = (c.old_lineno, c.new_lineno);
            if !anchors.contains(&anchor) {
                anchors.push(anchor);
            }
        }
        for (old, new) in anchors {
            let thread: Vec<&PrComment> = inline
                .iter()
                .copied()
                .filter(|c| {
                    c.path.as_deref() == Some(path) && (c.old_lineno, c.new_lineno) == (old, new)
                })
                .collect();
            inline_comment_card(
                ui,
                palette,
                diff_view,
                comment_diffs,
                changed_files,
                pr,
                path,
                new,
                &thread,
                action,
            );
        }
    }
}

/// The collapsed summary of a resolved thread (pull-requests.md §11): a tick,
/// "Resolved · N comment(s)" and a chevron that points down once expanded. The whole
/// row is the click target; returns whether it was clicked (the caller toggles).
fn resolved_header_row(ui: &mut egui::Ui, palette: &Palette, count: usize, expanded: bool) -> bool {
    let label = if count == 1 {
        "Resolved · 1 comment".to_owned()
    } else {
        format!("Resolved · {count} comments")
    };
    let (rect, response, hovered) = clickable(
        ui,
        egui::vec2(ui.available_width(), RESOLVED_ROW_HEIGHT),
        true,
    );
    let fill = if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(CARD_RADIUS),
        fill,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let cy = rect.center().y;
    let tick_x = rect.left() + 16.0;
    paint_icon(
        painter,
        egui::pos2(tick_x, cy),
        14.0,
        Icon::Check,
        palette.git_added,
    );
    painter.text(
        egui::pos2(tick_x + 14.0, cy),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(META_SIZE),
        palette.text_secondary,
    );
    let chevron = if expanded {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    paint_icon(
        painter,
        egui::pos2(rect.right() - 16.0, cy),
        14.0,
        chevron,
        palette.text_muted,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

/// One inline thread as a clickable card: the code snippet (GitHub's `diff_hunk`,
/// else a window of the loaded diff for the selected file), then the comments, then
/// a Reply affordance that mirrors the diff overlay's (pull-requests.md §11). The
/// reply state lives in the shared `diff_view`, so opening a reply here also opens
/// it on the overlay.
#[allow(clippy::too_many_arguments)]
fn inline_comment_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    diff_view: &mut DiffViewState,
    diffs: &[&FileDiff],
    files: &[CommitFile],
    pr: &PullRequest,
    path: &str,
    new: Option<u32>,
    thread: &[&PrComment],
    action: &mut PullRequestsPageAction,
) {
    ui.add_space(GAP_SM);
    let collapse_key = thread
        .first()
        .filter(|c| c.resolved)
        .and_then(|_| thread.iter().find_map(|c| c.id));
    if let Some(id) = collapse_key {
        let expanded = diff_view.is_resolved_expanded(id);
        if resolved_header_row(ui, palette, thread.len(), expanded) {
            diff_view.toggle_resolved(id);
        }
        if !expanded {
            return;
        }
        ui.add_space(GAP_SM);
    }
    let now = now_epoch_secs();
    let snippet = inline_snippet(diffs, path, new, thread);
    let open_label = match new {
        Some(line) => format!("Open {path} line {line}"),
        None => format!("Open {path}"),
    };
    let mut open_clicked = false;
    comment_frame(palette).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // The code preview is the "open in diff" target, so the comment text below
        // stays selectable (not buried under a whole-card click sense).
        if !snippet.is_empty() {
            let snippet_response = code_snippet(ui, palette, path, &snippet)
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            snippet_response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &open_label)
            });
            open_clicked |= snippet_response.clicked();
            ui.add_space(GAP_SM);
        }
        thread_members(ui, palette, pr, thread, now);
        // No hunk and no open diff to window (Bitbucket): keep the file reachable with a
        // slim text affordance, since there's no snippet to click.
        if snippet.is_empty() {
            ui.add_space(GAP_XS);
            let resp = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(&open_label)
                            .size(META_SIZE)
                            .color(palette.accent),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            open_clicked |= resp.clicked();
        }
        // The reply / resolve controls sit in the card's foot. The reply target and the
        // Bitbucket resolve handle are both the thread root's forge id.
        if let Some(reply_id) = thread.iter().find_map(|c| c.id) {
            let resolved = thread.first().is_some_and(|c| c.resolved);
            let thread_id = thread.iter().find_map(|c| c.thread_id.clone());
            ui.add_space(GAP_SM);
            center_reply_block(
                ui, palette, diff_view, reply_id, resolved, thread_id, action,
            );
        }
    });
    if open_clicked {
        if let Some(idx) = files.iter().position(|f| f.path == path) {
            action.open_inline_comment = Some((idx, new));
        }
    }
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

/// Full-width header for the center PR detail. It mirrors the accepted mockup:
/// Back on the left, title + context, compact PR-level actions on the right, then
/// a subtle divider before the existing detail content.
fn review_detail_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    let pr = review.pr;
    let width = ui.available_width();
    let number = format!("#{}", pr.number);
    let number_galley = ui.painter().layout_no_wrap(
        number.clone(),
        egui::FontId::proportional(CHIP_SIZE),
        palette.text_secondary,
    );
    let number_w = number_galley.size().x + 18.0;
    let actions_w = DETAIL_ACTION_OPEN_WIDTH
        + DETAIL_HEADER_GAP
        + DETAIL_ACTION_CHECKOUT_WIDTH
        + DETAIL_HEADER_GAP
        + number_w;
    let wide = width >= 560.0;
    let height = if wide {
        DETAIL_HEADER_WIDE_HEIGHT
    } else {
        DETAIL_HEADER_STACKED_HEIGHT
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    let back_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + 4.0),
        egui::vec2(DETAIL_HEADER_BACK_SIZE, DETAIL_HEADER_BACK_SIZE),
    );
    if detail_back_button(ui, palette, back_rect) {
        action.back = true;
    }

    let title_left = back_rect.right() + 12.0;
    let actions_left = if wide {
        (rect.right() - actions_w).max(title_left + 180.0)
    } else {
        rect.right() - number_w
    };
    let title_right = (actions_left - 14.0).max(title_left + 40.0);
    let title_clip = egui::Rect::from_min_max(
        egui::pos2(title_left, rect.top()),
        egui::pos2(title_right, rect.bottom()),
    );
    let title_painter = ui.painter().with_clip_rect(title_clip);
    let title_galley = ui.painter().layout_no_wrap(
        pr.title.clone(),
        egui::FontId::new(
            DETAIL_HEADER_TITLE_SIZE,
            crate::theme::medium_family(ui.ctx()),
        ),
        palette.text_primary,
    );
    title_painter.galley(
        egui::pos2(title_left, rect.top() + 8.0),
        title_galley,
        palette.text_primary,
    );
    let subtitle = format!("{} · {} → {}", pr.author, pr.source_branch, pr.dest_branch);
    let subtitle_galley = ui.painter().layout_no_wrap(
        subtitle.clone(),
        egui::FontId::proportional(DETAIL_HEADER_SUBTITLE_SIZE),
        palette.text_secondary,
    );
    title_painter.galley(
        egui::pos2(title_left, rect.top() + 30.0),
        subtitle_galley,
        palette.text_secondary,
    );
    detail_label_accessibility(ui, title_clip, "pr_detail_header_title", pr.title.clone());
    detail_label_accessibility(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(title_left, rect.top() + 28.0),
            egui::pos2(title_right, rect.top() + 46.0),
        ),
        "pr_detail_header_subtitle",
        subtitle,
    );

    if wide {
        let y = rect.top() + (DETAIL_HEADER_WIDE_HEIGHT - DETAIL_ACTION_HEIGHT) / 2.0;
        let mut x = rect.right() - actions_w;
        if detail_action_button(
            ui,
            palette,
            egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(DETAIL_ACTION_OPEN_WIDTH, DETAIL_ACTION_HEIGHT),
            ),
            Icon::ExternalLink,
            "Open in browser",
            "pr_detail_open_browser",
        ) {
            action.open_url = Some(pr.url.clone());
        }
        x += DETAIL_ACTION_OPEN_WIDTH + DETAIL_HEADER_GAP;
        if detail_action_button(
            ui,
            palette,
            egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(DETAIL_ACTION_CHECKOUT_WIDTH, DETAIL_ACTION_HEIGHT),
            ),
            Icon::GitBranch,
            "Checkout",
            "pr_detail_checkout",
        ) {
            action.checkout = true;
        }
        x += DETAIL_ACTION_CHECKOUT_WIDTH + DETAIL_HEADER_GAP;
        detail_number_chip(ui, palette, egui::pos2(x, y + 2.0), number, number_galley);
    } else {
        detail_number_chip(
            ui,
            palette,
            egui::pos2(rect.right() - number_w, rect.top() + 9.0),
            number,
            number_galley,
        );
        let y = rect.top() + 58.0;
        if detail_action_button(
            ui,
            palette,
            egui::Rect::from_min_size(
                egui::pos2(title_left, y),
                egui::vec2(DETAIL_ACTION_OPEN_WIDTH, DETAIL_ACTION_HEIGHT),
            ),
            Icon::ExternalLink,
            "Open in browser",
            "pr_detail_open_browser",
        ) {
            action.open_url = Some(pr.url.clone());
        }
        if detail_action_button(
            ui,
            palette,
            egui::Rect::from_min_size(
                egui::pos2(title_left + DETAIL_ACTION_OPEN_WIDTH + DETAIL_HEADER_GAP, y),
                egui::vec2(DETAIL_ACTION_CHECKOUT_WIDTH, DETAIL_ACTION_HEIGHT),
            ),
            Icon::GitBranch,
            "Checkout",
            "pr_detail_checkout",
        ) {
            action.checkout = true;
        }
    }

    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.bottom()),
            egui::pos2(rect.right(), rect.bottom()),
        ],
        egui::Stroke::new(1.0, palette.border_subtle),
    );
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

fn detail_action_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    icon: Icon,
    label: &'static str,
    id: &'static str,
) -> bool {
    let response = ui.interact(rect, ui.id().with(id), egui::Sense::click());
    let fill = if response.hovered() {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(RADIUS_BUTTON),
        fill,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let icon_center = egui::pos2(rect.left() + 13.0, rect.center().y);
    paint_icon(
        painter,
        icon_center,
        STATUS_ICON,
        icon,
        palette.text_secondary,
    );
    let label_left = rect.left() + 26.0;
    let label_clip = egui::Rect::from_min_max(
        egui::pos2(label_left, rect.top()),
        egui::pos2(rect.right() - 8.0, rect.bottom()),
    );
    let label_painter = painter.with_clip_rect(label_clip);
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        palette.text_primary,
    );
    label_painter.galley(
        egui::pos2(label_left, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text_primary,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.to_owned())
    });
    response.clicked()
}

fn detail_number_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    pos: egui::Pos2,
    label: String,
    galley: std::sync::Arc<egui::Galley>,
) {
    let size = galley.size() + egui::vec2(18.0, 8.0);
    let rect = egui::Rect::from_min_size(pos, size);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_PILL),
        palette.bg_canvas,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
    detail_label_accessibility(ui, rect, "pr_detail_number", label);
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
        .filter_map(|(idx, file)| (!unread_only || !viewed.contains(&file.path)).then_some(idx))
        .collect();
    if visible.is_empty() {
        ui.label(muted(palette, "All files viewed"));
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

    let max_width = ui.available_width();
    let new_job = |indent: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = (max_width - indent).max(80.0);
        job
    };
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
        } else {
            egui::FontFamily::Proportional
        };
        let mut color = if s.strong || heading.is_some() {
            palette.text_primary
        } else {
            palette.text_secondary
        };
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
                egui::Stroke::new(1.0, palette.accent)
            } else {
                egui::Stroke::NONE
            },
            strikethrough: if s.strike {
                egui::Stroke::new(1.0, color)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        };
        job.append(run, 0.0, format);
    };
    let flush = |ui: &mut egui::Ui, job: &mut egui::text::LayoutJob, indent: f32| {
        if job.text.trim().is_empty() {
            *job = new_job(indent);
            return;
        }
        let done = std::mem::replace(job, new_job(indent));
        if indent > 0.0 {
            // A horizontal layout defaults its labels to no-wrap (Extend), so a long
            // list item would run off the column and drag the parent's width with it;
            // force Wrap so the item folds within the reading column.
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                ui.add(egui::Label::new(done).wrap());
            });
        } else {
            ui.label(done);
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

    let indent_now = |list: &[Option<u64>], quote: usize| {
        list.len() as f32 * MD_LIST_INDENT + quote as f32 * MD_QUOTE_INDENT
    };

    for event in Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level),
            Event::Start(Tag::Strong) => style.strong = true,
            Event::Start(Tag::Emphasis) => style.emphasis = true,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::Start(Tag::Link { .. }) => style.link = true,
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
            Event::End(TagEnd::Heading(_)) => {
                flush(ui, &mut job, indent_now(&list_stack, quote_depth));
                heading = None;
            }
            Event::End(TagEnd::Paragraph) => {
                if list_stack.is_empty() {
                    flush(ui, &mut job, quote_depth as f32 * MD_QUOTE_INDENT);
                }
            }
            Event::End(TagEnd::Item) => flush(ui, &mut job, indent_now(&list_stack, quote_depth)),
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
            Event::End(TagEnd::Link) => style.link = false,
            Event::Text(t) => {
                if in_code_block {
                    code_block.push_str(&t);
                } else {
                    append(&mut job, &t, style, heading, quote_depth > 0);
                }
            }
            Event::Code(t) => {
                let mut s = style;
                s.code = true;
                append(&mut job, &t, s, heading, quote_depth > 0);
            }
            Event::SoftBreak => append(&mut job, " ", style, heading, quote_depth > 0),
            Event::HardBreak => append(&mut job, "\n", style, heading, quote_depth > 0),
            _ => {}
        }
    }
    flush(ui, &mut job, quote_depth as f32 * MD_QUOTE_INDENT);
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
fn review_meta(ui: &mut egui::Ui, palette: &Palette, review: &PrReviewView<'_>) {
    let pr = review.pr;
    let created = review
        .detail
        .map(|d| crate::pull_requests::model::relative_age(&d.created_at, now_epoch_secs()))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        author_avatar(ui, palette, &pr.author);
        ui.add_space(AVATAR_GAP);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                egui::RichText::new(&pr.author)
                    .size(AUTHOR_NAME_SIZE)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new(format!("{} → {}", pr.source_branch, pr.dest_branch))
                    .size(META_SIZE)
                    .color(palette.text_secondary),
            );
        });
        if !created.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("Created {created}"))
                        .size(META_SIZE)
                        .color(palette.text_muted),
                );
            });
        }
    });

    meta_row(ui, palette, pr);

    let body = review.detail.map(|d| d.body.trim()).unwrap_or("");
    if !body.is_empty() {
        ui.add_space(10.0);
        egui::Frame::new()
            .fill(palette.bg_surface)
            .stroke(egui::Stroke::new(1.0, palette.border_subtle))
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
fn meta_row(ui: &mut egui::Ui, palette: &Palette, pr: &PullRequest) {
    if pr.reviewers.is_empty() && pr.labels.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if !pr.reviewers.is_empty() {
            ui.label(
                egui::RichText::new("Reviewers")
                    .size(META_SIZE)
                    .color(palette.text_muted),
            );
            let shown = pr.reviewers.len().min(REVIEWER_MAX);
            let step = REVIEWER_AVATAR - REVIEWER_OVERLAP;
            let mut width = REVIEWER_AVATAR + step * shown.saturating_sub(1) as f32;
            if pr.reviewers.len() > shown {
                width += 22.0;
            }
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, REVIEWER_AVATAR), egui::Sense::hover());
            reviewer_stack(ui, palette, &pr.reviewers, rect.x_range(), rect.center().y);
        }
        for label in &pr.labels {
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
        egui::Stroke::new(1.0, palette.border_subtle),
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

/// The commit band atop the review rail (per-commit diff: T5): an "All commits" row —
/// the cumulative three-dot diff — then one row per commit. The selected row drives the
/// rail's files and the center diff over `commit^..commit`.
fn commits_band(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Commits")
                .size(SECTION_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(2.0);
        count_chip(ui, palette, review.commits.len());
    });
    ui.add_space(6.0);
    if commit_row(
        ui,
        palette,
        "All commits",
        None,
        review.selected_commit.is_none(),
    ) {
        action.select_commit = Some(CommitSelection::All);
    }
    for commit in review.commits {
        let selected = review.selected_commit == Some(commit.sha.as_str());
        if commit_row(ui, palette, &commit.subject, Some(&commit.short), selected) {
            action.select_commit = Some(CommitSelection::Commit(commit.sha.clone()));
        }
    }
}

/// One commit band row: a full-width clickable line with the hover/selection fill of
/// the file rows, an optional monospace short sha, then the (elided) subject.
fn commit_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    subject: &str,
    short: Option<&str>,
    selected: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
    if let Some(fill) = file_list::file_row_fill(palette, response.hovered(), selected) {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    }
    let center_y = rect.center().y;
    let mut text_left = rect.left() + 8.0;
    if let Some(short) = short {
        let galley = ui.painter().layout_no_wrap(
            short.to_owned(),
            egui::FontId::monospace(11.5),
            palette.text_secondary,
        );
        let advance = galley.size().x;
        ui.painter().galley(
            egui::pos2(text_left, center_y - galley.size().y / 2.0),
            galley,
            palette.text_secondary,
        );
        text_left += advance + 8.0;
    }
    let color = if selected {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let subject_max = (rect.right() - 8.0 - text_left).max(8.0);
    let mut job = egui::text::LayoutJob::single_section(
        subject.to_owned(),
        egui::text::TextFormat::simple(egui::FontId::proportional(12.5), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(subject_max);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(text_left, center_y - galley.size().y / 2.0),
        galley,
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, subject));
    response.clicked()
}

/// "Files changed" band (commit-detail's `files_header`, sans the flat/tree
/// toggle): the title + a count chip, with the ±totals and ratio bar pinned right.
fn files_band(
    ui: &mut egui::Ui,
    palette: &Palette,
    files: &[CommitFile],
    view: FileViewMode,
    unread_count: usize,
    unread_only: &mut bool,
) -> Option<FileViewMode> {
    let additions: usize = files.iter().map(|f| f.additions).sum();
    let deletions: usize = files.iter().map(|f| f.deletions).sum();
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
        unread_filter_chip(ui, palette, unread_count, unread_only);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            set_view = file_list::view_toggle(ui, palette, view);
            ui.add_space(8.0);
            ratio_bar(ui, palette, additions, deletions);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("−{deletions}"))
                    .size(TOTALS_SIZE)
                    .color(palette.git_deleted),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("+{additions}"))
                    .size(TOTALS_SIZE)
                    .color(palette.git_added),
            );
        });
    });
    set_view
}

fn unread_filter_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    unread_count: usize,
    unread_only: &mut bool,
) {
    let count_font = egui::FontId::proportional(11.0);
    let count_text = unread_count.to_string();
    let count_galley =
        ui.painter()
            .layout_no_wrap(count_text.clone(), count_font, egui::Color32::PLACEHOLDER);
    let icon_w = 12.0;
    let gap = 5.0;
    let size = egui::vec2(9.0 + icon_w + gap + count_galley.size().x + 9.0, 24.0);
    let enabled = unread_count > 0 || *unread_only;
    let (rect, response, hovered) = clickable(ui, size, enabled);
    let selected = *unread_only;
    let fill = if selected {
        palette.accent_subtle
    } else if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    let stroke = if selected {
        egui::Stroke::new(1.0, with_alpha(palette.accent, 150))
    } else {
        egui::Stroke::new(1.0, palette.border_subtle)
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
    paint_icon(ui.painter(), icon_center, icon_w, Icon::EyeOff, content);
    ui.painter().galley(
        egui::pos2(
            icon_center.x + icon_w / 2.0 + gap,
            center_y - count_galley.size().y / 2.0,
        ),
        count_galley,
        content,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, "Unread only")
    });
    if response.clicked() {
        *unread_only = !*unread_only;
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

/// Fixed height the review composer reserves at the foot of the rail; an extra
/// row when a post error needs surfacing.
fn composer_height(review: &PrReviewView<'_>) -> f32 {
    let base = 184.0;
    if review.post_error.is_some() {
        base + 22.0
    } else {
        base
    }
}

/// The "Finish review" composer pinned to the foot of the rail (pull-requests.md
/// §11): a verdict selector, a summary field and the Submit button. Posts the
/// accumulated draft line comments together with the chosen verdict + summary.
fn review_composer(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, palette.border_subtle),
    );
    let inner = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(rect.left() + PANEL_PAD_X, rect.right() - PANEL_PAD_X),
        egui::Rangef::new(rect.top() + PANEL_PAD_Y, rect.bottom() - PANEL_PAD_Y),
    );
    let mut panel = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    panel.set_width(inner.width());
    panel.label(
        egui::RichText::new("FINISH REVIEW")
            .size(HEADER_SIZE)
            .color(palette.text_muted),
    );
    panel.add_space(6.0);

    segmented_verdict(&mut panel, palette, review.verdict, inner.width());
    panel.add_space(6.0);

    panel.add(
        egui::TextEdit::multiline(&mut *review.summary)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .hint_text("Summary (optional)"),
    );
    panel.add_space(6.0);

    if let Some(error) = review.post_error {
        panel.label(
            egui::RichText::new(error)
                .size(12.0)
                .color(palette.git_deleted),
        );
        panel.add_space(4.0);
    }

    let count = crate::review::count(review.draft);
    let empty_comment_review =
        *review.verdict == ReviewVerdict::Comment && count == 0 && review.summary.trim().is_empty();
    let label = submit_review_label(*review.verdict, count, review.posting, empty_comment_review);
    let enabled = !review.posting && !empty_comment_review;
    let (rect, response, hovered) = clickable(&mut panel, egui::vec2(inner.width(), 32.0), enabled);
    let fill = if !enabled {
        palette.state_disabled
    } else if hovered {
        palette.accent_hover
    } else {
        palette.accent
    };
    panel
        .painter()
        .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    panel.painter().text(
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

/// Single framed segmented control for the review verdict. Active segments use a
/// subtle tint and colored label; only Request changes goes red when active.
fn segmented_verdict(
    ui: &mut egui::Ui,
    palette: &Palette,
    verdict: &mut ReviewVerdict,
    width: f32,
) {
    let height = 28.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_BUTTON),
        palette.bg_surface,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let segment_w = rect.width() / 3.0;
    let items = [
        (ReviewVerdict::Comment, "Comment", palette.accent),
        (ReviewVerdict::Approve, "Approve", palette.git_added),
        (
            ReviewVerdict::RequestChanges,
            "Request changes",
            palette.git_deleted,
        ),
    ];
    for (idx, (target, label, color)) in items.into_iter().enumerate() {
        let left = rect.left() + segment_w * idx as f32;
        let right = if idx == 2 {
            rect.right()
        } else {
            left + segment_w
        };
        let segment = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(right, rect.bottom()),
        )
        .shrink(1.0);
        if verdict_segment(ui, palette, segment, label, *verdict == target, color) {
            *verdict = target;
        }
        if idx > 0 {
            ui.painter().vline(
                left,
                egui::Rangef::new(rect.top() + 5.0, rect.bottom() - 5.0),
                egui::Stroke::new(1.0, palette.border_subtle),
            );
        }
    }
}

fn verdict_segment(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    label: &str,
    selected: bool,
    color: egui::Color32,
) -> bool {
    let response = ui
        .interact(
            rect,
            ui.id().with(("review_verdict", label)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    if selected || hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(RADIUS_BUTTON),
            if selected {
                with_alpha(color, 36)
            } else {
                palette.bg_surface_hover
            },
        );
    };
    let text_color = if selected {
        color
    } else {
        palette.text_secondary
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.clicked()
}

fn review_diff(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let mut area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if let Some(diff) = review.diff {
        let mut throwaway: Vec<crate::ui::git_panel::GitIntent> = Vec::new();
        let mut review_out: Vec<ReviewIntent> = Vec::new();
        let closed = crate::ui::diff_view::diff_view(
            &mut area,
            palette,
            diff,
            DiffSurface::PrReview,
            review.diff_view,
            &mut throwaway,
            Some(&mut DiffReview {
                comments: review.agent_notes,
                forge: Some(review.draft),
                existing: review.existing,
                agent: review.agent,
                intents: &mut review_out,
            }),
        );
        action.review_intents.append(&mut review_out);
        if closed {
            action.close_file = true;
        }
        return;
    }
    // No file selected: the center hosts the PR detail rather than a placeholder.
    if review.selected_file.is_none() {
        review_detail(&mut area, palette, review, rect, action);
        return;
    }
    let message = if let Some(error) = review.diff_error {
        error
    } else if review.diff_loading {
        "Loading diff…"
    } else {
        "No textual changes"
    };
    area.add_space(rect.height() / 2.0 - 20.0);
    area.vertical_centered(|ui| {
        ui.label(muted(palette, message));
    });
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
            if active { 2.0 } else { 1.0 },
            if active {
                palette.accent
            } else {
                palette.border_subtle
            },
        ),
    );
    if resp.dragged() {
        // The rail sits on the right, so dragging the split left (negative delta)
        // widens it.
        let max = (rect.width() - DIFF_MIN_WIDTH).max(RAIL_MIN_WIDTH);
        action.set_detail_width =
            Some((rail_width - resp.drag_delta().x).clamp(RAIL_MIN_WIDTH, max));
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
    let indices = |role: PrRole| -> Vec<usize> {
        prs.iter()
            .enumerate()
            .filter(|(_, p)| p.role == role)
            .map(|(i, _)| i)
            .collect()
    };
    let to_review = indices(PrRole::ToReview);
    let mine = indices(PrRole::Mine);

    let header_rect =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PAGE_HEADER_HEIGHT));
    page_header(ui, palette, header_rect, hints.loading, action);

    let body_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top() + PAGE_HEADER_HEIGHT, rect.bottom()),
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
                .inner_margin(egui::Margin::symmetric(PANEL_PAD_X as i8, 8))
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
                    group(
                        ui,
                        palette,
                        "To review",
                        PrRole::ToReview,
                        &to_review,
                        prs,
                        selected,
                        action,
                    );
                    group(
                        ui,
                        palette,
                        "Mine",
                        PrRole::Mine,
                        &mine,
                        prs,
                        selected,
                        action,
                    );
                });
        });
}

/// Browse-list header (pull-requests.md §5): the `Pull Requests` title and a
/// **Refresh** button — no global search / notification / theme chrome.
fn page_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    loading: bool,
    action: &mut PullRequestsPageAction,
) {
    let center_y = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + PANEL_PAD_X, center_y),
        egui::Align2::LEFT_CENTER,
        "Pull Requests",
        egui::FontId::new(PAGE_TITLE_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_primary,
    );

    let label = "Refresh";
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(META_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_secondary,
    );
    let icon_w = STATUS_ICON;
    let pad = 10.0;
    let w = galley.size().x + icon_w + 6.0 + 2.0 * pad;
    let h = DETAIL_ACTION_HEIGHT;
    let btn = egui::Rect::from_min_size(
        egui::pos2(rect.right() - PANEL_PAD_X - w, center_y - h / 2.0),
        egui::vec2(w, h),
    );
    let response = ui.interact(btn, ui.id().with("pr_refresh"), egui::Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(btn, RADIUS_BUTTON, palette.bg_surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else {
        ui.painter()
            .rect_filled(btn, RADIUS_BUTTON, palette.bg_surface);
    }
    let icon_center = egui::pos2(btn.left() + pad + icon_w / 2.0, center_y);
    if loading {
        Spinner::new()
            .size(icon_w)
            .color(palette.text_secondary)
            .paint_at(
                ui,
                egui::Rect::from_center_size(icon_center, egui::vec2(icon_w, icon_w)),
            );
    } else {
        paint_icon(
            ui.painter(),
            icon_center,
            icon_w,
            Icon::RefreshCw,
            palette.text_secondary,
        );
    }
    ui.painter().galley(
        egui::pos2(
            btn.left() + pad + icon_w + 6.0,
            center_y - galley.size().y / 2.0,
        ),
        galley,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if response.clicked() {
        action.refresh = true;
    }
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
    let message = if hints.no_repos {
        "No GitHub or Bitbucket repository in your workspace"
    } else {
        "No pull requests"
    };
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(message)
                .size(13.0)
                .color(palette.text_muted),
        );
    });
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

#[allow(clippy::too_many_arguments)]
fn group(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    role: PrRole,
    indices: &[usize],
    prs: &[PullRequest],
    selected: Option<usize>,
    action: &mut PullRequestsPageAction,
) {
    if indices.is_empty() {
        return;
    }
    group_header(ui, palette, title, indices.len());
    let backdrop = ui.painter().add(egui::Shape::Noop);
    let card = ui
        .scope(|ui| {
            column_header(ui, palette, role);
            for (row, &idx) in indices.iter().enumerate() {
                pr_row(
                    ui,
                    palette,
                    &prs[idx],
                    idx,
                    role,
                    selected == Some(idx),
                    row > 0,
                    action,
                );
            }
        })
        .response
        .rect;
    ui.painter().set(
        backdrop,
        egui::Shape::rect_filled(
            card,
            egui::CornerRadius::same(CARD_RADIUS),
            palette.bg_surface,
        ),
    );
    ui.painter().rect_stroke(
        card,
        egui::CornerRadius::same(CARD_RADIUS),
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.add_space(16.0);
}

/// The table's column-label row under a group header (pull-requests.md §5). The
/// columns differ by role: **Mine** drops the Author column (every row is me).
fn column_header(ui: &mut egui::Ui, palette: &Palette, role: PrRole) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), COL_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    let cols = columns(rect, role);
    let center_y = rect.center().y;
    let font = egui::FontId::proportional(HEADER_SIZE);
    let label = |left: f32, text: &str| {
        ui.painter().text(
            egui::pos2(left, center_y),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            palette.text_muted,
        );
    };
    label(rect.left() + PAD_X, "Title");
    label(cols.project.min, "Project");
    if let Some(author) = cols.author {
        label(author.min, "Author");
    }
    label(
        cols.reviewers.min,
        if role == PrRole::Mine {
            "Reviewers"
        } else {
            "Reviewer"
        },
    );
    label(cols.status.min, "Status");
    label(cols.updated.min, "Updated");
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.border_subtle),
    );
}

fn group_header(ui: &mut egui::Ui, palette: &Palette, title: &str, count: usize) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), GROUP_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    let center_y = rect.center().y;
    let label = ui.painter().layout_no_wrap(
        title.to_owned(),
        egui::FontId::new(HEADER_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_secondary,
    );
    let label_right = rect.left() + PAD_X + label.size().x;
    ui.painter().galley(
        egui::pos2(rect.left() + PAD_X, center_y - label.size().y / 2.0),
        label,
        palette.text_secondary,
    );
    count_pill(ui, palette, count, label_right + 8.0, center_y);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title.to_owned()));
}

fn count_pill(ui: &egui::Ui, palette: &Palette, count: usize, left: f32, center_y: f32) {
    let galley = ui.painter().layout_no_wrap(
        count.to_string(),
        egui::FontId::proportional(COUNT_BADGE_SIZE),
        palette.lane_node_text,
    );
    let h = COUNT_BADGE_SIZE + 5.0;
    let w = (galley.size().x + 10.0).max(h);
    let pill = egui::Rect::from_min_size(egui::pos2(left, center_y - h / 2.0), egui::vec2(w, h));
    ui.painter()
        .rect_filled(pill, h / 2.0, palette.bg_surface_hover);
    ui.painter().galley(
        pill.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
}

/// Per-role column x-ranges for the table (pull-requests.md §5). Fixed columns are
/// taken from the right edge; Title fills the rest. `author` is present only for the
/// **To review** group (every **Mine** row's author is the user).
struct Cols {
    title: egui::Rangef,
    project: egui::Rangef,
    author: Option<egui::Rangef>,
    reviewers: egui::Rangef,
    status: egui::Rangef,
    updated: egui::Rangef,
    chevron: egui::Rangef,
}

fn columns(rect: egui::Rect, role: PrRole) -> Cols {
    let title_left = rect.left() + PAD_X + STATE_ICON + 8.0;
    let right_edge = rect.right() - PAD_X;

    let author_w = matches!(role, PrRole::ToReview).then_some(COL_AUTHOR_W);
    let n_gaps = (5 + author_w.is_some() as usize) as f32;
    let fixed_sum = COL_PROJECT_W
        + author_w.unwrap_or(0.0)
        + COL_REVIEWERS_W
        + COL_STATUS_W
        + COL_UPDATED_W
        + COL_CHEVRON_W;

    let avail = right_edge - title_left;
    let title_w = (avail - fixed_sum - n_gaps * COL_GAP).clamp(TITLE_MIN_W, TITLE_MAX_W);
    let gap = ((avail - title_w - fixed_sum) / n_gaps).max(COL_GAP);

    let mut x = title_left + title_w;
    let mut next = |w: f32| {
        x += gap;
        let range = egui::Rangef::new(x, x + w);
        x += w;
        range
    };
    let project = next(COL_PROJECT_W);
    let author = author_w.map(&mut next);
    let reviewers = next(COL_REVIEWERS_W);
    let status = next(COL_STATUS_W);
    let updated = next(COL_UPDATED_W);
    let chevron = next(COL_CHEVRON_W);

    Cols {
        title: egui::Rangef::new(title_left, title_left + title_w),
        project,
        author,
        reviewers,
        status,
        updated,
        chevron,
    }
}

#[allow(clippy::too_many_arguments)]
fn pr_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    idx: usize,
    role: PrRole,
    selected: bool,
    divider: bool,
    action: &mut PullRequestsPageAction,
) {
    let (rect, response, hovered) =
        clickable(ui, egui::vec2(ui.available_width(), ROW_HEIGHT), true);
    if selected {
        ui.painter()
            .rect_filled(rect, 0, crate::ui::with_alpha(palette.accent, 28));
    } else if hovered {
        ui.painter().rect_filled(rect, 0, palette.bg_surface_hover);
    }
    if divider && !selected {
        ui.painter().hline(
            rect.x_range(),
            rect.top() - 0.5,
            egui::Stroke::new(1.0, palette.border_subtle),
        );
    }
    let cols = columns(rect, role);
    let center_y = rect.center().y;

    // Title cell: state icon, the title on the upper line, the branch beneath.
    let title_y = rect.top() + ROW_HEIGHT * 0.38;
    let branch_y = rect.top() + ROW_HEIGHT * 0.66;
    let (state_icon, state_color) = match pr.state {
        PrState::Open => (Icon::GitPullRequest, palette.git_added),
        PrState::Draft => (Icon::GitPullRequestDraft, palette.text_muted),
    };
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + PAD_X + STATE_ICON / 2.0, title_y),
        STATE_ICON,
        state_icon,
        state_color,
    );
    cell_text(
        ui,
        &pr.title,
        egui::FontId::new(TITLE_SIZE, crate::theme::medium_family(ui.ctx())),
        palette.text_primary,
        cols.title.min,
        title_y,
        cols.title.span(),
    );
    paint_icon(
        ui.painter(),
        egui::pos2(cols.title.min + CHIP_SIZE / 2.0, branch_y),
        CHIP_SIZE,
        Icon::GitBranch,
        palette.text_muted,
    );
    cell_text(
        ui,
        &pr.source_branch,
        egui::FontId::proportional(CHIP_SIZE),
        palette.text_muted,
        cols.title.min + CHIP_SIZE + 5.0,
        branch_y,
        cols.title.span() - CHIP_SIZE - 5.0,
    );

    // Project: a quiet repo glyph + label.
    paint_icon(
        ui.painter(),
        egui::pos2(cols.project.min + CHIP_SIZE / 2.0, center_y),
        CHIP_SIZE,
        Icon::Folder,
        palette.text_muted,
    );
    cell_text(
        ui,
        &pr.repo_label,
        egui::FontId::proportional(CHIP_SIZE),
        palette.text_secondary,
        cols.project.min + CHIP_SIZE + 5.0,
        center_y,
        cols.project.span() - CHIP_SIZE - 5.0,
    );

    // Author (To review only): avatar + name.
    if let Some(author) = cols.author {
        paint_avatar(
            ui.painter(),
            palette,
            &pr.author,
            egui::pos2(author.min + REVIEWER_AVATAR / 2.0, center_y),
            REVIEWER_AVATAR,
            None,
        );
        cell_text(
            ui,
            &pr.author,
            egui::FontId::proportional(META_SIZE),
            palette.text_secondary,
            author.min + REVIEWER_AVATAR + 7.0,
            center_y,
            author.span() - REVIEWER_AVATAR - 7.0,
        );
    }

    reviewer_stack(ui, palette, &pr.reviewers, cols.reviewers, center_y);

    // Status: a plain colored label encoding the PR's state/review (CI checks
    // live in the review detail, not the list — pull-requests.md §5).
    let (label, color) = pr_status(palette, pr.state, pr.review);
    cell_text(
        ui,
        label,
        egui::FontId::new(CHIP_SIZE, crate::theme::medium_family(ui.ctx())),
        color,
        cols.status.min,
        center_y,
        cols.status.span(),
    );

    let age = crate::pull_requests::model::relative_age(&pr.updated_at, now_epoch_secs());
    let updated = if age.is_empty() { &pr.updated_at } else { &age };
    cell_text(
        ui,
        updated,
        egui::FontId::proportional(META_SIZE),
        palette.text_muted,
        cols.updated.min,
        center_y,
        cols.updated.span(),
    );
    paint_icon(
        ui.painter(),
        egui::pos2(cols.chevron.center(), center_y),
        STATUS_ICON,
        Icon::ChevronRight,
        palette.text_muted,
    );

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, pr.title.clone())
    });
    if response.clicked() {
        action.select = Some(idx);
    }
}

/// Left-aligned, vertically-centered text truncated to a column width.
fn cell_text(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    left: f32,
    center_y: f32,
    max_w: f32,
) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(0.0));
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left, center_y - galley.size().y / 2.0),
        galley,
        color,
    );
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
        painter.circle_stroke(center, r, egui::Stroke::new(1.5, color));
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

/// Collapse `(state, review)` into one status label + color. The CI `checks` stay
/// out of the list (shown only in the review detail), so nothing regresses (§5).
fn pr_status(palette: &Palette, state: PrState, review: Review) -> (&'static str, egui::Color32) {
    match state {
        PrState::Draft => ("Draft", palette.text_muted),
        PrState::Open => match review {
            Review::Approved => ("Approved", palette.git_added),
            Review::ChangesRequested => ("Changes requested", palette.git_deleted),
            Review::Pending => ("In review", palette.git_modified),
            Review::None => ("Open", palette.accent),
        },
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
