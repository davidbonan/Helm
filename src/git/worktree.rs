use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};

use crate::git::status;

#[derive(Debug)]
pub enum DeleteError {
    Locked(Option<String>),
    Dirty(usize),
    Git(git2::Error),
}

impl From<git2::Error> for DeleteError {
    fn from(err: git2::Error) -> Self {
        DeleteError::Git(err)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    pub bare: bool,
    pub worktrees: Vec<WorktreeInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeSourceKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSource {
    /// Branch label shown to the user: local (`feat/x`) or full remote
    /// (`origin/feat/x`).
    pub name: String,
    pub kind: WorktreeSourceKind,
    /// Local branch that the worktree will checkout. For a remote source, this is
    /// the branch created with upstream tracking before the worktree is added.
    pub local_branch: String,
    /// Deterministic destination: `<root>.worktrees/<local_branch>`.
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorktree {
    pub source: WorktreeSource,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum CreateError {
    Unavailable(String),
    Io(std::io::Error),
    Git(git2::Error),
}

impl CreateError {
    pub fn message(&self) -> String {
        match self {
            CreateError::Unavailable(message) => message.clone(),
            CreateError::Io(err) => err.to_string(),
            CreateError::Git(err) => err.message().to_owned(),
        }
    }
}

impl From<std::io::Error> for CreateError {
    fn from(err: std::io::Error) -> Self {
        CreateError::Io(err)
    }
}

impl From<git2::Error> for CreateError {
    fn from(err: git2::Error) -> Self {
        CreateError::Git(err)
    }
}

pub fn resolve_root(path: &Path) -> Result<PathBuf, git2::Error> {
    let repo = git2::Repository::open(path)?;
    resolve_root_repo(&repo)
}

pub fn resolve_root_repo(repo: &git2::Repository) -> Result<PathBuf, git2::Error> {
    let root = if repo.is_worktree() {
        // commondir = <root>/.git; reopening the root (rather than taking the
        // parent) also covers the bare repo, whose commondir is the repo itself.
        let root_repo = git2::Repository::open(repo.commondir())?;
        root_dir(&root_repo)
    } else {
        root_dir(repo)
    };
    Ok(canonical(root))
}

pub fn list(root: &Path) -> Result<Listing, git2::Error> {
    let repo = git2::Repository::open(root)?;
    list_repo(&repo)
}

fn list_repo(repo: &git2::Repository) -> Result<Listing, git2::Error> {
    let mut worktrees = Vec::new();
    for name in repo.worktrees()?.iter() {
        // Non-UTF-8 name: unaddressable via find_worktree(&str), skipped.
        let Ok(Some(name)) = name else { continue };
        let wt = repo.find_worktree(name)?;
        let locked = matches!(wt.is_locked()?, git2::WorktreeLockStatus::Locked(_));
        worktrees.push(WorktreeInfo {
            name: name.to_string(),
            path: canonical(wt.path().to_path_buf()),
            locked,
            prunable: wt.is_prunable(None)?,
        });
    }
    Ok(Listing {
        bare: repo.is_bare(),
        worktrees,
    })
}

pub fn default_base(root: &Path) -> Result<PathBuf, git2::Error> {
    let parent = root
        .parent()
        .ok_or_else(|| git2::Error::from_str("repository root has no parent directory"))?;
    let mut name = root
        .file_name()
        .ok_or_else(|| git2::Error::from_str("repository root has no directory name"))?
        .to_os_string();
    name.push(".worktrees");
    Ok(parent.join(name))
}

/// Base directory new worktrees land under: the per-project override
/// (worktrees.md §6) when set, else `default_base`. A relative override is
/// resolved against the root; an empty one falls back to the default.
pub fn resolve_base(root: &Path, configured: Option<&Path>) -> Result<PathBuf, git2::Error> {
    match configured {
        Some(base) if base.as_os_str().is_empty() => default_base(root),
        Some(base) if base.is_absolute() => Ok(base.to_path_buf()),
        Some(base) => Ok(root.join(base)),
        None => default_base(root),
    }
}

pub fn path_for_branch(
    root: &Path,
    branch: &str,
    base: Option<&Path>,
) -> Result<PathBuf, git2::Error> {
    let relative = branch_relative_path(branch)
        .ok_or_else(|| git2::Error::from_str("branch name cannot be used as a worktree path"))?;
    Ok(resolve_base(root, base)?.join(relative))
}

pub fn available_sources(
    root: &Path,
    base: Option<&Path>,
) -> Result<Vec<WorktreeSource>, git2::Error> {
    let repo = git2::Repository::open(root)?;
    available_sources_repo(&repo, base)
}

pub fn available_sources_repo(
    repo: &git2::Repository,
    base: Option<&Path>,
) -> Result<Vec<WorktreeSource>, git2::Error> {
    let root = resolve_root_repo(repo)?;
    let root_repo = git2::Repository::open(&root)?;
    let checked_out = checked_out_branches(&root_repo)?;
    let local_names = branch_names(&root_repo, git2::BranchType::Local)?;
    let mut sources = Vec::new();

    for name in sorted_names(&local_names) {
        if checked_out.contains(&name) {
            continue;
        }
        if let Some(source) =
            source_if_creatable(&root, &name, WorktreeSourceKind::Local, &name, base)?
        {
            sources.push(source);
        }
    }

    for (local_branch, remote_name) in remote_sources(&root_repo, &local_names, &checked_out)? {
        if let Some(source) = source_if_creatable(
            &root,
            &remote_name,
            WorktreeSourceKind::Remote,
            &local_branch,
            base,
        )? {
            sources.push(source);
        }
    }

    Ok(sources)
}

/// Everything the create-worktree modal needs in one off-thread pass: the
/// checkout sources, the lowercased names already taken (so the modal can offer
/// an on-the-fly "Create branch" row), and the base a fly-created branch starts
/// from (worktrees.md §6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CreateOptions {
    pub sources: Vec<WorktreeSource>,
    pub taken: HashSet<String>,
    pub base: String,
}

pub fn create_options(root: &Path, base: Option<&Path>) -> Result<CreateOptions, git2::Error> {
    let root = resolve_root(root)?;
    let repo = git2::Repository::open(&root)?;
    Ok(CreateOptions {
        sources: available_sources_repo(&repo, base)?,
        taken: taken_branch_names(&repo)?,
        base: head_label(&repo)?,
    })
}

/// `name` overrides the destination folder (default: the local branch name).
/// Slashes nest folders like branch names do; the same validation applies
/// (relative, no `..`/`.`/empty segments).
pub fn create(
    root: &Path,
    source_name: &str,
    name: Option<&str>,
    base: Option<&Path>,
) -> Result<CreatedWorktree, CreateError> {
    let root = resolve_root(root)?;
    let mut source = available_sources(&root, base)?
        .into_iter()
        .find(|source| source.name == source_name)
        .ok_or_else(|| {
            CreateError::Unavailable(format!(
                "Branch “{source_name}” is not available for a worktree"
            ))
        })?;
    if let Some(name) = name {
        source.path = path_for_branch(&root, name, base).map_err(|_| {
            CreateError::Unavailable(format!("“{name}” cannot be used as a worktree folder"))
        })?;
    }

    if source.path.exists() {
        return Err(CreateError::Unavailable(format!(
            "Destination already exists: {}",
            source.path.display()
        )));
    }
    if let Some(parent) = source.path.parent() {
        fs::create_dir_all(parent)?;
    }

    let repo = git2::Repository::open(&root)?;
    let mut created_local = false;
    // Prior tip of a stale local homonym deleted to refresh from the remote;
    // restored if the worktree add fails so a failed create leaves no trace.
    let mut restored_local: Option<git2::Oid> = None;
    let reference = match source.kind {
        WorktreeSourceKind::Local => repo
            .find_branch(&source.local_branch, git2::BranchType::Local)?
            .into_reference(),
        WorktreeSourceKind::Remote => {
            let remote = repo.find_branch(&source.name, git2::BranchType::Remote)?;
            let target = remote.get().peel_to_commit()?;
            // A stale local homonym (validated as safe to refresh by
            // available_sources: not checked out, no unpushed commits) is
            // dropped so the branch can be recreated clean on the remote tip.
            if let Ok(mut existing) =
                repo.find_branch(&source.local_branch, git2::BranchType::Local)
            {
                restored_local = Some(existing.get().peel_to_commit()?.id());
                existing.delete()?;
            }
            let mut branch = repo.branch(&source.local_branch, &target, false)?;
            if let Err(err) = branch.set_upstream(Some(&source.name)) {
                let _ = branch.delete();
                restore_local(&repo, &source.local_branch, restored_local);
                return Err(CreateError::Git(err));
            }
            created_local = true;
            branch.into_reference()
        }
    };

    let mut opts = git2::WorktreeAddOptions::new();
    opts.reference(Some(&reference));
    let worktree_name = unique_worktree_name(&repo, name.unwrap_or(&source.local_branch));
    match repo.worktree(&worktree_name, &source.path, Some(&opts)) {
        Ok(_) => {
            normalize_commondir(&repo, &worktree_name)?;
            Ok(CreatedWorktree {
                path: canonical(source.path.clone()),
                source,
            })
        }
        Err(err) => {
            if created_local {
                let _ = repo
                    .find_branch(&source.local_branch, git2::BranchType::Local)
                    .and_then(|mut branch| branch.delete());
            }
            restore_local(&repo, &source.local_branch, restored_local);
            if source.path.exists() {
                let _ = fs::remove_dir_all(&source.path);
            }
            Err(CreateError::Git(err))
        }
    }
}

/// Creates a worktree on a branch made on the fly at the **root worktree's HEAD**
/// commit, with **no upstream** (worktrees.md §6). The name is revalidated here —
/// a valid branch path that still collides with no existing branch (see
/// `branch_name_is_taken`) — and the branch is deleted if the worktree add then
/// fails, so a failed create leaves no trace. The returned source carries the base
/// label as its `name` (⇒ `HELM_SOURCE_BRANCH` = the base, not the new branch).
pub fn create_branch(
    root: &Path,
    new_branch: &str,
    name: Option<&str>,
    base: Option<&Path>,
) -> Result<CreatedWorktree, CreateError> {
    let root = resolve_root(root)?;
    let repo = git2::Repository::open(&root)?;
    if branch_relative_path(new_branch).is_none() {
        return Err(CreateError::Unavailable(format!(
            "“{new_branch}” is not a valid branch name"
        )));
    }
    if branch_name_is_taken(&repo, new_branch)? {
        return Err(CreateError::Unavailable(format!(
            "Branch “{new_branch}” already exists"
        )));
    }

    let folder = name.unwrap_or(new_branch);
    let path = path_for_branch(&root, folder, base).map_err(|_| {
        CreateError::Unavailable(format!("“{folder}” cannot be used as a worktree folder"))
    })?;
    if path.exists() {
        return Err(CreateError::Unavailable(format!(
            "Destination already exists: {}",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let base_label = head_label(&repo)?;
    let commit = repo.head()?.peel_to_commit()?;
    let branch = repo.branch(new_branch, &commit, false)?;
    let reference = branch.into_reference();
    let mut opts = git2::WorktreeAddOptions::new();
    opts.reference(Some(&reference));
    let worktree_name = unique_worktree_name(&repo, folder);
    let source = WorktreeSource {
        name: base_label,
        kind: WorktreeSourceKind::Local,
        local_branch: new_branch.to_owned(),
        path: path.clone(),
    };
    match repo.worktree(&worktree_name, &path, Some(&opts)) {
        Ok(_) => {
            normalize_commondir(&repo, &worktree_name)?;
            Ok(CreatedWorktree {
                path: canonical(path),
                source,
            })
        }
        Err(err) => {
            let _ = repo
                .find_branch(new_branch, git2::BranchType::Local)
                .and_then(|mut branch| branch.delete());
            if path.exists() {
                let _ = fs::remove_dir_all(&path);
            }
            Err(CreateError::Git(err))
        }
    }
}

/// Base a fly-created branch starts from (worktrees.md §6): the root worktree's
/// HEAD branch shorthand, or its short commit id when detached. Surfaced in the
/// modal's "Create branch … from <base>" row.
pub fn base_label(root: &Path) -> Result<String, git2::Error> {
    let root = resolve_root(root)?;
    head_label(&git2::Repository::open(&root)?)
}

/// Whether `name` is eligible to be created on the fly as a new-branch worktree
/// (worktrees.md §6): a valid branch path that collides with no existing branch.
/// Shares its collision rule with `create_branch`'s revalidation so the modal row
/// never offers a name the create would refuse.
pub fn can_create_branch(root: &Path, name: &str) -> Result<bool, git2::Error> {
    if branch_relative_path(name).is_none() {
        return Ok(false);
    }
    Ok(!branch_name_is_taken(&git2::Repository::open(root)?, name)?)
}

fn head_label(repo: &git2::Repository) -> Result<String, git2::Error> {
    let head = repo.head()?;
    if head.is_branch() {
        if let Ok(name) = head.shorthand() {
            return Ok(name.to_owned());
        }
    }
    let short = head.peel_to_commit()?.as_object().short_id()?;
    Ok(short.as_str().unwrap_or_default().to_owned())
}

fn branch_name_is_taken(repo: &git2::Repository, name: &str) -> Result<bool, git2::Error> {
    Ok(taken_branch_names(repo)?.contains(&name.to_lowercase()))
}

/// Lowercased names every existing branch already occupies: each local branch
/// plus the local name of every remote-tracking branch (`origin/feat` ⇒ `feat`;
/// `origin/HEAD` ignored). Case folded because APFS loose refs collide on case;
/// the modal gates its on-the-fly "Create branch" row against this set.
fn taken_branch_names(repo: &git2::Repository) -> Result<HashSet<String>, git2::Error> {
    let mut taken = HashSet::new();
    for branch in repo.branches(Some(git2::BranchType::Local))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            taken.insert(name.to_lowercase());
        }
    }
    for branch in repo.branches(Some(git2::BranchType::Remote))? {
        let (branch, _) = branch?;
        if let Some(local) = branch
            .name()?
            .and_then(|n| n.split_once('/').map(|(_, l)| l))
        {
            if local != "HEAD" {
                taken.insert(local.to_lowercase());
            }
        }
    }
    Ok(taken)
}

/// libgit2's `worktree()` writes an absolute path into the worktree's `commondir`
/// metadata, whereas `git worktree add` writes the relative `../..`. Tools that
/// resolve the common dir with `join(gitdir, commondir)` (git-repo-info, used by
/// Rush) corrupt the path on the absolute form. The worktree gitdir is always
/// `<commondir>/worktrees/<name>`, so `../..` is the invariant relative form.
fn normalize_commondir(repo: &git2::Repository, worktree_name: &str) -> std::io::Result<()> {
    let file = repo
        .commondir()
        .join("worktrees")
        .join(worktree_name)
        .join("commondir");
    fs::write(file, "../..\n")
}

/// Best-effort recreate of a local branch at a captured tip after a failed
/// refresh: the create error is the one surfaced to the user.
fn restore_local(repo: &git2::Repository, name: &str, oid: Option<git2::Oid>) {
    if let Some(oid) = oid {
        if let Ok(commit) = repo.find_commit(oid) {
            let _ = repo.branch(name, &commit, true);
        }
    }
}

fn source_if_creatable(
    root: &Path,
    name: &str,
    kind: WorktreeSourceKind,
    local_branch: &str,
    base: Option<&Path>,
) -> Result<Option<WorktreeSource>, git2::Error> {
    let path = path_for_branch(root, local_branch, base)?;
    Ok((!path.exists()).then(|| WorktreeSource {
        name: name.to_owned(),
        kind,
        local_branch: local_branch.to_owned(),
        path,
    }))
}

fn checked_out_branches(repo: &git2::Repository) -> Result<HashSet<String>, git2::Error> {
    let mut checked_out = HashSet::new();
    if !repo.is_bare() {
        checked_out.extend(current_local_branch(repo));
    }
    for worktree in list_repo(repo)?
        .worktrees
        .into_iter()
        .filter(|w| !w.prunable)
    {
        if let Ok(repo) = git2::Repository::open(&worktree.path) {
            checked_out.extend(current_local_branch(&repo));
        }
    }
    Ok(checked_out)
}

fn current_local_branch(repo: &git2::Repository) -> Option<String> {
    let head = repo.head().ok()?;
    head.is_branch()
        .then(|| head.shorthand().ok().map(str::to_owned))
        .flatten()
}

fn branch_names(
    repo: &git2::Repository,
    kind: git2::BranchType,
) -> Result<HashSet<String>, git2::Error> {
    let mut names = HashSet::new();
    for branch in repo.branches(Some(kind))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            names.insert(name.to_owned());
        }
    }
    Ok(names)
}

fn sorted_names(names: &HashSet<String>) -> Vec<String> {
    let mut names: Vec<String> = names.iter().cloned().collect();
    names.sort_unstable();
    names
}

fn remote_sources(
    repo: &git2::Repository,
    local_names: &HashSet<String>,
    checked_out: &HashSet<String>,
) -> Result<Vec<(String, String)>, git2::Error> {
    let mut by_local: HashMap<String, String> = HashMap::new();
    // Configured remotes resolved once: `find_remote` re-reads the config at every
    // call — per remote **branch**, that was 1.7 s of every graph load on a repo
    // with ~2k remote branches.
    let mut configured: HashSet<String> = HashSet::new();
    for name in repo.remotes()?.iter() {
        if let Some(name) = name? {
            configured.insert(name.to_owned());
        }
    }
    let remote_names = branch_names(repo, git2::BranchType::Remote)?;
    for remote_name in sorted_names(&remote_names) {
        let Some((remote, branch)) = remote_name.split_once('/') else {
            continue;
        };
        if branch == "HEAD" || !configured.contains(remote) {
            continue;
        }
        // A local homonym normally hides the remote (creating its local branch
        // would clash). Exception: a stale leftover safe to refresh — not
        // checked out, no unpushed commits, strictly behind the remote — which
        // `create` drops and recreates on the remote tip.
        if local_names.contains(branch)
            && !local_replaceable_by_remote(repo, branch, &remote_name, checked_out)
        {
            continue;
        }
        by_local
            .entry(branch.to_owned())
            .or_insert_with(|| remote_name.to_owned());
    }
    let mut sources: Vec<(String, String)> = by_local.into_iter().collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(sources)
}

/// A stale local branch is replaceable by its remote homonym when refreshing it
/// loses no work: not checked out anywhere, no commits the remote lacks
/// (`ahead == 0`), and strictly behind it (`behind > 0`) so there is something
/// to refresh. Same-commit and ahead/diverged locals are kept as-is.
fn local_replaceable_by_remote(
    repo: &git2::Repository,
    local_branch: &str,
    remote_name: &str,
    checked_out: &HashSet<String>,
) -> bool {
    if checked_out.contains(local_branch) {
        return false;
    }
    match ahead_behind(repo, local_branch, remote_name) {
        Ok((ahead, behind)) => ahead == 0 && behind > 0,
        Err(_) => false,
    }
}

fn ahead_behind(
    repo: &git2::Repository,
    local_branch: &str,
    remote_name: &str,
) -> Result<(usize, usize), git2::Error> {
    let local = repo
        .find_branch(local_branch, git2::BranchType::Local)?
        .get()
        .peel_to_commit()?
        .id();
    let remote = repo
        .find_branch(remote_name, git2::BranchType::Remote)?
        .get()
        .peel_to_commit()?
        .id();
    repo.graph_ahead_behind(local, remote)
}

fn branch_relative_path(branch: &str) -> Option<PathBuf> {
    let path = Path::new(branch);
    if path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => out.push(part),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn unique_worktree_name(repo: &git2::Repository, branch: &str) -> String {
    let base = branch
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let base = if base.is_empty() {
        "worktree".to_owned()
    } else {
        base
    };
    if repo.find_worktree(&base).is_err() {
        return base;
    }
    for suffix in 2.. {
        let candidate = format!("{base}-{suffix}");
        if repo.find_worktree(&candidate).is_err() {
            return candidate;
        }
    }
    unreachable!("unbounded suffix search returns before overflowing")
}

pub fn delete(root: &Path, name: &str, force: bool) -> Result<(), DeleteError> {
    let repo = git2::Repository::open(root)?;
    let wt = repo.find_worktree(name)?;
    if let git2::WorktreeLockStatus::Locked(reason) = wt.is_locked()? {
        return Err(DeleteError::Locked(reason));
    }
    if !force {
        let wt_repo = git2::Repository::open(wt.path())?;
        let dirty = status::load_repo(&wt_repo)?.changed_file_count();
        if dirty > 0 {
            return Err(DeleteError::Dirty(dirty));
        }
    }
    // working_tree(true): libgit2 also removes the directory — a single path for
    // directory + metadata; valid(true) is required because the worktree is still valid.
    let mut opts = git2::WorktreePruneOptions::new();
    opts.valid(true).working_tree(true);
    wt.prune(Some(&mut opts))?;
    Ok(())
}

/// Path-based variant of `delete`: the worktree's libgit2 name may differ from the
/// directory name — recovered by enumerating from the root.
pub fn delete_by_path(root: &Path, target: &Path, force: bool) -> Result<(), DeleteError> {
    let target = canonical(target.to_path_buf());
    let listing = list(root)?;
    let Some(wt) = listing.worktrees.into_iter().find(|w| w.path == target) else {
        return Err(DeleteError::Git(git2::Error::from_str(
            "Worktree not found in its repository",
        )));
    };
    delete(root, &wt.name, force)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteRequest {
    pub root: PathBuf,
    /// Worktree path (key of its sidebar row); the libgit2 name is resolved in the
    /// thread (`delete_by_path`).
    pub path: PathBuf,
    /// Name displayed in the dirty / refusal modal.
    pub label: String,
    pub force: bool,
}

#[derive(Debug)]
pub struct DeleteReply {
    pub request: DeleteRequest,
    pub result: Result<(), DeleteError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRequest {
    pub root: PathBuf,
    /// Per-project worktree base (worktrees.md §6); `None` ⇒ `<root>.worktrees`.
    pub base: Option<PathBuf>,
}

#[derive(Debug)]
pub struct SourceReply {
    pub request: SourceRequest,
    pub result: Result<CreateOptions, git2::Error>,
}

/// What a create request checks out: an existing branch picked from the list, or
/// a branch created on the fly at the root's HEAD (worktrees.md §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateSource {
    Existing(String),
    NewBranch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    pub root: PathBuf,
    pub source: CreateSource,
    /// Custom destination folder; `None` = the local branch name.
    pub name: Option<String>,
    /// Per-project worktree base (worktrees.md §6); `None` ⇒ `<root>.worktrees`.
    pub base: Option<PathBuf>,
}

#[derive(Debug)]
pub struct CreateReply {
    pub request: CreateRequest,
    pub result: Result<CreatedWorktree, CreateError>,
}

/// Runs `available_sources` on a dedicated thread: branch enumeration can be slow
/// in large repos and opening the modal must stay instant.
pub struct SourceRunner {
    on_event: Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<SourceReply>,
    results_rx: Receiver<SourceReply>,
    in_flight: Option<SourceRequest>,
}

impl SourceRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn request(&mut self, request: SourceRequest) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(request.clone());
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = create_options(&request.root, request.base.as_deref());
            let _ = tx.send(SourceReply { request, result });
            on_event();
        });
        true
    }

    pub fn try_recv(&mut self) -> Option<SourceReply> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }

