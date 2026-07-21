#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Branch {
    Named(String),
    Detached(String),
    Unborn(String),
}

impl Branch {
    pub fn label(&self) -> &str {
        match self {
            Branch::Named(name) | Branch::Detached(name) | Branch::Unborn(name) => name,
        }
    }
}

/// Checks out a branch from a graph chip. Local branch ⇒ direct checkout; remote
/// ref `<remote>/x` ⇒ git DWIM: the local `x` as is if it points at the remote's
/// commit, **fast-forwarded** onto it if it is simply behind, **detached** checkout
/// on the remote commit if it has diverged (its own commits are untouched), created
/// with upstream if it is missing. Dirty working tree ⇒ changes (untracked included)
/// are first set aside in a stash — never a destructive checkout.
pub fn checkout(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    let owned = git2::Repository::open(repo.path())?;
    with_auto_stash(&owned, &format!("checkout {name}"), |repo| {
        finish_checkout(repo, name)
    })
}

/// Runs a checkout with the dirty-tree safety of [`checkout`]: the working tree
/// (untracked included) is set aside in a stash first, restored if the checkout
/// never lands. Shared by the branch checkout and the tag detached checkout
/// (git.md §9). `label` names the operation in the auto-stash message.
pub(crate) fn with_auto_stash(
    repo: &git2::Repository,
    label: &str,
    checkout: impl FnOnce(&git2::Repository) -> Result<(), git2::Error>,
) -> Result<(), git2::Error> {
    let stashed = crate::git::status::is_dirty(repo)?;
    if stashed {
        crate::git::stash::save(repo, &format!("helm: auto-stash before {label}"))?;
    }
    let result = checkout(repo);
    if let Err(err) = &result {
        // The tree was set aside but the checkout never landed: restore it —
        // without this the changes silently sat in the stash. A pop that fails
        // keeps the stash (recoverable); the error then says where they went.
        if stashed && crate::git::stash::pop(repo).is_err() {
            return Err(git2::Error::from_str(&format!(
                "{} — your changes were kept in the auto-stash",
                err.message()
            )));
        }
    }
    result
}

/// Resolves the double-clicked name and finishes the checkout: a local branch as
/// is; otherwise git DWIM on a remote ref `<remote>/x` — the same-named local `x`
/// (fast-forwarded if it is simply behind the remote, detached checkout on the
/// remote commit if it has diverged), otherwise `x` is created on the remote's
/// commit with upstream configured.
fn finish_checkout(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    match repo.find_branch(name, git2::BranchType::Local) {
        Ok(local) => return checkout_reference(repo, local.into_reference()),
        Err(err) if err.code() != git2::ErrorCode::NotFound => return Err(err),
        Err(_) => {}
    }
    let remote = repo.find_branch(name, git2::BranchType::Remote)?;
    let remote_oid = remote.get().peel_to_commit()?.id();
    let local_name = name.split_once('/').map_or(name, |(_, branch)| branch);
    match repo.find_branch(local_name, git2::BranchType::Local) {
        Ok(mut local) => {
            let local_oid = local.get().peel_to_commit()?.id();
            if local_oid == remote_oid {
                checkout_reference(repo, local.into_reference())
            } else if repo.graph_descendant_of(remote_oid, local_oid)? {
                let updated = local.get_mut().set_target(
                    remote_oid,
                    &format!("helm: fast-forward {local_name} to {name}"),
                )?;
                checkout_reference(repo, updated)
            } else {
                checkout_detached(repo, remote_oid)
            }
        }
        Err(err) if err.code() == git2::ErrorCode::NotFound => {
            let target = remote.get().peel_to_commit()?;
            let mut created = repo.branch(local_name, &target, false)?;
            created.set_upstream(Some(name))?;
            checkout_reference(repo, created.into_reference())
        }
        Err(err) => Err(err),
    }
}

fn checkout_reference(
    repo: &git2::Repository,
    reference: git2::Reference<'_>,
) -> Result<(), git2::Error> {
    let target = reference.peel(git2::ObjectType::Commit)?;
    repo.checkout_tree(&target, None)?;
    repo.set_head(reference.name()?)
}

pub(crate) fn checkout_detached(
    repo: &git2::Repository,
    oid: git2::Oid,
) -> Result<(), git2::Error> {
    let target = repo.find_object(oid, None)?;
    repo.checkout_tree(&target, None)?;
    repo.set_head_detached(oid)
}

/// Preflight for combined remote+local deletion: catches the stable local
/// refusals before the remote branch is touched.
pub fn validate_local_deletable(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    let branch = repo.find_branch(name, git2::BranchType::Local)?;
    if branch.is_head() {
        return Err(git2::Error::from_str(
            "cannot delete the checked-out branch",
        ));
    }
    Ok(())
}

/// Deletes the **local** branch `name` (graph context menu, git.md §9). Current
/// branch or one checked out in a worktree ⇒ libgit2 `Err`, nothing is deleted.
pub fn delete_local(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    validate_local_deletable(repo, name)?;
    repo.find_branch(name, git2::BranchType::Local)?.delete()
}

/// `git2` validation of a branch name (Branch popover, git.md §10).
pub fn valid_branch_name(name: &str) -> bool {
    git2::Branch::name_is_valid(name).unwrap_or(false)
}

