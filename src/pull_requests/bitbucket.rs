//! Bitbucket Cloud source for the cockpit (pull-requests.md §3): pure `2.0` URL
//! builders, the Basic-auth header, and I/O-free mappers from the REST JSON onto
//! the domain model. No request is made here — the runner (PR4) drives `curl`
//! (the `update.rs` idiom); this module only builds the URLs/header and reads
//! the replies. Roles are derived from the cached account uuid (§1).

use serde_json::Value;

use crate::pull_requests::model::{
    Checks, ForgeKind, PrComment, PrDetail, PrRole, PrState, PullRequest, Review, Reviewer,
};

const API: &str = "https://api.bitbucket.org/2.0";

/// `GET /2.0/user` — resolves "me" once per session into an account uuid (§1).
pub fn current_user_url() -> String {
    format!("{API}/user")
}

/// Open PRs of a repo (drafts included); roles are classified afterwards.
pub fn pull_requests_url(workspace: &str, repo: &str) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests?state=OPEN&pagelen=50")
}

/// A single PR (carries the rendered description for the detail panel).
pub fn pull_request_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}")
}

/// A PR's comment thread (read-only in the cockpit).
pub fn comments_url(workspace: &str, repo: &str, id: u64) -> String {
    format!("{API}/repositories/{workspace}/{repo}/pullrequests/{id}/comments?pagelen=100")
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

/// Map a paginated `pullrequests` page onto domain PRs, dropping any that
/// concern neither role for `me_uuid`.
pub fn parse_list(
    json: &str,
    me_uuid: &str,
    repo_label: &str,
) -> serde_json::Result<Vec<PullRequest>> {
    let value: Value = serde_json::from_str(json)?;
    let prs = value["values"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_pr(item, me_uuid, repo_label))
                .collect()
        })
        .unwrap_or_default();
    Ok(prs)
}

