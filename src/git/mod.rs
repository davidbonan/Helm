pub mod ai_rebase;
pub mod branch;
pub mod cli;
pub mod commit;
pub mod commit_detail;
pub mod conflict;
pub mod diff;
pub mod discard;
pub mod file_tree;
pub mod forge;
pub mod graph;
pub mod rebase;
pub mod stage;
pub mod stash;
pub mod status;
pub mod sync;
pub mod tag;
pub mod worker;
pub mod worktree;

use std::path::Path;

pub fn is_repo(path: &Path) -> bool {
    git2::Repository::open(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_repo_detects_git_dir_and_rejects_plain_dir() {
        let repo = tempfile::tempdir().unwrap();
        git2::Repository::init(repo.path()).unwrap();
        assert!(is_repo(repo.path()));

        let plain = tempfile::tempdir().unwrap();
        assert!(!is_repo(plain.path()));
    }
}
