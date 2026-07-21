pub fn commit(repo: &git2::Repository, message: &str) -> Result<git2::Oid, git2::Error> {
    if repo.state() != git2::RepositoryState::Clean {
        return Err(git2::Error::from_str(
            "merge/rebase in progress — finish it in the conflict panel",
        ));
    }
    let signature = repo
        .signature()
        .map_err(|_| git2::Error::from_str("configure git user.name / user.email"))?;

    // Refreshed like every index mutation (cf. stage::fresh_index): the tree
    // must capture what is staged on disk now, not the cached snapshot.
    let mut index = crate::git::stage::fresh_index(repo)?;
    let tree_id = index.write_tree()?;
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .into_iter()
        .collect();
    if parents
        .first()
        .is_some_and(|parent| parent.tree_id() == tree_id)
        || (parents.is_empty() && index.is_empty())
    {
        return Err(git2::Error::from_str("nothing staged to commit"));
    }
    let tree = repo.find_tree(tree_id)?;
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parent_refs,
    )
}

/// Amends **HEAD**'s message (a reword): the committed tree and author are kept,
/// the committer refreshed like `git commit --amend`. Staged changes are *not*
/// folded in — this edits the message alone. Requires a clean repo state (no
/// merge/rebase mid-flight) and an existing HEAD commit.
pub fn amend_message(repo: &git2::Repository, message: &str) -> Result<git2::Oid, git2::Error> {
    if repo.state() != git2::RepositoryState::Clean {
        return Err(git2::Error::from_str(
            "merge/rebase in progress — finish it in the conflict panel",
        ));
    }
    if message.trim().is_empty() {
        return Err(git2::Error::from_str("commit message cannot be empty"));
    }
    let committer = repo
        .signature()
        .map_err(|_| git2::Error::from_str("configure git user.name / user.email"))?;
    let head = repo.head()?.peel_to_commit()?;
    head.amend(
        Some("HEAD"),
        None,
        Some(&committer),
        None,
        Some(message),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo_with_commit(dir: &Path) -> (git2::Repository, git2::Oid) {
        let repo = git2::Repository::init(dir).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Author One").unwrap();
            cfg.set_str("user.email", "one@example.com").unwrap();
        }
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let oid = commit(&repo, "original subject").unwrap();
        (repo, oid)
    }

    #[test]
    fn amend_rewrites_message_and_moves_head() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, first) = repo_with_commit(tmp.path());
        let new = amend_message(&repo, "reworded subject\n\nbody").unwrap();
        assert_ne!(new, first, "amend produces a new commit oid");
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.id(), new, "HEAD moved to the amended commit");
        assert_eq!(head.message().unwrap(), "reworded subject\n\nbody");
    }

    #[test]
    fn amend_preserves_tree_and_keeps_author_but_refreshes_committer() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, first) = repo_with_commit(tmp.path());
        let before = repo.find_commit(first).unwrap();
        let tree_before = before.tree_id();
        let author_before = (
            before.author().name().unwrap().to_string(),
            before.author().email().unwrap().to_string(),
        );
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Author Two").unwrap();
            cfg.set_str("user.email", "two@example.com").unwrap();
        }
        let new = amend_message(&repo, "reworded").unwrap();
        let after = repo.find_commit(new).unwrap();
        assert_eq!(
            after.tree_id(),
            tree_before,
            "tree preserved (message-only reword, staged changes not folded in)"
        );
        assert_eq!(after.author().name().unwrap(), author_before.0);
        assert_eq!(after.author().email().unwrap(), author_before.1);
        assert_eq!(
            after.committer().name().unwrap(),
            "Author Two",
            "committer refreshed like `git commit --amend`"
        );
    }

    #[test]
    fn amend_rejects_a_blank_message() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, _) = repo_with_commit(tmp.path());
        assert!(amend_message(&repo, "   ").is_err());
    }
}
