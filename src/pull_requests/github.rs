//! GitHub source for the cockpit (pull-requests.md §3): pure `gh` argument
//! builders and I/O-free mappers from `gh … --json` output onto the domain
//! model. No process is spawned here — the runner (PR4) owns the shell-out;
//! this module only describes *what* to run and *how* to read the reply.
//!
//! Two list queries per repo (`is:open author:@me` and `review-requested:@me`)
//! because GitHub's `involves` qualifier excludes requested reviewers; roles are
//! re-derived from the cached login so an overlap collapses through `dedupe`.

use serde_json::{json, Value};

use crate::pull_requests::model::{
    CheckRun, Checks, DraftComment, ForgeKind, PrComment, PrCommit, PrDetail, PrRole, PrState,
    PullRequest, Review, ReviewVerdict, Reviewer,
};

const LIST_FIELDS: &str = "number,title,author,headRefName,baseRefName,url,updatedAt,isDraft,reviewDecision,reviewRequests,latestReviews,statusCheckRollup";
const DETAIL_FIELDS: &str = "body,comments,commits,statusCheckRollup";

/// `gh auth status` — exit 0 ⇒ the GitHub source is usable (pull-requests.md §3).
pub fn auth_status_args() -> Vec<String> {
    vec!["auth".into(), "status".into()]
}

/// `gh api user --jq .login` — resolves "me" once per session (§1).
pub fn current_login_args() -> Vec<String> {
    vec!["api".into(), "user".into(), "--jq".into(), ".login".into()]
}

/// Open PRs authored by me in `repo` (`owner/name`).
pub fn list_authored_args(repo: &str) -> Vec<String> {
    list_args(repo, "is:open author:@me")
}

/// Open PRs in `repo` where I am a requested reviewer.
pub fn list_review_requested_args(repo: &str) -> Vec<String> {
    list_args(repo, "is:open review-requested:@me")
}

fn list_args(repo: &str, search: &str) -> Vec<String> {
    vec![
        "pr".into(),
        "list".into(),
        "-R".into(),
        repo.into(),
        "--limit".into(),
        "100".into(),
        "--search".into(),
        search.into(),
        "--json".into(),
        LIST_FIELDS.into(),
    ]
}

/// `gh pr view <number>` for the detail panel of a selection.
pub fn view_args(repo: &str, number: u64) -> Vec<String> {
    vec![
        "pr".into(),
        "view".into(),
        number.to_string(),
        "-R".into(),
        repo.into(),
        "--json".into(),
        DETAIL_FIELDS.into(),
    ]
}

/// `gh api repos/{repo}/pulls/{n}/comments` — the REST **review** (line-level)
/// comments, which `gh pr view` omits. `--paginate` walks every page.
pub fn review_comments_args(repo: &str, number: u64) -> Vec<String> {
    vec![
        "api".into(),
        format!("repos/{repo}/pulls/{number}/comments"),
        "--paginate".into(),
    ]
}

