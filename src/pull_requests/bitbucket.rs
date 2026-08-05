//! Bitbucket Cloud source for the cockpit (pull-requests.md §3): pure `2.0` URL
//! builders, the Basic-auth header, and I/O-free mappers from the REST JSON onto
//! the domain model. No request is made here — the runner (PR4) drives `curl`
//! (the `update.rs` idiom); this module only builds the URLs/header and reads
//! the replies. Roles are derived from the cached account uuid (§1).

use serde_json::{json, Value};

use crate::pull_requests::model::{
    Checks, ForgeKind, PrComment, PrCommit, PrDetail, PrRole, PrState, PullRequest, Review,
    Reviewer,
};

const API: &str = "https://api.bitbucket.org/2.0";

/// `GET /2.0/user` — resolves "me" once per session into an account uuid (§1).
pub fn current_user_url() -> String {
    format!("{API}/user")
}

/// Open PRs `me_uuid` authored (`role` ⇒ `Mine`). The role can't be re-derived
/// from the reply (the query, not the payload, says why a PR matched), so it comes
/// from which query found the PR, like the GitHub two-query split.
pub fn authored_url(workspace: &str, repo: &str, me_uuid: &str) -> String {
    role_filtered_url(workspace, repo, "author.uuid", me_uuid)
}

/// Open PRs where `me_uuid` is a requested reviewer (`role` ⇒ `ToReview`).
pub fn reviewing_url(workspace: &str, repo: &str, me_uuid: &str) -> String {
    role_filtered_url(workspace, repo, "reviewers.uuid", me_uuid)
}

/// `…/pullrequests?q=<state+role filter>` — the state is folded into `q` (a
/// separate `state=` param is ignored once `q` is present) and the whole BBQL
/// expression is percent-encoded, since the runner hands the URL to `curl` raw.
/// `fields=+values.participants` augments the list's partial representation with
/// the per-reviewer decision (omitted by default), so the cockpit can show the
/// reviewer roster and a review status without a per-PR round-trip.
fn role_filtered_url(workspace: &str, repo: &str, field: &str, me_uuid: &str) -> String {
    let q = encode(&format!("state=\"OPEN\" AND {field}=\"{me_uuid}\""));
    format!("{API}/repositories/{workspace}/{repo}/pullrequests?q={q}&fields=%2Bvalues.participants&pagelen=50")
}

/// A single PR (carries the rendered description for the detail panel).
pub fn pull_request_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}")
}

/// A PR's comment thread (read-only in the cockpit).
pub fn comments_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/comments?pagelen=100")
}

/// A PR's commits (per-commit diff: T5). Paginated like comments; Bitbucket returns
/// them newest-first, so the runner reverses to the oldest-first invariant.
pub fn commits_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/commits?pagelen=100")
}

/// `…/pullrequests/{id}/comments` — the POST target for a new comment (no query,
/// unlike the read `comments_url`). Used for both summary and inline comments.
pub fn post_comment_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/comments")
}

/// `…/pullrequests/{id}/approve` — POST records the caller's approval (§11).
pub fn approve_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/approve")
}

/// `…/pullrequests/{id}/request-changes` — POST records "changes requested" (§11).
pub fn request_changes_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/request-changes")
}

/// `…/pullrequests/{id}/merge` — POST merges the PR (pull-requests.md §5).
pub fn merge_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/merge")
}

/// POST body for a merge: the plain merge-commit strategy, matching `gh pr merge
/// --merge`, and `close_source_branch` left off — helm may hold a worktree on that
/// branch (pull-requests.md §7).
pub fn merge_body() -> String {
    serde_json::json!({ "merge_strategy": "merge_commit", "close_source_branch": false })
        .to_string()
}

/// `…/comments/{comment_id}/resolve` — POST resolves the thread, DELETE reopens it
/// (pull-requests.md §11). The comment id is the thread root's numeric id.
pub fn resolve_comment_url(workspace: &str, repo: &str, id: u64, comment_id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/comments/{comment_id}/resolve")
}

