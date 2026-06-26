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
use helm::review::{FileComments, ForgeThreads, LineComment};
use helm::theme::Palette;
use helm::ui::diff_view::DiffViewState;
use helm::ui::file_list::FileViewMode;
use helm::ui::pull_requests_view::{
    pull_requests_page, CommitSelection, PrReviewView, PrSourceHints,
};

#[derive(Default)]
struct Captured {
    select: Cell<Option<usize>>,
    open_url: Cell<Option<String>>,
    checkout: Cell<bool>,
    set_detail_width: Cell<Option<f32>>,
    back: Cell<bool>,
    close_file: Cell<bool>,
    select_file: Cell<Option<usize>>,
    submit_review: Cell<bool>,
    set_file_view: Cell<Option<FileViewMode>>,
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
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: None,
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
    harness.get_by_label("Approve").click();
    harness.step();
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
    harness.step();
    assert!(cap.back.get());
}

fn sample_diff() -> FileDiff {
    FileDiff {
        path: "src/main.rs".to_owned(),
        binary: false,
        oversize: false,
        hunks: vec![Hunk {
            header: "@@ -1,2 +1,3 @@".to_owned(),
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
    }
}

/// Closing the open file clears the selection but stays on the review surface — it
/// is **not** the same as Back (which leaves to the list).
#[test]
fn closing_the_open_file_emits_close_not_back() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/main.rs")];
    let diff = sample_diff();
    let mut diff_view = DiffViewState::default();
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diff: Some(&diff),
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
        harness.query_by_label("Open in browser").is_none(),
        "PR-level actions live in the central detail and disappear while a file diff is open",
    );
    assert!(
        harness.query_by_label("Checkout").is_none(),
        "PR-level actions live in the central detail and disappear while a file diff is open",
    );
    harness.get_by_label("Close").click();
    harness.step();
    assert!(cap.close_file.get(), "Close emits close_file");
    assert!(
        !cap.back.get(),
        "closing the file does not leave the review surface",
    );
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

/// The commit band lists "All commits" plus one row per commit, and clicking a commit
/// emits a `select_commit` for that sha (per-commit diff: T5).
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &detail.commits,
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
    assert!(
        harness.query_by_label("All commits").is_some(),
        "the band offers the cumulative range",
    );
    harness.get_by_label("Wire the submit handler").click();
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
    harness.get_by_label("src");
    harness.get_by_label("src/lib.rs");
}

#[test]
fn changed_file_rows_show_quiet_review_and_agent_icons_without_counts() {
    let palette = Palette::light();
    let pr_value = pr("acme/web", 1, "Fix the login flow", PrRole::ToReview);
    let files = vec![changed_file("src/lib.rs")];
    let mut diff_view = DiffViewState::default();
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: Some(0),
                commits: &[],
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
    harness.get_by_label("Unread only").click();
    harness.step();
    assert!(
        harness.query_by_label("src/lib.rs").is_none(),
        "the already-opened file is hidden by the unread-only filter",
    );
    harness.get_by_label("src/main.rs");
}

#[test]
fn dragging_the_split_resizes_the_rail_width() {
    // The rail sits on the right; its resize handle is on the split line at
    // `body.right() - rail_width` = 1200 - 460 = 740. Dragging left widens the rail
    // (`rail_width - drag_delta.x`).
    let (mut harness, cap) = review_harness(
        pr("acme/web", 1, "Fix the login flow", PrRole::ToReview),
        Vec::new(),
        460.0,
    );
    let start = egui::pos2(740.0, 400.0);
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
        .expect("drag emits a new rail width");
    assert!(
        width > 460.0,
        "dragging the split left widens the rail, got {width}"
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
    harness.get_by_label("feature → main");
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
            },
            PrComment {
                author: "reviewer-inline".to_owned(),
                body: "rename this".to_owned(),
                path: Some("src/main.rs".to_owned()),
                old_lineno: None,
                new_lineno: Some(2),
                id: None,
                parent_id: None,
            },
        ],
        check_runs: Vec::new(),
        commits: Vec::new(),
    };
    let files = vec![changed_file("src/main.rs")];
    let mut diff_view = DiffViewState::default();
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
                detail_error: None,
                files: &files,
                files_loading: false,
                files_error: None,
                selected_file: None,
                commits: &[],
                selected_commit: None,
                diff: None,
                diff_loading: false,
                diff_error: None,
                diff_view: &mut diff_view,
                existing: &existing,
                draft: &draft,
                agent_notes: &agent_notes,
                agent: "claude",
                verdict: &mut verdict,
                summary: &mut summary,
                posting: false,
                post_error: None,
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
    harness.get_by_label("src/main.rs");
    harness.get_by_label("Conversation");
    harness.get_by_label("reviewer-top");
    assert!(
        harness.query_by_label("reviewer-inline").is_none(),
        "inline comments belong to the diff, not the detail conversation",
    );
}
