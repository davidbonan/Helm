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
/// HEAD must not be one slip away). A tag is offered whatever HEAD is: when HEAD
/// is already detached on its commit the checkout is a no-op, and the auto-stash
/// would set the working tree aside for nothing (never popped back).
pub fn checkout_detached(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    let owned = git2::Repository::open(repo.path())?;
    let oid = owned
        .revparse_single(&format!("refs/tags/{name}"))?
        .peel_to_commit()?
        .id();
    if owned.head_detached()? && owned.head()?.target() == Some(oid) {
        return Ok(());
    }
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

/// `git2` validation of a tag's short name — `git tag`'s own rules: the
/// `check-ref-format` rules on `refs/tags/<name>` plus the refusals a raw ref
/// name would let through (leading `-`, `HEAD`).
pub fn valid_tag_name(name: &str) -> bool {
    git2::Tag::is_valid_name(name)
}

#[cfg(test)]
mod tests {
    use super::valid_tag_name;

    #[test]
    fn tag_names_follow_gits_tag_rules() {
        assert!(valid_tag_name("v1.0"));
        assert!(valid_tag_name("release/1.0-rc1"));
        // Refused by `git tag` although `refs/tags/<name>` passes check-ref-format.
        assert!(!valid_tag_name("-rc1"));
        assert!(!valid_tag_name("HEAD"));
        assert!(!valid_tag_name(""));
        assert!(!valid_tag_name("v1..2"));
        assert!(!valid_tag_name("with space"));
    }
}
