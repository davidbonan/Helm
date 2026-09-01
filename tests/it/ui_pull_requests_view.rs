//! UI E2E for the Pull Requests cockpit (pull-requests.md §5/§11): drives
//! `pull_requests_page` headless across both surfaces — the browse list (groups,
//! a row, the empty state, the row → select intent) and the review surface (the
//! detail header's Open-in-browser / Checkout intents, Back, a changed-file
//! click, and the draggable rail width).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::commit_detail::CommitFile;
use helm::git::diff::{DiffLine, FileDiff, Hunk, LineOrigin};
use helm::git::status::ChangeKind;
use helm::pull_requests::model::{
    Checks, ForgeKind, PrCommit, PrDetail, PrRole, PrState, PullRequest, Review, ReviewVerdict,
    Reviewer,
};
use helm::review::{FileComments, ForgeThreads, LineComment, ReviewIntent};
use helm::theme::Palette;
use helm::ui::diff_view::DiffViewState;
use helm::ui::file_list::FileViewMode;
use helm::ui::pull_requests_view::{
    pull_requests_page, CommitSelection, PrReviewView, PrSourceHints,
};

#[derive(Default)]
struct Captured {
    select: Cell<Option<usize>>,
    refresh: Cell<bool>,
    open_url: Cell<Option<String>>,
    checkout: Cell<bool>,
    set_detail_width: Cell<Option<f32>>,
    back: Cell<bool>,
    close_file: Cell<bool>,
    select_file: Cell<Option<usize>>,
    submit_review: Cell<bool>,
    set_file_view: Cell<Option<FileViewMode>>,
    merge: Cell<Option<usize>>,
    merge_open: Cell<bool>,
}

fn pr(repo: &str, number: u64, title: &str, role: PrRole) -> PullRequest {
    PullRequest {
        forge_kind: ForgeKind::GitHub,
        repo_label: repo.to_owned(),
        number,
        title: title.to_owned(),
        role,
        state: PrState::Open,
        author: "octocat".to_owned(),
        source_branch: "feature".to_owned(),
        dest_branch: "main".to_owned(),
        source_commit: String::new(),
        dest_commit: String::new(),
        url: format!("https://example.test/{repo}/pull/{number}"),
        updated_at: "2 days ago".to_owned(),
        checks: Checks::Passing,
        review: Review::Pending,
        reviewers: vec![Reviewer {
            name: "reviewer".to_owned(),
            state: Review::Pending,
        }],
        labels: Vec::new(),
        diffstat: None,
        comment_count: None,
    }
}

fn harness(
    prs: Vec<PullRequest>,
    selected: Option<usize>,
    detail_width: f32,
) -> (Harness<'static>, Rc<Captured>) {
    let palette = Palette::light();
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let action = pull_requests_page(
                ui,
                &palette,
                &prs,
                selected,
                &PrSourceHints::default(),
                None,
                detail_width,
                false,
                FileViewMode::Flat,
            );
            if action.select.is_some() {
                sink.select.set(action.select);
            }
            if action.refresh {
                sink.refresh.set(true);
            }
            if action.merge.is_some() {
                sink.merge.set(action.merge);
            }
        });
    harness.step();
    harness.step();
    (harness, cap)
}

/// Drives the review surface for one PR with the given changed files. The diff and
/// detail stay unloaded (placeholders); the closure owns the `DiffViewState`.
fn review_harness(
    pr_value: PullRequest,
    files: Vec<CommitFile>,
    rail_width: f32,
) -> (Harness<'static>, Rc<Captured>) {
    let palette = Palette::light();
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                rail_width,
                false,
                FileViewMode::Flat,
            );
            if action.open_url.is_some() {
                sink.open_url.set(action.open_url.clone());
            }
            if action.checkout {
                sink.checkout.set(true);
            }
            if action.set_detail_width.is_some() {
                sink.set_detail_width.set(action.set_detail_width);
            }
            if action.back {
                sink.back.set(true);
            }
            if action.close_file {
                sink.close_file.set(true);
            }
            if action.select_file.is_some() {
                sink.select_file.set(action.select_file);
            }
            if action.submit_review {
                sink.submit_review.set(true);
            }
            if action.set_file_view.is_some() {
                sink.set_file_view.set(action.set_file_view);
            }
            if action.merge_open {
                sink.merge_open.set(true);
            }
        });
    harness.step();
    harness.step();
    (harness, cap)
}

/// The changed-files rail belongs to the **Files** tab, so anything asserting on it
/// opens that tab first (the surface starts on Conversation).
fn open_files(harness: &mut Harness<'static>) {
    harness.get_by_label("Files").click();
    harness.run();
}

/// Drives the review surface for a PR whose forge detail is still in flight: `detail`
/// is `None` and `detail_loading` is set, the state the app holds before the detail
/// fetch lands.
fn review_loading_harness(pr_value: PullRequest) -> (Harness<'static>, Rc<Captured>) {
    let palette = Palette::light();
    let cap = Rc::new(Captured::default());
    let files: Vec<CommitFile> = Vec::new();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: true,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    (harness, cap)
}

fn changed_file(path: &str) -> CommitFile {
    CommitFile {
        path: path.to_owned(),
        kind: ChangeKind::Modified,
        additions: 3,
        deletions: 1,
    }
}

fn line_comment(note: &str) -> LineComment {
    LineComment {
        old_lineno: None,
        new_lineno: Some(2),
        code: "work();".to_owned(),
        note: note.to_owned(),
    }
}

#[test]
fn rows_group_into_their_actionability_bands() {
    let (harness, _) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::Mine),
        ],
        None,
        460.0,
    );
    // A PR I am asked to review leads; my own, still awaiting a verdict, follows.
    harness.get_by_label("WAITING ON YOUR REVIEW");
    harness.get_by_label("IN REVIEW");
    harness.get_by_label("Fix the login flow");
    harness.get_by_label("Bump the cache TTL");
}

#[test]
fn a_draft_and_a_red_build_fall_into_the_waiting_on_author_band() {
    let mut draft = pr("acme/web", 1, "Sketch the importer", PrRole::Mine);
    draft.state = PrState::Draft;
    let mut broken = pr("acme/api", 2, "Bump the cache TTL", PrRole::ToReview);
    broken.checks = Checks::Failing;

    let (harness, _) = harness(vec![draft, broken], None, 460.0);
    harness.get_by_label("WAITING ON THE AUTHOR");
    harness.get_by_label("Sketch the importer");
    harness.get_by_label("Bump the cache TTL");
}

