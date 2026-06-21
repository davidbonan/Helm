pub fn commit(repo: &git2::Repository, message: &str) -> Result<git2::Oid, git2::Error> {
    if repo.state() != git2::RepositoryState::Clean {
        return Err(git2::Error::from_str(
            "merge/rebase in progress — resolve from the terminal",
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
