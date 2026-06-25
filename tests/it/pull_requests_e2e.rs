//! PR runner business e2e (pull-requests.md §6): the command/URL construction is
//! driven from a *real* throwaway git repo, so it covers the libgit2 `origin`
//! resolution (`forges_of_roots`) the in-module unit tests can't — then asserts
//! the per-forge query plan built from it. No network: the plan is data.

use std::path::{Path, PathBuf};

use helm::git::forge::Forge;
use helm::pull_requests::model::PrRole;
use helm::pull_requests::runner::{forges_of_roots, plan, PrQuery};
use helm::pull_requests::{bitbucket, github};

fn repo_with_origin(dir: &Path, url: &str) -> git2::Repository {
    std::fs::create_dir_all(dir).unwrap();
    let repo = git2::Repository::init(dir).unwrap();
    repo.remote("origin", url).unwrap();
    repo
}

#[test]
fn github_origin_resolves_to_two_gh_search_queries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("web");
    repo_with_origin(&root, "git@github.com:acme/web.git");

    let forges = forges_of_roots(&[root]);
    assert_eq!(
        forges,
        vec![(
            Forge::GitHub {
                owner: "acme".to_owned(),
                repo: "web".to_owned(),
            },
            "acme/web".to_owned(),
        )]
    );

    assert_eq!(
        plan(&forges, None),
        vec![
            PrQuery::Gh {
                repo_label: "acme/web".to_owned(),
                args: github::list_authored_args("acme/web"),
            },
            PrQuery::Gh {
                repo_label: "acme/web".to_owned(),
                args: github::list_review_requested_args("acme/web"),
            },
        ]
    );
}

#[test]
fn bitbucket_origin_lists_only_when_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repo_with_origin(&root, "https://user@bitbucket.org/team/repo.git");

    let forges = forges_of_roots(&[root]);
    assert_eq!(
        forges,
        vec![(
            Forge::Bitbucket {
                workspace: "team".to_owned(),
                repo: "repo".to_owned(),
            },
            "team/repo".to_owned(),
        )]
    );

    assert!(plan(&forges, None).is_empty());
    assert_eq!(
        plan(&forges, Some("{me}")),
        vec![
            PrQuery::Bitbucket {
                repo_label: "team/repo".to_owned(),
                url: bitbucket::authored_url("team", "repo", "{me}"),
                role: PrRole::Mine,
            },
            PrQuery::Bitbucket {
                repo_label: "team/repo".to_owned(),
                url: bitbucket::reviewing_url("team", "repo", "{me}"),
                role: PrRole::ToReview,
            },
        ]
    );
}

#[test]
fn worktrees_of_one_remote_are_queried_once() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    repo_with_origin(&a, "git@github.com:acme/web.git");
    repo_with_origin(&b, "https://github.com/acme/web");

    let forges = forges_of_roots(&[a, b]);
    assert_eq!(forges.len(), 1);
    assert_eq!(plan(&forges, None).len(), 2);
}

#[test]
fn repos_without_a_known_forge_origin_are_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let gitlab = tmp.path().join("gl");
    let bare = tmp.path().join("bare");
    repo_with_origin(&gitlab, "git@gitlab.com:a/b.git");
    git2::Repository::init(&bare).unwrap(); // no `origin`

    let forges = forges_of_roots(&[gitlab, bare, PathBuf::from("/nonexistent")]);
    assert!(forges.is_empty());
    assert!(plan(&forges, None).is_empty());
}

/// Stage every change and commit it on `HEAD`, returning the new commit.
fn commit_all(repo: &git2::Repository, message: &str) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Tester", "t@e.com").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

#[test]
fn pr_changed_files_lists_the_base_to_head_delta() {
    use helm::git::diff::{pr_changed_files, pr_file_diff};
    use helm::git::status::ChangeKind;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let repo = git2::Repository::init(&root).unwrap();
    std::fs::write(root.join("a.txt"), "1\n2\n3\n").unwrap();
    let base = commit_all(&repo, "base");

    std::fs::write(root.join("a.txt"), "1\n2 changed\n3\n").unwrap();
    std::fs::write(root.join("b.txt"), "new file\n").unwrap();
    let head = commit_all(&repo, "head");

    let files = pr_changed_files(&repo, base, head).unwrap();
    let mut by_path: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.kind)).collect();
    by_path.sort_by_key(|(path, _)| *path);
    assert_eq!(
        by_path,
        vec![
            ("a.txt", ChangeKind::Modified),
            ("b.txt", ChangeKind::Added)
        ]
    );

    let diff = pr_file_diff(&repo, base, head, "a.txt").unwrap();
    assert!(!diff.binary && !diff.oversize);
    let added: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.origin == helm::git::diff::LineOrigin::Addition)
        .map(|l| l.content.trim_end())
        .collect();
    assert_eq!(added, vec!["2 changed"]);
}