#[test]
fn an_approved_green_pr_is_ready_to_merge_and_offers_the_inline_button() {
    let mut approved = pr("acme/web", 1, "Add cursor pagination", PrRole::Mine);
    approved.review = Review::Approved;
    approved.checks = Checks::Passing;

    let (mut harness, cap) = harness(vec![approved], None, 460.0);
    harness.get_by_label("READY TO MERGE");
    harness.get_by_label("Merge").click();
    harness.step();
    assert_eq!(cap.merge.get(), Some(0));
}

#[test]
fn the_tab_bar_filters_the_list_down_to_one_role() {
    let (mut harness, _) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::Mine),
        ],
        None,
        460.0,
    );
    harness.get_by_label("Mine").click();
    harness.step();
    harness.get_by_label("Bump the cache TTL");
    assert!(harness.query_by_label("Fix the login flow").is_none());
}

#[test]
fn the_search_field_narrows_the_list_and_can_come_up_empty() {
    let (mut harness, _) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::ToReview),
        ],
        None,
        460.0,
    );
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .type_text("login");
    harness.run();
    harness.get_by_label("Fix the login flow");
    assert!(harness.query_by_label("Bump the cache TTL").is_none());
}

#[test]
fn clicking_refresh_emits_the_intent() {
    let (mut harness, cap) = harness(
        vec![pr("acme/web", 1, "Fix the login flow", PrRole::ToReview)],
        None,
        460.0,
    );
    harness.get_by_label("Refresh").click();
    harness.step();
    assert!(cap.refresh.get());
}

#[test]
fn empty_state_when_no_prs() {
    let (harness, _) = harness(Vec::new(), None, 460.0);
    harness.get_by_label("No pull requests");
}

#[test]
fn list_loading_shows_a_spinner_not_the_empty_state() {
    let palette = Palette::light();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints {
                    loading: true,
                    ..Default::default()
                },
                None,
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness.get_by_label("Loading pull requests…");
    assert!(
        harness.query_by_label("No pull requests").is_none(),
        "the empty state must not show while loading"
    );
}

/// A chain of PRs each targeting the one below it, all awaiting the same review.
fn stack(len: u64) -> Vec<PullRequest> {
    (0..len)
        .map(|i| {
            let mut p = pr(
                "acme/web",
                100 + i,
                &format!("Stacked step {}", i + 1),
                PrRole::ToReview,
            );
            p.review = Review::Pending;
            p.source_branch = format!("feat/step-{i}");
            p.dest_branch = if i == 0 {
                "main".to_owned()
            } else {
                format!("feat/step-{}", i - 1)
            };
            p
        })
        .collect()
}

#[test]
fn a_stack_lists_under_one_header_that_folds_it_away() {
    let (mut harness, _) = harness(stack(3), None, 460.0);
    harness.get_by_label("Stack · 3 PRs");
    harness.get_by_label("Stacked step 1");
    harness.get_by_label("Stacked step 3");

    // The header is the fold: collapsed, the chain is a one-line summary.
    harness.get_by_label("Stack · 3 PRs").click();
    harness.step();
    harness.step();
    harness.get_by_label("Stack · 3 PRs");
    assert!(
        harness.query_by_label("Stacked step 1").is_none(),
        "a folded stack must not leave its rows on screen"
    );
}

#[test]
fn a_lone_pr_gets_no_stack_header() {
    let (harness, _) = harness(
        vec![pr("acme/web", 1, "Fix the login flow", PrRole::ToReview)],
        None,
        460.0,
    );
    harness.get_by_label("Fix the login flow");
    assert!(harness.query_by_label("Stack · 1 PR").is_none());
}

#[test]
fn a_stacked_row_still_selects_by_its_own_title() {
    let (mut harness, cap) = harness(stack(3), None, 460.0);
    harness.get_by_label("Stacked step 2").click();
    harness.step();
    assert_eq!(cap.select.get(), Some(1));
}

#[test]
fn the_footer_reports_what_the_filters_let_through() {
    let (mut harness, _) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::Mine),
        ],
        None,
        460.0,
    );
    harness.get_by_label("End of list · 2 pull requests");
    harness.get_by_label("Mine").click();
    harness.step();
    harness.get_by_label("End of list · 1 pull request");
}

#[test]
fn the_search_field_offers_a_clear_once_it_holds_a_query() {
    let (mut harness, _) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::ToReview),
        ],
        None,
        460.0,
    );
    assert!(harness.query_by_label("Clear search").is_none());
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "TextInput")
        .type_text("login");
    harness.run();
    assert!(harness.query_by_label("Bump the cache TTL").is_none());

    harness.get_by_label("Clear search").click();
    harness.run();
    harness.get_by_label("Bump the cache TTL");
}

#[test]
fn clicking_a_row_selects_it() {
    let (mut harness, cap) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::Mine),
        ],
        None,
        460.0,
    );
    harness.get_by_label("Bump the cache TTL").click();
    harness.step();
    assert_eq!(cap.select.get(), Some(1));
}

#[test]
fn review_detail_header_shows_pr_context() {
    let (harness, _) = review_harness(
        pr("acme/web", 42, "PR cockpit", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("PR cockpit");
    harness.get_by_label("octocat · feature → main");
    harness.get_by_label("#42");
}

#[test]
fn review_detail_open_in_browser_emits_the_url() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Open in browser").click();
    harness.step();
    assert_eq!(
        cap.open_url.take(),
        Some("https://example.test/acme/web/pull/1".to_owned())
    );
}

#[test]
fn review_detail_checkout_emits_the_intent() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Checkout").click();
    harness.step();
    assert!(cap.checkout.get());
}

#[test]
fn review_composer_submit_emits_the_intent() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Finish review").click();
    harness.run();
    harness.get_by_label("Approve").click();
    harness.run();
    // Two now: the popover's verdict button and its Submit, which named itself after
    // the chosen verdict.
    harness.get_all_by_label("Approve").last().unwrap().click();
    harness.step();
    assert!(cap.submit_review.get());
}

#[test]
fn review_back_returns_to_the_list() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Back").click();
    // The review slides off before it hands the app back to the list.
    harness.run();
    assert!(cap.back.get());
}

