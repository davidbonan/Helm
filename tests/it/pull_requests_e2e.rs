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
