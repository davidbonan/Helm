//! Rendering for the Pull Requests cockpit (pull-requests.md §5): a two-pane
//! central page — a left list of the workspace PRs grouped **To review** then
//! **Mine**, and a right detail panel for the selection (header, branches,
//! description, checks, reviewers, read-only comments, Open-in-browser /
//! Checkout). Pure `fn(&mut egui::Ui, …)`: the app owns the cache, the selection
//! and the persisted split width, and consumes the returned intents.

use lucide_icons::Icon;

use crate::pull_requests::model::{Checks, PrDetail, PrRole, PrState, PullRequest, Review};
use crate::theme::Palette;
use crate::ui::{clickable, paint_icon};

/// List never narrower than this; below `LIST_MIN_WIDTH + DETAIL_MIN_WIDTH` the
/// detail panel folds away and the list spans the full width.
const LIST_MIN_WIDTH: f32 = 360.0;
const DETAIL_MIN_WIDTH: f32 = 360.0;

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
    /// A list row was clicked: the app stores it as the selection.
    pub select: Option<usize>,
    /// **Open in browser** was clicked: the app opens this PR's URL.
    pub open_url: Option<String>,
    /// **Checkout** was clicked: the app brings the PR branch up as a worktree
    /// (pull-requests.md §7). Carries the selected PR's index in the list.
    pub checkout: Option<usize>,
    /// The detail split was dragged: the app stores and persists the new width.
    pub set_detail_width: Option<f32>,
}

/// The cockpit page: a grouped list on the left, the selection's detail on the
/// right behind a draggable split. `detail` is the lazily-fetched body / comments
/// / check runs for the selection (`None` until loaded — the panel still shows
/// what the list row already carries).
pub fn pull_requests_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    prs: &[PullRequest],
    selected: Option<usize>,
    detail: Option<&PrDetail>,
    detail_width: f32,
) -> PullRequestsPageAction {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, palette.bg_canvas);
    let mut action = PullRequestsPageAction::default();

    let selected_pr = selected
        .and_then(|i| prs.get(i))
        .map(|pr| (selected.unwrap(), pr));
    let show_detail = selected_pr.is_some() && rect.width() >= LIST_MIN_WIDTH + DETAIL_MIN_WIDTH;
    let detail_w = if show_detail {
        detail_width.clamp(DETAIL_MIN_WIDTH, rect.width() - LIST_MIN_WIDTH)
    } else {
        0.0
    };
    let split_x = rect.right() - detail_w;

    let list_right = if show_detail { split_x } else { rect.right() };
    let list_rect =
        egui::Rect::from_x_y_ranges(egui::Rangef::new(rect.left(), list_right), rect.y_range());
    render_list(ui, palette, prs, selected, list_rect, &mut action);

    if let Some((idx, pr)) = selected_pr.filter(|_| show_detail) {
        detail_resize_handle(ui, palette, split_x, rect, detail_width, &mut action);
        let detail_rect =
            egui::Rect::from_x_y_ranges(egui::Rangef::new(split_x, rect.right()), rect.y_range());
        render_detail(ui, palette, pr, idx, detail, detail_rect, &mut action);
    }
    action
}

fn detail_resize_handle(
    ui: &mut egui::Ui,
    palette: &Palette,
    x: f32,
    rect: egui::Rect,
    detail_width: f32,
    action: &mut PullRequestsPageAction,
) {
    let handle = egui::Rect::from_x_y_ranges(egui::Rangef::new(x - 3.0, x + 3.0), rect.y_range());
    let resp = ui.interact(
        handle,
        ui.id().with("pr_detail_resize"),
        egui::Sense::drag(),
    );
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
        let max = rect.width() - LIST_MIN_WIDTH;
        action.set_detail_width =
            Some((detail_width - resp.drag_delta().x).clamp(DETAIL_MIN_WIDTH, max));
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

fn render_detail(
    ui: &mut egui::Ui,
    palette: &Palette,
    pr: &PullRequest,
    idx: usize,
    detail: Option<&PrDetail>,
    rect: egui::Rect,
    action: &mut PullRequestsPageAction,
) {
    ui.painter().rect_filled(rect, 0, palette.bg_sidebar);
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
        .id_salt("pr_detail")
        .show(&mut panel, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(PANEL_PAD_Y);

            ui.label(
                egui::RichText::new(&pr.title)
                    .size(15.0)
                    .color(palette.text_primary)
                    .strong(),
            );
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("#{}", pr.number)).color(palette.text_secondary),
                );
                ui.label(
                    egui::RichText::new(state_label(pr.state))
                        .color(palette.text_muted)
                        .size(12.0),
                );
            });
            ui.add_space(4.0);
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

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if action_button(ui, palette, Icon::ExternalLink, "Open in browser") {
                    action.open_url = Some(pr.url.clone());
                }
                if action_button(ui, palette, Icon::GitBranch, "Checkout") {
                    action.checkout = Some(idx);
                }
            });

            section(ui, palette, "Description");
            let body = detail.map(|d| d.body.trim()).unwrap_or("");
            if body.is_empty() {
                ui.label(muted(palette, "No description"));
            } else {
                ui.label(
                    egui::RichText::new(body)
                        .size(12.5)
                        .color(palette.text_secondary),
                );
            }

            section(ui, palette, "Checks");
            match detail.map(|d| d.check_runs.as_slice()).unwrap_or(&[]) {
                [] => status_line(
                    ui,
                    palette,
                    checks_status(palette, pr.checks),
                    checks_label(pr.checks),
                ),
                runs => {
                    for run in runs {
                        status_line(ui, palette, checks_status(palette, run.status), &run.name);
                    }
                }
            }

            section(ui, palette, "Reviewers");
            if pr.reviewers.is_empty() {
                ui.label(muted(palette, "None"));
            } else {
                for r in &pr.reviewers {
                    status_line(ui, palette, review_status(palette, r.state), &r.name);
                }
            }

            let comments = detail.map(|d| d.comments.as_slice()).unwrap_or(&[]);
            if !comments.is_empty() {
                section(ui, palette, "Comments");
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

fn state_label(state: PrState) -> &'static str {
    match state {
        PrState::Open => "Open",
        PrState::Draft => "Draft",
    }
}

fn checks_label(checks: Checks) -> &'static str {
    match checks {
        Checks::Passing => "All checks passing",
        Checks::Failing => "Some checks failing",
        Checks::Pending => "Checks pending",
        Checks::None => "No checks",
    }
}