/// Feed one trackpad phase into the harness the way `egui-winit` does on macOS: a
/// point-unit wheel event carrying the phase, then a frame to fold it in.
fn wheel(harness: &mut Harness<'static>, phase: egui::TouchPhase, delta: egui::Vec2) {
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
}

/// Play a whole two-finger run over the review surface.
fn trackpad_swipe(harness: &mut Harness<'static>, delta: egui::Vec2) {
    wheel(harness, egui::TouchPhase::Start, egui::Vec2::ZERO);
    wheel(harness, egui::TouchPhase::Move, delta);
    wheel(harness, egui::TouchPhase::End, egui::Vec2::ZERO);
}

#[test]
fn a_two_finger_swipe_right_returns_to_the_list() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    trackpad_swipe(&mut harness, egui::vec2(120.0, 4.0));
    assert!(cap.back.get());
}

#[test]
fn a_two_finger_scroll_does_not_return_to_the_list() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    // Down the page.
    trackpad_swipe(&mut harness, egui::vec2(0.0, -300.0));
    // Leftward, which reveals content to the right rather than the list.
    trackpad_swipe(&mut harness, egui::vec2(-200.0, 0.0));
    // Rightward but barely.
    trackpad_swipe(&mut harness, egui::vec2(30.0, 0.0));
    assert!(!cap.back.get());
}

#[test]
fn a_mouse_wheel_never_reads_as_a_swipe() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    // A wheel reports lines, not points, and has no gesture to recognize.
    for phase in [
        egui::TouchPhase::Start,
        egui::TouchPhase::Move,
        egui::TouchPhase::End,
    ] {
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(40.0, 0.0),
            phase,
            modifiers: egui::Modifiers::default(),
        });
        harness.step();
    }
    assert!(!cap.back.get());
}

/// Height the column measured for a file's band, read from the view's own record
/// (`pr_review_band_sizes`) — the diff rows themselves are painted, not accessibility
/// nodes, so this is what tells a laid-out band from a folded one.
fn band_height(harness: &Harness<'static>, pr_url: &str, path: &str) -> Option<f32> {
    let sizes: std::collections::HashMap<String, (f32, f32)> = harness.ctx.data(|d| {
        d.get_temp(egui::Id::new(("pr_review_band_sizes", pr_url)))
            .unwrap_or_default()
    });
    sizes.get(path).map(|(_, h)| *h)
}

fn sample_diff() -> FileDiff {
    diff_for("src/main.rs", "@@ -1,2 +1,3 @@")
}

fn diff_for(path: &str, header: &str) -> FileDiff {
    FileDiff {
        path: path.to_owned(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: header.to_owned(),
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 3,
            lines: vec![
                DiffLine {
                    origin: LineOrigin::Context,
                    content: "fn main() {\n".to_owned(),
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                },
                DiffLine {
                    origin: LineOrigin::Addition,
                    content: "    work();\n".to_owned(),
                    old_lineno: None,
                    new_lineno: Some(2),
                },
            ],
        }],
        source_lines: Vec::new(),
        image: None,
        editable: false,
    }
}

/// The Files tab reads as one continuous document: **every** changed file's diff is
/// on screen at once, in rail order, with nothing selected (pull-requests.md §11).
#[test]
fn the_files_tab_stacks_every_diff_in_one_column() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let lib = diff_for("src/lib.rs", "@@ -10,2 +10,3 @@");
    let main = diff_for("src/main.rs", "@@ -1,2 +1,3 @@");
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: vec![Some(&lib), Some(&main)],
                diff_errors: vec![None, None],
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    open_files(&mut harness);
    let url = "https://example.test/acme/web/pull/1";
    // Both bands laid out their rows: a header alone measures well under 60pt.
    for path in ["src/lib.rs", "src/main.rs"] {
        let height = band_height(&harness, url, path).unwrap_or_default();
        assert!(
            height > 80.0,
            "{path} is diffed in the column, not just listed ({height}pt)",
        );
    }
}

/// Every changed file is diffed in one column, each band foldable in place: the
/// chevron hides that file's rows and leaves the column (and the review) standing.
#[test]
fn folding_a_band_hides_its_rows_and_keeps_the_column() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/main.rs")];
    let diff = sample_diff();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diffs: vec![Some(&diff)],
                diff_errors: vec![None],
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
            if action.back {
                sink.back.set(true);
            }
            if action.close_file {
                sink.close_file.set(true);
            }
        });
    harness.step();
    harness.step();
    assert!(
        harness.query_by_label("Open in browser").is_some(),
        "PR-level actions live in the surface header and stay put while a diff is open",
    );
    assert!(
        harness.query_by_label("Checkout").is_some(),
        "PR-level actions live in the surface header and stay put while a diff is open",
    );
    let url = "https://example.test/acme/web/pull/1";
    let open = band_height(&harness, url, "src/main.rs").unwrap_or_default();
    assert!(open > 80.0, "the band diffs its file in place ({open}pt)");
    harness.get_by_label("Collapse").click();
    harness.step();
    harness.step();
    let folded = band_height(&harness, url, "src/main.rs").unwrap_or_default();
    assert!(
        folded < open / 2.0,
        "folding the band takes its rows away ({folded}pt vs {open}pt)",
    );
    assert!(
        harness.query_by_label("Expand").is_some(),
        "the folded band keeps its header, ready to unfold",
    );
    assert!(
        !cap.back.get() && !cap.close_file.get(),
        "folding a band is not leaving the file, let alone the review",
    );
}

#[test]
fn clicking_a_changed_file_loads_its_diff() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        vec![changed_file("src/lib.rs"), changed_file("src/main.rs")],
        460.0,
    );
    open_files(&mut harness);
    // The rail is the column's table of contents: its row is the first node with
    // that label, the band's own header the second.
    harness
        .get_all_by_label("src/main.rs")
        .next()
        .unwrap()
        .click();
    harness.step();
    assert_eq!(cap.select_file.get(), Some(1));
}

/// ↑/↓ over the changed-files rail step the selection through the list (like the
/// git sidebar): with the middle file open, ↓ picks the next file and ↑ the
/// previous, emitting `select_file` so the diff follows.
#[test]
fn arrows_navigate_between_changed_files() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![
        changed_file("src/a.rs"),
        changed_file("src/b.rs"),
        changed_file("src/c.rs"),
    ];
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(1),
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
            if action.select_file.is_some() {
                sink.select_file.set(action.select_file);
            }
        });
    harness.step();
    harness.step();

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowDown);
    harness.step();
    assert_eq!(cap.select_file.get(), Some(2));

    harness.key_press_modifiers(egui::Modifiers::default(), egui::Key::ArrowUp);
    harness.step();
    assert_eq!(cap.select_file.get(), Some(0));
}