    pub fn recv(&mut self) -> Option<SourceReply> {
        let reply = self.results_rx.recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }
}

/// Runs `create` on a dedicated thread. The operation is one-shot and revalidates
/// the source at execution time, so a stale modal/menu cannot create an invalid
/// worktree if Git changed behind it.
pub struct CreateRunner {
    on_event: Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<CreateReply>,
    results_rx: Receiver<CreateReply>,
    in_flight: Option<CreateRequest>,
}

impl CreateRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn in_flight(&self) -> Option<&CreateRequest> {
        self.in_flight.as_ref()
    }

    pub fn request(&mut self, request: CreateRequest) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(request.clone());
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = match &request.source {
                CreateSource::Existing(name) => create(
                    &request.root,
                    name,
                    request.name.as_deref(),
                    request.base.as_deref(),
                ),
                CreateSource::NewBranch(branch) => create_branch(
                    &request.root,
                    branch,
                    request.name.as_deref(),
                    request.base.as_deref(),
                ),
            };
            let _ = tx.send(CreateReply { request, result });
            on_event();
        });
        true
    }

    pub fn try_recv(&mut self) -> Option<CreateReply> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }

    pub fn recv(&mut self) -> Option<CreateReply> {
        let reply = self.results_rx.recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }
}

/// Runs `delete_by_path` on a **dedicated thread per op**: pruning the directory
/// can take many seconds on a large worktree (target/, node_modules/…) and used to
/// freeze the UI thread. Same contract as `SyncRunner` (worker.rs): one op at a
/// time, `request` is ignored until the previous one has been drained.
pub struct DeleteRunner {
    on_event: Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<DeleteReply>,
    results_rx: Receiver<DeleteReply>,
    in_flight: Option<DeleteRequest>,
}

