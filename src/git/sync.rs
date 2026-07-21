use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git::cli::{self, CliError, CliOutput};
use crate::git::rebase::{self, RebaseAction, RebaseStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    Ff,
    FfOnly,
    Rebase,
}

/// Default operation of the Pull split-button (git.md §10), persisted in
/// `prefs.toml` in kebab-case (`fetch-all` / `ff` / `ff-only` / `rebase`,
/// M12-7). Labels and command mapping: `ui::graph_toolbar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PullDefault {
    FetchAll,
    #[default]
    Ff,
    FfOnly,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    UpToDate,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    NoRemote,
    /// Force push attempted on a branch without an upstream (git.md §10): the UI
    /// greys the entry out, the first publication goes through the plain `-u` push.
    NoUpstream,
    FfOnlyRefused,
    /// Merge / rebase stopped on a conflict — left as is (git.md §10).
    Conflicts,
    /// Push rejected; never forced.
    NonFastForward,
    /// `--force-with-lease` refused: the remote moved past the last fetch
    /// (git.md §10) — the toast suggests a fetch first.
    StaleInfo,
    /// The current branch's upstream no longer exists on the remote (merged &
    /// deleted elsewhere, e.g. the Bitbucket UI): `pull` prunes the stale tracking
    /// ref and the UI stays silent — no toast (D-2026-06-16-pull-remote-branch-gone).
    RemoteBranchGone,
    AuthFailed,
    GitNotFound,
    TimedOut,
    Other(String),
}

pub fn fetch_all(workdir: &Path) -> Result<SyncOutcome, SyncError> {
    ensure_remote(workdir)?;
    let out = exec(workdir, &["fetch", "--all"])?;
    if out.success() {
        Ok(fetch_outcome(&out.stderr))
    } else {
        Err(classify_failure(&out))
    }
}

/// `fetch_all` for the silent background fetch (worker.rs `FetchRunner`):
/// auto-maintenance is disabled so the 10 s cadence never triggers a repack /
/// `packed-refs` rewrite. The fetch then only writes loose `refs/remotes/*` +
/// objects + `FETCH_HEAD` — disjoint from the index/local refs the mutation lock
/// guards, which is what makes the background fetch safe to run lock-free.
pub fn background_fetch_all(workdir: &Path) -> Result<SyncOutcome, SyncError> {
    ensure_remote(workdir)?;
    let out = exec(
        workdir,
        &[
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "fetch",
            "--all",
        ],
    )?;
    if out.success() {
        Ok(fetch_outcome(&out.stderr))
    } else {
        Err(classify_failure(&out))
    }
}

pub fn pull(workdir: &Path, mode: PullMode) -> Result<SyncOutcome, SyncError> {
    let repo = open_repo(workdir)?;
    let upstream = current_upstream(&repo);
    let args = pull_args(
        mode,
        upstream.as_ref().map(|(r, b)| (r.as_str(), b.as_str())),
    );
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = exec(workdir, &args)?;
    if out.success() {
        return Ok(pull_outcome(&out.stdout));
    }
    let error = classify_failure(&out);
    // Upstream merged & deleted on the remote: drop the stale tracking ref so the
    // graph stops folding it into the local chip (`also_remote`); surfaced silently
    // by the UI (git.md §10).
    if error == SyncError::RemoteBranchGone {
        if let Some((remote, branch)) = &upstream {
            prune_tracking_ref(&repo, remote, branch);
        }
    }
    Err(error)
}

/// Drops the stale `refs/remotes/<remote>/<branch>`. Best-effort: a missing ref
/// is a no-op.
fn prune_tracking_ref(repo: &git2::Repository, remote: &str, branch: &str) {
    if let Ok(mut reference) = repo.find_reference(&format!("refs/remotes/{remote}/{branch}")) {
        let _ = reference.delete();
    }
}

/// Upstream of the current branch (`branch.<name>.remote` + `branch.<name>.merge`):
/// the pull is limited to this branch instead of fetching the whole remote
/// (D-2026-06-04-pull-branche-courante). `None` (detached, no tracking) ⇒ falls
/// back to a bare `git pull`, which produces the standard git error.
fn current_upstream(repo: &git2::Repository) -> Option<(String, String)> {
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return None;
    }
    let name = head.shorthand().ok()?;
    let config = repo.config().ok()?;
    let remote = config.get_string(&format!("branch.{name}.remote")).ok()?;
    let merge = config.get_string(&format!("branch.{name}.merge")).ok()?;
    let branch = merge.strip_prefix("refs/heads/")?.to_string();
    Some((remote, branch))
}