/// The Files toolbar's commit scope lists "All commits" plus one row per commit, and
/// picking a commit emits a `select_commit` for that sha (per-commit diff: T5).
#[test]
fn clicking_a_commit_row_selects_that_commit() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        commits: vec![
            PrCommit {
                sha: "1111111111111111111111111111111111111111".to_owned(),
                short: "1111111".to_owned(),
                subject: "Add login form".to_owned(),
                author: "octocat".to_owned(),
            },
            PrCommit {
                sha: "2222222222222222222222222222222222222222".to_owned(),
                short: "2222222".to_owned(),
                subject: "Wire the submit handler".to_owned(),
                author: "octocat".to_owned(),
            },
        ],
        ..PrDetail::default()
    };
    let files = vec![changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let captured: Rc<RefCell<Option<CommitSelection>>> = Rc::new(RefCell::new(None));
    let sink = captured.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &detail.commits,
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
            if let Some(sel) = action.select_commit {
                *sink.borrow_mut() = Some(sel);
            }
        });
    harness.step();
    harness.step();
    open_files(&mut harness);
    assert!(
        harness.query_by_label("All commits").is_some(),
        "the Files toolbar's commit scope defaults to the cumulative range",
    );
    harness.get_by_label("All commits").click();
    harness.run();
    harness
        .get_by_label("2222222  Wire the submit handler")
        .click();
    harness.step();
    assert_eq!(
        captured.borrow().clone(),
        Some(CommitSelection::Commit(
            "2222222222222222222222222222222222222222".to_owned()
        )),
    );
}

#[test]
fn files_header_toggle_requests_tree_view() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        vec![changed_file("src/lib.rs"), changed_file("src/main.rs")],
        460.0,
    );
    open_files(&mut harness);
    harness.get_by_label("Tree view").click();
    harness.step();
    assert_eq!(cap.set_file_view.get(), Some(FileViewMode::Tree));
}

#[test]
fn tree_view_groups_changed_files_under_directory_rows() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![
        changed_file("README.md"),
        changed_file("src/lib.rs"),
        changed_file("src/main.rs"),
    ];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Tree,
            );
        });
    harness.step();
    harness.step();
    open_files(&mut harness);
    harness.get_by_label("src");
    // Twice on screen, and not as two lists: the rail's row, and the header of the
    // band diffing that file in the column.
    assert_eq!(harness.query_all_by_label("src/lib.rs").count(), 2);
}

#[test]
fn changed_file_rows_show_quiet_review_and_agent_icons_without_counts() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/lib.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let mut draft = FileComments::new();
    draft.insert("src/lib.rs".to_owned(), vec![line_comment("review this")]);
    let mut agent_notes = FileComments::new();
    agent_notes.insert("src/lib.rs".to_owned(), vec![line_comment("inspect this")]);
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness.get_by_label("src/lib.rs: has review comments");
    harness.get_by_label("src/lib.rs: has agent notes");
    assert!(
        harness.query_by_label("Viewed src/lib.rs").is_none(),
        "viewed state is represented by the unread filter, not a row badge",
    );
    assert!(
        harness
            .query_by_label("src/lib.rs: 1 review comments")
            .is_none(),
        "comment icons should not expose a visible count badge",
    );
    assert!(
        harness
            .query_by_label("src/lib.rs: 1 agent notes")
            .is_none(),
        "agent-note icons should not expose a visible count badge",
    );
}

#[test]
fn unread_only_filters_out_files_opened_in_this_review() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    assert_eq!(harness.query_all_by_label("src/lib.rs").count(), 2);
    harness.get_by_label("Unread only").click();
    harness.run();
    assert_eq!(
        harness.query_all_by_label("src/lib.rs").count(),
        0,
        "the already-opened file leaves both the rail and the column",
    );
    assert_eq!(
        harness.query_all_by_label("src/main.rs").count(),
        2,
        "the unopened file keeps its row and its band",
    );
}

#[test]
fn dragging_the_split_resizes_the_rail_width() {
    // The rail sits on the left; its resize handle is on the split line at
    // `body.left() + rail_width` = 460. Dragging right widens the rail
    // (`rail_width + drag_delta.x`).
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    open_files(&mut harness);
    let start = egui::pos2(460.0, 400.0);
    let end = start + egui::vec2(60.0, 0.0);
    harness.event(egui::Event::PointerMoved(start));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.event(egui::Event::PointerMoved(end));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    let width = cap
        .set_detail_width
        .get()
        .expect("drag emits a new rail width");
    assert!(
        width > 460.0,
        "dragging the split right widens the rail, got {width}"
    );
}

#[test]
fn collapsed_rail_hides_the_changed_files_but_keeps_the_center_area() {
    // The rail toggle now lives in the title bar (outside this view); collapsing it
    // hides the rail file list and composer; the center area (here the PR detail,
    // since no file is open) expands to the full width and keeps PR-level actions.
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    // The branches are named once, in the surface header — the detail below no longer
    // repeats them (§11).
    harness.get_by_label("octocat · feature → main");
    assert!(
        harness.query_by_label("src/main.rs").is_none(),
        "the changed-files rail is hidden when collapsed",
    );
    harness.get_by_label("Checkout");
}

#[test]
fn detail_conversation_lists_only_top_level_comments() {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![
            PrComment {
                author: "reviewer-top".to_owned(),
                body: "overall looks good".to_owned(),
                path: None,
                old_lineno: None,
                new_lineno: None,
                id: None,
                parent_id: None,
                context: None,
                created_at: String::new(),
                resolved: false,
                thread_id: None,
            },
            PrComment {
                author: "reviewer-inline".to_owned(),
                body: "rename this".to_owned(),
                path: Some("src/main.rs".to_owned()),
                old_lineno: None,
                new_lineno: Some(2),
                id: None,
                parent_id: None,
                context: None,
                created_at: String::new(),
                resolved: false,
                thread_id: None,
            },
        ],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    // Detail (body + conversation) renders in the center; the file list stays in the rail.
    harness.get_by_label("Describe the change");
    // Both kinds of open thread sit in the one conversation card, under its section head
    // — the separate "Inline comments" band is gone.
    harness.get_by_label("NEEDS ATTENTION · 2");
    harness.get_by_label("reviewer-top");
    harness.get_by_label("reviewer-inline");
    // The anchored one still names the line it hangs on.
    harness.get_by_label("src/main.rs:2");
    assert!(
        harness.query_by_label("Inline comments").is_none(),
        "inline threads are folded into the conversation card, not a band of their own",
    );
}