/// POST body for an inline comment: the text plus a `{path, to}` anchor on the
/// new side of the diff (matches how `parse_detail` reads `inline.to`).
pub fn inline_comment_body(path: &str, line: u32, raw: &str) -> String {
    json!({ "content": { "raw": raw }, "inline": { "path": path, "to": line } }).to_string()
}

/// POST body for a top-level (summary) comment — text only, no anchor.
pub fn summary_comment_body(raw: &str) -> String {
    json!({ "content": { "raw": raw } }).to_string()
}

/// POST body for a reply: the text plus a `parent` id, so Bitbucket nests it under
/// the thread root rather than starting a new comment (pull-requests.md §11).
pub fn reply_comment_body(parent_id: u64, raw: &str) -> String {
    json!({ "content": { "raw": raw }, "parent": { "id": parent_id } }).to_string()
}

/// `Authorization: Basic base64(email:token)` for the `curl` requests (§3).
pub fn basic_auth_header(email: &str, token: &str) -> String {
    format!("Basic {}", base64(&format!("{email}:{token}")))
}

/// Account uuid from `GET /2.0/user`, the identity PRs are classified against.
pub fn parse_current_user(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value["uuid"].as_str().map(str::to_owned)
}

/// Display name from `GET /2.0/user` (`display_name`, then `nickname`) — the
/// conversation composer avatar's initials source (§11), since the uuid isn't
/// human-readable.
pub fn parse_current_user_display_name(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value["display_name"]
        .as_str()
        .or_else(|| value["nickname"].as_str())
        .map(str::to_owned)
}

/// The human-readable `message` from a Bitbucket error reply
/// (`{"error":{"message":"…"}}`) — surfaced verbatim so a 403/404 names its real
/// cause (e.g. a missing scope) instead of a bare "unreachable".
pub fn parse_error_message(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value["error"]["message"].as_str().map(str::to_owned)
}

/// Map a paginated `pullrequests` page onto domain PRs, all carrying `role` (the
/// query already filtered by author/reviewer, so every entry concerns that role).
pub fn parse_list(
    json: &str,
    repo_label: &str,
    role: PrRole,
) -> serde_json::Result<Vec<PullRequest>> {
    let value: Value = serde_json::from_str(json)?;
    let prs = value["values"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| parse_pr(item, repo_label, role))
                .collect()
        })
        .unwrap_or_default();
    Ok(prs)
}

/// Map a PR object (`description`) and its comments page onto the detail. Bitbucket
/// has no rollup on the PR, so `check_runs` stays empty (pull-requests.md §10).
pub fn parse_detail(detail_json: &str, comments_json: &str) -> serde_json::Result<PrDetail> {
    Ok(PrDetail {
        body: parse_body(detail_json)?,
        comments: parse_comments(comments_json)?,
        check_runs: Vec::new(),
        commits: Vec::new(),
        created_at: parse_created_on(detail_json)?,
    })
}

