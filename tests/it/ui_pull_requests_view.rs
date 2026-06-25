//! UI E2E for the Pull Requests cockpit (pull-requests.md §5): drives
//! `pull_requests_page` headless and checks the To-review / Mine groups, a row
//! rendering, the empty state, the row → select intent, the detail panel's
//! Open-in-browser / Checkout intents, and the draggable split width.

use std::cell::Cell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use helm::pull_requests::model::{
    Checks, ForgeKind, PrRole, PrState, PullRequest, Review, Reviewer,
};
use helm::theme::Palette;
use helm::ui::pull_requests_view::pull_requests_page;

#[derive(Default)]
struct Captured {
    select: Cell<Option<usize>>,
    open_url: Cell<Option<String>>,
    checkout: Cell<Option<usize>>,
    set_detail_width: Cell<Option<f32>>,
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
            if action.open_url.is_some() {
                sink.open_url.set(action.open_url.clone());
            }
            if action.checkout.is_some() {
                sink.checkout.set(action.checkout);
            }
            if action.set_detail_width.is_some() {
                sink.set_detail_width.set(action.set_detail_width);
            }
        });
    harness.step();
    harness.step();
    (harness, cap)
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
fn detail_panel_open_in_browser_emits_the_url() {
    let (mut harness, cap) = harness(
        vec![pr("acme/web", 1, "Fix the login flow", PrRole::ToReview)],
        Some(0),
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
fn detail_panel_checkout_emits_the_index() {
    let (mut harness, cap) = harness(
        vec![pr("acme/web", 1, "Fix the login flow", PrRole::ToReview)],
        Some(0),
        460.0,
    );
    harness.get_by_label("Checkout").click();
    harness.step();
    assert_eq!(cap.checkout.get(), Some(0));
}

#[test]
fn dragging_the_split_resizes_the_detail_width() {
    // The handle sits on the split line, just left of the detail panel; the panel's
    // inner content (the "Open in browser" button) starts PANEL_PAD_X (18) past it.
    // Dragging left widens the detail panel (`detail_width - drag_delta.x`).
    let (mut harness, cap) = harness(
        vec![pr("acme/web", 1, "Fix the login flow", PrRole::ToReview)],
        Some(0),
        460.0,
    );
    let button = harness.get_by_label("Open in browser").rect();
    let start = egui::pos2(button.left() - 18.0, button.center().y);
    let end = start - egui::vec2(60.0, 0.0);
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
        .expect("drag emits a new detail width");
    assert!(
        width > 460.0,
        "dragging the split left widens the detail panel, got {width}"
    );
}