/// Map the REST `pulls/{n}/comments` array onto inline `PrComment`s. `line` falls
/// back to `original_line` for comments on an outdated diff; `in_reply_to_id`
/// becomes `parent_id`.
pub fn parse_review_comments(json: &str) -> serde_json::Result<Vec<PrComment>> {
    let value: Value = serde_json::from_str(json)?;
    let comments = value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|c| {
                    // `side` is LEFT for a comment left on the deleted side, else
                    // the new side (the REST default); the number falls back to
                    // `original_line` for a comment on an outdated diff.
                    let num = c["line"]
                        .as_u64()
                        .or_else(|| c["original_line"].as_u64())
                        .map(|n| n as u32);
                    let (old_lineno, new_lineno) = match c["side"].as_str() {
                        Some("LEFT") => (num, None),
                        _ => (None, num),
                    };
                    PrComment {
                        author: c["user"]["login"].as_str().unwrap_or_default().to_owned(),
                        body: c["body"].as_str().unwrap_or_default().to_owned(),
                        path: c["path"].as_str().map(str::to_owned),
                        old_lineno,
                        new_lineno,
                        id: c["id"].as_u64(),
                        parent_id: c["in_reply_to_id"].as_u64(),
                        context: c["diff_hunk"].as_str().map(str::to_owned),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(comments)
}

/// `gh api repos/{repo}/pulls/{n}/reviews --method POST --input -` — submit a
/// review in one call (verdict + summary + inline comments); the body is fed on
/// stdin so paths and text never touch the argv.
pub fn submit_review_args(repo: &str, number: u64) -> Vec<String> {
    vec![
        "api".into(),
        format!("repos/{repo}/pulls/{number}/reviews"),
        "--method".into(),
        "POST".into(),
        "--input".into(),
        "-".into(),
    ]
}

/// JSON body for the reviews endpoint: the `event` for the verdict, an optional
/// summary `body`, and one `{path, line, body}` per drafted line comment.
pub fn submit_review_body(
    verdict: ReviewVerdict,
    summary: &str,
    comments: &[DraftComment],
) -> String {
    let event = match verdict {
        ReviewVerdict::Approve => "APPROVE",
        ReviewVerdict::RequestChanges => "REQUEST_CHANGES",
        ReviewVerdict::Comment => "COMMENT",
    };
    let mut body = json!({ "event": event });
    if !summary.trim().is_empty() {
        body["body"] = json!(summary);
    }
    if !comments.is_empty() {
        body["comments"] = json!(comments
            .iter()
            .map(|c| json!({ "path": c.path, "line": c.line, "body": c.body }))
            .collect::<Vec<_>>());
    }
    body.to_string()
}

/// `gh api repos/{repo}/pulls/{n}/comments/{id}/replies --method POST --input -` —
/// reply to an existing review-comment thread; the body is fed on stdin (§11).
pub fn reply_comment_args(repo: &str, number: u64, comment_id: u64) -> Vec<String> {
    vec![
        "api".into(),
        format!("repos/{repo}/pulls/{number}/comments/{comment_id}/replies"),
        "--method".into(),
        "POST".into(),
        "--input".into(),
        "-".into(),
    ]
}

/// JSON body for the replies endpoint: just the reply text under `body`.
pub fn reply_comment_body(body: &str) -> String {
    json!({ "body": body }).to_string()
}

/// `gh pr checkout <number>` (the plain-checkout path; PR7 prefers a worktree).
pub fn checkout_args(repo: &str, number: u64) -> Vec<String> {
    vec![
        "pr".into(),
        "checkout".into(),
        number.to_string(),
        "-R".into(),
        repo.into(),
    ]
}

/// Map a `gh pr list --json` array onto domain PRs, dropping any that concern
/// neither role for `me_login` (commented-on, assigned, …).
pub fn parse_list(
    json: &str,
    me_login: &str,
    repo_label: &str,
) -> serde_json::Result<Vec<PullRequest>> {
    let value: Value = serde_json::from_str(json)?;
    let prs = value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_pr(item, me_login, repo_label))
                .collect()
        })
        .unwrap_or_default();
    Ok(prs)
}

/// Map a `gh pr view --json` object onto the lazily-fetched detail.
pub fn parse_detail(json: &str) -> serde_json::Result<PrDetail> {
    let value: Value = serde_json::from_str(json)?;
    let comments = value["comments"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|c| PrComment {
                    author: c["author"]["login"].as_str().unwrap_or_default().to_owned(),
                    body: c["body"].as_str().unwrap_or_default().to_owned(),
                    path: None,
                    old_lineno: None,
                    new_lineno: None,
                    id: None,
                    parent_id: None,
                    context: None,
                })
                .collect()
        })
        .unwrap_or_default();
    let check_runs = value["statusCheckRollup"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| CheckRun {
                    name: check_item_name(item),
                    status: check_item_status(item),
                })
                .collect()
        })
        .unwrap_or_default();
    let commits = value["commits"]
        .as_array()
        .map(|items| items.iter().map(parse_commit).collect())
        .unwrap_or_default();
    Ok(PrDetail {
        body: value["body"].as_str().unwrap_or_default().to_owned(),
        comments,
        check_runs,
        commits,
    })
}