/// An embedded image is fetched, not dropped: until its bytes land the body shows a
/// placeholder naming it, which is also what asks the app for the fetch.
#[test]
fn markdown_image_stands_in_until_it_loads() {
    use helm::pull_requests::model::PrDetail;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Before:\n\n![Step 1](https://example.test/step-1.png)\n".to_owned(),
        ..PrDetail::default()
    };
    let files: Vec<CommitFile> = Vec::new();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness.get_by_label("Step 1 — loading image…");
    // The alt text is the image's, not a paragraph run: the surrounding prose stands.
    harness.get_by_label("Before:");
    let wanted: Vec<String> = harness.ctx.data(|d| {
        d.get_temp(helm::ui::pull_requests_view::md_image_wanted_id())
            .unwrap_or_default()
    });
    assert_eq!(
        wanted,
        vec!["https://example.test/step-1.png".to_owned()],
        "drawing the placeholder is what asks the app to fetch it",
    );
}

/// Drives the review surface over a PR whose detail carries `body`, on the
/// Conversation tab — for the markdown cases, which only need that body.
fn body_harness(body: &str) -> Harness<'static> {
    use helm::pull_requests::model::PrDetail;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: body.to_owned(),
        ..PrDetail::default()
    };
    let files: Vec<CommitFile> = Vec::new();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness
}

/// The conversation's scroll bar rides in the gutter, clear of the cards — egui floats
/// it against the scroll area's right edge, and an area left to shrink to its content
/// drags it back onto their border (§11).
#[test]
fn the_conversation_scroll_bar_clears_the_cards() {
    // A right-aligned table column ends flush against the card's own margin, which makes
    // it the rightmost thing the cards draw.
    let harness = body_harness(&format!(
        "| Step | Evidence |\n|------|-------:|\n{}",
        "| 1 | flush |\n".repeat(60)
    ));
    let bar = harness
        .get_all_by(|n| format!("{:?}", n.role()) == "ScrollBar")
        .map(|n| n.rect())
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .expect("the conversation scrolls, so it has a bar");
    let card = harness
        .get_all_by_label("flush")
        .map(|n| n.rect().right())
        .fold(0.0_f32, f32::max);
    assert!(
        bar.left() - card >= 40.0,
        "the bar starts at {} with the cards ending around {card} — it is riding on them",
        bar.left(),
    );
}

/// Table columns take what their content asks for: a column of single digits must not
/// hold a quarter of the table while the sentence beside it wraps for nothing (§11).
#[test]
fn a_table_column_of_digits_gives_its_room_to_the_sentence_beside_it() {
    let sentence = "Row action \"Emettre une nouvelle demande\" opens the request form";
    let harness = body_harness(&format!(
        "| Step | Action |\n|------|--------|\n| 1 | {sentence} |\n"
    ));
    let row = harness.get_by_label("1").rect().height();
    let action = harness.get_by_label(sentence).rect().height();
    assert!(
        action <= row * 1.5,
        "the sentence should sit on one line ({action} high, a row being {row}) — \
         an even split would wrap it",
    );
}

/// A screenshot embedded **in a table cell** is a picture there too — an evidence
/// column that names the file instead of showing it is not evidence (§11).
#[test]
fn markdown_image_in_a_table_cell_is_fetched_like_any_other() {
    let harness = body_harness(
        "| Step | Evidence |\n|------|----------|\n\
         | 1 | ![step-06](https://example.test/step-06.png) |\n",
    );
    harness.get_by_label("step-06 — loading image…");
    let wanted: Vec<String> = harness.ctx.data(|d| {
        d.get_temp(helm::ui::pull_requests_view::md_image_wanted_id())
            .unwrap_or_default()
    });
    assert_eq!(
        wanted,
        vec!["https://example.test/step-06.png".to_owned()],
        "a cell's picture asks for its bytes like one in a paragraph",
    );
}

/// A loaded picture opens full-surface on a click, and `Esc` closes it again — that
/// press being the viewer's, not the review's (§11).
#[test]
fn a_loaded_image_opens_in_the_viewer_and_esc_closes_it() {
    use helm::ui::pull_requests_view::{md_image_cache_id, md_viewer_open, MdImage};
    let url = "https://example.test/step-1.png";
    let mut harness = body_harness("![Step 1](https://example.test/step-1.png)\n");
    // Stand in for the fetch: the app writes the decoded texture into this cache.
    let texture = harness.ctx.load_texture(
        "test-image",
        egui::ColorImage::filled([8, 8], egui::Color32::RED),
        egui::TextureOptions::default(),
    );
    harness.ctx.data_mut(|d| {
        let images: &mut std::collections::HashMap<String, MdImage> =
            d.get_temp_mut_or_default(md_image_cache_id());
        images.insert(url.to_owned(), MdImage::Ready(texture));
    });
    harness.run();

    let opened = |h: &Harness<'static>| md_viewer_open(&h.ctx);
    assert!(!opened(&harness), "the viewer is closed until the click");
    harness
        .get_by(|n| format!("{:?}", n.role()) == "Image")
        .click();
    harness.run();
    assert!(opened(&harness), "clicking the picture opens the viewer");

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!opened(&harness), "Esc closes the viewer");
}

/// A link in a body is a link: clicking its run hands the URL to the app, which is
/// what opens the browser (§11).
#[test]
fn markdown_link_click_hands_the_url_to_the_app() {
    let mut harness = body_harness("See [the ticket](https://jira.test/browse/BNG-1) for why.\n");
    harness
        .get_by_label("https://jira.test/browse/BNG-1")
        .click();
    harness.run();
    let clicked: Vec<String> = harness.ctx.data(|d| {
        d.get_temp(helm::ui::pull_requests_view::md_link_clicked_id())
            .unwrap_or_default()
    });
    assert_eq!(clicked, vec!["https://jira.test/browse/BNG-1".to_owned()]);
}

