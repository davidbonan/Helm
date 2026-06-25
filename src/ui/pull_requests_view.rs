//! Rendering for the Pull Requests cockpit (pull-requests.md §5/§11). Two states:
//! a **browse** list of the workspace PRs grouped **To review** then **Mine**, and
//! — once a row is opened — a full-width **review** surface (back bar + PR header,
//! a changed-files rail, and the selected file's read-only diff). Pure
//! `fn(&mut egui::Ui, …)`: the app owns the cache, the selection, the fetched
//! detail/diff and the persisted rail width, and consumes the returned intents.

use lucide_icons::Icon;

use crate::git::commit_detail::CommitFile;
use crate::git::diff::FileDiff;
use crate::pull_requests::model::{
    Checks, PrDetail, PrRole, PrState, PullRequest, Review, ReviewVerdict,
};
use crate::review::{FileComments, ForgeThreads, ReviewIntent};
use crate::theme::Palette;
use crate::ui::diff_view::{DiffReview, DiffViewState};
use crate::ui::file_list::{file_row, FileRow};
use crate::ui::{clickable, paint_icon};

/// Review-surface split bounds: the changed-files rail and the diff each keep a
/// floor; the persisted rail width is clamped between them.
const RAIL_MIN_WIDTH: f32 = 260.0;
const DIFF_MIN_WIDTH: f32 = 420.0;
const REVIEW_HEADER_HEIGHT: f32 = 46.0;

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

/// What a click on the cockpit targeted. Collected into one struct and returned
/// each frame; every field is an independent `Option`, mirroring
/// `AgentsPageAction`.
#[derive(Default)]
pub struct PullRequestsPageAction {
    /// A list row was clicked: the app opens it in the review surface.
    pub select: Option<usize>,
    /// **Open in browser** was clicked: the app opens this PR's URL.
    pub open_url: Option<String>,
    /// **Checkout** was clicked: the app brings the PR branch up as a worktree
    /// (pull-requests.md §7). Carries the selected PR's index in the list.
    pub checkout: Option<usize>,
    /// The review surface's **Back** (or `Esc`) was hit: return to the list.
    pub back: bool,
    /// A changed-file row was clicked in the review rail: load its diff.
    pub select_file: Option<usize>,
    /// **Ask Claude** was clicked: launch an agent on the PR branch (§11).
    pub ask_claude: bool,
    /// The rail/diff split was dragged: the app stores and persists the new width.
    pub set_detail_width: Option<f32>,
    /// Draft-review actions the embedded diff raised (save / delete a line note,
    /// send to agent) — the app applies them to the PR's draft store (§11).
    pub review_intents: Vec<ReviewIntent>,
    /// **Submit review** in the composer was clicked: the app posts the draft
    /// comments + verdict + summary to the forge (§11).
    pub submit_review: bool,
}

/// Everything the review surface renders for the open PR. The app owns the state;
/// this is the per-frame borrow it hands the view (diff scroll state is `&mut` so
/// the view can record it). Loading/error flags drive the placeholders.
pub struct PrReviewView<'a> {
    /// Index of the open PR in the cockpit list (echoed back on Checkout).
    pub index: usize,
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
    /// The user's in-progress draft notes for this PR (editable in the diff).
    pub draft: &'a FileComments,
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
pub fn pull_requests_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    prs: &[PullRequest],
    selected: Option<usize>,
    review: Option<&mut PrReviewView<'_>>,
    rail_width: f32,
) -> PullRequestsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let mut action = PullRequestsPageAction::default();

    match review {
        Some(review) => render_review(ui, palette, review, rect, rail_width, &mut action),
        None => render_list(ui, palette, prs, selected, rect, &mut action),
    }
    action
}

/// The review surface (pull-requests.md §11): a back+header bar, a changed-files
/// rail on the left and the selected file's read-only diff on the right.
fn render_review(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    rail_width: f32,
    action: &mut PullRequestsPageAction,
) {
    // `Esc` returns to the list (the diff view has no open note editor here).
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        action.back = true;
    }

    let header_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top(), rect.top() + REVIEW_HEADER_HEIGHT),
    );
    review_header(ui, palette, review, header_rect, action);

    let body = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(header_rect.bottom(), rect.bottom()),
    );
    ui.painter().hline(
        rect.x_range(),
        header_rect.bottom(),
        egui::Stroke::new(1.0, palette.border_subtle),
    );

    let rail_w = rail_width.clamp(
        RAIL_MIN_WIDTH,
        (body.width() - DIFF_MIN_WIDTH).max(RAIL_MIN_WIDTH),
    );
    let split_x = body.left() + rail_w;
    let rail_rect =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(body.left(), split_x), body.y_range());
    review_rail(ui, palette, review, rail_rect, action);

    rail_resize_handle(ui, palette, split_x, body, rail_width, action);

    let diff_rect =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(split_x, body.right()), body.y_range());
    review_diff(ui, palette, review, diff_rect, action);
}