/// Creates `name` on HEAD and checks it out (reflog via `set_head`). Invalid or
/// duplicate name ⇒ `Err` without creating anything; repo with no commit (unborn
/// HEAD) ⇒ a clean `Err`. The working tree is untouched (the branch is born on HEAD).
pub fn create_and_checkout(repo: &git2::Repository, name: &str) -> Result<(), git2::Error> {
    if !valid_branch_name(name) {
        return Err(git2::Error::from_str("invalid branch name"));
    }
    let head = repo.head()?.peel_to_commit()?;
    let created = repo.branch(name, &head, false)?;
    checkout_reference(repo, created.into_reference())
}

/// Creates the local branch `name` at the commit pointed to by `committish`
/// (graph chip context menu, git.md §9) **without checking it out** — HEAD and
/// the working tree are left untouched. `committish` is the source ref, passed
/// fully qualified (`refs/heads|remotes|tags/…`) so a branch and a tag sharing a
/// name never collide. Invalid or duplicate name ⇒ clean `Err`, nothing created.
pub fn create_at(repo: &git2::Repository, name: &str, committish: &str) -> Result<(), git2::Error> {
    if !valid_branch_name(name) {
        return Err(git2::Error::from_str("invalid branch name"));
    }
    let target = repo.revparse_single(committish)?.peel_to_commit()?;
    repo.branch(name, &target, false)?;
    Ok(())
}

/// Moves the current branch to `target` (graph row menu — git.md §9). `kind`
/// selects the git semantics: **Soft** keeps the index and working tree (the
/// difference shows up staged), **Mixed** resets the index only (it shows up
/// unstaged), **Hard** resets both (destructive — untracked files survive, git
/// semantics). Detached HEAD is gated out in the UI; a missing branch surfaces
/// as git's `Err`.
///
/// An operation in progress refuses (git.md §9, like `commit` / `rebase_onto`):
/// a merge/cherry-pick/revert conflict keeps HEAD on a branch, and a libgit2
/// reset there wipes `MERGE_HEAD`+`MERGE_MSG` — killing `git merge --abort`.
pub fn reset(
    repo: &git2::Repository,
    target: git2::Oid,
    kind: git2::ResetType,
) -> Result<(), git2::Error> {
    if repo.state() != git2::RepositoryState::Clean {
        return Err(git2::Error::from_str(
            "an operation is in progress — resolve or abort it first",
        ));
    }
    let object = repo.find_object(target, None)?;
    repo.reset(&object, kind, None)
}

/// Renames the **local** branch `from` to `to` (graph chip menu — git.md §9):
/// `git branch -m` semantics via libgit2 — the symbolic `HEAD` follows when the
/// current branch is renamed and the branch's config (upstream) moves with it.
/// `force = false` never overwrites an existing branch: a duplicate name is a
/// clean `Err`. Invalid name ⇒ `Err` before anything moves.
pub fn rename(repo: &git2::Repository, from: &str, to: &str) -> Result<(), git2::Error> {
    if !valid_branch_name(to) {
        return Err(git2::Error::from_str("invalid branch name"));
    }
    let mut branch = repo.find_branch(from, git2::BranchType::Local)?;
    branch.rename(to, false)?;
    Ok(())
}

pub fn current(repo: &git2::Repository) -> Result<Branch, git2::Error> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(err) if err.code() == git2::ErrorCode::UnbornBranch => {
            let head_ref = repo.find_reference("HEAD")?;
            let target = head_ref
                .symbolic_target()?
                .and_then(strip_branch_prefix)
                .unwrap_or("main")
                .to_string();
            return Ok(Branch::Unborn(target));
        }
        Err(err) => return Err(err),
    };

    if repo.head_detached()? {
        let oid = head
            .target()
            .ok_or_else(|| git2::Error::from_str("detached HEAD has no target"))?;
        let short = repo.find_object(oid, None)?.short_id()?;
        let hash = short.as_str().unwrap_or_default().to_string();
        return Ok(Branch::Detached(hash));
    }

    Ok(Branch::Named(head.shorthand()?.to_string()))
}

fn strip_branch_prefix(reference: &str) -> Option<&str> {
    reference.strip_prefix("refs/heads/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_branch_prefix_keeps_last_segment() {
        assert_eq!(strip_branch_prefix("refs/heads/main"), Some("main"));
        assert_eq!(strip_branch_prefix("HEAD"), None);
    }

    #[test]
    fn label_returns_inner_name_for_each_variant() {
        assert_eq!(Branch::Named("main".into()).label(), "main");
        assert_eq!(Branch::Detached("d383ef8".into()).label(), "d383ef8");
        assert_eq!(Branch::Unborn("main".into()).label(), "main");
    }

    #[test]
    fn valid_branch_name_follows_git_ref_rules() {
        assert!(valid_branch_name("feature/login"));
        assert!(valid_branch_name("fix-1"));
        assert!(!valid_branch_name(""));
        assert!(!valid_branch_name("with space"));
        assert!(!valid_branch_name("a..b"));
        assert!(!valid_branch_name("end.lock"));
        assert!(!valid_branch_name("-dash"));
    }
}
