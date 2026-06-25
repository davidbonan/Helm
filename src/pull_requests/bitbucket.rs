//! Bitbucket Cloud source for the cockpit (pull-requests.md §3): pure `2.0` URL
//! builders, the Basic-auth header, and I/O-free mappers from the REST JSON onto
//! the domain model. No request is made here — the runner (PR4) drives `curl`
//! (the `update.rs` idiom); this module only builds the URLs/header and reads
//! the replies. Roles are derived from the cached account uuid (§1).

use serde_json::Value;

use crate::pull_requests::model::{
    Checks, ForgeKind, PrComment, PrDetail, PrRole, PrState, PullRequest, Review,
};

const API: &str = "https://api.bitbucket.org/2.0";

/// `GET /2.0/user` — resolves "me" once per session into an account uuid (§1).
pub fn current_user_url() -> String {
    format!("{API}/user")
}

/// Open PRs `me_uuid` authored (`role` ⇒ `Mine`). The list endpoint omits
/// `reviewers`/`participants`, so the role can't be re-derived from the reply —
/// it comes from which query found the PR, like the GitHub two-query split.
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
fn role_filtered_url(workspace: &str, repo: &str, field: &str, me_uuid: &str) -> String {
    let q = encode(&format!("state=\"OPEN\" AND {field}=\"{me_uuid}\""));
    format!("{API}/repositories/{workspace}/{repo}/pullrequests?q={q}&pagelen=50")
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

/// Map one PR object onto the domain. The list reply omits review state and the
/// reviewer roster (detail-only on Bitbucket), so both stay empty here.
fn parse_pr(o: &Value, repo_label: &str, role: PrRole) -> PullRequest {
    let state = if o["draft"].as_bool().unwrap_or(false) {
        PrState::Draft
    } else {
        PrState::Open
    };

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
        review: Review::None,
        reviewers: Vec::new(),
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

    #[test]
    fn url_builders_target_the_cloud_2_0_endpoints() {
        assert_eq!(current_user_url(), "https://api.bitbucket.org/2.0/user");
        // The role filter folds OPEN into a percent-encoded `q` (state= alone is
        // dropped once q is present), differing only by author/reviewers field.
        assert_eq!(
            authored_url("team", "repo", "{me}"),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests?q=state%3D%22OPEN%22%20AND%20author.uuid%3D%22%7Bme%7D%22&pagelen=50"
        );
        assert_eq!(
            reviewing_url("team", "repo", "{me}"),
            "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests?q=state%3D%22OPEN%22%20AND%20reviewers.uuid%3D%22%7Bme%7D%22&pagelen=50"
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
        // The list reply has no reviewer/review data — both stay empty.
        assert_eq!(first.review, Review::None);
        assert!(first.reviewers.is_empty());

        // The same page mapped under Mine carries Mine throughout.
        let mine = parse_list(LIST, "team/repo", PrRole::Mine).unwrap();
        assert!(mine.iter().all(|p| p.role == PrRole::Mine));
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