#[test]
fn pr_changed_files_uses_the_three_dot_base() {
    use helm::git::diff::pr_changed_files;

    // base → (dest adds d.txt) and base → (feature adds f.txt). Diffing the
    // merge-base(dest, feature)..feature must show only f.txt, never dest's d.txt.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let repo = git2::Repository::init(&root).unwrap();
    std::fs::write(root.join("base.txt"), "base\n").unwrap();
    let base = commit_all(&repo, "base");

    // dest branch advances.
    std::fs::write(root.join("d.txt"), "on dest\n").unwrap();
    let dest = commit_all(&repo, "dest");

    // feature branches off `base`, not `dest`.
    repo.branch("feature", &repo.find_commit(base).unwrap(), false)
        .unwrap();
    repo.set_head("refs/heads/feature").unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    std::fs::write(root.join("f.txt"), "on feature\n").unwrap();
    let feature = commit_all(&repo, "feature");

    let merge_base = repo.merge_base(dest, feature).unwrap();
    let files = pr_changed_files(&repo, merge_base, feature).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["f.txt"], "only the feature's own change shows");
}

#[test]
fn draft_store_flattens_into_github_review_payload() {
    use helm::pull_requests::model::{draft_comments, ReviewVerdict};
    use helm::review::{add_comment, FileComments, LineComment};

    let mut draft = FileComments::new();
    add_comment(
        &mut draft,
        "src/a.rs",
        LineComment {
            old_lineno: None,
            new_lineno: Some(12),
            code: "fn work() {}".to_owned(),
            note: "rename this".to_owned(),
        },
    );
    // A note without a line anchor (old-side only is still postable) and a blank
    // note that must be dropped from the payload.
    add_comment(
        &mut draft,
        "src/b.rs",
        LineComment {
            old_lineno: None,
            new_lineno: Some(3),
            code: "let x = y.clone();".to_owned(),
            note: "   ".to_owned(),
        },
    );

    let comments = draft_comments(&draft);
    assert_eq!(comments.len(), 1, "blank notes are not posted");
    assert_eq!(comments[0].path, "src/a.rs");
    assert_eq!(comments[0].line, 12);

    let body = github::submit_review_body(ReviewVerdict::RequestChanges, "almost there", &comments);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["event"], "REQUEST_CHANGES");
    assert_eq!(parsed["body"], "almost there");
    assert_eq!(parsed["comments"][0]["path"], "src/a.rs");
    assert_eq!(parsed["comments"][0]["line"], 12);
    assert_eq!(parsed["comments"][0]["body"], "rename this");
}

#[test]
fn draft_store_flattens_into_bitbucket_comment_bodies() {
    use helm::pull_requests::model::draft_comments;
    use helm::review::{add_comment, FileComments, LineComment};

    let mut draft = FileComments::new();
    add_comment(
        &mut draft,
        "src/billing.rs",
        LineComment {
            old_lineno: None,
            new_lineno: Some(42),
            code: "total += line;".to_owned(),
            note: "off-by-one".to_owned(),
        },
    );

    let comments = draft_comments(&draft);
    assert_eq!(comments.len(), 1);
    let inline: serde_json::Value = serde_json::from_str(&bitbucket::inline_comment_body(
        &comments[0].path,
        comments[0].line,
        &comments[0].body,
    ))
    .unwrap();
    assert_eq!(inline["content"]["raw"], "off-by-one");
    assert_eq!(inline["inline"]["path"], "src/billing.rs");
    assert_eq!(inline["inline"]["to"], 42);

    assert_eq!(
        bitbucket::approve_url("team", "repo", 7),
        "https://api.bitbucket.org/2.0/repositories/team/repo/pullrequests/7/approve"
    );
}