/// One entry of `gh pr view --json commits`: `oid` is the full hash (abbreviated to the
/// git-default 7 chars), `messageHeadline` the subject, the first `authors` entry the
/// display name (falling back to its login).
fn parse_commit(c: &Value) -> PrCommit {
    let sha = c["oid"].as_str().unwrap_or_default().to_owned();
    let short = sha.chars().take(7).collect();
    let author = c["authors"]
        .as_array()
        .and_then(|a| a.first())
        .map(|a| {
            let name = a["name"].as_str().unwrap_or_default();
            if name.is_empty() {
                a["login"].as_str().unwrap_or_default()
            } else {
                name
            }
        })
        .unwrap_or_default()
        .to_owned();
    PrCommit {
        sha,
        short,
        subject: c["messageHeadline"].as_str().unwrap_or_default().to_owned(),
        author,
    }
}

fn parse_pr(o: &Value, me: &str, repo_label: &str) -> Option<PullRequest> {
    let author = o["author"]["login"].as_str().unwrap_or_default().to_owned();
    let requested: Vec<String> = o["reviewRequests"]
        .as_array()
        .map(|items| items.iter().filter_map(reviewer_name).collect())
        .unwrap_or_default();
    let is_author = !me.is_empty() && author == me;
    let is_requested = requested.iter().any(|r| r == me);
    let role = PrRole::classify(is_author, is_requested)?;

    let state = if o["isDraft"].as_bool().unwrap_or(false) {
        PrState::Draft
    } else {
        PrState::Open
    };

    let mut reviewers: Vec<Reviewer> = o["latestReviews"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|r| {
                    Some(Reviewer {
                        name: r["author"]["login"].as_str()?.to_owned(),
                        state: map_review_state(r["state"].as_str().unwrap_or_default()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    for name in requested {
        if !reviewers.iter().any(|rv| rv.name == name) {
            reviewers.push(Reviewer {
                name,
                state: Review::Pending,
            });
        }
    }

    Some(PullRequest {
        forge_kind: ForgeKind::GitHub,
        repo_label: repo_label.to_owned(),
        number: o["number"].as_u64().unwrap_or_default(),
        title: o["title"].as_str().unwrap_or_default().to_owned(),
        role,
        state,
        author,
        source_branch: o["headRefName"].as_str().unwrap_or_default().to_owned(),
        dest_branch: o["baseRefName"].as_str().unwrap_or_default().to_owned(),
        url: o["url"].as_str().unwrap_or_default().to_owned(),
        updated_at: o["updatedAt"].as_str().unwrap_or_default().to_owned(),
        checks: aggregate_checks(&o["statusCheckRollup"]),
        review: map_review_decision(o["reviewDecision"].as_str().unwrap_or_default()),
        reviewers,
    })
}

/// A requested-reviewer entry is a `User {login}` or a `Team {name|slug}`.
fn reviewer_name(v: &Value) -> Option<String> {
    v.get("login")
        .or_else(|| v.get("name"))
        .or_else(|| v.get("slug"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn map_review_state(state: &str) -> Review {
    match state {
        "APPROVED" => Review::Approved,
        "CHANGES_REQUESTED" => Review::ChangesRequested,
        _ => Review::Pending,
    }
}

fn map_review_decision(decision: &str) -> Review {
    match decision {
        "APPROVED" => Review::Approved,
        "CHANGES_REQUESTED" => Review::ChangesRequested,
        "REVIEW_REQUIRED" => Review::Pending,
        _ => Review::None,
    }
}

fn check_item_name(item: &Value) -> String {
    item.get("name")
        .or_else(|| item.get("context"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Status of one `statusCheckRollup` entry: a GraphQL `CheckRun` (status +
/// conclusion) or a legacy `StatusContext` (state).
fn check_item_status(item: &Value) -> Checks {
    if item.get("__typename").and_then(Value::as_str) == Some("StatusContext") {
        return match item
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "SUCCESS" => Checks::Passing,
            "FAILURE" | "ERROR" => Checks::Failing,
            _ => Checks::Pending,
        };
    }
    if item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        != "COMPLETED"
    {
        return Checks::Pending;
    }
    match item
        .get("conclusion")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "SUCCESS" | "NEUTRAL" | "SKIPPED" => Checks::Passing,
        "" => Checks::Pending,
        _ => Checks::Failing,
    }
}

/// Aggregate a rollup: any failing ⇒ Failing, else any pending ⇒ Pending, else
/// Passing; an empty/absent rollup ⇒ None.
fn aggregate_checks(rollup: &Value) -> Checks {
    let Some(items) = rollup.as_array() else {
        return Checks::None;
    };
    if items.is_empty() {
        return Checks::None;
    }
    let mut pending = false;
    for item in items {
        match check_item_status(item) {
            Checks::Failing => return Checks::Failing,
            Checks::Pending => pending = true,
            _ => {}
        }
    }
    if pending {
        Checks::Pending
    } else {
        Checks::Passing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../../tests/fixtures/github_pr_list.json");
    const DETAIL: &str = include_str!("../../tests/fixtures/github_pr_detail.json");

    #[test]
    fn list_args_carry_repo_search_and_fields() {
        assert_eq!(
            list_authored_args("acme/web"),
            vec![
                "pr",
                "list",
                "-R",
                "acme/web",
                "--limit",
                "100",
                "--search",
                "is:open author:@me",
                "--json",
                LIST_FIELDS,
            ]
        );
        assert_eq!(
            list_review_requested_args("acme/web")[7],
            "is:open review-requested:@me"
        );
    }

    #[test]
    fn view_and_checkout_args_target_the_number() {
        assert_eq!(
            view_args("acme/web", 42),
            vec![
                "pr",
                "view",
                "42",
                "-R",
                "acme/web",
                "--json",
                DETAIL_FIELDS
            ]
        );
        assert_eq!(
            checkout_args("acme/web", 7),
            vec!["pr", "checkout", "7", "-R", "acme/web"]
        );
    }

    #[test]
    fn parse_list_classifies_roles_and_drops_uninvolved() {
        let prs = parse_list(LIST, "alice", "acme/webapp").unwrap();
        // PR 9 (alice only commented) is dropped.
        assert_eq!(prs.len(), 2);

        let mine = &prs[0];
        assert_eq!(mine.number, 42);
        assert_eq!(mine.role, PrRole::Mine);
        assert_eq!(mine.state, PrState::Open);
        assert_eq!(mine.author, "alice");
        assert_eq!(mine.source_branch, "feature/login");
        assert_eq!(mine.dest_branch, "main");
        assert_eq!(mine.url, "https://github.com/acme/webapp/pull/42");

        let to_review = &prs[1];
        assert_eq!(to_review.number, 7);
        assert_eq!(to_review.role, PrRole::ToReview);
        assert_eq!(to_review.state, PrState::Draft);
    }

    #[test]
    fn parse_list_aggregates_checks_and_review_decision() {
        let prs = parse_list(LIST, "alice", "acme/webapp").unwrap();
        // PR 42: SUCCESS + SKIPPED + StatusContext SUCCESS ⇒ Passing.
        assert_eq!(prs[0].checks, Checks::Passing);
        assert_eq!(prs[0].review, Review::Approved);
        // PR 7: a FAILURE ⇒ Failing; REVIEW_REQUIRED ⇒ Pending.
        assert_eq!(prs[1].checks, Checks::Failing);
        assert_eq!(prs[1].review, Review::Pending);
    }

    #[test]
    fn parse_list_collects_requested_and_latest_reviewers() {
        let prs = parse_list(LIST, "alice", "acme/webapp").unwrap();
        let reviewers = &prs[0].reviewers;
        // carol reviewed (Approved), bob is still requested (Pending).
        assert!(reviewers
            .iter()
            .any(|r| r.name == "carol" && r.state == Review::Approved));
        assert!(reviewers
            .iter()
            .any(|r| r.name == "bob" && r.state == Review::Pending));
    }

    #[test]
    fn parse_detail_reads_body_comments_and_check_runs() {
        let detail = parse_detail(DETAIL).unwrap();
        assert!(detail.body.starts_with("This adds the login page."));
        assert_eq!(detail.comments.len(), 2);
        assert_eq!(detail.comments[0].author, "dave");
        assert_eq!(detail.comments[0].body, "Looks good to me");

        assert_eq!(detail.check_runs.len(), 3);
        assert_eq!(detail.check_runs[0].status, Checks::Passing);
        assert_eq!(detail.check_runs[1].status, Checks::Failing);
        assert_eq!(detail.check_runs[2].name, "ci/circleci: deploy");
        assert_eq!(detail.check_runs[2].status, Checks::Pending);

        assert_eq!(detail.commits.len(), 2);
        assert_eq!(
            detail.commits[0].sha,
            "1111111111111111111111111111111111111111"
        );
        assert_eq!(detail.commits[0].short, "1111111");
        assert_eq!(detail.commits[0].subject, "Add login form");
        assert_eq!(detail.commits[0].author, "Alice Doe");
        // No display name → fall back to the login.
        assert_eq!(detail.commits[1].author, "bob");
    }

    #[test]
    fn parse_review_comments_reads_inline_anchors_and_replies() {
        let json = r#"[
          {"id": 1, "user": {"login": "dave"}, "body": "nit: rename", "path": "src/a.rs", "line": 12, "diff_hunk": "@@ -10,3 +10,4 @@\n ctx\n-old\n+new"},
          {"id": 2, "in_reply_to_id": 1, "user": {"login": "alice"}, "body": "done", "path": "src/a.rs", "original_line": 9}
        ]"#;
        let comments = parse_review_comments(json).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].path.as_deref(), Some("src/a.rs"));
        assert_eq!(comments[0].new_lineno, Some(12));
        assert_eq!(comments[0].old_lineno, None);
        assert_eq!(comments[0].id, Some(1));
        assert_eq!(comments[0].parent_id, None);
        // the inline comment carries the diff hunk it was left on as code context.
        assert_eq!(
            comments[0].context.as_deref(),
            Some("@@ -10,3 +10,4 @@\n ctx\n-old\n+new"),
        );
        // line falls back to original_line; the reply links to its parent and has no hunk.
        assert_eq!(comments[1].new_lineno, Some(9));
        assert_eq!(comments[1].parent_id, Some(1));
        assert_eq!(comments[1].context, None);
    }

    #[test]
    fn review_comments_args_target_the_rest_endpoint() {
        assert_eq!(
            review_comments_args("acme/web", 42),
            vec!["api", "repos/acme/web/pulls/42/comments", "--paginate"]
        );
    }

    #[test]
    fn submit_review_args_post_the_reviews_endpoint_via_stdin() {
        assert_eq!(
            submit_review_args("acme/web", 42),
            vec![
                "api",
                "repos/acme/web/pulls/42/reviews",
                "--method",
                "POST",
                "--input",
                "-"
            ]
        );
    }

    #[test]
    fn submit_review_body_carries_event_summary_and_line_comments() {
        let comments = vec![
            DraftComment {
                path: "src/a.rs".to_owned(),
                line: 12,
                body: "rename this".to_owned(),
            },
            DraftComment {
                path: "src/b.rs".to_owned(),
                line: 3,
                body: "drop the clone".to_owned(),
            },
        ];
        let body = submit_review_body(
            ReviewVerdict::RequestChanges,
            "overall looks close",
            &comments,
        );
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["event"], "REQUEST_CHANGES");
        assert_eq!(parsed["body"], "overall looks close");
        let arr = parsed["comments"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["path"], "src/a.rs");
        assert_eq!(arr[0]["line"], 12);
        assert_eq!(arr[0]["body"], "rename this");
        assert_eq!(arr[1]["path"], "src/b.rs");
    }

    #[test]
    fn submit_review_body_omits_empty_summary_and_comments() {
        let body = submit_review_body(ReviewVerdict::Approve, "  ", &[]);
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["event"], "APPROVE");
        assert!(parsed.get("body").is_none());
        assert!(parsed.get("comments").is_none());
    }

    #[test]
    fn reply_comment_args_post_the_thread_replies_endpoint_via_stdin() {
        assert_eq!(
            reply_comment_args("acme/web", 42, 99),
            vec![
                "api",
                "repos/acme/web/pulls/42/comments/99/replies",
                "--method",
                "POST",
                "--input",
                "-"
            ]
        );
    }

    #[test]
    fn reply_comment_body_carries_only_the_text() {
        let parsed: Value = serde_json::from_str(&reply_comment_body("on it")).unwrap();
        assert_eq!(parsed["body"], "on it");
    }

    #[test]
    fn pending_check_run_makes_aggregate_pending() {
        // PR 9 in the fixture has an IN_PROGRESS run; parse it directly since
        // role classification drops it from the list for "alice".
        let prs = parse_list(LIST, "dave", "acme/webapp").unwrap();
        let pr9 = prs.iter().find(|p| p.number == 9).unwrap();
        assert_eq!(pr9.checks, Checks::Pending);
    }
}