/// Bitbucket's smart-link attribute run is markup, not prose: it never reaches the page.
#[test]
fn markdown_drops_the_bitbucket_smart_link_attributes() {
    let harness = body_harness(
        "Puisque [BNG-2](https://jira.test/browse/BNG-2){: data-inline-card='' }  résout tout.\n",
    );
    assert!(
        harness
            .query_by_label_contains("data-inline-card")
            .is_none(),
        "the attribute run must not read as prose",
    );
    harness.get_by_label_contains("résout tout");
}

/// A cell with no alt text still names the file while it loads.
#[test]
fn markdown_image_without_alt_names_the_file_while_it_loads() {
    let harness = body_harness("![](https://example.test/shots/step-06.png)\n");
    harness.get_by_label("step-06.png — loading image…");
}

/// A GFM table in a PR body is a **table**: one label per cell, not a paragraph of
/// pipes (which is what an unextended `pulldown-cmark` parser hands back).
#[test]
fn markdown_table_renders_as_cells_not_a_wall_of_pipes() {
    use helm::pull_requests::model::PrDetail;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "| Step | Result |\n|------|--------|\n| Open the list | OK |\n".to_owned(),
        ..PrDetail::default()
    };
    let files: Vec<CommitFile> = Vec::new();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness.get_by_label("Step");
    harness.get_by_label("Result");
    harness.get_by_label("Open the list");
    harness.get_by_label("OK");
}

#[test]
fn review_detail_loading_shows_a_loader_not_the_sections() {
    let (harness, _) =
        review_loading_harness(pr("acme/web", 1, "Fix the login flow", PrRole::ToReview));
    harness.get_by_label("Loading pull request…");
    assert!(
        harness.query_by_label("Markdown supported").is_none(),
        "detail sections must not render while the detail is loading"
    );
}

/// An inline-comment card shows the code it was left on (GitHub's diff hunk, stripped
/// of its `@@` header and diff markers) over the thread, and clicking it opens that
/// file at the commented line (T6).
#[test]
fn inline_comment_card_shows_context_and_opens_the_file() {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer-inline".to_owned(),
            body: "rename this".to_owned(),
            path: Some("src/main.rs".to_owned()),
            old_lineno: None,
            new_lineno: Some(2),
            id: Some(7),
            parent_id: None,
            context: Some("@@ -1,2 +1,3 @@\n fn main() {\n+    work();".to_owned()),
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    type Opened = Option<(usize, Option<u32>)>;
    let opened: Rc<Cell<Opened>> = Rc::new(Cell::new(None));
    let sink = opened.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
            if let Some(open) = action.open_inline_comment {
                sink.set(Some(open));
            }
        });
    harness.step();
    harness.step();
    // The code snippet is the click-to-open target for the anchored file.
    harness.get_by_label("Open src/main.rs line 2").click();
    harness.step();
    // src/main.rs is the second changed file; the card carries its new-side line.
    assert_eq!(opened.get(), Some((1, Some(2))));
}

/// Bitbucket inline comments carry no forge hunk; on the conversation page no file is
/// open, so the card windows the prefetched local diff (`comment_diffs`) into a code
/// preview — the snippet, not the bare "Open …" link, becomes the click target.
#[test]
fn inline_comment_card_windows_comment_diff_when_no_hunk() {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer-inline".to_owned(),
            body: "rename this".to_owned(),
            path: Some("src/main.rs".to_owned()),
            old_lineno: None,
            new_lineno: Some(2),
            id: Some(7),
            parent_id: None,
            context: None,
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let fd = FileDiff {
        path: "src/main.rs".to_owned(),
        binary: false,
        oversize: false,
        hunks: Vec::new(),
        source_lines: vec!["fn main() {".to_owned(), "    work();".to_owned()],
        image: None,
        editable: false,
    };
    let comment_diffs = vec![&fd];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    type Opened = Option<(usize, Option<u32>)>;
    let opened: Rc<Cell<Opened>> = Rc::new(Cell::new(None));
    let sink = opened.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: comment_diffs.clone(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
            if let Some(open) = action.open_inline_comment {
                sink.set(Some(open));
            }
        });
    harness.step();
    harness.step();
    // The snippet (not the fallback link) is the click target: a Button, not a Label.
    harness
        .get_by(|n| {
            format!("{:?}", n.role()) == "Button"
                && n.label().as_deref() == Some("Open src/main.rs line 2")
        })
        .click();
    harness.step();
    assert_eq!(opened.get(), Some((1, Some(2))));
}

#[test]
fn inline_comment_card_reply_emits_reply_to_thread() {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer-inline".to_owned(),
            body: "rename this".to_owned(),
            path: Some("src/main.rs".to_owned()),
            old_lineno: None,
            new_lineno: Some(2),
            id: Some(7),
            parent_id: None,
            context: Some("@@ -1,2 +1,3 @@\n fn main() {\n+    work();".to_owned()),
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let intents: Rc<RefCell<Vec<ReviewIntent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                // Collapse the rail so the composer's summary field isn't the
                // (ambiguous) second multiline input the reply editor is found by.
                true,
                FileViewMode::Flat,
            );
            sink.borrow_mut().extend(action.review_intents);
        });
    harness.run();
    // Open the reply editor (its state lives in the shared diff_view), type, send.
    harness.get_by_label("Reply").click();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput" && n.is_focused())
        .type_text("on it");
    harness.run();
    harness.get_by_label("Send reply").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::ReplyToThread { comment_id, body }
                if *comment_id == 7 && body == "on it"
        )),
        "the center card's reply editor must emit ReplyToThread, got {:?}",
        intents.borrow(),
    );
}

/// Review surface on the Conversation tab with one PR-level comment (so the card
/// carries a Reply pill) and the rail collapsed, capturing `back`: the `Esc` cascade
/// tests drive its composers.
fn conversation_harness() -> (Harness<'static>, Rc<Captured>) {
    use helm::pull_requests::model::PrComment;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer".to_owned(),
            body: "looks good".to_owned(),
            path: None,
            old_lineno: None,
            new_lineno: None,
            id: Some(11),
            parent_id: None,
            context: None,
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs")];
    let cap = Rc::new(Captured::default());
    let sink = cap.clone();
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                // Rail collapsed: the conversation composer is then the only multiline
                // field on the surface.
                true,
                FileViewMode::Flat,
            );
            if action.back {
                sink.back.set(true);
            }
        });
    harness.run();
    (harness, cap)
}

