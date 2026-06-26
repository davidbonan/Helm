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
use std::collections::HashSet;

use crate::git::commit_detail::CommitFile;
use crate::git::diff::FileDiff;
use crate::git::file_tree::{self, TreeRow};
use crate::pull_requests::model::{
    Checks, PrComment, PrCommit, PrDetail, PrRole, PrState, PullRequest, Review, ReviewVerdict,
};
use crate::review::{FileComments, ForgeThreads, ReviewIntent};
use crate::theme::{Palette, BODY_SIZE, RADIUS_BUTTON, SECTION_TITLE_SIZE};
use crate::ui::detail::{author_avatar, count_chip};
use crate::ui::diff_view::{DiffReview, DiffSurface, DiffViewState};
use crate::ui::file_list::{self, file_row, row_separator, FileRow, FileViewMode};
use crate::ui::git_panel::ratio_bar;
use crate::ui::{clickable, paint_icon, with_alpha, SECTION_TOP_MARGIN, TITLEBAR_HEIGHT};

/// Review-surface split bounds: the changed-files rail and the diff each keep a
/// floor; the persisted rail width is clamped between them.
const RAIL_MIN_WIDTH: f32 = 260.0;
const DIFF_MIN_WIDTH: f32 = 420.0;

/// Reading-width cap for the center PR detail so the body/comments don't stretch
/// into one long line when the rail is collapsed (or on a wide window).
const DETAIL_MAX_WIDTH: f32 = 760.0;
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
}

/// Per-source banners for the browse list (pull-requests.md §5): each forge's
/// one-line unavailability hint (`None` when usable, so the other source still
/// lists), and whether the workspace has no recognized-forge repo at all.
#[derive(Default)]
pub struct PrSourceHints<'a> {
    pub github: Option<&'a str>,
    pub bitbucket: Option<&'a str>,
    pub no_repos: bool,
}

/// Everything the review surface renders for the open PR. The app owns the state;
/// this is the per-frame borrow it hands the view (diff scroll state is `&mut` so
/// the view can record it). Loading/error flags drive the placeholders.
pub struct PrReviewView<'a> {
    pub pr: &'a PullRequest,
    pub detail: Option<&'a PrDetail>,
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
    review: &PrReviewView<'_>,
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
            let full_width = ui.available_width();
            ui.set_width(full_width);
            ui.add_space(PANEL_PAD_Y);
            review_detail_header(ui, palette, review, action);
            ui.set_width(full_width.min(DETAIL_MAX_WIDTH));
            ui.add_space(SECTION_TOP_MARGIN);
            review_meta(ui, palette, review);
            let pr = review.pr;

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

            // Conversation-level comments only; the inline ones (path + line set)
            // are anchored in the diff via ForgeThreads (pull-requests.md §11).
            let conversation: Vec<&PrComment> = review
                .detail
                .map(|d| d.comments.iter().filter(|c| c.path.is_none()).collect())
                .unwrap_or_default();
            if !conversation.is_empty() {
                band_title(ui, palette, "Conversation");
                for c in conversation {
                    ui.label(
                        egui::RichText::new(&c.author)
                            .size(META_SIZE)
                            .color(palette.text_secondary)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(&c.body)
                            .size(12.5)
                            .color(palette.text_secondary),
                    );
                    ui.add_space(6.0);
                }
            }

            inline_comments_section(ui, palette, review, action);
            ui.add_space(PANEL_PAD_Y);
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
    review: &PrReviewView<'_>,
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
    band_title(ui, palette, "Inline comments");
    let mut files: Vec<&str> = Vec::new();
    for c in &inline {
        if let Some(path) = c.path.as_deref() {
            if !files.contains(&path) {
                files.push(path);
            }
        }
    }
    for path in files {
        ui.add_space(4.0);
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
            inline_comment_card(ui, palette, review, path, new, &thread, action);
        }
    }
}