/// Rebases the **current branch** onto `onto` (graph context menu — git.md §9),
/// via the `git` subprocess like Pull/Push: hooks, `rerere` and the user config
/// apply, and the conflict classification matches the CLI output. Local op: no
/// remote required. A conflict leaves the rebase **in progress** (banner §10,
/// resolution in the terminal), same rule as a Pull conflict.
pub fn rebase_onto(workdir: &Path, onto: &str) -> Result<SyncOutcome, SyncError> {
    // A ref can begin with '-' via plumbing (update-ref) or a hostile remote:
    // never let it reach the CLI as a flag (`git rebase --exec=…`).
    if onto.starts_with('-') {
        return Err(SyncError::Other(format!("invalid ref name '{onto}'")));
    }
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    if repo.state() != git2::RepositoryState::Clean {
        return Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into(),
        ));
    }
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    // A branch with no commits of its own that has diverged from the target:
    // `git rebase <onto>` would replay the shared mainline commits onto it (they
    // are absent from `onto`). Move the branch onto the target instead — no
    // replay (git.md §9). Every other case keeps the plain rebase.
    let plain = ["rebase", onto];
    let move_onto = ["rebase", "--onto", onto, "HEAD"];
    let args: &[&str] = if rebase_moves_empty_branch(&repo, &head, onto) {
        &move_onto
    } else {
        &plain
    };
    let out = exec(workdir, args)?;
    if out.success() {
        Ok(rebase_outcome(&out.stdout, &out.stderr))
    } else {
        Err(classify_failure(&out))
    }
}

/// The current branch carries no commits of its own — its tip lives in another
/// **local** branch (equal to it, or an ancestor of it) — **and** it has diverged
/// from `onto`: the case where `git rebase <onto>` would replay the shared
/// mainline commits onto the target (git.md §9). The branch is then moved onto
/// the target with no replay. Only local branches count as "another branch": a
/// remote mirror (`origin/<self>`) holding the tip is the branch's own line, not
/// a separate one — treating it as one would drop already-pushed commits. Any
/// read error ⇒ `false` (fall back to the plain rebase, git decides).
fn rebase_moves_empty_branch(repo: &git2::Repository, head: &git2::Reference, onto: &str) -> bool {
    let Some(tip) = head.target() else {
        return false;
    };
    let Ok(onto_oid) = repo
        .revparse_single(onto)
        .and_then(|obj| obj.peel_to_commit())
        .map(|commit| commit.id())
    else {
        return false;
    };
    let diverged = tip != onto_oid
        && !repo.graph_descendant_of(tip, onto_oid).unwrap_or(false)
        && !repo.graph_descendant_of(onto_oid, tip).unwrap_or(false);
    if !diverged {
        return false;
    }
    let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) else {
        return false;
    };
    branches.flatten().any(|(branch, _)| {
        branch.get().name() != head.name()
            && branch.get().target().is_some_and(|oid| {
                oid == tip || repo.graph_descendant_of(oid, tip).unwrap_or(false)
            })
    })
}

/// Replays the commit `sha` on the **current branch** (graph row menu — git.md
/// §9), via `git cherry-pick <sha>` like Rebase onto: hooks, `rerere` and the
/// user config apply, the conflict classification matches the CLI. Local op: no
/// remote required. A merge commit is refused before anything runs (the entry is
/// absent in the UI, but git itself refuses without a mainline). A conflict
/// leaves the cherry-pick **in progress** (banner §10, Abort follows the state);
/// a dirty tree or an empty result surfaces git's refusal as-is — no auto-stash.
pub fn cherry_pick(workdir: &Path, sha: &str) -> Result<SyncOutcome, SyncError> {
    // A commit-ish from a hostile source could begin with '-': never let it reach
    // the CLI as a flag (same guard as `rebase_onto`).
    if sha.starts_with('-') {
        return Err(SyncError::Other(format!("invalid commit '{sha}'")));
    }
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    if repo.state() != git2::RepositoryState::Clean {
        return Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into(),
        ));
    }
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    let out = exec(workdir, &["cherry-pick", sha])?;
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Commits the inverse of `sha` on the **current branch** (graph row menu —
/// git.md §9), via `git revert --no-edit <sha>` — no editor ever opens. Same
/// execution rules, refusals and conflict behavior as [`cherry_pick`].
pub fn revert(workdir: &Path, sha: &str) -> Result<SyncOutcome, SyncError> {
    if sha.starts_with('-') {
        return Err(SyncError::Other(format!("invalid commit '{sha}'")));
    }
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    if repo.state() != git2::RepositoryState::Clean {
        return Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into(),
        ));
    }
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    let out = exec(workdir, &["revert", "--no-edit", sha])?;
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Merges `from` into the **current branch** (graph context menu — git.md §9),
/// via the `git` subprocess like Rebase onto: hooks, `rerere` and the user
/// config apply, and the conflict classification matches the CLI output. Local
/// op: no remote required. A conflict leaves the merge **in progress** (banner
/// §10, resolution in the terminal), same rule as a Pull conflict.
pub fn merge(workdir: &Path, from: &str) -> Result<SyncOutcome, SyncError> {
    // Same argument-injection guard as `rebase_onto`.
    if from.starts_with('-') {
        return Err(SyncError::Other(format!("invalid ref name '{from}'")));
    }
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    if repo.state() != git2::RepositoryState::Clean {
        return Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into(),
        ));
    }
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    let out = exec(workdir, &["merge", from])?;
    if out.success() {
        // Same stdout sentence as a no-op pull ("Already up to date.").
        Ok(pull_outcome(&out.stdout))
    } else {
        Err(classify_failure(&out))
    }
}