/// The Conversation tab's metadata rail names the PR's reviewers — each by name, with
/// their verdict on the ones who gave it — plus the checks and the labels (§11).
#[test]
fn conversation_rail_lists_the_reviewers_and_their_verdict() {
    let mut pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    pr_value.reviewers = vec![
        Reviewer {
            name: "dana".to_owned(),
            state: Review::Approved,
        },
        Reviewer {
            name: "sam".to_owned(),
            state: Review::Pending,
        },
    ];
    pr_value.labels = vec!["bug".to_owned()];
    let (harness, _) = review_harness(pr_value, vec![changed_file("src/lib.rs")], 460.0);
    assert!(harness.query_by_label("Reviewers").is_some());
    // The rail also carries what the conversation column used to repeat.
    assert!(harness.query_by_label("Checks").is_some());
    assert!(harness.query_by_label("Labels").is_some());
    assert!(harness.query_by_label("bug").is_some());
    assert!(harness.query_by_label("dana").is_some());
    assert!(harness.query_by_label("sam").is_some());
    // Only the approval is marked; the reviewer who still owes one says so.
    assert!(harness.query_by_label("Approved").is_some());
    assert!(harness.query_by_label("Awaiting review").is_some());
    assert!(harness.query_by_label("Changes requested").is_none());
}

/// `Esc` over an open reply editor closes that editor and stops there — leaving the
/// review takes a second press, with nothing left to close (pull-requests.md §11).
#[test]
fn esc_closes_the_open_reply_editor_before_leaving_the_review() {
    let (mut harness, cap) = conversation_harness();
    harness.get_by_label("Reply").click();
    harness.run();
    assert!(
        harness.query_by_label("Send reply").is_some(),
        "the Reply pill must have opened the composer",
    );

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(
        harness.query_by_label("Send reply").is_none(),
        "Esc must close the reply editor",
    );
    assert!(
        !cap.back.get(),
        "the press that closed the composer must not leave the review",
    );

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(
        cap.back.get(),
        "with no composer open, Esc returns to the list",
    );
}

/// The "Add a comment…" composer is always on screen, so `Esc` in it only drops the
/// field's focus — it must not fall through and leave the review (pull-requests.md §11).
#[test]
fn esc_in_the_add_comment_field_does_not_leave_the_review() {
    let (mut harness, cap) = conversation_harness();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .click();
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(
        !cap.back.get(),
        "Esc out of the comment field must not leave the review",
    );

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(
        cap.back.get(),
        "with the field no longer focused, Esc returns to the list",
    );
}

/// Review surface on the Conversation tab with one **line-anchored** open thread, so
/// the block carries the Reply + Resolve pair; the intents it raises are captured.
fn anchored_thread_harness() -> (Harness<'static>, Rc<RefCell<Vec<ReviewIntent>>>) {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer-inline".to_owned(),
            body: "rename this".to_owned(),
            path: Some("src/main.rs".to_owned()),
            old_lineno: None,
            new_lineno: Some(2),
            id: Some(7),
            parent_id: None,
            context: Some("@@ -1,2 +1,3 @@\n fn main() {\n+    work();".to_owned()),
            created_at: String::new(),
            resolved: false,
            thread_id: Some("PRRT_9".to_owned()),
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let intents: Rc<RefCell<Vec<ReviewIntent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
            sink.borrow_mut().extend(action.review_intents);
        });
    harness.run();
    (harness, intents)
}

#[test]
fn inline_comment_card_resolve_emits_resolve_thread() {
    let (mut harness, intents) = anchored_thread_harness();
    harness.get_by_label("Resolve").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::ResolveThread { thread_id, comment_id, resolved }
                if thread_id.as_deref() == Some("PRRT_9") && *comment_id == 7 && *resolved
        )),
        "the resolve pill must emit ResolveThread toggling to resolved, got {:?}",
        intents.borrow(),
    );
}

#[test]
fn a_conversation_thread_closes_on_an_action_bar_at_its_right_edge() {
    // The block reads body / hairline / bar, like the inline thread card and both
    // editors: the controls sit under the body, pushed to the block's right gutter —
    // not inline at the foot of the text column — and still read Reply then Resolve.
    let (harness, _) = anchored_thread_harness();
    let body = harness.get_by_label("rename this").rect();
    let reply = harness.get_by_label("Reply").rect();
    let resolve = harness.get_by_label("Resolve").rect();

    assert!(
        reply.top() >= body.bottom(),
        "the controls belong under the comment body, not beside it ({reply:?} vs {body:?})",
    );
    assert!(
        reply.left() > body.center().x,
        "the bar is right-aligned: its first control starts past the block's midline \
         ({reply:?} vs {body:?})",
    );
    assert!(
        reply.right() <= resolve.left(),
        "laid out right to left, the pair must still read Reply then Resolve",
    );
}

#[test]
fn resolved_inline_thread_collapses_and_reopens_on_click() {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer-inline".to_owned(),
            body: "rename this".to_owned(),
            path: Some("src/main.rs".to_owned()),
            old_lineno: None,
            new_lineno: Some(2),
            id: Some(7),
            parent_id: None,
            context: Some("@@ -1,2 +1,3 @@\n fn main() {\n+    work();".to_owned()),
            created_at: String::new(),
            resolved: true,
            thread_id: Some("PRRT_9".to_owned()),
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs"), changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
        });
    harness.run();
    // Folded into the resolved block: its header tallies what is put away, the thread is a
    // one-line row, and the body (its clickable snippet) is not drawn.
    assert!(harness
        .query_by_label("Resolved · 1 thread · 1 comment · 1 file")
        .is_some());
    assert!(harness.query_by_label("Open src/main.rs line 2").is_none());
    harness.get_by_label("src/main.rs:2 · 1 comment").click();
    harness.run();
    // Opened in place: the body is back.
    assert!(harness.query_by_label("Open src/main.rs line 2").is_some());
}

#[test]
fn conversation_composer_emits_post_conversation_comment() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: Vec::new(),
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let intents: Rc<RefCell<Vec<ReviewIntent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
            sink.borrow_mut().extend(action.review_intents);
        });
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .focus();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput")
        .type_text("ship it");
    harness.run();
    harness.get_by_label("Comment").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::PostConversationComment { parent: None, body }
                if body == "ship it"
        )),
        "the always-visible composer must emit a parent-less PostConversationComment, got {:?}",
        intents.borrow(),
    );
}

