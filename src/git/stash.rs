use crate::git::{cli, status};
use std::path::Path;

pub const STASH_MESSAGE: &str = "helm: stash";

/// Stashes the whole working tree (untracked included) (git.md §10). Clean tree ⇒
/// a clean `Err` (the button is greyed out, but the command stays safe).
pub fn stash(repo: &git2::Repository) -> Result<(), git2::Error> {
    save(repo, STASH_MESSAGE)
}

/// Full stash (untracked included) under `message` — shared with the checkout
/// auto-stash (`git::branch`). Runs the system `git`: libgit2's `stash_save`
/// re-hashes the whole worktree single-threaded (~13 s on a 20k-file repo, ~0.6 s
/// via the CLI).
pub fn save(repo: &git2::Repository, message: &str) -> Result<(), git2::Error> {
    finish(
        repo,
        run(
            repo,
            &["stash", "push", "--include-untracked", "--message", message],
        )?,
    )
}

/// Stashes only `paths` — **both** the staged and the unstaged changes of each
/// (a per-file stash is never partial here), untracked included. WIP sidebar
/// context menu (git.md §3). Empty paths ⇒ a clean `Err`.
///
/// Built by hand rather than via `git stash push -- <paths>`: that form snapshots
/// the **whole** index, so any *other* staged file leaks into the entry — the
/// "stashed all my files" bug (D-2026-06-23-scoped-stash-index-leak). Here the
/// entry is assembled from a scratch index seeded with HEAD, advanced only for
/// `paths`, leaving every other file untouched.
pub fn stash_paths(repo: &git2::Repository, paths: &[String]) -> Result<(), git2::Error> {
    if paths.is_empty() {
        return Err(git2::Error::from_str("nothing to stash"));
    }
    let paths = &with_rename_sources(repo, paths)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working tree"))?;

    // HEAD anchors the entry; an unborn branch has nothing tracked to scope.
    let head_ref = repo
        .head()
        .map_err(|_| git2::Error::from_str("You do not have the initial commit yet"))?;
    let branch = head_ref.shorthand().unwrap_or("HEAD").to_owned();
    let head = head_ref
        .peel_to_commit()
        .map_err(|_| git2::Error::from_str("You do not have the initial commit yet"))?;
    let head_oid = head.id().to_string();
    let head_tree = head.tree()?;
    let head_tree_oid = head_tree.id().to_string();

    // 1. Worktree tree = HEAD advanced to the worktree state of `paths` only.
    let index = repo
        .path()
        .join(format!("helm-scoped-stash-{}.index", std::process::id()));
    let env = [("GIT_INDEX_FILE", index.to_string_lossy().into_owned())];
    plumb(workdir, &["read-tree", &head_oid], &env)?;
    let mut add: Vec<&str> = vec!["add", "-A", "--"];
    add.extend(paths.iter().map(String::as_str));
    plumb(workdir, &add, &env)?;
    let w_tree = plumb(workdir, &["write-tree"], &env)?;
    let _ = std::fs::remove_file(&index);

    if w_tree == head_tree_oid {
        return Err(git2::Error::from_str("nothing to stash"));
    }

    // 2. Stash commits (clean index commit + worktree commit) and the ref + reflog
    // entry that `git stash list/show/pop` and the graph rows read.
    let index_msg = format!("index on {branch}: {STASH_MESSAGE}");
    let i_commit = plumb(
        workdir,
        &[
            "commit-tree",
            &head_tree_oid,
            "-p",
            &head_oid,
            "-m",
            &index_msg,
        ],
        &[],
    )?;
    let stash_msg = format!("On {branch}: {STASH_MESSAGE}");
    let w_commit = plumb(
        workdir,
        &[
            "commit-tree",
            &w_tree,
            "-p",
            &head_oid,
            "-p",
            &i_commit,
            "-m",
            &stash_msg,
        ],
        &[],
    )?;
    plumb(
        workdir,
        &[
            "update-ref",
            "--create-reflog",
            "-m",
            &stash_msg,
            "refs/stash",
            &w_commit,
        ],
        &[],
    )?;

    // 3. Roll back only `paths` in the real index + worktree: files in HEAD go back
    // to their committed state, the rest (untracked / freshly added) are removed —
    // their content now lives in the stash.
    let (in_head, fresh): (Vec<&String>, Vec<&String>) = paths
        .iter()
        .partition(|path| head_tree.get_path(Path::new(path)).is_ok());
    if !in_head.is_empty() {
        let mut checkout: Vec<&str> = vec!["checkout", "HEAD", "--"];
        checkout.extend(in_head.iter().map(|path| path.as_str()));
        plumb(workdir, &checkout, &[])?;
    }
    if !fresh.is_empty() {
        let mut remove: Vec<&str> = vec!["rm", "--cached", "--ignore-unmatch", "-q", "--"];
        remove.extend(fresh.iter().map(|path| path.as_str()));
        plumb(workdir, &remove, &[])?;
        for path in fresh {
            let _ = std::fs::remove_file(workdir.join(path));
        }
    }
    Ok(())
}