/// Executes the plan prepared on the rebase page (git.md §9): `git rebase -i`
/// with the **todo injected** via `GIT_SEQUENCE_EDITOR` — no editor ever opens
/// (`GIT_EDITOR=true` keeps git's combined message for squashes; rewords run
/// as `exec git commit --amend -F <file>`, the message never crosses a shell).
/// The plan is **re-derived and compared** before running: HEAD or `onto`
/// moved since the page opened ⇒ a stale todo would silently drop the new
/// commits — clean refusal instead; same for `current` (the branch the page
/// was opened on), so a checkout to a same-tip branch (e.g. a fresh backup)
/// never rewrites a branch the page did not show. Same conflict rule as
/// [`rebase_onto`].
pub fn interactive_rebase(
    workdir: &Path,
    current: &str,
    onto: &str,
    steps: &[RebaseStep],
) -> Result<SyncOutcome, SyncError> {
    // Same argument-injection guard as `rebase_onto`.
    if onto.starts_with('-') {
        return Err(SyncError::Other(format!("invalid ref name '{onto}'")));
    }
    if steps.is_empty() {
        return Err(SyncError::Other("nothing to rebase".into()));
    }
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    // An op can start in the terminal while the page is open: same refusal as
    // the page-open guard, re-checked where it cannot be raced.
    if repo.state() != git2::RepositoryState::Clean {
        return Err(SyncError::Other(
            "a merge or rebase is already in progress — resolve or abort it first".into(),
        ));
    }
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    if head.shorthand().ok() != Some(current) {
        return Err(SyncError::Other(
            "the checked-out branch changed since the plan was prepared — \
             reopen Interactive rebase"
                .into(),
        ));
    }
    let shape: Vec<(rebase::RebaseChoice, bool)> = steps
        .iter()
        .map(|step| {
            let blank = matches!(&step.action, RebaseAction::Reword(m) if m.trim().is_empty());
            (step.action.choice(), blank)
        })
        .collect();
    if let Some(error) = rebase::plan_error(&shape) {
        return Err(SyncError::Other(error));
    }
    let replayed = rebase::rebase_commits(&repo, onto).map_err(|err| git_err(&err))?;
    if !replayed
        .iter()
        .map(|c| c.oid)
        .eq(steps.iter().map(|s| s.oid))
    {
        return Err(SyncError::Other(
            "the branch changed since the plan was prepared — reopen Interactive rebase".into(),
        ));
    }
    // Todo + reword message files live under `.git` (per-worktree gitdir), NOT
    // in a tempdir: a rebase stopped on a conflict still has pending
    // `exec … -F` steps that `git rebase --continue` must find long after this
    // function returned. Removed on success / by `abort_op`; stale leftovers
    // are replaced wholesale on the next run.
    let scratch = repo.path().join(SCRATCH_DIR);
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|err| SyncError::Other(err.to_string()))?;
    let todo = build_todo(steps, &scratch).map_err(|err| SyncError::Other(err.to_string()))?;
    let todo_path = scratch.join("todo");
    std::fs::write(&todo_path, todo).map_err(|err| SyncError::Other(err.to_string()))?;
    let out = exec_with_env(
        workdir,
        &["rebase", "-i", onto],
        &[
            (
                "GIT_SEQUENCE_EDITOR",
                format!("cp {}", shell_single_quote(&todo_path.to_string_lossy())),
            ),
            ("GIT_EDITOR", "true".to_string()),
        ],
    )?;
    if out.success() {
        let _ = std::fs::remove_dir_all(&scratch);
        Ok(SyncOutcome::Updated)
    } else {
        // Kept on purpose: a conflict-stopped rebase reads the message files
        // from the remaining exec steps when continued from the terminal.
        Err(classify_failure(&out))
    }
}

/// Scratch folder under the gitdir carrying the injected todo and the reword
/// message files for the lifetime of the rebase (conflict stops included).
const SCRATCH_DIR: &str = "helm-rebase";