/// Map a PR object (`description`) and its comments page onto the detail. Bitbucket
/// has no rollup on the PR, so `check_runs` stays empty (pull-requests.md §10).
pub fn parse_detail(detail_json: &str, comments_json: &str) -> serde_json::Result<PrDetail> {
    let detail: Value = serde_json::from_str(detail_json)?;
    let body = detail["summary"]["raw"]
        .as_str()
        .or_else(|| detail["description"].as_str())
        .unwrap_or_default()
        .to_owned();

    let comments_page: Value = serde_json::from_str(comments_json)?;
    let comments = comments_page["values"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|c| {
                    let body = c["content"]["raw"].as_str()?.to_owned();
                    Some(PrComment {
                        author: c["user"]["display_name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        body,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(PrDetail {
        body,
        comments,
        check_runs: Vec::new(),
    })
}

fn parse_pr(o: &Value, me: &str, repo_label: &str) -> Option<PullRequest> {
    let author_uuid = o["author"]["uuid"].as_str().unwrap_or_default();
    let reviewer_uuids: Vec<&str> = o["reviewers"]
        .as_array()
        .map(|items| items.iter().filter_map(|r| r["uuid"].as_str()).collect())
        .unwrap_or_default();
    let is_author = !me.is_empty() && author_uuid == me;
    let is_requested = reviewer_uuids.contains(&me);
    let role = PrRole::classify(is_author, is_requested)?;

    let state = if o["draft"].as_bool().unwrap_or(false) {
        PrState::Draft
    } else {
        PrState::Open
    };

    Some(PullRequest {
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
        review: aggregate_review(o),
        reviewers: collect_reviewers(o),
    })
}

/// Aggregate the requested reviewers' decisions: any changes-requested ⇒
/// ChangesRequested, else any approval ⇒ Approved, else Pending while reviewers
/// remain, else None.
fn aggregate_review(o: &Value) -> Review {
    let Some(participants) = o["participants"].as_array() else {
        return Review::None;
    };
    let states: Vec<&str> = participants
        .iter()
        .filter(|p| p["role"].as_str() == Some("REVIEWER"))
        .map(|p| p["state"].as_str().unwrap_or_default())
        .collect();
    if states.is_empty() {
        return Review::None;
    }
    if states.contains(&"changes_requested") {
        Review::ChangesRequested
    } else if states.contains(&"approved") {
        Review::Approved
    } else {
        Review::Pending
    }
}

/// The requested reviewers and where each stands, looked up from `participants`.
fn collect_reviewers(o: &Value) -> Vec<Reviewer> {
    let participants = o["participants"].as_array();
    o["reviewers"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|r| {
                    let uuid = r["uuid"].as_str().unwrap_or_default();
                    let state = participants
                        .and_then(|ps| ps.iter().find(|p| p["user"]["uuid"].as_str() == Some(uuid)))
                        .and_then(|p| p["state"].as_str())
                        .map(map_review_state)
                        .unwrap_or(Review::Pending);
                    Reviewer {
                        name: r["display_name"].as_str().unwrap_or_default().to_owned(),
                        state,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn map_review_state(state: &str) -> Review {
    match state {
        "approved" => Review::Approved,
        "changes_requested" => Review::ChangesRequested,
        _ => Review::Pending,
    }
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

    #[test]
    fn url_builders_target_the_cloud_2_0_endpoints() {
        assert_eq!(current_user_url(), "https://api.bitbucket.org/2.0/user");
        assert_eq!(
            pull_requests_url("team", "repo"),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests?state=OPEN&pagelen=50"
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
    fn parse_list_classifies_by_uuid_and_drops_uninvolved() {
        let prs = parse_list(LIST, "{alice}", "team/repo").unwrap();
        // PR 5 (alice neither author nor reviewer) is dropped.
        assert_eq!(prs.len(), 2);

        let mine = &prs[0];
        assert_eq!(mine.number, 101);
        assert_eq!(mine.role, PrRole::Mine);
        assert_eq!(mine.state, PrState::Open);
        assert_eq!(mine.author, "Alice");
        assert_eq!(mine.source_branch, "feature/billing");
        assert_eq!(mine.dest_branch, "main");
        assert_eq!(
            mine.url,
            "https://bitbucket.org/team/repo/pull-requests/101"
        );

        let to_review = &prs[1];
        assert_eq!(to_review.number, 77);
        assert_eq!(to_review.role, PrRole::ToReview);
        assert_eq!(to_review.state, PrState::Draft);
    }

    #[test]
    fn parse_list_aggregates_review_from_participants() {
        let prs = parse_list(LIST, "{alice}", "team/repo").unwrap();
        // PR 101: carol approved, bob undecided ⇒ Approved.
        assert_eq!(prs[0].review, Review::Approved);
        // PR 77: alice requested changes ⇒ ChangesRequested.
        assert_eq!(prs[1].review, Review::ChangesRequested);
    }

    #[test]
    fn parse_list_collects_requested_reviewers_with_state() {
        let prs = parse_list(LIST, "{alice}", "team/repo").unwrap();
        assert_eq!(prs[0].reviewers.len(), 1);
        assert_eq!(prs[0].reviewers[0].name, "Bob");
        assert_eq!(prs[0].reviewers[0].state, Review::Pending);

        assert_eq!(prs[1].reviewers[0].name, "Alice");
        assert_eq!(prs[1].reviewers[0].state, Review::ChangesRequested);
    }

    #[test]
    fn parse_detail_reads_description_and_comments_no_checks() {
        let detail = parse_detail(DETAIL, COMMENTS).unwrap();
        assert!(detail.body.starts_with("This adds billing."));
        assert_eq!(detail.comments.len(), 2);
        assert_eq!(detail.comments[0].author, "Bob");
        assert_eq!(detail.comments[0].body, "Nice work");
        assert!(detail.check_runs.is_empty());
    }
}
