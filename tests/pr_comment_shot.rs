//! One-off: render the PR detail comment sections (Conversation + Inline) to a PNG so
//! the comment-card grammar can be eyeballed. Run on demand, not part of the gate:
//!   cargo test --features headless-verify --test pr_comment_shot -- --nocapture
#![cfg(feature = "headless-verify")]

use egui_kittest::Harness;

use helm::git::commit_detail::CommitFile;
use helm::git::status::ChangeKind;
use helm::pull_requests::model::{
    Checks, ForgeKind, PrComment, PrDetail, PrRole, PrState, PullRequest, Review, ReviewVerdict,
    Reviewer,
};
use helm::review::{FileComments, ForgeThreads};
use helm::theme::Palette;
use helm::ui::diff_view::DiffViewState;
use helm::ui::file_list::FileViewMode;
use helm::ui::pull_requests_view::{pull_requests_page, PrReviewView, PrSourceHints};

fn comment(author: &str, body: &str, id: u64, parent: Option<u64>, created_at: &str) -> PrComment {
    PrComment {
        author: author.to_owned(),
        body: body.to_owned(),
        path: None,
        old_lineno: None,
        new_lineno: None,
        id: Some(id),
        parent_id: parent,
        context: None,
        created_at: created_at.to_owned(),
        resolved: false,
        thread_id: None,
    }
}

#[test]
fn shot_pr_comments() {
    let palette = Palette::dark();
    let pr_value = PullRequest {
        forge_kind: ForgeKind::Bitbucket,
        repo_label: "acme/web".to_owned(),
        number: 142,
        title: "Harden the login flow".to_owned(),
        role: PrRole::ToReview,
        state: PrState::Open,
        author: "octocat".to_owned(),
        source_branch: "feature/login".to_owned(),
        dest_branch: "main".to_owned(),
        url: "https://example.test/acme/web/pull/142".to_owned(),
        updated_at: "2 days ago".to_owned(),
        checks: Checks::Passing,
        review: Review::Pending,
        reviewers: vec![Reviewer {
            name: "maria".to_owned(),
            state: Review::Pending,
        }],
        labels: Vec::new(),
    };
    let detail = PrDetail {
        body: "Adds rate limiting on the auth endpoint and tightens the session check. \
               Splitting the retry logic out next."
            .to_owned(),
        comments: vec![
            comment(
                "maria",
                "Overall this looks solid. One thing: can we cap the retry window so a \
                 burst can't keep the lock alive? Otherwise `try_acquire` may starve.",
                1,
                None,
                "2026-06-26T09:12:00Z",
            ),
            comment(
                "octocat",
                "Good catch — capping at 5 attempts then backing off.",
                2,
                Some(1),
                "2026-06-28T14:03:00Z",
            ),
            comment(
                "maria",
                "Perfect, that works for me.",
                3,
                Some(1),
                "2026-06-29T08:30:00Z",
            ),
            comment(
                "priya",
                "Left a couple of nits inline but nothing blocking.",
                4,
                None,
                "2026-06-29T11:00:00Z",
            ),
            PrComment {
                author: "maria".to_owned(),
                body: "Prefer an explicit error here over the silent `unwrap_or_default`.".to_owned(),
                path: Some("src/auth.rs".to_owned()),
                old_lineno: None,
                new_lineno: Some(2),
                id: Some(10),
                parent_id: None,
                context: Some("@@ -1,3 +1,4 @@\n fn login(req: Request) {\n+    rate_limit(&req);\n     verify(&req);".to_owned()),
                created_at: "2026-06-27T16:45:00Z".to_owned(),
                resolved: false,
                thread_id: Some("t-10".to_owned()),
            },
            PrComment {
                author: "octocat".to_owned(),
                body: "Done — returns `AuthError::RateLimited` now.".to_owned(),
                path: Some("src/auth.rs".to_owned()),
                old_lineno: None,
                new_lineno: Some(2),
                id: Some(11),
                parent_id: Some(10),
                context: None,
                created_at: "2026-06-29T09:05:00Z".to_owned(),
                resolved: false,
                thread_id: Some("t-10".to_owned()),
            },
        ],
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: "2026-06-26T08:00:00Z".to_owned(),
    };
    let files = vec![CommitFile {
        path: "src/auth.rs".to_owned(),
        kind: ChangeKind::Modified,
        additions: 12,
        deletions: 3,
    }];
    let mut diff_view = DiffViewState::default();
    let existing = ForgeThreads::new();
    let draft = FileComments::new();
    let agent_notes = FileComments::new();
    let mut verdict = ReviewVerdict::default();
    let mut summary = String::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(840.0, 1560.0))
        .build_ui(move |ui| {
            let mut review = PrReviewView {
                pr: &pr_value,
                detail: Some(&detail),
                detail_loading: false,
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
                360.0,
                true,
                FileViewMode::Flat,
            );
        });
    harness.step();
    harness.step();
    harness
        .render()
        .expect("wgpu render")
        .save("/private/tmp/claude-502/-Users-dbonan-Documents-dev-helm-studio/f22a2ca6-fd48-4ed5-b7bd-302351002226/scratchpad/pr_comments.png")
        .unwrap();
}