/// Todo lines for the injected plan, **oldest first** (git todo order). A
/// reword becomes `pick` + an `exec` amending from a message file — the
/// sequence stays non-interactive whatever the message contains. The `exec` is
/// **guarded on the original message**: it runs on whatever `HEAD` is, and a
/// `pick` conflict the user resolves with git's own `git rebase --skip` hint
/// would otherwise hand the new message to the commit below (the replayed
/// target tip). Both sides of the comparison are read from git, so nothing but
/// the commit oid and the message file's path crosses the shell.
fn build_todo(steps: &[RebaseStep], dir: &Path) -> std::io::Result<String> {
    use std::fmt::Write;
    let mut todo = String::new();
    for (index, step) in steps.iter().enumerate() {
        let oid = step.oid;
        let line = match &step.action {
            RebaseAction::Pick => format!("pick {oid}"),
            RebaseAction::Squash => format!("squash {oid}"),
            RebaseAction::Fixup => format!("fixup {oid}"),
            RebaseAction::Drop => format!("drop {oid}"),
            RebaseAction::Reword(message) => {
                let path = dir.join(format!("message-{index}"));
                std::fs::write(&path, message)?;
                let file = shell_single_quote(&path.to_string_lossy());
                format!(
                    "pick {oid}\nexec if test \"$(git log -1 --format=%B)\" = \
                     \"$(git log -1 --format=%B {oid})\"; then git commit --amend -F {file}; \
                     else echo 'helm: reword skipped, HEAD is not the commit it targeted' >&2; \
                     false; fi"
                )
            }
        };
        let _ = writeln!(todo, "{line}");
    }
    Ok(todo)
}

