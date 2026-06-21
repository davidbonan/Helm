/// Creates a **lightweight** tag `name` on the commit `at` (graph row menu,
/// git.md §9) — no message, no signature (annotated tags stay out of scope).
/// HEAD and the working tree are untouched and nothing is pushed. Invalid or
/// duplicate name ⇒ clean `Err`, nothing created; a branch sharing the name does
/// not collide (the ref is created under `refs/tags/`).
pub fn create_lightweight(
    repo: &git2::Repository,
    name: &str,
    at: git2::Oid,
) -> Result<(), git2::Error> {
    if !valid_tag_name(name) {
        return Err(git2::Error::from_str("invalid tag name"));
    }
    let target = repo.find_object(at, None)?;
    repo.tag_lightweight(name, &target, false)?;
    Ok(())
}

/// **Detached** checkout on the tag's commit (graph tag menu, git.md §9): same
/// dirty-tree safety as the branch checkout — the working tree (untracked
/// included) is auto-stashed first, restored if the checkout never lands. The tag
/// name is resolved before stashing so a missing tag fails without setting the
/// tree aside. Menu-only: a tag is never checked out by a double-click (a detached
/// HEAD must not be one slip away).
pub fn checkout_detached(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    let owned = git2::Repository::open(repo.path())?;
    let oid = owned
        .revparse_single(&format!("refs/tags/{name}"))?
        .peel_to_commit()?
        .id();
    crate::git::branch::with_auto_stash(&owned, &format!("checkout {name}"), |repo| {
        crate::git::branch::checkout_detached(repo, oid)
    })
}

/// Deletes the **local** tag `name` (graph tag menu, git.md §9). Missing tag ⇒
/// libgit2 `Err`, nothing else touched; the remote tag (if any) is removed
/// separately on the sync runner.
pub fn delete(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    repo.tag_delete(name)
}

/// `git2` validation of a tag's short name, via its fully-qualified ref
/// (`refs/tags/<name>`) — the same `check-ref-format` rules as a branch name.
pub fn valid_tag_name(name: &str) -> bool {
    git2::Reference::is_valid_name(&format!("refs/tags/{name}"))
}
