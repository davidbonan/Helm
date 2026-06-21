//! Interactive rebase plan (git.md §9): the commits `onto..HEAD` to replay and
//! the per-commit action chosen on the rebase page. Pure git2 reads +
//! validation — the execution itself (todo injection, subprocess) lives in
//! [`crate::git::sync::interactive_rebase`].

/// Hard cap on the plan size: beyond this the page would not be a review tool
/// anymore (and is almost always a wrong target click) — explicit refusal,
/// never a silent truncation (git.md §9).
pub const MAX_PLAN_COMMITS: usize = 500;

/// One commit of the plan, oldest-first (git todo order — the page displays
/// them newest-first like the graph).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseCommit {
    pub oid: git2::Oid,
    pub short_id: String,
    pub summary: String,
    /// Full message (summary + body): prefills the Reword editor.
    pub message: String,
    pub author: String,
}

/// Target state chosen for a commit on the rebase page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RebaseChoice {
    #[default]
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

/// One todo step sent to the execution: the choice plus its payload (the new
/// Reword message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseStep {
    pub oid: git2::Oid,
    pub action: RebaseAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseAction {
    Pick,
    Reword(String),
    Squash,
    Fixup,
    Drop,
}

impl RebaseAction {
    pub fn choice(&self) -> RebaseChoice {
        match self {
            RebaseAction::Pick => RebaseChoice::Pick,
            RebaseAction::Reword(_) => RebaseChoice::Reword,
            RebaseAction::Squash => RebaseChoice::Squash,
            RebaseAction::Fixup => RebaseChoice::Fixup,
            RebaseAction::Drop => RebaseChoice::Drop,
        }
    }
}

/// Commits the rebase would replay: `onto..HEAD` without merge commits (what
/// `git rebase -i` itself lists — merges are flattened), **oldest first**.
/// `onto` is any committish (branch, remote ref). More than
/// [`MAX_PLAN_COMMITS`] ⇒ explicit error.
pub fn rebase_commits(
    repo: &git2::Repository,
    onto: &str,
) -> Result<Vec<RebaseCommit>, git2::Error> {
    let target = repo.revparse_single(onto)?.peel_to_commit()?;
    let mut walk = repo.revwalk()?;
    walk.push_head()?;
    walk.hide(target.id())?;
    let mut commits = Vec::new();
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        if commit.parent_count() > 1 {
            continue;
        }
        if commits.len() >= MAX_PLAN_COMMITS {
            return Err(git2::Error::from_str(&format!(
                "more than {MAX_PLAN_COMMITS} commits to replay — rebase from the terminal"
            )));
        }
        let short_id = commit.as_object().short_id()?;
        commits.push(RebaseCommit {
            oid: commit.id(),
            short_id: short_id.as_str().unwrap_or_default().to_string(),
            summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
            message: commit.message().unwrap_or("").trim_end().to_string(),
            author: commit.author().name().unwrap_or("").to_string(),
        });
    }
    commits.reverse();
    Ok(commits)
}

/// First invalid point of a plan, if any — `entries` **oldest first**, one
/// `(choice, reword message is blank)` pair per commit. Checked live by the
/// page (inline error + Start disabled) and re-checked by the execution.
pub fn plan_error(entries: &[(RebaseChoice, bool)]) -> Option<String> {
    let mut kept_below = false;
    for (choice, blank_message) in entries {
        match choice {
            RebaseChoice::Squash | RebaseChoice::Fixup if !kept_below => {
                return Some(
                    "the oldest kept commit cannot squash or fixup — \
                     there is no commit below to meld into"
                        .to_string(),
                );
            }
            RebaseChoice::Reword if *blank_message => {
                return Some("a reworded commit needs a non-empty message".to_string());
            }
            _ => {}
        }
        if !matches!(choice, RebaseChoice::Drop) {
            kept_below = true;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_error_accepts_plain_picks_and_melds_onto_kept_commits() {
        assert_eq!(plan_error(&[]), None);
        assert_eq!(
            plan_error(&[(RebaseChoice::Pick, false), (RebaseChoice::Squash, false)]),
            None
        );
        assert_eq!(
            plan_error(&[(RebaseChoice::Reword, false), (RebaseChoice::Fixup, false)]),
            None
        );
    }

    #[test]
    fn plan_error_refuses_a_meld_with_nothing_kept_below() {
        assert!(plan_error(&[(RebaseChoice::Squash, false)]).is_some());
        assert!(plan_error(&[(RebaseChoice::Fixup, false)]).is_some());
        // Dropped commits do not count as a meld base.
        assert!(
            plan_error(&[(RebaseChoice::Drop, false), (RebaseChoice::Squash, false)]).is_some()
        );
    }

    #[test]
    fn plan_error_refuses_a_blank_reword_message() {
        assert!(plan_error(&[(RebaseChoice::Reword, true)]).is_some());
        assert_eq!(plan_error(&[(RebaseChoice::Reword, false)]), None);
    }

    #[test]
    fn plan_error_allows_dropping_everything() {
        // Resetting the branch onto the target is a legitimate explicit choice;
        // the page states the consequence instead of refusing.
        assert_eq!(
            plan_error(&[(RebaseChoice::Drop, false), (RebaseChoice::Drop, false)]),
            None
        );
    }

    #[test]
    fn every_action_maps_back_to_its_choice() {
        assert_eq!(RebaseAction::Pick.choice(), RebaseChoice::Pick);
        assert_eq!(
            RebaseAction::Reword("m".into()).choice(),
            RebaseChoice::Reword
        );
        assert_eq!(RebaseAction::Squash.choice(), RebaseChoice::Squash);
        assert_eq!(RebaseAction::Fixup.choice(), RebaseChoice::Fixup);
        assert_eq!(RebaseAction::Drop.choice(), RebaseChoice::Drop);
    }
}