/// `'…'` with embedded single quotes escaped (`'\''`): the value crosses one
/// shell evaluation (`GIT_SEQUENCE_EDITOR` and the todo's `exec` lines run via
/// `sh -c`) — a `TMPDIR` with spaces or quotes must not split the command.
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Aborts the operation in progress (banner button, git.md §10): the abort
/// flavor follows `Repository::state()` — the banner also covers an op started
/// in the terminal. Resolved in the meantime ⇒ clean failure, nothing runs.
pub fn abort_op(workdir: &Path) -> Result<SyncOutcome, SyncError> {
    use git2::RepositoryState as State;
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    let args: [&str; 2] = match repo.state() {
        State::Rebase | State::RebaseInteractive | State::RebaseMerge => ["rebase", "--abort"],
        State::Merge => ["merge", "--abort"],
        State::CherryPick | State::CherryPickSequence => ["cherry-pick", "--abort"],
        State::Revert | State::RevertSequence => ["revert", "--abort"],
        State::ApplyMailbox | State::ApplyMailboxOrRebase => ["am", "--abort"],
        // `bisect reset` is its abort: back to the pre-bisect HEAD.
        State::Bisect => ["bisect", "reset"],
        State::Clean => {
            return Err(SyncError::Other("no operation in progress".into()));
        }
    };
    let out = exec(workdir, &args)?;
    if out.success() {
        // An aborted interactive rebase no longer needs its injected plan.
        let _ = std::fs::remove_dir_all(repo.path().join(SCRATCH_DIR));
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Continues the operation in progress once its conflicts are resolved (conflict
/// editor "Continue", conflicts.md §2/§5): the `--continue` flavor follows
/// `Repository::state()`. `GIT_EDITOR=true` keeps it non-interactive — git takes
/// the prepared message without opening an editor. A rebase `--continue` may
/// immediately surface the next commit's conflicts: the subprocess then fails
/// with `CONFLICT`, mapped to `SyncError::Conflicts`, and the op stays in progress.
pub fn continue_op(workdir: &Path) -> Result<SyncOutcome, SyncError> {
    use git2::RepositoryState as State;
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    let args: &[&str] = match repo.state() {
        State::Rebase | State::RebaseInteractive | State::RebaseMerge => &["rebase", "--continue"],
        State::Merge => &["merge", "--continue"],
        State::CherryPick | State::CherryPickSequence => &["cherry-pick", "--continue"],
        State::Revert | State::RevertSequence => &["revert", "--continue"],
        State::ApplyMailbox | State::ApplyMailboxOrRebase => &["am", "--continue"],
        State::Bisect | State::Clean => {
            return Err(SyncError::Other("no operation in progress".into()));
        }
    };
    let out = exec_with_env(workdir, args, &[("GIT_EDITOR", "true".to_string())])?;
    if out.success() {
        let _ = std::fs::remove_dir_all(repo.path().join(SCRATCH_DIR));
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

pub fn push(workdir: &Path) -> Result<SyncOutcome, SyncError> {
    let repo = open_repo(workdir)?;
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    let branch_name = head.shorthand().unwrap_or_default().to_string();
    let has_upstream = repo
        .find_branch(&branch_name, git2::BranchType::Local)
        .and_then(|b| b.upstream())
        .is_ok();
    let out = if has_upstream {
        exec(workdir, &["push"])?
    } else {
        exec(workdir, &["push", "-u", "origin", &branch_name])?
    };
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Force-pushes `branch` to its upstream **with a lease pinned to `lease`**
/// (toolbar Push chevron — git.md §10):
/// `git push --force-with-lease=refs/heads/<branch>:<lease> <remote> <branch>`.
/// `lease` is the remote-tracking oid captured when the confirmation modal was
/// armed — i.e. the tip helm was *displaying*. A bare `--force-with-lease` would
/// re-read that ref at push time, and the background fetch refreshes it every
/// 10 s, so it would always agree with itself and never refuse; pinned, a remote
/// that moved after the user looked yields `StaleInfo` ⇒ toast suggesting a fetch
/// first. Bare `--force` is never used. The deliberate outlet for a rebased branch
/// (§9) whose plain push is rejected. Requires an upstream — the UI greys it out
/// otherwise (the first publication goes through the plain `-u` push, [`push`]).
pub fn force_push(
    workdir: &Path,
    branch: &str,
    lease: git2::Oid,
) -> Result<SyncOutcome, SyncError> {
    let repo = open_repo(workdir)?;
    let head = repo.head().map_err(|err| git_err(&err))?;
    if !head.is_branch() {
        return Err(SyncError::Other("HEAD is detached".into()));
    }
    // The lease is only meaningful for the branch it was captured on: a checkout
    // between arming the modal and confirming it would aim it at another branch.
    if head.shorthand() != Ok(branch) {
        return Err(SyncError::Other(format!("HEAD is no longer '{branch}'")));
    }
    let Some((remote, _)) = current_upstream(&repo) else {
        return Err(SyncError::NoUpstream);
    };
    let out = exec(
        workdir,
        &["push", &lease_arg(branch, lease), &remote, branch],
    )?;
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// `--force-with-lease=<ref>:<oid>`: the ref is **fully qualified** so a same-named
/// tag can never be the one leased, and the oid is the expected current tip.
fn lease_arg(branch: &str, lease: git2::Oid) -> String {
    format!("--force-with-lease=refs/heads/{branch}:{lease}")
}

/// Deletes a branch on its remote (`git push <remote> --delete <branch>`, graph
/// context menu — git.md §9). `name` is the name displayed by the chip: a remote
/// ref `origin/x` as is, or a **local** branch name whose same-named remote
/// `<remote>/x` is resolved — same local+remote merging rule as the graph chips
/// (the upstream may point elsewhere).
pub fn delete_remote_branch(workdir: &Path, name: &str) -> Result<SyncOutcome, SyncError> {
    let repo = open_repo(workdir)?;
    let (remote, branch) = resolve_remote_branch(&repo, name)?;
    let out = exec(workdir, &["push", &remote, "--delete", &branch])?;
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Pushes the tag `name` to `origin` (`git push origin <tag>`, graph tag menu —
/// git.md §9). `origin`-only: multi-remote selection stays out of scope (§10) —
/// a missing `origin` surfaces git's error as a toast like any push failure.
pub fn push_tag(workdir: &Path, name: &str) -> Result<SyncOutcome, SyncError> {
    // A tag name from a hostile ref could begin with '-': never let it reach the
    // CLI as a flag (same guard as `rebase_onto`).
    if name.starts_with('-') {
        return Err(SyncError::Other(format!("invalid tag name '{name}'")));
    }
    open_repo(workdir)?;
    let out = exec(workdir, &["push", "origin", name])?;
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Deletes the tag on `origin` (`git push origin --delete refs/tags/<tag>`, graph
/// tag menu — git.md §9): the refspec is **fully qualified** so a same-named
/// branch on the remote is never touched. `origin`-only; a tag absent on the
/// remote surfaces git's error as a toast (the graph cannot know — `refs/tags`
/// is a local namespace).
pub fn delete_remote_tag(workdir: &Path, name: &str) -> Result<SyncOutcome, SyncError> {
    if name.starts_with('-') {
        return Err(SyncError::Other(format!("invalid tag name '{name}'")));
    }
    open_repo(workdir)?;
    let refspec = format!("refs/tags/{name}");
    let out = exec(workdir, &["push", "origin", "--delete", &refspec])?;
    if out.success() {
        Ok(SyncOutcome::Updated)
    } else {
        Err(classify_failure(&out))
    }
}

/// Resolves `(remote, branch)` from a chip's name: an existing remote ref
/// (`origin/x`), otherwise the first same-named remote `<remote>/<name>`.
fn resolve_remote_branch(
    repo: &git2::Repository,
    name: &str,
) -> Result<(String, String), SyncError> {
    if repo.find_branch(name, git2::BranchType::Remote).is_ok() {
        if let Some((remote, branch)) = name.split_once('/') {
            return Ok((remote.to_string(), branch.to_string()));
        }
    }
    let branches = repo
        .branches(Some(git2::BranchType::Remote))
        .map_err(|err| git_err(&err))?;
    for entry in branches {
        let (branch, _) = entry.map_err(|err| git_err(&err))?;
        let reference = branch.into_reference();
        let Ok(shorthand) = reference.shorthand() else {
            continue;
        };
        if let Some((remote, candidate)) = shorthand.split_once('/') {
            if candidate == name {
                return Ok((remote.to_string(), candidate.to_string()));
            }
        }
    }
    Err(SyncError::Other(format!("no remote branch named '{name}'")))
}

pub fn pull_args(mode: PullMode, upstream: Option<(&str, &str)>) -> Vec<String> {
    // `--ff` alone still rebases under `pull.rebase=true` (git 2.55), which rewrites
    // the local commits behind the default button — pin the merge explicitly.
    let flags: &[&str] = match mode {
        PullMode::Ff => &["--no-rebase", "--ff"],
        PullMode::FfOnly => &["--ff-only"],
        PullMode::Rebase => &["--rebase"],
    };
    let mut args = vec!["pull".to_string()];
    args.extend(flags.iter().map(|flag| flag.to_string()));
    if let Some((remote, branch)) = upstream {
        args.push(remote.to_string());
        args.push(branch.to_string());
    }
    args
}

fn ensure_remote(workdir: &Path) -> Result<(), SyncError> {
    open_repo(workdir).map(|_| ())
}

/// Opens the repo and checks that at least one remote is configured (`NoRemote`).
fn open_repo(workdir: &Path) -> Result<git2::Repository, SyncError> {
    let repo = git2::Repository::open(workdir).map_err(|err| git_err(&err))?;
    let remotes = repo.remotes().map_err(|err| git_err(&err))?;
    if remotes.is_empty() {
        return Err(SyncError::NoRemote);
    }
    Ok(repo)
}

fn git_err(err: &git2::Error) -> SyncError {
    SyncError::Other(err.message().to_string())
}

fn exec(workdir: &Path, args: &[&str]) -> Result<CliOutput, SyncError> {
    exec_with_env(workdir, args, &[])
}

fn exec_with_env(
    workdir: &Path,
    args: &[&str],
    envs: &[(&str, String)],
) -> Result<CliOutput, SyncError> {
    match cli::run_with_env(workdir, args, envs) {
        Ok(out) => Ok(out),
        Err(CliError::NotFound) => Err(SyncError::GitNotFound),
        Err(CliError::TimedOut(_)) => Err(SyncError::TimedOut),
        Err(CliError::Io(err)) => Err(SyncError::Other(err.to_string())),
    }
}

fn pull_outcome(stdout: &str) -> SyncOutcome {
    // "Already up to date." (or "up-to-date" on older git).
    if stdout.contains("Already up") {
        SyncOutcome::UpToDate
    } else {
        SyncOutcome::Updated
    }
}

// "Current branch <name> is up to date." (stdout); the success summary
// "Successfully rebased and updated <ref>." goes to stderr. Anchored on the
// whole git sentence: hook chatter quoting "is up to date" mid-output must
// not turn a real rewrite into an "already up to date" toast.
fn rebase_outcome(stdout: &str, stderr: &str) -> SyncOutcome {
    let up_to_date = |text: &str| {
        text.lines().any(|line| {
            line.starts_with("Current branch ") && line.trim_end().ends_with("is up to date.")
        })
    };
    if up_to_date(stdout) || up_to_date(stderr) {
        SyncOutcome::UpToDate
    } else {
        SyncOutcome::Updated
    }
}

// The fetch summary goes to stderr: every ref update adds a line
// "<old>..<new>  <ref> -> <remote>/<ref>"; nothing new ⇒ none.
fn fetch_outcome(stderr: &str) -> SyncOutcome {
    if stderr.contains("->") {
        SyncOutcome::Updated
    } else {
        SyncOutcome::UpToDate
    }
}

// These keywords (like pull_outcome/fetch_outcome above) only match because
// cli::run pins the subprocess to LC_ALL=C — a localized git breaks them.
fn classify_failure(out: &CliOutput) -> SyncError {
    let combined = format!("{}\n{}", out.stdout, out.stderr);
    if combined.contains("CONFLICT") || combined.contains("Automatic merge failed") {
        return SyncError::Conflicts;
    }
    if combined.contains("Not possible to fast-forward") {
        return SyncError::FfOnlyRefused;
    }
    // `--force-with-lease` lease miss: "(stale info)" — distinct from a plain
    // push rejection ("(fetch first)" / "(non-fast-forward)"), so it never
    // collides with the `[rejected]` branch below.
    if out.stderr.contains("stale info") {
        return SyncError::StaleInfo;
    }
    if out.stderr.contains("[rejected]") {
        return SyncError::NonFastForward;
    }
    // `git pull <remote> <branch>` when the branch was merged & deleted on the
    // remote: the named ref is gone. `pull` then prunes the stale tracking ref.
    if combined.contains("couldn't find remote ref") {
        return SyncError::RemoteBranchGone;
    }
    if combined.contains("Authentication failed")
        || combined.contains("could not read Username")
        || combined.contains("could not read Password")
        || combined.contains("Permission denied (publickey")
    {
        return SyncError::AuthFailed;
    }
    SyncError::Other(summarize(out))
}

fn summarize(out: &CliOutput) -> String {
    let lines: Vec<&str> = out
        .stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or(lines.last())
        .map(|l| l.to_string())
        .unwrap_or_else(|| format!("git exited with code {:?}", out.code))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(stdout: &str, stderr: &str) -> CliOutput {
        CliOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            code: Some(1),
        }
    }

    #[test]
    fn pull_args_map_modes_to_flags() {
        assert_eq!(
            pull_args(PullMode::Ff, None),
            ["pull", "--no-rebase", "--ff"]
        );
        assert_eq!(pull_args(PullMode::FfOnly, None), ["pull", "--ff-only"]);
        assert_eq!(pull_args(PullMode::Rebase, None), ["pull", "--rebase"]);
    }

    #[test]
    fn pull_args_target_the_upstream_branch_when_known() {
        assert_eq!(
            pull_args(PullMode::Ff, Some(("origin", "main"))),
            ["pull", "--no-rebase", "--ff", "origin", "main"]
        );
        assert_eq!(
            pull_args(PullMode::Rebase, Some(("upstream", "feat/x"))),
            ["pull", "--rebase", "upstream", "feat/x"]
        );
    }

    #[test]
    fn pull_outcome_detects_up_to_date() {
        assert_eq!(pull_outcome("Already up to date.\n"), SyncOutcome::UpToDate);
        assert_eq!(
            pull_outcome("Updating 1a2b3c..4d5e6f\nFast-forward\n"),
            SyncOutcome::Updated
        );
    }

    #[test]
    fn rebase_outcome_detects_up_to_date_on_either_pipe() {
        assert_eq!(
            rebase_outcome("Current branch feat is up to date.\n", ""),
            SyncOutcome::UpToDate
        );
        assert_eq!(
            rebase_outcome("", "Current branch feat is up to date.\n"),
            SyncOutcome::UpToDate
        );
        assert_eq!(
            rebase_outcome("", "Successfully rebased and updated refs/heads/feat.\n"),
            SyncOutcome::Updated
        );
        // Hook chatter quoting the phrase mid-line never downgrades a real
        // rewrite to "already up to date".
        assert_eq!(
            rebase_outcome(
                "pre-rebase hook: cache is up to date, skipping\n",
                "Successfully rebased and updated refs/heads/feat.\n"
            ),
            SyncOutcome::Updated
        );
    }

    fn repo_commit(
        repo: &git2::Repository,
        content: &str,
        msg: &str,
        update_ref: Option<&str>,
        parents: &[git2::Oid],
    ) -> git2::Oid {
        let sig = git2::Signature::now("T", "t@e").unwrap();
        std::fs::write(repo.workdir().unwrap().join("f.txt"), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("f.txt")).unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let parents: Vec<git2::Commit> = parents
            .iter()
            .map(|oid| repo.find_commit(*oid).unwrap())
            .collect();
        let refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(update_ref, &sig, &sig, msg, &tree, &refs)
            .unwrap()
    }

    #[test]
    fn rebase_moves_only_a_committless_branch_that_diverged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        // master: A -> B ; other: A -> D (diverged at A) ; feat == master (B).
        let a = repo_commit(&repo, "a\n", "A", Some("HEAD"), &[]);
        let d = repo_commit(&repo, "d\n", "D", None, &[a]);
        repo.branch("other", &repo.find_commit(d).unwrap(), false)
            .unwrap();
        let b = repo_commit(&repo, "b\n", "B", Some("HEAD"), &[a]);
        repo.branch("feat", &repo.find_commit(b).unwrap(), false)
            .unwrap();
        repo.set_head("refs/heads/feat").unwrap();

        let head = repo.head().unwrap();
        // feat has no commits of its own and diverged from other → move onto it.
        assert!(rebase_moves_empty_branch(&repo, &head, "other"));
        // Onto the branch it shares its tip with (master): not diverged → plain.
        assert!(!rebase_moves_empty_branch(&repo, &head, "master"));

        // feat gains a commit of its own → plain rebase (its work is replayed).
        repo_commit(&repo, "e\n", "E", Some("refs/heads/feat"), &[b]);
        let head = repo.head().unwrap();
        assert!(!rebase_moves_empty_branch(&repo, &head, "other"));
    }

    #[test]
    fn fetch_outcome_detects_ref_updates() {
        assert_eq!(fetch_outcome("Fetching origin\n"), SyncOutcome::UpToDate);
        assert_eq!(
            fetch_outcome(
                "Fetching origin\nFrom file:///r\n   1a2b..3c4d  master -> origin/master\n"
            ),
            SyncOutcome::Updated
        );
    }

    #[test]
    fn build_todo_maps_each_action_to_its_line() {
        let dir = tempfile::tempdir().unwrap();
        let oid = |c: char| git2::Oid::from_str(&c.to_string().repeat(40)).unwrap();
        let steps = [
            RebaseStep {
                oid: oid('a'),
                action: RebaseAction::Pick,
            },
            RebaseStep {
                oid: oid('b'),
                action: RebaseAction::Squash,
            },
            RebaseStep {
                oid: oid('c'),
                action: RebaseAction::Fixup,
            },
            RebaseStep {
                oid: oid('d'),
                action: RebaseAction::Drop,
            },
        ];
        let todo = build_todo(&steps, dir.path()).unwrap();
        assert_eq!(
            todo,
            format!(
                "pick {}\nsquash {}\nfixup {}\ndrop {}\n",
                oid('a'),
                oid('b'),
                oid('c'),
                oid('d')
            )
        );
    }

    #[test]
    fn build_todo_rewords_via_an_exec_amend_from_a_message_file() {
        let dir = tempfile::tempdir().unwrap();
        let oid = git2::Oid::from_str(&"a".repeat(40)).unwrap();
        let steps = [RebaseStep {
            oid,
            action: RebaseAction::Reword("new subject\n\nbody with 'quotes'".into()),
        }];
        let todo = build_todo(&steps, dir.path()).unwrap();
        let message_path = dir.path().join("message-0");
        assert_eq!(
            todo,
            format!(
                "pick {oid}\nexec if test \"$(git log -1 --format=%B)\" = \
                 \"$(git log -1 --format=%B {oid})\"; then git commit --amend -F {}; \
                 else echo 'helm: reword skipped, HEAD is not the commit it targeted' >&2; \
                 false; fi\n",
                shell_single_quote(&message_path.to_string_lossy())
            )
        );
        assert_eq!(
            std::fs::read_to_string(message_path).unwrap(),
            "new subject\n\nbody with 'quotes'"
        );
    }

    #[test]
    fn shell_single_quote_survives_embedded_quotes_and_spaces() {
        assert_eq!(shell_single_quote("/tmp/plain"), "'/tmp/plain'");
        assert_eq!(shell_single_quote("/tmp/with space"), "'/tmp/with space'");
        assert_eq!(shell_single_quote("/tmp/o'brien"), r"'/tmp/o'\''brien'");
    }

    #[test]
    fn classify_ff_only_refusal() {
        let out = failed("", "fatal: Not possible to fast-forward, aborting.\n");
        assert_eq!(classify_failure(&out), SyncError::FfOnlyRefused);
    }

    #[test]
    fn classify_merge_and_rebase_conflicts() {
        let merge = failed(
            "CONFLICT (content): Merge conflict in a.txt\nAutomatic merge failed; fix conflicts and then commit the result.\n",
            "",
        );
        assert_eq!(classify_failure(&merge), SyncError::Conflicts);

        let rebase = failed(
            "CONFLICT (content): Merge conflict in a.txt\n",
            "error: could not apply 1a2b3c... c3\n",
        );
        assert_eq!(classify_failure(&rebase), SyncError::Conflicts);
    }

    #[test]
    fn classify_rejected_push() {
        let out = failed(
            "",
            "To file:///remote.git\n ! [rejected]        master -> master (fetch first)\nerror: failed to push some refs to 'file:///remote.git'\n",
        );
        assert_eq!(classify_failure(&out), SyncError::NonFastForward);
    }

    #[test]
    fn lease_arg_pins_the_fully_qualified_ref_to_the_expected_oid() {
        let oid = git2::Oid::from_str("0123456789abcdef0123456789abcdef01234567").unwrap();
        assert_eq!(
            lease_arg("feat/x", oid),
            "--force-with-lease=refs/heads/feat/x:0123456789abcdef0123456789abcdef01234567"
        );
    }

    #[test]
    fn classify_force_with_lease_refusal_as_stale_info() {
        let out = failed(
            "",
            "To file:///remote.git\n ! [rejected]        main -> main (stale info)\nerror: failed to push some refs to 'file:///remote.git'\n",
        );
        assert_eq!(classify_failure(&out), SyncError::StaleInfo);
    }

    #[test]
    fn classify_auth_failures() {
        let https = failed(
            "",
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled\n",
        );
        assert_eq!(classify_failure(&https), SyncError::AuthFailed);

        let ssh = failed("", "git@example.com: Permission denied (publickey).\n");
        assert_eq!(classify_failure(&ssh), SyncError::AuthFailed);
    }

    #[test]
    fn classify_unknown_failure_summarizes_stderr() {
        let out = failed(
            "",
            "warning: something\nfatal: unable to access 'file:///gone': No such file or directory\n",
        );
        assert_eq!(
            classify_failure(&out),
            SyncError::Other(
                "fatal: unable to access 'file:///gone': No such file or directory".to_string()
            )
        );
    }
}