#[test]
fn conversation_card_reply_on_flat_comment_emits_top_level_comment() {
    use helm::pull_requests::model::PrComment;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer".to_owned(),
            body: "what about edge cases?".to_owned(),
            path: None,
            old_lineno: None,
            new_lineno: None,
            id: None,
            parent_id: None,
            context: None,
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let intents: Rc<RefCell<Vec<ReviewIntent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
            sink.borrow_mut().extend(action.review_intents);
        });
    harness.run();
    // A flat (GitHub) comment still carries a Reply pill; it has no id to nest under.
    harness.get_by_label("Reply").click();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput" && n.is_focused())
        .type_text("yes, covered");
    harness.run();
    harness.get_by_label("Send reply").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::PostConversationComment { parent: None, body }
                if body == "yes, covered"
        )),
        "replying under a flat card must post a parent-less top-level comment, got {:?}",
        intents.borrow(),
    );
}

#[test]
fn conversation_card_reply_emits_nested_post_conversation_comment() {
    use helm::pull_requests::model::PrComment;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer".to_owned(),
            body: "what about edge cases?".to_owned(),
            path: None,
            old_lineno: None,
            new_lineno: None,
            id: Some(7),
            parent_id: None,
            context: None,
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let intents: Rc<RefCell<Vec<ReviewIntent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = intents.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            let action = pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
            sink.borrow_mut().extend(action.review_intents);
        });
    harness.run();
    // The top-level card carries the only "Reply" affordance (no inline comments here).
    harness.get_by_label("Reply").click();
    harness.run();
    harness
        .get_by(|n| format!("{:?}", n.role()) == "MultilineTextInput" && n.is_focused())
        .type_text("covered below");
    harness.run();
    harness.get_by_label("Send reply").click();
    harness.run();

    assert!(
        intents.borrow().iter().any(|i| matches!(
            i,
            ReviewIntent::PostConversationComment { parent: Some(parent), body }
                if *parent == 7 && body == "covered below"
        )),
        "replying under a top-level card must nest via parent, got {:?}",
        intents.borrow(),
    );
}

/// The surface header's Merge asks the app to merge the *open* PR — distinct from a
/// list row's Merge, which names its row (pull-requests.md §5).
#[test]
fn review_header_merge_emits_the_open_pr_intent() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Merge").click();
    harness.step();
    assert!(cap.merge_open.get());
}

/// The verdict group lives in the Finish-review popover, right above the Submit whose
/// label follows the chosen verdict.
#[test]
fn the_verdict_group_drives_the_submit_label() {
    let (mut harness, _) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    assert!(
        harness.query_by_label("Nothing to submit").is_none(),
        "the composer only exists once the popover is open",
    );
    harness.get_by_label("Finish review").click();
    harness.run();
    // A comment-only review with nothing drafted has nothing to send.
    harness.get_by_label("Nothing to submit");
    harness.get_by_label("Approve").click();
    harness.run();
    // Two now: the verdict button and the submit, which named itself after the
    // chosen verdict.
    assert_eq!(harness.get_all_by_label("Approve").count(), 2);
    assert!(
        harness.query_by_label("Nothing to submit").is_none(),
        "an approval is submittable on its own",
    );
}

/// **Hide tests** drops test scaffolding from the rail's file list (§11).
#[test]
fn hide_tests_filters_test_files_from_the_rail() {
    let (mut harness, _) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        vec![
            changed_file("src/lib.rs"),
            changed_file("tests/lib_spec.rs"),
        ],
        460.0,
    );
    open_files(&mut harness);
    assert_eq!(harness.query_all_by_label("tests/lib_spec.rs").count(), 2);
    harness.get_by_label("Hide tests").click();
    harness.run();
    assert_eq!(harness.query_all_by_label("tests/lib_spec.rs").count(), 0);
    assert_eq!(
        harness.query_all_by_label("src/lib.rs").count(),
        2,
        "only the test scaffolding is filtered out",
    );
}

#[test]
fn conversation_reply_nests_under_its_parent() {
    use helm::pull_requests::model::PrComment;
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let comment = |id: u64, parent: Option<u64>, body: &str| PrComment {
        author: "reviewer".to_owned(),
        body: body.to_owned(),
        path: None,
        old_lineno: None,
        new_lineno: None,
        id: Some(id),
        parent_id: parent,
        context: None,
        created_at: String::new(),
        resolved: false,
        thread_id: None,
    };
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![
            comment(1, None, "root question"),
            comment(2, Some(1), "nested answer"),
        ],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: String::new(),
    };
    let files = vec![changed_file("src/lib.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: false,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                true,
                FileViewMode::Flat,
            );
        });
    harness.run();
    // Both comments render, but the reply nests under the root: a single thread →
    // a single Reply affordance (two top-level cards would carry two).
    harness.get_by_label_contains("root question");
    harness.get_by_label_contains("nested answer");
    assert_eq!(
        harness.get_all_by_label("Reply").count(),
        1,
        "a parented conversation reply must nest under its root, not stand as a second top-level card",
    );
}

/// The PR itself is in but its comments still load (the partial detail reply): the
/// conversation keeps the threads it already has and shows a loader under them, so
/// the body reads at once and the card never claims "no comments" early.
#[test]
fn review_comments_loading_shows_a_loader_under_the_threads() {
    use helm::pull_requests::model::{PrComment, PrDetail};
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let detail = PrDetail {
        body: "Describe the change".to_owned(),
        comments: vec![PrComment {
            author: "reviewer-top".to_owned(),
            body: "overall looks good".to_owned(),
            path: None,
            old_lineno: None,
            new_lineno: None,
            id: None,
            parent_id: None,
            context: None,
            created_at: String::new(),
            resolved: false,
            thread_id: None,
        }],
        ..PrDetail::default()
    };
    let files = vec![changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
    let mut file_views = std::collections::HashMap::new();
    let mut scroll_to_file = None;
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
                comments_loading: true,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diffs: Vec::new(),
                diff_errors: Vec::new(),
                scroll_to_file: &mut scroll_to_file,
                file_views: &mut file_views,
                comment_diffs: Vec::new(),
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
                current_user: None,
            };
            pull_requests_page(
                ui,
                &palette,
                &[],
                None,
                &PrSourceHints::default(),
                Some(&mut review),
                460.0,
                false,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness.get_by_label("Describe the change");
    harness.get_by_label("reviewer-top");
    harness.get_by_label("Loading comments…");
    assert!(
        harness.query_by_label("Loading pull request…").is_none(),
        "the partial detail must render its sections, not the whole-detail loader"
    );
}