/// `paths` plus the old path of every rename they name. A renamed entry is a
/// single row carrying its **new** path, but the change spans both (git.md §3):
/// stashing the new side alone would shelve the addition and leave the old path
/// deleted with nothing recording it. Expanded here rather than at the call
/// sites so every caller of [`stash_paths`] is covered.
fn with_rename_sources(
    repo: &git2::Repository,
    paths: &[String],
) -> Result<Vec<String>, git2::Error> {
    let mut out = paths.to_vec();
    for path in paths {
        let old = match status::rename_old_path(repo, path, false)? {
            Some(old) => Some(old),
            None => status::rename_old_path(repo, path, true)?,
        };
        if let Some(old) = old {
            if !out.contains(&old) {
                out.push(old);
            }
        }
    }
    Ok(out)
}

/// Reads `git stash push`'s outcome (shared by the whole-tree and the path-scoped
/// saves): a real failure surfaces, an empty stash maps to "nothing to stash".
fn finish(repo: &git2::Repository, out: cli::CliOutput) -> Result<(), git2::Error> {
    if !out.success() {
        // Unborn HEAD on a clean tree fails with "You do not have the initial
        // commit yet": same contract as the dedicated message below. The scan
        // only runs on this failure path, never on the hot one.
        if !status::is_dirty(repo)? {
            return Err(git2::Error::from_str("nothing to stash"));
        }
        return Err(failure(&out));
    }
    // Exit 0 without creating a stash (cli.rs pins LC_ALL=C).
    if out
        .stdout
        .trim_start()
        .starts_with("No local changes to save")
    {
        return Err(git2::Error::from_str("nothing to stash"));
    }
    Ok(())
}

/// Applies then drops `stash@{0}`. Conflict ⇒ stash **kept** + `Err` (git.md §10):
/// `git stash pop` only drops after a truly clean application — the rule the
/// earlier libgit2 path implemented by hand
/// (D-2026-06-03-stash-pop-conserve-conflit).
pub fn pop(repo: &git2::Repository) -> Result<(), git2::Error> {
    pop_ref(repo, "stash@{0}")
}

/// Applies then drops the stash whose **stash commit** is `oid` (stash row
/// context menu, git.md §9). The index is re-resolved at execution — stashes
/// come and go between render and click. Same conflict rule as [`pop`]: the
/// stash is kept and the error is shown.
pub fn pop_at(repo: &git2::Repository, oid: git2::Oid) -> Result<(), git2::Error> {
    let mut owned = git2::Repository::open(repo.path())?;
    let index = index_of(&mut owned, oid)?;
    pop_ref(repo, &format!("stash@{{{index}}}"))
}

fn pop_ref(repo: &git2::Repository, stash_ref: &str) -> Result<(), git2::Error> {
    let out = run(repo, &["stash", "pop", stash_ref])?;
    if out.success() {
        return Ok(());
    }
    // Reopened: the worker's handle would serve a stale index snapshot.
    let owned = git2::Repository::open(repo.path())?;
    if owned.index()?.has_conflicts() {
        return Err(git2::Error::from_str(
            "conflicts while applying — the stash was kept",
        ));
    }
    Err(failure(&out))
}