impl DeleteRunner {
    pub fn new(on_event: impl Fn() + Send + Sync + 'static) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            on_event: Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Worktree currently being deleted, if any: its sidebar row is greyed out +
    /// spinner, inert, during the op.
    pub fn in_flight(&self) -> Option<&DeleteRequest> {
        self.in_flight.as_ref()
    }

    /// Starts the deletion; returns `false` (request ignored) if an op is running.
    pub fn request(&mut self, request: DeleteRequest) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(request.clone());
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        std::thread::spawn(move || {
            let result = delete_by_path(&request.root, &request.path, request.force);
            let _ = tx.send(DeleteReply { request, result });
            on_event();
        });
        true
    }

    pub fn try_recv(&mut self) -> Option<DeleteReply> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }

    pub fn recv(&mut self) -> Option<DeleteReply> {
        let reply = self.results_rx.recv().ok();
        if reply.is_some() {
            self.in_flight = None;
        }
        reply
    }
}

fn root_dir(repo: &git2::Repository) -> PathBuf {
    match repo.workdir() {
        Some(dir) => dir.to_path_buf(),
        None => repo.path().to_path_buf(),
    }
}

// A prunable worktree's path may no longer exist on disk: fall back to the raw path.
fn canonical(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_worktree_has_relative_commondir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = git2::Repository::init(&root).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.email", "t@t.t").unwrap();
            cfg.set_str("user.name", "t").unwrap();
        }
        let sig = repo.signature().unwrap();
        let tree = {
            let mut index = repo.index().unwrap();
            let oid = index.write_tree().unwrap();
            repo.find_tree(oid).unwrap()
        };
        let head = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        let commit = repo.find_commit(head).unwrap();
        repo.branch("feat", &commit, false).unwrap();

        create(&root, "feat", None, None).unwrap();

        let commondir = root.join(".git/worktrees/feat/commondir");
        assert_eq!(
            fs::read_to_string(&commondir).unwrap(),
            "../..\n",
            "commondir must be relative like `git worktree add`, not the absolute path libgit2 writes"
        );

        // git-repo-info-style resolution: join(gitdir, commondir) must land on the
        // main common dir, which is what the absolute form corrupted.
        let gitdir = root.join(".git/worktrees/feat");
        let resolved = fs::canonicalize(gitdir.join("../..")).unwrap();
        assert_eq!(resolved, fs::canonicalize(root.join(".git")).unwrap());
    }

    fn repo_with_commit(dir: &Path) -> (git2::Repository, git2::Oid) {
        let repo = git2::Repository::init(dir).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.email", "t@t.t").unwrap();
            cfg.set_str("user.name", "t").unwrap();
        }
        let head = {
            let sig = repo.signature().unwrap();
            let tree = {
                let mut index = repo.index().unwrap();
                let oid = index.write_tree().unwrap();
                repo.find_tree(oid).unwrap()
            };
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap()
        };
        (repo, head)
    }

    #[test]
    fn base_label_is_the_head_branch_then_the_short_id_when_detached() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("main");
        let (repo, head) = repo_with_commit(&dir);

        let branch = repo.head().unwrap().shorthand().unwrap().to_owned();
        assert_eq!(base_label(&dir).unwrap(), branch);

        repo.set_head_detached(head).unwrap();
        let short = repo
            .find_object(head, None)
            .unwrap()
            .short_id()
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            base_label(&dir).unwrap(),
            short,
            "a detached HEAD is labelled by its short commit id"
        );
    }

    #[test]
    fn can_create_branch_rejects_invalid_paths_and_case_insensitive_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("main");
        let (repo, head) = repo_with_commit(&dir);
        let commit = repo.find_commit(head).unwrap();
        repo.branch("Feat", &commit, false).unwrap();
        repo.reference("refs/remotes/origin/bar", head, true, "r")
            .unwrap();
        repo.reference("refs/remotes/origin/HEAD", head, true, "r")
            .unwrap();

        for invalid in ["", "../x", "/abs", ".", "a/../b"] {
            assert!(
                !can_create_branch(&dir, invalid).unwrap(),
                "“{invalid}” is not a valid branch path"
            );
        }
        for taken in ["Feat", "feat", "bar", "BAR"] {
            assert!(
                !can_create_branch(&dir, taken).unwrap(),
                "“{taken}” collides case-insensitively with an existing branch"
            );
        }
        assert!(can_create_branch(&dir, "fresh/name").unwrap());
    }

    #[test]
    fn plain_repo_resolves_to_itself() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();

        let expected = fs::canonicalize(tmp.path()).unwrap();
        assert_eq!(resolve_root(tmp.path()).unwrap(), expected);
    }

    #[test]
    fn non_git_dir_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve_root(tmp.path()).is_err());
        assert!(list(tmp.path()).is_err());
    }

    #[test]
    fn repo_without_worktrees_lists_empty_non_bare() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();

        let listing = list(tmp.path()).unwrap();

        assert!(!listing.bare);
        assert!(listing.worktrees.is_empty());
    }

    #[test]
    fn resolve_base_honors_override_and_falls_back_to_default() {
        let root = Path::new("/Users/dev/helm");
        let default = default_base(root).unwrap();
        assert_eq!(default, Path::new("/Users/dev/helm.worktrees"));

        assert_eq!(resolve_base(root, None).unwrap(), default);
        assert_eq!(
            resolve_base(root, Some(Path::new(""))).unwrap(),
            default,
            "an empty override falls back to the default base"
        );
        assert_eq!(
            resolve_base(root, Some(Path::new("/wt/helm"))).unwrap(),
            Path::new("/wt/helm"),
            "an absolute override is used verbatim"
        );
        assert_eq!(
            resolve_base(root, Some(Path::new("../trees"))).unwrap(),
            Path::new("/Users/dev/helm/../trees"),
            "a relative override is resolved against the root"
        );
        assert_eq!(
            path_for_branch(root, "feat/x", Some(Path::new("/wt"))).unwrap(),
            Path::new("/wt/feat/x"),
            "the branch path nests under the configured base"
        );
    }

    #[test]
    fn canonical_falls_back_to_raw_missing_path() {
        let missing = PathBuf::from("/nonexistent/helm-worktree");
        assert_eq!(canonical(missing.clone()), missing);
    }

    #[test]
    fn delete_by_path_unknown_worktree_is_a_git_error() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();

        let err = delete_by_path(tmp.path(), &tmp.path().join("nope"), false).unwrap_err();

        assert!(
            matches!(&err, DeleteError::Git(e) if e.message().contains("not found")),
            "expected a not-found Git error, got {err:?}"
        );
    }

    #[test]
    fn runner_ignores_requests_while_busy_and_busy_clears_on_drain() {
        let tmp = tempfile::tempdir().unwrap();
        let mut runner = DeleteRunner::new(|| {});
        assert!(!runner.busy());

        let request = DeleteRequest {
            root: tmp.path().to_path_buf(),
            path: tmp.path().join("wt"),
            label: "wt".to_owned(),
            force: false,
        };
        assert!(runner.request(request.clone()));
        assert!(runner.busy());
        assert_eq!(runner.in_flight(), Some(&request));
        assert!(!runner.request(request.clone()));

        let reply = runner.recv().unwrap();
        assert!(
            reply.result.is_err(),
            "a non-git root cannot delete anything"
        );
        assert!(!runner.busy());

        assert!(runner.request(request));
    }
}