/// One `pullrequests/{id}/commits` page mapped onto domain commits (per-commit diff:
/// T5); the runner follows `next_page` to accumulate every page. `hash` is the full
/// sha (abbreviated to the git-default 7 chars), the message's first line the subject,
/// and the author's `display_name` (falling back to the raw `Name <email>`) the name.
pub fn parse_commits(commits_json: &str) -> serde_json::Result<Vec<PrCommit>> {
    let page: Value = serde_json::from_str(commits_json)?;
    let commits = page["values"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|c| {
                    let sha = c["hash"].as_str().unwrap_or_default().to_owned();
                    let short = sha.chars().take(7).collect();
                    let subject = c["message"]
                        .as_str()
                        .unwrap_or_default()
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    let author = c["author"]["user"]["display_name"]
                        .as_str()
                        .or_else(|| c["author"]["raw"].as_str())
                        .unwrap_or_default()
                        .to_owned();
                    PrCommit {
                        sha,
                        short,
                        subject,
                        author,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(commits)
}

/// The PR description (`summary.raw`, falling back to `description`).
pub fn parse_body(detail_json: &str) -> serde_json::Result<String> {
    let detail: Value = serde_json::from_str(detail_json)?;
    Ok(detail["summary"]["raw"]
        .as_str()
        .or_else(|| detail["description"].as_str())
        .unwrap_or_default()
        .replace('\r', ""))
}

/// The PR creation timestamp (`created_on`, ISO-8601).
pub fn parse_created_on(detail_json: &str) -> serde_json::Result<String> {
    let detail: Value = serde_json::from_str(detail_json)?;
    Ok(detail["created_on"].as_str().unwrap_or_default().to_owned())
}

/// One `pullrequests/{id}/comments` page mapped onto domain comments; the runner
/// follows `next_page` to accumulate every page (pull-requests.md §10).
pub fn parse_comments(comments_json: &str) -> serde_json::Result<Vec<PrComment>> {
    let comments_page: Value = serde_json::from_str(comments_json)?;
    let comments = comments_page["values"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|c| {
                    // A deleted comment is kept as a tombstone (`deleted: true`, blank
                    // `content.raw`) so its replies survive; drop it, else it renders as
                    // an empty, never-disappearing card.
                    if c["deleted"].as_bool() == Some(true) {
                        return None;
                    }
                    let body = c["content"]["raw"].as_str()?.to_owned();
                    let inline = &c["inline"];
                    // `to` anchors the new side (added/context), `from` the old
                    // (deleted) side; an unchanged line carries both, so prefer
                    // `to` and keep the anchor single-sided.
                    let (old_lineno, new_lineno) = match inline["to"].as_u64() {
                        Some(to) => (None, Some(to as u32)),
                        None => (inline["from"].as_u64().map(|n| n as u32), None),
                    };
                    Some(PrComment {
                        author: c["user"]["display_name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        body: body.replace('\r', ""),
                        path: inline["path"].as_str().map(str::to_owned),
                        old_lineno,
                        new_lineno,
                        id: c["id"].as_u64(),
                        parent_id: c["parent"]["id"].as_u64(),
                        context: None,
                        created_at: c["created_on"].as_str().unwrap_or_default().to_owned(),
                        // `resolution` is an object when the thread is resolved, null
                        // otherwise; Bitbucket has no thread node id (it resolves by
                        // comment id), so `thread_id` stays None.
                        resolved: !c["resolution"].is_null(),
                        thread_id: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(comments)
}

/// The absolute URL of the next page of a paginated `2.0` collection, or `None`
/// on the last page — Bitbucket caps a page at 50 entries (pull-requests.md §3).
pub fn next_page(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value["next"].as_str().map(str::to_owned)
}

/// Map one PR object onto the domain. The reviewer roster and each reviewer's
/// decision come from `participants` (requested via `fields`, see `role_filtered_url`);
/// labels stay empty (Bitbucket has no PR labels).
fn parse_pr(o: &Value, repo_label: &str, role: PrRole) -> PullRequest {
    let state = if o["draft"].as_bool().unwrap_or(false) {
        PrState::Draft
    } else {
        PrState::Open
    };
    let reviewers = parse_reviewers(o);
    let review = aggregate_review(&reviewers);

    PullRequest {
        forge_kind: ForgeKind::Bitbucket,
        repo_label: repo_label.to_owned(),
        number: o["id"].as_u64().unwrap_or_default(),
        title: o["title"].as_str().unwrap_or_default().to_owned(),
        role,
        state,
        author: o["author"]["display_name"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        source_branch: o["source"]["branch"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        dest_branch: o["destination"]["branch"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        url: o["links"]["html"]["href"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        updated_at: o["updated_on"].as_str().unwrap_or_default().to_owned(),
        checks: Checks::None,
        review,
        reviewers,
        labels: Vec::new(),
        // A ± tally would need one `diffstat` request per PR (model §4).
        diffstat: None,
        comment_count: o["comment_count"].as_u64().map(|n| n as u32),
    }
}

/// The reviewer roster with each reviewer's decision, read from `participants`
/// (entries with `role == "REVIEWER"`); a reviewer who hasn't decided is Pending.
fn parse_reviewers(o: &Value) -> Vec<Reviewer> {
    o["participants"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .filter(|p| p["role"].as_str() == Some("REVIEWER"))
                .filter_map(|p| {
                    Some(Reviewer {
                        name: p["user"]["display_name"].as_str()?.to_owned(),
                        state: map_participant_state(p),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One participant's review decision (`state`, with `approved` as a fallback).
fn map_participant_state(p: &Value) -> Review {
    match p["state"].as_str() {
        Some("approved") => Review::Approved,
        Some("changes_requested") => Review::ChangesRequested,
        _ if p["approved"].as_bool().unwrap_or(false) => Review::Approved,
        _ => Review::Pending,
    }
}

/// Collapse the roster into the list's one-line review status: changes-requested
/// dominates, then any approval; a roster with no decision yet is Pending ("In
/// review"), an empty roster is None ("Open") — parity with GitHub's decision.
fn aggregate_review(reviewers: &[Reviewer]) -> Review {
    if reviewers
        .iter()
        .any(|r| r.state == Review::ChangesRequested)
    {
        Review::ChangesRequested
    } else if reviewers.iter().any(|r| r.state == Review::Approved) {
        Review::Approved
    } else if reviewers.is_empty() {
        Review::None
    } else {
        Review::Pending
    }
}

/// Percent-encode a BBQL `q` value (RFC 3986 unreserved set kept verbatim) so the
/// spaces/quotes/braces survive being passed to `curl` as a raw URL.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Standard base64 (no external dep — spec §3 forbids a new runtime crate).
fn base64(input: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../../tests/fixtures/bitbucket_pr_list.json");
    const DETAIL: &str = include_str!("../../tests/fixtures/bitbucket_pr_detail.json");
    const COMMENTS: &str = include_str!("../../tests/fixtures/bitbucket_pr_comments.json");
    const COMMITS: &str = include_str!("../../tests/fixtures/bitbucket_pr_commits.json");

    #[test]
    fn url_builders_target_the_cloud_2_0_endpoints() {
        assert_eq!(current_user_url(), "https://api.bitbucket.org/2.0/user");
        // The role filter folds OPEN into a percent-encoded `q` (state= alone is
        // dropped once q is present), differing only by author/reviewers field.
        assert_eq!(
            authored_url("team", "repo", "{me}"),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests?q=state%3D%22OPEN%22%20AND%20author.uuid%3D%22%7Bme%7D%22&fields=%2Bvalues.participants&pagelen=50"
        );
        assert_eq!(
            reviewing_url("team", "repo", "{me}"),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests?q=state%3D%22OPEN%22%20AND%20reviewers.uuid%3D%22%7Bme%7D%22&fields=%2Bvalues.participants&pagelen=50"
        );
        assert_eq!(
            pull_request_url("team", "repo", 101),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/101"
        );
        assert_eq!(
            comments_url("team", "repo", 101),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/101/comments?pagelen=100"
        );
    }

    #[test]
    fn write_url_builders_target_the_post_endpoints() {
        assert_eq!(
            post_comment_url("team", "repo", 101),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/101/comments"
        );
        assert_eq!(
            approve_url("team", "repo", 101),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/101/approve"
        );
        assert_eq!(
            request_changes_url("team", "repo", 101),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/101/request-changes"
        );
    }

    #[test]
    fn comment_bodies_anchor_inline_and_omit_anchor_for_summary() {
        let inline: Value =
            serde_json::from_str(&inline_comment_body("src/a.rs", 42, "nit")).unwrap();
        assert_eq!(inline["content"]["raw"], "nit");
        assert_eq!(inline["inline"]["path"], "src/a.rs");
        assert_eq!(inline["inline"]["to"], 42);

        let summary: Value = serde_json::from_str(&summary_comment_body("looks good")).unwrap();
        assert_eq!(summary["content"]["raw"], "looks good");
        assert!(summary.get("inline").is_none());
    }

    #[test]
    fn reply_comment_body_nests_under_the_parent() {
        let reply: Value = serde_json::from_str(&reply_comment_body(7, "on it")).unwrap();
        assert_eq!(reply["content"]["raw"], "on it");
        assert_eq!(reply["parent"]["id"], 7);
        // A reply is not anchored — it inherits the thread's location via `parent`.
        assert!(reply.get("inline").is_none());
    }

    #[test]
    fn basic_auth_header_base64_encodes_email_and_token() {
        // base64("user:pass") == "dXNlcjpwYXNz".
        assert_eq!(basic_auth_header("user", "pass"), "Basic dXNlcjpwYXNz");
        // Padding for non-multiple-of-3 input.
        assert_eq!(base64("M"), "TQ==");
        assert_eq!(base64("Ma"), "TWE=");
        assert_eq!(base64("Man"), "TWFu");
    }

    #[test]
    fn parse_current_user_extracts_uuid() {
        assert_eq!(
            parse_current_user(r#"{"uuid":"{alice}","nickname":"alice"}"#),
            Some("{alice}".to_owned())
        );
    }

    #[test]
    fn parse_current_user_display_name_prefers_display_name_then_nickname() {
        assert_eq!(
            parse_current_user_display_name(r#"{"display_name":"Alice Reed","nickname":"alice"}"#),
            Some("Alice Reed".to_owned())
        );
        assert_eq!(
            parse_current_user_display_name(r#"{"nickname":"alice"}"#),
            Some("alice".to_owned())
        );
        assert_eq!(parse_current_user_display_name(r#"{"uuid":"{a}"}"#), None);
    }

    #[test]
    fn parse_error_message_reads_the_reason_else_none() {
        let forbidden = r#"{"type":"error","error":{"message":"Your credentials lack one or more required privilege scopes."}}"#;
        assert_eq!(
            parse_error_message(forbidden).as_deref(),
            Some("Your credentials lack one or more required privilege scopes.")
        );
        assert_eq!(parse_error_message(r#"{"uuid":"{alice}"}"#), None);
        assert_eq!(parse_error_message("not json"), None);
    }

    #[test]
    fn parse_list_maps_every_entry_under_the_query_role() {
        // The query already filtered by role, so nothing is dropped and every PR
        // carries the role passed in (here ToReview).
        let prs = parse_list(LIST, "team/repo", PrRole::ToReview).unwrap();
        assert_eq!(prs.len(), 3);
        assert!(prs.iter().all(|p| p.role == PrRole::ToReview));

        let first = &prs[0];
        assert_eq!(first.number, 101);
        assert_eq!(first.state, PrState::Open);
        assert_eq!(first.author, "Alice");
        assert_eq!(first.source_branch, "feature/billing");
        assert_eq!(first.dest_branch, "main");
        assert_eq!(
            first.url,
            "https://bitbucket.org/team/repo/pull-requests/101"
        );
        // The same page mapped under Mine carries Mine throughout.
        let mine = parse_list(LIST, "team/repo", PrRole::Mine).unwrap();
        assert!(mine.iter().all(|p| p.role == PrRole::Mine));
    }

    #[test]
    fn parse_list_reads_reviewers_and_review_from_participants() {
        let prs = parse_list(LIST, "team/repo", PrRole::ToReview).unwrap();

        // 101: Bob undecided + Carol approved ⇒ roster of two, decision Approved.
        assert_eq!(
            prs[0].reviewers,
            vec![
                Reviewer {
                    name: "Bob".to_owned(),
                    state: Review::Pending,
                },
                Reviewer {
                    name: "Carol".to_owned(),
                    state: Review::Approved,
                },
            ]
        );
        assert_eq!(prs[0].review, Review::Approved);

        // 77: a lone changes-requested reviewer dominates.
        assert_eq!(prs[1].reviewers.len(), 1);
        assert_eq!(prs[1].reviewers[0].state, Review::ChangesRequested);
        assert_eq!(prs[1].review, Review::ChangesRequested);

        // 5: no participants ⇒ empty roster, no decision.
        assert!(prs[2].reviewers.is_empty());
        assert_eq!(prs[2].review, Review::None);
    }

    #[test]
    fn parse_detail_reads_description_and_comments_no_checks() {
        let detail = parse_detail(DETAIL, COMMENTS).unwrap();
        assert!(detail.body.starts_with("This adds billing."));
        assert_eq!(detail.comments.len(), 3);
        assert_eq!(detail.comments[0].author, "Bob");
        assert_eq!(detail.comments[0].body, "Nice work");
        // A conversation comment carries no anchor.
        assert_eq!(detail.comments[0].path, None);
        assert_eq!(detail.comments[0].old_lineno, None);
        assert_eq!(detail.comments[0].new_lineno, None);
        // The inline reply anchors to its new-side file line and links to its parent.
        let inline = &detail.comments[2];
        assert_eq!(inline.path.as_deref(), Some("src/billing.rs"));
        assert_eq!(inline.old_lineno, None);
        assert_eq!(inline.new_lineno, Some(42));
        assert_eq!(inline.id, Some(3));
        assert_eq!(inline.parent_id, Some(2));
        assert!(detail.check_runs.is_empty());
    }

    #[test]
    fn parse_detail_reads_created_on_and_strips_cr() {
        let detail = json!({
            "summary": {"raw": "a\r\nb"},
            "created_on": "2024-05-06T07:08:09+00:00"
        })
        .to_string();
        let comments = json!({
            "values": [{
                "id": 1, "user": {"display_name": "x"},
                "content": {"raw": "c\r\nd"}, "inline": {}
            }]
        })
        .to_string();
        let parsed = parse_detail(&detail, &comments).unwrap();
        assert_eq!(parsed.created_at, "2024-05-06T07:08:09+00:00");
        assert_eq!(parsed.body, "a\nb");
        assert_eq!(parsed.comments[0].body, "c\nd");
    }

    #[test]
    fn parse_pr_leaves_labels_empty() {
        let value = json!({
            "id": 1, "title": "T", "author": {"display_name": "x"},
            "source": {"branch": {"name": "f"}},
            "destination": {"branch": {"name": "main"}},
            "links": {"html": {"href": "u"}}, "updated_on": "", "draft": false
        });
        let pr = parse_pr(&value, "team/repo", PrRole::Mine);
        assert!(pr.labels.is_empty());
    }

    #[test]
    fn parse_commits_maps_hash_subject_and_author() {
        // The page is newest-first as Bitbucket returns it; the runner reverses it.
        let commits = parse_commits(COMMITS).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(commits[0].short, "bbbbbbb");
        // Subject is the first message line only.
        assert_eq!(commits[0].subject, "Wire the submit handler");
        assert_eq!(commits[0].author, "Bob Roe");
        // No display name → fall back to the raw `Name <email>`.
        assert_eq!(commits[1].author, "Alice Doe <alice@example.com>");
    }

    #[test]
    fn resolve_comment_url_targets_the_resolve_endpoint() {
        assert_eq!(
            resolve_comment_url("team", "repo", 101, 7),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/101/comments/7/resolve"
        );
    }

    #[test]
    fn parse_comments_reads_created_on_and_resolution() {
        let json = json!({
            "values": [
                {
                    "id": 1, "user": {"display_name": "x"}, "content": {"raw": "open"},
                    "created_on": "2024-05-06T07:08:09+00:00"
                },
                {
                    "id": 2, "user": {"display_name": "y"}, "content": {"raw": "done"},
                    "created_on": "2024-05-07T01:02:03+00:00",
                    "resolution": {"type": "resolution", "user": {"display_name": "y"}}
                }
            ]
        })
        .to_string();
        let comments = parse_comments(&json).unwrap();
        assert_eq!(comments[0].created_at, "2024-05-06T07:08:09+00:00");
        assert!(!comments[0].resolved);
        assert_eq!(comments[1].created_at, "2024-05-07T01:02:03+00:00");
        assert!(comments[1].resolved);
        // Bitbucket resolves by comment id, never a thread node id.
        assert_eq!(comments[1].thread_id, None);
    }

    #[test]
    fn parse_comments_drops_deleted_tombstones() {
        let json = json!({
            "values": [
                {"id": 1, "user": {"display_name": "x"}, "content": {"raw": "kept"}},
                {"id": 2, "deleted": true, "content": {"raw": ""}}
            ]
        })
        .to_string();
        let comments = parse_comments(&json).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "kept");
    }
    #[test]
    fn merge_url_and_body_target_a_plain_merge_commit() {
        assert_eq!(
            merge_url("acme", "web", 1284),
            "https://api.bitbucket.org/2.0/repositories/acme/web/pullrequests/1284/merge"
        );
        let body: serde_json::Value = serde_json::from_str(&merge_body()).unwrap();
        assert_eq!(body["merge_strategy"], "merge_commit");
        assert_eq!(body["close_source_branch"], false);
    }
}