/// Applies the stash whose **stash commit** is `oid` **without dropping it** —
/// the no-drop twin of [`pop_at`] (stash row context menu, git.md §9). The index
/// is re-resolved at execution. The stash stays either way: `apply` never drops,
/// so even a conflict leaves it in the list (the error is surfaced as a toast).
pub fn apply_at(repo: &git2::Repository, oid: git2::Oid) -> Result<(), git2::Error> {
    let mut owned = git2::Repository::open(repo.path())?;
    let index = index_of(&mut owned, oid)?;
    apply_ref(repo, &format!("stash@{{{index}}}"))
}

fn apply_ref(repo: &git2::Repository, stash_ref: &str) -> Result<(), git2::Error> {
    let out = run(repo, &["stash", "apply", stash_ref])?;
    if out.success() {
        return Ok(());
    }
    let owned = git2::Repository::open(repo.path())?;
    if owned.index()?.has_conflicts() {
        return Err(git2::Error::from_str(
            "conflicts while applying — the stash was kept",
        ));
    }
    Err(failure(&out))
}

fn run(repo: &git2::Repository, args: &[&str]) -> Result<cli::CliOutput, git2::Error> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("repository has no working tree"))?;
    cli::run(workdir, args).map_err(map_cli)
}

fn map_cli(err: cli::CliError) -> git2::Error {
    match err {
        cli::CliError::NotFound => git2::Error::from_str("git binary not found"),
        cli::CliError::TimedOut(timeout) => {
            git2::Error::from_str(&format!("git timed out after {}s", timeout.as_secs()))
        }
        cli::CliError::Io(err) => git2::Error::from_str(&err.to_string()),
    }
}

/// Runs one plumbing step of the scoped stash and fails on a non-zero exit.
fn plumb(workdir: &Path, args: &[&str], envs: &[(&str, String)]) -> Result<String, git2::Error> {
    let out = cli::run_with_env(workdir, args, envs).map_err(map_cli)?;
    if !out.success() {
        return Err(failure(&out));
    }
    Ok(out.stdout.trim().to_owned())
}

fn failure(out: &cli::CliOutput) -> git2::Error {
    let line = out
        .stderr
        .lines()
        .chain(out.stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("git stash failed");
    git2::Error::from_str(line.strip_prefix("error: ").unwrap_or(line))
}

/// Drops the stash whose **stash commit** is `oid` (Delete entry of the stash
/// row menu, confirmed by a modal on the caller side).
pub fn drop_at(repo: &git2::Repository, oid: git2::Oid) -> Result<(), git2::Error> {
    let mut owned = git2::Repository::open(repo.path())?;
    let index = index_of(&mut owned, oid)?;
    owned.stash_drop(index)
}

/// Index of the stash whose commit is `oid`. Gone in the meantime (popped or
/// dropped elsewhere) ⇒ clean `Err`.
fn index_of(repo: &mut git2::Repository, oid: git2::Oid) -> Result<usize, git2::Error> {
    let mut found = None;
    repo.stash_foreach(|index, _, stash_oid| {
        if *stash_oid == oid {
            found = Some(index);
        }
        true
    })?;
    found.ok_or_else(|| git2::Error::from_str("stash not found — already applied or deleted"))
}

/// Tree of the stash's **untracked commit** (3rd parent) when `commit` is a
/// stash saved with INCLUDE_UNTRACKED — `None` for a regular commit or a stash
/// without untracked files. Stash-ness is checked against the `refs/stash`
/// reflog (same source as the graph rows): 3 parents alone could be an octopus
/// merge.
pub fn untracked_tree<'r>(
    repo: &'r git2::Repository,
    commit: &git2::Commit,
) -> Result<Option<git2::Tree<'r>>, git2::Error> {
    if commit.parent_count() < 3 || !is_stash_commit(repo, commit.id())? {
        return Ok(None);
    }
    Ok(Some(repo.find_commit(commit.parent_id(2)?)?.tree()?))
}

fn is_stash_commit(repo: &git2::Repository, oid: git2::Oid) -> Result<bool, git2::Error> {
    match repo.reflog("refs/stash") {
        Ok(log) => Ok(log.iter().any(|entry| entry.id_new() == oid)),
        Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// Number of stashes (enables/disables **Pop**). Stashes live in the `refs/stash`
/// reflog; missing ref ⇒ 0.
pub fn count(repo: &git2::Repository) -> Result<usize, git2::Error> {
    match repo.reflog("refs/stash") {
        Ok(log) => Ok(log.len()),
        Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(0),
        Err(err) => Err(err),
    }
}
