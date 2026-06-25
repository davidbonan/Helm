//! UI E2E for the Pull Requests cockpit (pull-requests.md §5/§11): drives
//! `pull_requests_page` headless across both surfaces — the browse list (groups,
//! a row, the empty state, the row → select intent) and the review surface (the
//! header's Open-in-browser / Checkout / Ask-Claude intents, Back, a changed-file
//! click, and the draggable rail width).

use std::cell::Cell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::git::commit_detail::CommitFile;
use helm::git::status::ChangeKind;
use helm::pull_requests::model::{
    Checks, ForgeKind, PrRole, PrState, PullRequest, Review, ReviewVerdict, Reviewer,
};
use helm::review::{FileComments, ForgeThreads};
use helm::theme::Palette;
use helm::ui::diff_view::DiffViewState;
use helm::ui::pull_requests_view::{pull_requests_page, PrReviewView};

#[derive(Default)]
struct Captured {
    select: Cell<Option<usize>>,
    open_url: Cell<Option<String>>,
    checkout: Cell<Option<usize>>,
    set_detail_width: Cell<Option<f32>>,
    back: Cell<bool>,
    select_file: Cell<Option<usize>>,
    ask_claude: Cell<bool>,
    submit_review: Cell<bool>,
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
        url: format!("https://example.test/{repo}/pull/{number}"),
        updated_at: "2 days ago".to_owned(),
        checks: Checks::Passing,
        review: Review::Pending,
        reviewers: vec![Reviewer {
            name: "reviewer".to_owned(),
            state: Review::Pending,
        }],
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
            let action = pull_requests_page(ui, &palette, &prs, selected, None, detail_width);
            if action.select.is_some() {
                sink.select.set(action.select);
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
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                index: 0,
                pr: &pr_value,
                detail: None,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
            };
            let action = pull_requests_page(ui, &palette, &[], None, Some(&mut review), rail_width);
            if action.open_url.is_some() {
                sink.open_url.set(action.open_url.clone());
            }
            if action.checkout.is_some() {
                sink.checkout.set(action.checkout);
            }
            if action.set_detail_width.is_some() {
                sink.set_detail_width.set(action.set_detail_width);
            }
            if action.back {
                sink.back.set(true);
            }
            if action.select_file.is_some() {
                sink.select_file.set(action.select_file);
            }
            if action.ask_claude {
                sink.ask_claude.set(true);
            }
            if action.submit_review {
                sink.submit_review.set(true);
            }
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

#[test]
fn groups_to_review_then_mine_with_their_rows() {
    let (harness, _) = harness(
        vec![
            pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
            pr("acme/api", 2, "Bump the cache TTL", PrRole::Mine),
        ],
        None,
        460.0,
    );
    harness.get_by_label("To review");
    harness.get_by_label("Mine");
    harness.get_by_label("Fix the login flow");
    harness.get_by_label("Bump the cache TTL");
}

#[test]
fn empty_state_when_no_prs() {
    let (harness, _) = harness(Vec::new(), None, 460.0);
    harness.get_by_label("No pull requests");
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
fn review_header_open_in_browser_emits_the_url() {
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
fn review_header_checkout_emits_the_index() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Checkout").click();
    harness.step();
    assert_eq!(cap.checkout.get(), Some(0));
}

#[test]
fn review_header_ask_claude_emits_the_intent() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Ask Claude").click();
    harness.step();
    assert!(cap.ask_claude.get());
}

#[test]
fn review_composer_submit_emits_the_intent() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    harness.get_by_label("Submit review").click();
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
    harness.step();
    assert!(cap.back.get());
}

#[test]
fn clicking_a_changed_file_loads_its_diff() {
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        vec![changed_file("src/lib.rs"), changed_file("src/main.rs")],
        460.0,
    );
    harness.get_by_label("src/main.rs").click();
    harness.step();
    assert_eq!(cap.select_file.get(), Some(1));
}

#[test]
fn dragging_the_split_resizes_the_rail_width() {
    // The rail sits on the left; its resize handle is on the split line at
    // `body.left() + rail_width`. Dragging right widens the rail
    // (`rail_width + drag_delta.x`).
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
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
