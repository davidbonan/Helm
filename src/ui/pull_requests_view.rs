//! Rendering for the Pull Requests cockpit (pull-requests.md §5/§11). Two states:
//! a **browse** list of the workspace PRs grouped **To review** then **Mine**, and
//! — once a row is opened — a **review** surface: the center area holds the open
//! file's read-only diff, or the PR detail (Back + title, author, body, checks,
//! conversation) when no file is selected; a changed-files rail on the right
//! carries the Open / Checkout actions, the file list and the composer (so
//! collapsing the rail from the title bar hides the whole apparatus). Pure
//! `fn(&mut egui::Ui, …)`: the app owns the cache, the selection, the fetched
//! detail/diff and the persisted rail width, and consumes the returned intents.

use lucide_icons::Icon;

use crate::git::commit_detail::CommitFile;
use crate::git::diff::FileDiff;
use crate::pull_requests::model::{
    Checks, PrComment, PrDetail, PrRole, PrState, PullRequest, Review, ReviewVerdict,
};
use crate::review::{FileComments, ForgeThreads, ReviewIntent};
use crate::theme::{Palette, BODY_SIZE, RADIUS_BUTTON, SECTION_TITLE_SIZE};
use crate::ui::detail::{author_avatar, count_chip};
use crate::ui::diff_view::{DiffReview, DiffSurface, DiffViewState};
use crate::ui::file_list::{file_row, row_separator, FileRow};
use crate::ui::git_panel::ratio_bar;
use crate::ui::{clickable, paint_icon, SECTION_TOP_MARGIN, TITLEBAR_HEIGHT};

/// Review-surface split bounds: the changed-files rail and the diff each keep a
/// floor; the persisted rail width is clamped between them.
const RAIL_MIN_WIDTH: f32 = 260.0;
const DIFF_MIN_WIDTH: f32 = 420.0;

/// Reading-width cap for the center PR detail so the body/comments don't stretch
/// into one long line when the rail is collapsed (or on a wide window).
const DETAIL_MAX_WIDTH: f32 = 760.0;

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
    /// The rail/diff split was dragged: the app stores and persists the new width.
    pub set_detail_width: Option<f32>,
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
/// file's read-only diff, or the PR detail (a Back control + the PR title heading
/// the author/body/checks/conversation) when no file is selected; a changed-files
/// rail on the **right** — the commit-detail sidebar's place — carries the Open in
/// browser / Checkout actions, the file list and the composer. The title-bar
/// toggle collapses the rail, leaving the center full-width.
fn render_review(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    rail_width: f32,
    rail_collapsed: bool,
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
    review_rail(ui, palette, review, rail_rect, action);

    rail_resize_handle(ui, palette, split_x, rect, rail_width, action);
}

/// The rail's PR actions, stacked full-width so collapsing the rail hides them:
/// Open in browser and Checkout. The Back control and the PR title head the
/// center detail instead.
fn review_actions(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    action: &mut PullRequestsPageAction,
) {
    let pr = review.pr;
    let w = ui.available_width();
    if action_button(ui, palette, w, Icon::ExternalLink, "Open in browser") {
        action.open_url = Some(pr.url.clone());
    }
    ui.add_space(6.0);
    if action_button(ui, palette, w, Icon::GitBranch, "Checkout") {
        action.checkout = true;
    }
}

fn back_button(ui: &mut egui::Ui, palette: &Palette) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::vec2(30.0, 28.0), true);
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(RADIUS_BUTTON),
        if hovered {
            palette.bg_surface_hover
        } else {
            palette.bg_surface
        },
    );
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

/// Right rail of the review surface (git.md §9 visual language): the PR actions
/// (Open in browser, Checkout), then a **Files changed** band (count chip,
/// ±totals, ratio bar) and the file list, with the review composer pinned to the
/// foot. The Back control, the PR title and the detail live in the center area.
fn review_rail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
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
            review_actions(ui, palette, review, action);
            ui.add_space(SECTION_TOP_MARGIN);
            files_band(ui, palette, review.files);
            ui.add_space(6.0);
            review_file_list(ui, palette, review, action);
            ui.add_space(PANEL_PAD_Y);
        });

    review_composer(ui, palette, review, footer_rect, action);
}