/// One inline thread as a clickable card: the code snippet (GitHub's `diff_hunk`,
/// else a window of the loaded diff for the selected file), then the comments.
fn inline_comment_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    path: &str,
    new: Option<u32>,
    thread: &[&PrComment],
    action: &mut PullRequestsPageAction,
) {
    ui.add_space(6.0);
    let response = egui::Frame::new()
        .fill(palette.bg_surface)
        .stroke(egui::Stroke::new(1.0, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(RADIUS_BUTTON))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let snippet = inline_snippet(review, path, new, thread);
            for line in &snippet {
                ui.label(
                    egui::RichText::new(line)
                        .size(11.5)
                        .monospace()
                        .color(palette.text_muted),
                );
            }
            if !snippet.is_empty() {
                ui.add_space(6.0);
            }
            for c in thread {
                ui.label(
                    egui::RichText::new(&c.author)
                        .size(META_SIZE)
                        .strong()
                        .color(palette.text_secondary),
                );
                ui.label(
                    egui::RichText::new(&c.body)
                        .size(12.5)
                        .color(palette.text_secondary),
                );
            }
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let label = match new {
        Some(line) => format!("Open {path} line {line}"),
        None => format!("Open {path}"),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if response.clicked() {
        if let Some(idx) = review.files.iter().position(|f| f.path == path) {
            action.open_inline_comment = Some((idx, new));
        }
    }
}

/// The few lines of code shown atop an inline-comment card. GitHub carries the hunk
/// on the comment (`diff_hunk`); for Bitbucket (no hunk) fall back to a window of the
/// loaded diff when the comment is on the open file, else nothing (pull-requests.md §5).
fn inline_snippet(
    review: &PrReviewView<'_>,
    path: &str,
    new: Option<u32>,
    thread: &[&PrComment],
) -> Vec<String> {
    if let Some(hunk) = thread.iter().find_map(|c| c.context.as_deref()) {
        let lines: Vec<String> = hunk
            .lines()
            .filter(|l| !l.starts_with("@@"))
            .map(|l| l.strip_prefix(['+', '-', ' ']).unwrap_or(l).to_owned())
            .collect();
        let take = lines.len().min(4);
        return lines[lines.len() - take..].to_vec();
    }
    if let (Some(diff), Some(new)) = (review.diff, new) {
        if diff.path == path && !diff.source_lines.is_empty() {
            let end = (new as usize).min(diff.source_lines.len());
            let start = end.saturating_sub(3);
            return diff.source_lines[start..end].to_vec();
        }
    }
    Vec::new()
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
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        match view {
            FileViewMode::Flat => {
                for (row_idx, idx) in visible.iter().copied().enumerate() {
                    if row_idx > 0 {
                        row_separator(ui, palette);
                    }
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

/// Author block of the rail (commit-detail's `meta_block`): initials avatar +
/// name, the `source → dest` branch flow, then the PR body when present.
fn review_meta(ui: &mut egui::Ui, palette: &Palette, review: &PrReviewView<'_>) {
    let pr = review.pr;
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
    });
    let body = review.detail.map(|d| d.body.trim()).unwrap_or("");
    if !body.is_empty() {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(body)
                .size(BODY_SIZE)
                .color(palette.text_secondary),
        );
    }
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

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("pr_list")
        .show(&mut content, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(8.0);
            if let Some(text) = hints.github {
                hint_banner(ui, palette, "GitHub", text);
            }
            if let Some(text) = hints.bitbucket {
                hint_banner(ui, palette, "Bitbucket", text);
            }
            if prs.is_empty() {
                empty_state(ui, palette, hints);
                return;
            }
            group(ui, palette, "To review", &to_review, prs, selected, action);
            group(ui, palette, "Mine", &mine, prs, selected, action);
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

fn group(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    indices: &[usize],
    prs: &[PullRequest],
    selected: Option<usize>,
    action: &mut PullRequestsPageAction,
) {
    if indices.is_empty() {
        return;
    }
    group_header(ui, palette, title, indices.len());
    for &idx in indices {
        pr_row(ui, palette, &prs[idx], idx, selected == Some(idx), action);
    }
    ui.add_space(8.0);
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

fn pr_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    idx: usize,
    selected: bool,
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

    let (state_icon, state_color) = match pr.state {
        PrState::Open => (Icon::GitPullRequest, palette.git_added),
        PrState::Draft => (Icon::GitPullRequestDraft, palette.text_muted),
    };
    let icon_x = rect.left() + PAD_X + STATE_ICON / 2.0;
    let title_y = rect.top() + ROW_HEIGHT * 0.34;
    paint_icon(
        ui.painter(),
        egui::pos2(icon_x, title_y),
        STATE_ICON,
        state_icon,
        state_color,
    );

    // Status cluster on the right of the title line; the title truncates before it.
    let mut status_x = rect.right() - PAD_X;
    for (icon, color) in [
        review_status(palette, pr.review),
        checks_status(palette, pr.checks),
    ]
    .into_iter()
    .flatten()
    {
        status_x -= STATUS_ICON;
        paint_icon(
            ui.painter(),
            egui::pos2(status_x + STATUS_ICON / 2.0, title_y),
            STATUS_ICON,
            icon,
            color,
        );
        status_x -= 8.0;
    }

    let title_left = rect.left() + PAD_X + STATE_ICON + 8.0;
    let title_max = (status_x - 8.0 - title_left).max(0.0);
    let mut job = egui::text::LayoutJob::single_section(
        pr.title.clone(),
        egui::TextFormat::simple(
            egui::FontId::new(TITLE_SIZE, crate::theme::medium_family(ui.ctx())),
            palette.text_primary,
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(title_max);
    let title = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(title_left, title_y - title.size().y / 2.0),
        title,
        palette.text_primary,
    );

    // Meta line: repo chip, branch chip, then author · age.
    let meta_y = rect.top() + ROW_HEIGHT * 0.7;
    let mut x = title_left;
    x = chip(ui, palette, &pr.repo_label, None, x, meta_y);
    x = chip(
        ui,
        palette,
        &pr.source_branch,
        Some(Icon::GitBranch),
        x,
        meta_y,
    );
    let age = if pr.updated_at.is_empty() {
        pr.author.clone()
    } else {
        format!("{} · {}", pr.author, pr.updated_at)
    };
    ui.painter().text(
        egui::pos2(x + 2.0, meta_y),
        egui::Align2::LEFT_CENTER,
        age,
        egui::FontId::proportional(META_SIZE),
        palette.text_muted,
    );

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, pr.title.clone())
    });
    if response.clicked() {
        action.select = Some(idx);
    }
}

/// A rounded tag with optional leading icon; returns the x just past it.
fn chip(
    ui: &egui::Ui,
    palette: &Palette,
    text: &str,
    icon: Option<Icon>,
    left: f32,
    center_y: f32,
) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(CHIP_SIZE),
        palette.text_secondary,
    );
    let icon_w = if icon.is_some() { CHIP_SIZE + 4.0 } else { 0.0 };
    let pad = 7.0;
    let w = galley.size().x + icon_w + 2.0 * pad;
    let h = CHIP_SIZE + 7.0;
    let rect = egui::Rect::from_min_size(egui::pos2(left, center_y - h / 2.0), egui::vec2(w, h));
    ui.painter().rect_filled(rect, 5.0, palette.bg_surface);
    let mut text_left = rect.left() + pad;
    if let Some(icon) = icon {
        paint_icon(
            ui.painter(),
            egui::pos2(text_left + CHIP_SIZE / 2.0, center_y),
            CHIP_SIZE,
            icon,
            palette.text_muted,
        );
        text_left += icon_w;
    }
    ui.painter().galley(
        egui::pos2(text_left, center_y - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );
    rect.right() + 6.0
}

fn checks_status(palette: &Palette, checks: Checks) -> Option<(Icon, egui::Color32)> {
    match checks {
        Checks::Passing => Some((Icon::Check, palette.git_added)),
        Checks::Failing => Some((Icon::X, palette.git_deleted)),
        Checks::Pending => Some((Icon::Clock, palette.text_muted)),
        Checks::None => None,
    }
}

fn review_status(palette: &Palette, review: Review) -> Option<(Icon, egui::Color32)> {
    match review {
        Review::Approved => Some((Icon::CheckCheck, palette.git_added)),
        Review::ChangesRequested => Some((Icon::CircleX, palette.git_deleted)),
        Review::Pending => Some((Icon::Eye, palette.text_muted)),
        Review::None => None,
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