fn review_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_sidebar);
    let pr = review.pr;
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(PANEL_PAD_X, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if back_button(&mut bar, palette) {
        action.back = true;
    }
    bar.add_space(10.0);
    let (state_icon, state_color) = match pr.state {
        PrState::Open => (Icon::GitPullRequest, palette.git_added),
        PrState::Draft => (Icon::GitPullRequestDraft, palette.text_muted),
    };
    let (icon_rect, _) =
        bar.allocate_exact_size(egui::vec2(STATE_ICON, STATE_ICON), egui::Sense::hover());
    paint_icon(
        bar.painter(),
        icon_rect.center(),
        STATE_ICON,
        state_icon,
        state_color,
    );
    bar.add_space(6.0);
    bar.label(
        egui::RichText::new(&pr.title)
            .size(14.5)
            .color(palette.text_primary)
            .strong(),
    );
    bar.label(
        egui::RichText::new(format!("#{}", pr.number))
            .size(13.0)
            .color(palette.text_muted),
    );

    bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |bar| {
        if action_button(bar, palette, Icon::Bot, "Ask Claude") {
            action.ask_claude = true;
        }
        bar.add_space(6.0);
        if action_button(bar, palette, Icon::GitBranch, "Checkout") {
            action.checkout = Some(review.index);
        }
        bar.add_space(6.0);
        if action_button(bar, palette, Icon::ExternalLink, "Open in browser") {
            action.open_url = Some(pr.url.clone());
        }
    });
}

fn back_button(ui: &mut egui::Ui, palette: &Palette) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::vec2(30.0, 28.0), true);
    ui.painter().rect_filled(
        rect,
        6.0,
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

/// Left rail of the review surface: collapsible PR detail (branches / description
/// / checks / comments) above the changed-files list.
fn review_rail(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &mut PrReviewView<'_>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_sidebar);
    let footer_h = composer_height(review);
    let scroll_rect = egui::Rect::from_x_y_ranges(
        rect.x_range(),
        egui::Rangef::new(rect.top(), (rect.bottom() - footer_h).max(rect.top())),
    );
    let footer_rect = egui::Rect::from_x_y_ranges(rect.x_range(), {
        let top = (rect.bottom() - footer_h).max(rect.top());
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
            let pr = review.pr;
            ui.label(
                egui::RichText::new(format!("{} → {}", pr.source_branch, pr.dest_branch))
                    .size(12.0)
                    .color(palette.text_secondary),
            );
            ui.label(
                egui::RichText::new(format!("by {}", pr.author))
                    .size(12.0)
                    .color(palette.text_muted),
            );

            section(ui, palette, "Changed files");
            review_file_list(ui, palette, review, action);

            if let Some(error) = review.detail_error {
                section(ui, palette, "Detail unavailable");
                ui.label(muted(palette, error));
            }

            let body = review.detail.map(|d| d.body.trim()).unwrap_or("");
            if !body.is_empty() {
                section(ui, palette, "Description");
                ui.label(
                    egui::RichText::new(body)
                        .size(12.5)
                        .color(palette.text_secondary),
                );
            }

            let checks = review
                .detail
                .map(|d| d.check_runs.as_slice())
                .unwrap_or(&[]);
            if !checks.is_empty() || pr.checks != Checks::None {
                section(ui, palette, "Checks");
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

            let comments = review.detail.map(|d| d.comments.as_slice()).unwrap_or(&[]);
            if !comments.is_empty() {
                section(ui, palette, "Conversation");
                for c in comments {
                    ui.label(
                        egui::RichText::new(&c.author)
                            .size(12.0)
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

    review_composer(ui, palette, review, footer_rect, action);
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
    for (idx, file) in review.files.iter().enumerate() {
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
        out.response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &file.path));
        if out.response.clicked() {
            action.select_file = Some(idx);
        }
    }
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
    ui.painter().rect_filled(rect, 0, palette.bg_sidebar);
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
    panel.painter().rect_filled(rect, 6.0, fill);
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
    ui.painter().rect_filled(rect, 6.0, fill);
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
            false,
            true,
            review.diff_view,
            &mut throwaway,
            Some(&mut DiffReview {
                comments: review.draft,
                existing: review.existing,
                agent: review.agent,
                intents: &mut review_out,
            }),
        );
        action.review_intents.append(&mut review_out);
        if closed {
            action.back = true;
        }
        return;
    }
    let message = if let Some(error) = review.diff_error {
        error
    } else if review.diff_loading {
        "Loading diff…"
    } else if review.selected_file.is_none() {
        "Select a file to view its diff"
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
            if prs.is_empty() {
                empty_state(ui, palette);
                return;
            }
            group(ui, palette, "To review", &to_review, prs, selected, action);
            group(ui, palette, "Mine", &mine, prs, selected, action);
        });
}

fn empty_state(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new("No pull requests")
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

fn section(ui: &mut egui::Ui, palette: &Palette, title: &str) {
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(HEADER_SIZE)
            .color(palette.text_muted),
    );
    ui.add_space(4.0);
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

fn action_button(ui: &mut egui::Ui, palette: &Palette, icon: Icon, label: &str) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        palette.text_primary,
    );
    let w = galley.size().x + STATUS_ICON + 26.0;
    let (rect, response, hovered) = clickable(ui, egui::vec2(w, 30.0), true);
    ui.painter().rect_filled(
        rect,
        6.0,
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
