//! AI rebase (git.md §9): the configured agentic AI CLI rebases the current
//! branch onto a target **itself** — running git commands, resolving conflicts,
//! honoring the user's extra instructions (e.g. squash into one commit) — then
//! reports what it did. It never pushes: pushes are denied at the CLI level
//! where the provider supports it, and by the prompt contract everywhere.
//! Unlike [`crate::ai`] (plain `-p` text reply), the invocation here grants the
//! provider command execution.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use crate::ai::{self, AiProvider};
use crate::git::cli::{self, CliError};
use crate::git::rebase::{self, RebaseCommit};
use crate::git::sync;
use crate::git::worker::MutationLock;

/// An agentic run replays commits one by one and may iterate on conflicts:
/// minutes are normal where the commit-message generation takes seconds.
pub const AI_REBASE_TIMEOUT: Duration = Duration::from_secs(1800);

/// What the Start button hands to the runner: the recap the user approved.
/// `expected` (oldest first) is re-derived and compared before running — same
/// stale-plan rule as the interactive rebase (the branch moved since the modal
/// opened ⇒ the user approved a recap that no longer exists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiRebaseRequest {
    pub current: String,
    pub onto: String,
    pub instructions: String,
    pub expected: Vec<git2::Oid>,
}

/// The provider's account plus the outcome **verified on the repo** — never
/// trusted from the provider's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiRebaseReport {
    pub summary: String,
    pub outcome: AiRebaseOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiRebaseOutcome {
    /// Repo clean and HEAD moved: the branch was rewritten.
    Completed,
    /// Repo clean and HEAD where it was: nothing changed (the provider found
    /// nothing to do, or aborted and said why in its report).
    Unchanged,
    /// A rebase/merge is still in progress: lasting state told by the sidebar
    /// banner — resolution in the terminal or Abort, like a Pull conflict.
    LeftInProgress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiRebaseError {
    /// Provider binary missing from PATH.
    NotFound(String),
    /// Refused up front (guards), errored process or timeout.
    Failed(String),
}

impl AiRebaseError {
    /// Toast message (git.md §10): the action spelled out + the useful detail.
    pub fn message(&self) -> String {
        match self {
            AiRebaseError::NotFound(command) => {
                format!("'{command}' not found — pick another AI rebase provider in Preferences")
            }
            AiRebaseError::Failed(detail) => format!("AI rebase failed — {detail}"),
        }
    }
}

/// Agentic invocation per provider. Claude: print mode with Bash and the file
/// tools pre-approved, `git push` denied (deny rules win over allows). Codex:
/// `exec --full-auto` (workspace-write sandbox, network blocked — a push fails
/// on its own). OpenCode: `run`. The no-push prompt contract applies to all.
pub fn invocation(provider: AiProvider, prompt: &str) -> (PathBuf, Vec<String>) {
    let string = |s: &str| s.to_string();
    let args = match provider {
        AiProvider::Claude => vec![
            string("-p"),
            string(prompt),
            string("--allowedTools"),
            string("Bash"),
            string("Read"),
            string("Edit"),
            string("Write"),
            string("Grep"),
            string("Glob"),
            string("--disallowedTools"),
            string("Bash(git push *)"),
            string("Bash(git push:*)"),
        ],
        AiProvider::Codex => vec![string("exec"), string("--full-auto"), string(prompt)],
        AiProvider::Opencode => vec![string("run"), string(prompt)],
    };
    (PathBuf::from(provider.command()), args)
}

pub fn run(
    workdir: &Path,
    provider: AiProvider,
    request: &AiRebaseRequest,
    cancel: &AtomicBool,
) -> Result<AiRebaseReport, AiRebaseError> {
    let (prompt, head_before) = prepare(workdir, request)?;
    let (program, mut args) = invocation(provider, &prompt);
    // codex floods stdout with its session log (banner, commands): the final
    // message lands in a scratch file instead — stdout stays the fallback.
    let report_file = (provider == AiProvider::Codex).then(report_file_path);
    if let Some(path) = &report_file {
        insert_report_flag(&mut args, path);
    }
    execute(
        &program,
        &args,
        workdir,
        head_before,
        cancel,
        report_file.as_deref(),
    )
}

/// Seam: same guards and pipeline with an explicit program invoked as
/// `<program> -p <prompt>` (fake binary in tests — no permission flags needed).
pub fn run_with(
    program: &Path,
    workdir: &Path,
    request: &AiRebaseRequest,
) -> Result<AiRebaseReport, AiRebaseError> {
    run_with_cancel(program, workdir, request, &AtomicBool::new(false))
}

/// [`run_with`] under the runner's cancellation flag.
pub fn run_with_cancel(
    program: &Path,
    workdir: &Path,
    request: &AiRebaseRequest,
    cancel: &AtomicBool,
) -> Result<AiRebaseReport, AiRebaseError> {
    let (prompt, head_before) = prepare(workdir, request)?;
    let args = ["-p".to_string(), prompt];
    execute(program, &args, workdir, head_before, cancel, None)
}

/// Inserts codex's report-file option **before** the prompt positional — a
/// trailing option after a positional is at the CLI parser's mercy.
fn insert_report_flag(args: &mut Vec<String>, path: &Path) {
    let at = args.len() - 1;
    args.insert(at, "--output-last-message".to_string());
    args.insert(at + 1, path.display().to_string());
}

/// Unique scratch path for codex's `--output-last-message`: one run at a time
/// per app, but a cancelled run's provider may overlap the next request.
fn report_file_path() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "helm-ai-rebase-report-{}-{}.txt",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Guards (mirroring `sync::interactive_rebase`, checked again where they
/// cannot be raced) then the prompt. The dirty-tree refusal is **stricter** than
/// the plain rebase on purpose: `git rebase` would refuse anyway, and the
/// provider must never be tempted to stash or commit the user's WIP.
fn prepare(
    workdir: &Path,
    request: &AiRebaseRequest,
) -> Result<(String, git2::Oid), AiRebaseError> {
    let failed = |detail: &str| AiRebaseError::Failed(detail.to_string());
    if request.onto.starts_with('-') {
        return Err(AiRebaseError::Failed(format!(
            "invalid ref name '{}'",
            request.onto
        )));
    }
    let repo = git2::Repository::open(workdir)
        .map_err(|err| AiRebaseError::Failed(err.message().into()))?;
    if repo.state() != git2::RepositoryState::Clean {
        return Err(failed(
            "a merge or rebase is already in progress — resolve or abort it first",
        ));
    }
    let head = repo
        .head()
        .map_err(|err| AiRebaseError::Failed(err.message().into()))?;
    if !head.is_branch() {
        return Err(failed("HEAD is detached"));
    }
    if head.shorthand().ok() != Some(request.current.as_str()) {
        return Err(failed(
            "the checked-out branch changed since the recap was prepared — reopen AI rebase",
        ));
    }
    let head_before = head
        .target()
        .ok_or_else(|| failed("HEAD does not point at a commit"))?;
    if has_tracked_changes(&repo).map_err(|err| AiRebaseError::Failed(err.message().into()))? {
        return Err(failed(
            "the working tree has uncommitted changes — commit or stash them first",
        ));
    }
    let commits = rebase::rebase_commits(&repo, &request.onto)
        .map_err(|err| AiRebaseError::Failed(err.message().into()))?;
    if commits.is_empty() {
        return Err(AiRebaseError::Failed(format!(
            "nothing to rebase — {} is already contained in {}",
            request.current, request.onto
        )));
    }
    if !commits
        .iter()
        .map(|c| c.oid)
        .eq(request.expected.iter().copied())
    {
        return Err(failed(
            "the branch changed since the recap was prepared — reopen AI rebase",
        ));
    }
    Ok((
        build_prompt(
            &request.current,
            &request.onto,
            &request.instructions,
            &commits,
        ),
        head_before,
    ))
}

/// Index or worktree changes to **tracked** files; untracked files do not block
/// a rebase and stay out of the provider's way.
fn has_tracked_changes(repo: &git2::Repository) -> Result<bool, git2::Error> {
    let mut options = git2::StatusOptions::new();
    options.include_untracked(false);
    Ok(!repo.statuses(Some(&mut options))?.is_empty())
}

pub fn build_prompt(
    current: &str,
    onto: &str,
    instructions: &str,
    commits: &[RebaseCommit],
) -> String {
    use std::fmt::Write;
    let mut prompt = format!(
        "You are working inside a git repository, checked out on branch '{current}'.\n\
         Task: rebase '{current}' onto '{onto}' by running git commands yourself.\n\n\
         Hard rules:\n\
         - NEVER push and NEVER touch a remote (no git push, fetch or pull).\n\
         - Resolve any conflict yourself, preserving the intent of both sides.\n\
         - If the rebase cannot be completed safely, run `git rebase --abort` and explain why.\n\
         - End checked out on '{current}', with no rebase left in progress.\n\
         - The working tree is clean; never create commits unrelated to the rebase."
    );
    let instructions = instructions.trim();
    if !instructions.is_empty() {
        prompt.push_str("\n\nAdditional instructions from the user:\n");
        prompt.push_str(instructions);
    }
    let _ = write!(
        prompt,
        "\n\nCommits to replay, oldest first ({}):",
        commits.len()
    );
    for commit in commits {
        let _ = write!(prompt, "\n{} {}", commit.short_id, commit.summary);
    }
    prompt.push_str(
        "\n\nWhen finished, reply with a concise plain-text report (no markdown): \
         what you did, each conflict you hit and how you resolved it, and anything \
         the user should verify.",
    );
    prompt
}

fn execute(
    program: &Path,
    args: &[String],
    workdir: &Path,
    head_before: git2::Oid,
    cancel: &AtomicBool,
    report_file: Option<&Path>,
) -> Result<AiRebaseReport, AiRebaseError> {
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let run = cli::run_program_cancellable(program, workdir, &argv, AI_REBASE_TIMEOUT, &[], cancel);
    let file_report = report_file.and_then(take_report_file);
    let out = match run {
        Ok(Some(out)) => out,
        Ok(None) => return Ok(cancelled_report(workdir, head_before)),
        Err(CliError::NotFound) => {
            return Err(AiRebaseError::NotFound(program.display().to_string()))
        }
        // A timed-out provider was killed mid-work like a cancel: same restore,
        // the failure toast tells what was put back.
        Err(CliError::TimedOut(duration)) => {
            return Err(AiRebaseError::Failed(timed_out_detail(
                workdir,
                head_before,
                duration,
            )))
        }
        Err(CliError::Io(err)) => return Err(AiRebaseError::Failed(err.to_string())),
    };
    if !out.success() {
        // The provider stopped **on its own** (quota, crash, deliberate abort):
        // the repo stays as it left it — the banner (status refresh) tells the
        // lasting state, the toast says why it stopped. Only helm's own kills
        // (cancel, timeout) restore.
        return Err(AiRebaseError::Failed(ai::failure_detail(&out)));
    }
    Ok(AiRebaseReport {
        summary: report_summary(file_report.as_deref(), &out.stdout),
        outcome: classify(workdir, head_before),
    })
}

/// Reads then removes codex's last-message file; `None` when the provider died
/// before writing it (report falls back to stdout).
fn take_report_file(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok();
    let _ = std::fs::remove_file(path);
    content
}

/// After a kill, restore what can be — Cancel means "give me my branch back" —
/// then report the **verified** state: the provider may already have finished,
/// and a rewrite must never pass silently.
fn cancelled_report(workdir: &Path, head_before: git2::Oid) -> AiRebaseReport {
    let aborted = abort_in_progress(workdir);
    let outcome = classify(workdir, head_before);
    let summary = match outcome {
        AiRebaseOutcome::LeftInProgress => {
            "Cancelled — the rebase in progress could not be aborted; resolve it \
             in the terminal or Abort from the sidebar banner."
        }
        AiRebaseOutcome::Unchanged if aborted => {
            "Cancelled — the rebase in progress was aborted and the branch restored."
        }
        AiRebaseOutcome::Unchanged => {
            "Cancelled — the rebase had not started; the branch is unchanged."
        }
        AiRebaseOutcome::Completed => {
            "Cancelled — the provider had already rewritten the branch; the rebase result is kept."
        }
    };
    AiRebaseReport {
        summary: summary.to_string(),
        outcome,
    }
}

/// Toast detail for a timed-out run, after the same restore as a cancel: the
/// state told is **verified** on the repo, like every outcome.
fn timed_out_detail(workdir: &Path, head_before: git2::Oid, timeout: Duration) -> String {
    let aborted = abort_in_progress(workdir);
    let state = match classify(workdir, head_before) {
        AiRebaseOutcome::LeftInProgress => {
            "the rebase in progress could not be aborted; resolve it in the \
             terminal or Abort from the sidebar banner"
        }
        AiRebaseOutcome::Unchanged if aborted => {
            "the rebase in progress was aborted and the branch restored"
        }
        AiRebaseOutcome::Unchanged => "the branch is unchanged",
        AiRebaseOutcome::Completed => "the rebase itself had completed; the result is kept",
    };
    format!("timed out after {}s — {state}", timeout.as_secs())
}

/// `git <flavor> --abort` (sync.rs, flavor per repo state) when the kill left
/// an operation in progress; `true` when one was attempted — the verified
/// outcome right after tells whether it worked.
fn abort_in_progress(workdir: &Path) -> bool {
    let in_progress = git2::Repository::open(workdir)
        .is_ok_and(|repo| repo.state() != git2::RepositoryState::Clean);
    if in_progress {
        let _ = sync::abort_op(workdir);
    }
    in_progress
}

/// A successful run with an empty reply still reports: the outcome below is
/// verified on the repo, hiding it behind an error would lose a real rebase.
fn report_summary(file_report: Option<&str>, stdout: &str) -> String {
    let text = file_report
        .map(str::trim)
        .filter(|report| !report.is_empty())
        .unwrap_or_else(|| stdout.trim());
    if text.is_empty() {
        "The provider returned no report.".to_string()
    } else {
        text.to_string()
    }
}

fn classify(workdir: &Path, head_before: git2::Oid) -> AiRebaseOutcome {
    let Ok(repo) = git2::Repository::open(workdir) else {
        return AiRebaseOutcome::Unchanged;
    };
    if repo.state() != git2::RepositoryState::Clean {
        return AiRebaseOutcome::LeftInProgress;
    }
    let head_after = repo.head().ok().and_then(|head| head.target());
    if head_after == Some(head_before) {
        AiRebaseOutcome::Unchanged
    } else {
        AiRebaseOutcome::Completed
    }
}

/// Runs the AI rebase on a **dedicated thread per request**, holding the repo's
/// [`MutationLock`] for the whole run: staging, commits and sync ops are refused
/// while the provider rewrites history (and a busy lock refuses the request —
/// same one-op-at-a-time rule as the SyncRunner). [`AiRebaseRunner::cancel`]
/// kills the provider and restores the branch; dropping the runner mid-run
/// (repo switch, quit) does the same — an abandoned provider would keep
/// rewriting history with no lock and no report.
pub struct AiRebaseRunner {
    repo_path: PathBuf,
    on_event: Arc<dyn Fn() + Send + Sync>,
    results_tx: Sender<Result<AiRebaseReport, AiRebaseError>>,
    results_rx: Receiver<Result<AiRebaseReport, AiRebaseError>>,
    in_flight: bool,
    mutation_lock: MutationLock,
    /// Raised by [`AiRebaseRunner::cancel`] (and on drop): the provider is
    /// killed at the next wait tick, then the run restores what it can and
    /// reports. Fresh per request — a stale flag must not skip the next run.
    cancel: Arc<AtomicBool>,
    /// Wall-clock start of the in-flight run (toolbar chip timer).
    started_at: Option<Instant>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AiRebaseRunner {
    pub fn new(
        repo_path: &Path,
        mutation_lock: MutationLock,
        on_event: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (results_tx, results_rx) = crossbeam_channel::unbounded();
        Self {
            repo_path: repo_path.to_path_buf(),
            on_event: Arc::new(on_event),
            results_tx,
            results_rx,
            in_flight: false,
            mutation_lock,
            cancel: Arc::new(AtomicBool::new(false)),
            started_at: None,
            handle: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight
    }

    /// Cancel asked but the run hasn't replied yet: the toolbar's Cancel turns
    /// inert ("Cancelling…") while the kill + restore complete.
    pub fn cancelling(&self) -> bool {
        self.in_flight && self.cancel.load(Ordering::Relaxed)
    }

    /// How long the in-flight run has been going; `None` when idle.
    pub fn elapsed(&self) -> Option<Duration> {
        self.in_flight
            .then(|| self.started_at.map_or(Duration::ZERO, |at| at.elapsed()))
    }

    /// Stops the in-flight run: the provider is killed, a rebase it left in
    /// progress is aborted, and the report tells the verified result. No-op
    /// when idle.
    pub fn cancel(&self) {
        if self.in_flight {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Starts the run; `false` (request ignored) when one is in flight or
    /// another mutating git command holds the lock.
    pub fn request(&mut self, provider: AiProvider, request: AiRebaseRequest) -> bool {
        self.launch(move |path, cancel| run(path, provider, &request, cancel))
    }

    /// Seam: same execution through [`run_with_cancel`] (fake binary in tests).
    pub fn request_program(&mut self, program: PathBuf, request: AiRebaseRequest) -> bool {
        self.launch(move |path, cancel| run_with_cancel(&program, path, &request, cancel))
    }

    fn launch(
        &mut self,
        job: impl FnOnce(&Path, &AtomicBool) -> Result<AiRebaseReport, AiRebaseError> + Send + 'static,
    ) -> bool {
        if self.in_flight {
            return false;
        }
        let Some(guard) = self.mutation_lock.try_acquire() else {
            return false;
        };
        self.in_flight = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.started_at = Some(Instant::now());
        let cancel = Arc::clone(&self.cancel);
        let path = self.repo_path.clone();
        let tx = self.results_tx.clone();
        let on_event = Arc::clone(&self.on_event);
        self.handle = Some(std::thread::spawn(move || {
            // Guard released **before** the reply: when the UI drains it, the
            // lock is guaranteed free — a Stage clicked right after the report
            // must not hit a few-µs stale refusal.
            let result = {
                let _guard = guard;
                job(&path, &cancel)
            };
            let _ = tx.send(result);
            on_event();
        }));
        true
    }

    pub fn try_recv(&mut self) -> Option<Result<AiRebaseReport, AiRebaseError>> {
        let reply = self.results_rx.try_recv().ok();
        if reply.is_some() {
            self.settle();
        }
        reply
    }

    pub fn recv(&mut self) -> Option<Result<AiRebaseReport, AiRebaseError>> {
        let reply = self.results_rx.recv().ok();
        if reply.is_some() {
            self.settle();
        }
        reply
    }

    fn settle(&mut self) {
        self.in_flight = false;
        self.started_at = None;
        self.handle = None;
    }
}

/// The kill lands within one wait tick (~25 ms) and the restore is local git:
/// the join stays short — worth it, an unjoined run would race the fresh
/// [`MutationLock`] of the session reopened on the same repo.
impl Drop for AiRebaseRunner {
    fn drop(&mut self) {
        if !self.in_flight {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(short: &str, summary: &str) -> RebaseCommit {
        RebaseCommit {
            oid: git2::Oid::from_str(&"a".repeat(40)).unwrap(),
            short_id: short.to_string(),
            summary: summary.to_string(),
            message: summary.to_string(),
            author: "t".to_string(),
        }
    }

    #[test]
    fn the_prompt_carries_the_task_the_rules_and_the_commits() {
        let prompt = build_prompt(
            "feat",
            "main",
            "",
            &[
                commit("1a2b3c", "Add parser"),
                commit("4d5e6f", "Fix lexer"),
            ],
        );
        assert!(prompt.contains("rebase 'feat' onto 'main'"));
        assert!(prompt.contains("NEVER push"));
        assert!(prompt.contains("git rebase --abort"));
        assert!(prompt.contains("Commits to replay, oldest first (2):"));
        assert!(prompt.contains("1a2b3c Add parser"));
        assert!(prompt.contains("4d5e6f Fix lexer"));
        assert!(prompt.contains("how you resolved it"));
        assert!(
            !prompt.contains("Additional instructions"),
            "no instructions block when the field is blank"
        );
    }

    #[test]
    fn the_prompt_appends_the_user_instructions() {
        let prompt = build_prompt(
            "feat",
            "main",
            "  Squash everything into a single commit.  ",
            &[commit("1a2b3c", "Add parser")],
        );
        assert!(prompt.contains(
            "Additional instructions from the user:\nSquash everything into a single commit."
        ));
    }

    #[test]
    fn claude_invocation_grants_bash_and_denies_push() {
        let (program, args) = invocation(AiProvider::Claude, "PROMPT");
        assert_eq!(program, PathBuf::from("claude"));
        assert_eq!(args[..2], ["-p".to_string(), "PROMPT".to_string()]);
        let allowed_at = args.iter().position(|a| a == "--allowedTools").unwrap();
        let denied_at = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert!(allowed_at < denied_at);
        assert!(args.contains(&"Bash".to_string()));
        assert!(
            args[denied_at..]
                .iter()
                .any(|a| a.starts_with("Bash(git push")),
            "push must be denied: {args:?}"
        );
    }

    #[test]
    fn codex_and_opencode_run_their_agentic_subcommand() {
        let (program, args) = invocation(AiProvider::Codex, "PROMPT");
        assert_eq!(program, PathBuf::from("codex"));
        assert_eq!(args, ["exec", "--full-auto", "PROMPT"]);

        let (program, args) = invocation(AiProvider::Opencode, "PROMPT");
        assert_eq!(program, PathBuf::from("opencode"));
        assert_eq!(args, ["run", "PROMPT"]);
    }

    #[test]
    fn the_codex_report_flag_lands_before_the_prompt_positional() {
        let (_, mut args) = invocation(AiProvider::Codex, "PROMPT");
        insert_report_flag(&mut args, Path::new("/tmp/report.txt"));
        assert_eq!(
            args,
            [
                "exec",
                "--full-auto",
                "--output-last-message",
                "/tmp/report.txt",
                "PROMPT"
            ]
        );
    }

    #[test]
    fn an_empty_reply_still_reports_with_a_placeholder() {
        assert_eq!(
            report_summary(None, "  \n"),
            "The provider returned no report."
        );
        assert_eq!(
            report_summary(None, "Rebased 3 commits.\n"),
            "Rebased 3 commits."
        );
    }

    #[test]
    fn the_last_message_file_wins_over_a_chatty_stdout() {
        assert_eq!(
            report_summary(Some("Rebased cleanly.\n"), "codex vX\nsession log…"),
            "Rebased cleanly."
        );
        assert_eq!(
            report_summary(Some("  \n"), "fallback"),
            "fallback",
            "an empty file falls back to stdout"
        );
    }

    #[test]
    fn a_timeout_tells_the_verified_state_after_the_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "T").unwrap();
        cfg.set_str("user.email", "t@example.com").unwrap();
        cfg.set_bool("commit.gpgsign", false).unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo
            .commit(Some("HEAD"), &sig, &sig, "c1", &tree, &[])
            .unwrap();

        // Clean repo, HEAD untouched: nothing to abort — the detail says so.
        // The mid-rebase abort path is shared with cancel (covered e2e).
        assert_eq!(
            timed_out_detail(tmp.path(), head, AI_REBASE_TIMEOUT),
            "timed out after 1800s — the branch is unchanged"
        );
    }

    #[test]
    fn errors_spell_out_the_action_for_the_toast() {
        assert!(AiRebaseError::NotFound("claude".into())
            .message()
            .contains("Preferences"));
        assert!(AiRebaseError::Failed("boom".into())
            .message()
            .contains("AI rebase failed — boom"));
    }
}