/// The PR detail in the **center** area when no file is open (pull-requests.md
/// §11): a Back control and the PR title head the author block + branch flow +
/// body, then Checks and the conversation-level comments. A selected file swaps
/// this for its diff; the rail keeps the actions and the changed-files list.
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
            ui.set_width(ui.available_width().min(DETAIL_MAX_WIDTH));
            ui.add_space(PANEL_PAD_Y);
            let pr = review.pr;
            ui.horizontal(|ui| {
                if back_button(ui, palette) {
                    action.back = true;
                }
                ui.add_space(10.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&pr.title)
                            .size(TITLE_SIZE)
                            .strong()
                            .color(palette.text_primary),
                    )
                    .wrap(),
                );
            });
            ui.add_space(SECTION_TOP_MARGIN);
            review_meta(ui, palette, review);

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
            ui.add_space(PANEL_PAD_Y);
        });
}

fn review_file_list(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
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
    // Rows abut with hairline separators, matching the commit-detail file list;
    // scoped so the zeroed spacing doesn't bleed into the sections below.
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (idx, file) in review.files.iter().enumerate() {
            if idx > 0 {
                row_separator(ui, palette);
            }
            let out = file_row(
                ui,
                palette,
                egui::Sense::click(),
                &FileRow {
                    path: &file.path,
                    kind: file.kind,
                    additions: file.additions,
                    deletions: file.deletions,
                    selected: review.selected_file == Some(idx),
                    stats_hidden_on_hover: false,
                    indent: 0.0,
                },
            );
            out.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &file.path)
            });
            if out.response.clicked() {
                action.select_file = Some(idx);
            }
        }
    });
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

/// "Files changed" band (commit-detail's `files_header`, sans the flat/tree
/// toggle): the title + a count chip, with the ±totals and ratio bar pinned right.
fn files_band(ui: &mut egui::Ui, palette: &Palette, files: &[CommitFile]) {
    let additions: usize = files.iter().map(|f| f.additions).sum();
    let deletions: usize = files.iter().map(|f| f.deletions).sum();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Files changed")
                .size(SECTION_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(2.0);
        count_chip(ui, palette, files.len());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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

    let gap = 6.0;
    let btn_w = (inner.width() - gap * 2.0) / 3.0;
    panel.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if verdict_button(
            ui,
            palette,
            btn_w,
            "Comment",
            *review.verdict == ReviewVerdict::Comment,
            palette.accent,
        ) {
            *review.verdict = ReviewVerdict::Comment;
        }
        if verdict_button(
            ui,
            palette,
            btn_w,
            "Approve",
            *review.verdict == ReviewVerdict::Approve,
            palette.git_added,
        ) {
            *review.verdict = ReviewVerdict::Approve;
        }
        if verdict_button(
            ui,
            palette,
            btn_w,
            "Request changes",
            *review.verdict == ReviewVerdict::RequestChanges,
            palette.git_deleted,
        ) {
            *review.verdict = ReviewVerdict::RequestChanges;
        }
    });
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
    let label = if review.posting {
        "Submitting…".to_owned()
    } else if count > 0 {
        format!("Submit review ({count})")
    } else {
        "Submit review".to_owned()
    };
    let enabled = !review.posting;
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
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Submit review"));
    if response.clicked() {
        action.submit_review = true;
    }
}

/// One pill of the verdict selector; `selected_fill` colors it when active.
fn verdict_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    width: f32,
    label: &str,
    selected: bool,
    selected_fill: egui::Color32,
) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, 28.0), true);
    let fill = if selected {
        selected_fill
    } else if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(RADIUS_BUTTON), fill);
    let text_color = if selected {
        palette.lane_node_text
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

fn action_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    width: f32,
    icon: Icon,
    label: &str,
) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::vec2(width, 30.0), true);
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(RADIUS_BUTTON),
        if hovered {
            palette.bg_surface_hover
        } else {
            palette.bg_surface
        },
    );
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 11.0 + STATUS_ICON / 2.0, rect.center().y),
        STATUS_ICON,
        icon,
        palette.text_secondary,
    );
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        palette.text_primary,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + 11.0 + STATUS_ICON + 6.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.text_primary,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.to_owned())
    });
    response.clicked()
}

fn muted(palette: &Palette, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(12.0)
        .color(palette.text_muted)
}

fn checks_label(checks: Checks) -> &'static str {
    match checks {
        Checks::Passing => "All checks passing",
        Checks::Failing => "Some checks failing",
        Checks::Pending => "Checks pending",
        Checks::None => "No checks",
    }
}
