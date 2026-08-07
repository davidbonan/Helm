//! Command-line entry point and the `helm://` URL contract (specs/cli.md).
//!
//! The GUI binary is also the CLI: `main` dispatches on argv. In CLI mode the
//! target is resolved and validated **here** — the terminal is where the error
//! belongs — then handed to LaunchServices as a `helm://open?path=…` URL, which
//! launches the app if needed and raises it. The app re-validates on arrival:
//! a URL can come from anywhere, not just from us.

use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Scheme registered by the bundle's `CFBundleURLTypes` (scripts/bundle.sh).
const URL_SCHEME: &str = "helm";
/// The only verb; any other host is ignored by the app (forward compatibility).
const OPEN_HOST: &str = "open";
const PATH_PARAM: &str = "path";

/// Directory the `helm` shell command is symlinked into. On the default macOS
/// `PATH` (`/etc/paths`) and outside the bundle, so an app update never breaks
/// the link's target.
pub const SHELL_COMMAND_DIR: &str = "/usr/local/bin";

const USAGE: &str = "\
helm — native dev workspace (terminal + git)

Usage:
  helm                 Launch the app
  helm <path>          Open the repository or worktree at <path>
  helm run <command>   Drive the Run server of a worktree (see below)
  helm init claude     Teach Claude Code to use `helm run` (writes HELM.md)
  helm --help          Show this message
  helm --version       Show the version

Run commands (they talk to the running helm; exit 3 when it cannot answer):
  helm run status [path]      State of that worktree's Run server (default: .)
  helm run list               Every worktree of the workspace
  helm run start [path]       Start it — a running server is left alone
  helm run stop [path]        Stop it (kills the process tree)
  helm run relaunch [path]    Stop then start
  helm run logs [path]        Tail of what the server printed
  -n <lines>                  Lines to tail (logs only, default 40)
  --json                      Machine-readable output";

/// Lines `helm run logs` tails when `-n` is left out: a screenful, the size of a
/// stack trace one wants to read without asking for the whole scrollback.
const DEFAULT_LOG_LINES: usize = 40;

/// Exit code when no answer can be had — helm is down, or up but silent. Distinct
/// from a plain failure so a script can tell "ask someone else" from "helm said
/// no" (that worktree has no run command): on 3, do the job yourself.
pub const EXIT_UNREACHABLE: i32 = 3;

/// What argv asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Args {
    /// Windowed mode. `open_url` injects a startup target without going through
    /// LaunchServices — the dev/test hook, since `cargo run` is not a bundle and
    /// therefore never receives an Apple Event.
    Gui {
        open_url: Option<String>,
    },
    Open(PathBuf),
    /// `helm run …`: the Run strip, driven from outside the app (§9).
    Run(RunArgs),
    /// `helm init <agent>`: install helm's instructions for a coding agent (§10).
    Init(InitTarget),
    Help,
    Version,
    /// Misuse: the message to print on stderr.
    Usage(String),
}

/// Agent `helm init` knows how to equip. One today; the verb takes a target so a
/// second one costs a match arm, not a new syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitTarget {
    Claude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOp {
    Status,
    List,
    Start,
    Stop,
    Relaunch,
    Logs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    pub op: RunOp,
    /// Worktree to act on, `None` for `list` alone. Left unresolved here: the
    /// resolution belongs to `execute`, which reports its errors on stderr.
    pub path: Option<PathBuf>,
    /// `-n`, `logs` only; `None` ⇒ `DEFAULT_LOG_LINES`.
    pub lines: Option<usize>,
    pub json: bool,
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Args {
    let args: Vec<OsString> = args.into_iter().collect();
    let first = args.first().map(|a| a.to_string_lossy().into_owned());
    if first.as_deref() == Some("run") {
        return parse_run(&args[1..]);
    }
    if first.as_deref() == Some("init") {
        return parse_init(&args[1..]);
    }
    match (args.len(), first.as_deref()) {
        (0, _) => Args::Gui { open_url: None },
        // Carbon-era process serial number: some LaunchServices launches still
        // append it. It is a launch, not a CLI invocation.
        (_, Some(arg)) if arg.starts_with("-psn_") => Args::Gui { open_url: None },
        (2, Some("--open-url")) => Args::Gui {
            open_url: Some(args[1].to_string_lossy().into_owned()),
        },
        (1, Some("-h" | "--help")) => Args::Help,
        (1, Some("-V" | "--version")) => Args::Version,
        (1, Some(arg)) if arg.starts_with('-') => Args::Usage(format!("unknown option “{arg}”")),
        (1, _) => Args::Open(PathBuf::from(&args[0])),
        _ => Args::Usage("expected a single path".to_owned()),
    }
}

/// `helm run <op> [path] [-n <lines>] [--json]`. Options are accepted anywhere; a
/// missing path means the current directory, which is what an agent sitting in a
/// worktree types.
fn parse_run(rest: &[OsString]) -> Args {
    let mut json = false;
    let mut lines: Option<usize> = None;
    let mut words: Vec<String> = Vec::new();
    let mut rest = rest.iter().map(|a| a.to_string_lossy().into_owned());
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "-n" | "--lines" => {
                let Some(value) = rest.next() else {
                    return Args::Usage(format!("“{arg}” expects a number of lines"));
                };
                match value.parse::<usize>() {
                    Ok(parsed) => lines = Some(parsed),
                    Err(_) => return Args::Usage(format!("“{value}” is not a number of lines")),
                }
            }
            other if other.starts_with('-') => {
                return Args::Usage(format!("unknown option “{other}”"))
            }
            other => words.push(other.to_owned()),
        }
    }
    let Some(op) = words.first() else {
        return Args::Usage("run: expected status, list, start, stop, relaunch or logs".to_owned());
    };
    let op = match op.as_str() {
        "status" => RunOp::Status,
        "list" => RunOp::List,
        "start" => RunOp::Start,
        "stop" => RunOp::Stop,
        "relaunch" => RunOp::Relaunch,
        "logs" => RunOp::Logs,
        other => return Args::Usage(format!("unknown run command “{other}”")),
    };
    if lines.is_some() && op != RunOp::Logs {
        return Args::Usage("“-n” only applies to run logs".to_owned());
    }
    let path = match (op, words.len()) {
        (RunOp::List, 1) => None,
        (RunOp::List, _) => return Args::Usage("run list takes no path".to_owned()),
        (_, 1) => Some(PathBuf::from(".")),
        (_, 2) => Some(PathBuf::from(&words[1])),
        _ => return Args::Usage("expected a single path".to_owned()),
    };
    Args::Run(RunArgs {
        op,
        path,
        lines,
        json,
    })
}

fn parse_init(rest: &[OsString]) -> Args {
    let words: Vec<String> = rest
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    match words.len() {
        0 => Args::Usage("init: expected an agent to equip (claude)".to_owned()),
        1 => match words[0].as_str() {
            "claude" => Args::Init(InitTarget::Claude),
            other => Args::Usage(format!("helm cannot equip “{other}” yet")),
        },
        _ => Args::Usage("init takes a single agent".to_owned()),
    }
}

/// Runs everything but `Args::Gui` and returns the process exit code.
pub fn execute(args: Args) -> i32 {
    match args {
        Args::Gui { .. } => 0,
        Args::Help => {
            println!("{USAGE}");
            0
        }
        Args::Version => {
            println!("helm {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Args::Usage(message) => {
            eprintln!("helm: {message}\n\n{USAGE}");
            2
        }
        Args::Open(path) => match resolve_target(&path) {
            Ok(target) => open_target(&target),
            Err(err) => {
                eprintln!("helm: {}", err.message(&path));
                1
            }
        },
        Args::Run(run) => execute_run(run),
        Args::Init(InitTarget::Claude) => execute_init_claude(),
    }
}

/// Instructions file helm owns inside the Claude config dir — its own file, so a
/// rewrite never touches a line the user wrote.
const CLAUDE_INSTRUCTIONS_FILE: &str = "HELM.md";
/// Claude Code's user-level memory, which pulls the above in.
const CLAUDE_MEMORY_FILE: &str = "CLAUDE.md";
/// The `@` include, resolved by Claude Code against the memory file's own folder.
const CLAUDE_INCLUDE: &str = "@HELM.md";

/// What helm tells an agent about `helm run` (§10), baked into the binary the same
/// way the release notes are: one source, and `helm init claude` after an update
/// refreshes it.
const AGENT_INSTRUCTIONS: &str = include_str!("../agent-instructions.md");

/// Claude Code's config folder: `CLAUDE_CONFIG_DIR` when set (it is what Claude
/// itself honours), else `~/.claude`.
fn claude_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".claude"))
}

/// What the install did, so the CLI can report it instead of claiming work.
#[derive(Debug, PartialEq, Eq)]
pub struct InitReport {
    pub instructions: PathBuf,
    /// False when the file already held exactly these instructions.
    pub instructions_written: bool,
    pub memory: PathBuf,
    /// False when the include line was already there.
    pub include_added: bool,
}

/// Writes helm's instructions into `dir` and makes the memory file pull them in.
/// Idempotent, and additive on the memory file: one appended line, never a rewrite
/// — that file is the user's. Opened in append mode, so a symlinked `CLAUDE.md`
/// (a dotfiles setup) is followed rather than replaced.
pub fn install_claude_instructions(dir: &Path) -> std::io::Result<InitReport> {
    std::fs::create_dir_all(dir)?;
    let instructions = dir.join(CLAUDE_INSTRUCTIONS_FILE);
    let instructions_written =
        std::fs::read_to_string(&instructions).ok().as_deref() != Some(AGENT_INSTRUCTIONS);
    if instructions_written {
        std::fs::write(&instructions, AGENT_INSTRUCTIONS)?;
    }

    let memory = dir.join(CLAUDE_MEMORY_FILE);
    let existing = std::fs::read_to_string(&memory).unwrap_or_default();
    let include_added = !existing.lines().any(|line| line.trim() == CLAUDE_INCLUDE);
    if include_added {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&memory)?;
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        writeln!(file, "{separator}{CLAUDE_INCLUDE}")?;
    }

    Ok(InitReport {
        instructions,
        instructions_written,
        memory,
        include_added,
    })
}

fn execute_init_claude() -> i32 {
    let Some(dir) = claude_config_dir() else {
        eprintln!("helm: cannot locate Claude's config folder (no home directory)");
        return 1;
    };
    match install_claude_instructions(&dir) {
        Ok(report) => {
            let (verb, path) = match report.instructions_written {
                true => ("wrote", &report.instructions),
                false => ("unchanged", &report.instructions),
            };
            println!("{verb:<10}{}", tilde(path));
            match report.include_added {
                true => println!("linked    {CLAUDE_INCLUDE} in {}", tilde(&report.memory)),
                false => println!("linked    already in {}", tilde(&report.memory)),
            }
            0
        }
        Err(err) => {
            eprintln!("helm: cannot write into {}: {err}", dir.display());
            1
        }
    }
}

/// Home-relative display: these paths are read by a human, and `~/.claude/…` says
/// more at a glance than the absolute one.
fn tilde(path: &Path) -> String {
    let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    match home.and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Asks the running app (§9). The worktree is resolved **here**, like `helm <path>`:
/// a bad path is a terminal error, and the app is only reached with a canonical
/// working tree.
fn execute_run(args: RunArgs) -> i32 {
    let request = match args.path {
        None => crate::ipc::Request::List,
        Some(path) => {
            let target = match resolve_target(&path) {
                Ok(target) => target,
                Err(err) => {
                    eprintln!("helm: {}", err.message(&path));
                    return 1;
                }
            };
            match args.op {
                RunOp::Status => crate::ipc::Request::Status { path: target },
                RunOp::Start => crate::ipc::Request::Start { path: target },
                RunOp::Stop => crate::ipc::Request::Stop { path: target },
                RunOp::Relaunch => crate::ipc::Request::Relaunch { path: target },
                RunOp::Logs => crate::ipc::Request::Logs {
                    path: target,
                    lines: args.lines.unwrap_or(DEFAULT_LOG_LINES),
                },
                // `list` carries no path.
                RunOp::List => crate::ipc::Request::List,
            }
        }
    };
    match crate::ipc::request(&request) {
        Ok(crate::ipc::Response::Runs { runs }) => {
            print_runs(&runs, args.json);
            0
        }
        Ok(crate::ipc::Response::Logs { entry, lines }) => {
            print_logs(&entry, &lines, args.json);
            0
        }
        Ok(crate::ipc::Response::Error { message }) => {
            eprintln!("helm: {message}");
            1
        }
        Err(crate::ipc::ClientError::NotRunning) => {
            eprintln!("helm: helm is not running — launch it first");
            EXIT_UNREACHABLE
        }
        Err(crate::ipc::ClientError::NotAnswering) => {
            eprintln!(
                "helm: helm is running but not answering — unhide its window \
                 (a hidden or minimized app stops drawing, and the answer is \
                 written on a frame)"
            );
            EXIT_UNREACHABLE
        }
        Err(crate::ipc::ClientError::Failed(message)) => {
            eprintln!("helm: {message}");
            1
        }
    }
}

fn print_runs(runs: &[crate::ipc::RunEntry], json: bool) {
    if json {
        match serde_json::to_string_pretty(runs) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("helm: {err}"),
        }
        return;
    }
    if runs.is_empty() {
        println!("no worktree open in helm");
        return;
    }
    for entry in runs {
        println!("{}", run_line(entry));
        if let Some(error) = &entry.error {
            println!("  {error}");
        }
    }
}

/// Captured output goes to **stdout alone** so `helm run logs | grep …` works;
/// the state line and the empty-buffer note go to stderr.
fn print_logs(entry: &crate::ipc::RunEntry, lines: &[String], json: bool) {
    if json {
        let payload = serde_json::json!({ "entry": entry, "lines": lines });
        match serde_json::to_string_pretty(&payload) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("helm: {err}"),
        }
        return;
    }
    eprintln!("{}", run_line(entry));
    if lines.is_empty() {
        eprintln!("  (no output — a stopped strip keeps no buffer)");
        return;
    }
    for line in lines {
        println!("{line}");
    }
}

/// One aligned line per worktree: state, where it is, its port, what it runs.
/// An exited process carries its code — "the server is down" and "the server died
/// with 1" are not the same news.
fn run_line(entry: &crate::ipc::RunEntry) -> String {
    let state = match entry.exit_code {
        Some(code) => format!("{} {code}", entry.state.label()),
        None => entry.state.label().to_owned(),
    };
    let label = match &entry.branch {
        Some(branch) => format!("{}/{branch}", entry.project),
        None => entry.project.clone(),
    };
    let port = entry.port.map(|p| format!(":{p}")).unwrap_or_default();
    let command = if entry.launch_command.trim().is_empty() {
        "(no run command)"
    } else {
        entry.launch_command.trim()
    };
    format!("{state:<8}  {label:<28}  {port:<7}  {command}")
}

/// Why a path cannot be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetError {
    Missing,
    NotGit,
    /// A bare repository owns no working tree, so it has no selectable row
    /// (worktrees.md §8) — there is nothing to open at that address.
    Bare,
    /// The workspace is persisted as TOML, which only round-trips UTF-8 paths.
    NonUtf8,
}

impl TargetError {
    pub fn message(self, path: &Path) -> String {
        let path = path.display();
        match self {
            Self::Missing => format!("“{path}”: no such directory"),
            Self::NotGit => format!("“{path}” is not a git repository"),
            Self::Bare => {
                format!("“{path}” is a bare repository — point at one of its worktrees")
            }
            Self::NonUtf8 => format!("“{path}”: path is not valid UTF-8"),
        }
    }
}

/// Path ⇒ the working tree to activate. Walks **up** from the given path
/// (`discover`), so `helm .` works from anywhere inside a checkout; a linked
/// worktree resolves to itself, and the app derives its group root from there.
pub fn resolve_target(path: &Path) -> Result<PathBuf, TargetError> {
    let path = std::fs::canonicalize(path).map_err(|_| TargetError::Missing)?;
    let repo = git2::Repository::discover(&path).map_err(|_| TargetError::NotGit)?;
    let workdir = repo.workdir().ok_or(TargetError::Bare)?;
    let target = std::fs::canonicalize(workdir).map_err(|_| TargetError::Missing)?;
    if target.to_str().is_none() {
        return Err(TargetError::NonUtf8);
    }
    Ok(target)
}

/// Hands the target to LaunchServices, which launches the app when it is not
/// already running and raises it either way.
fn open_target(target: &Path) -> i32 {
    let Some(url) = open_url(target) else {
        eprintln!("helm: “{}”: path is not valid UTF-8", target.display());
        return 1;
    };
    match Command::new("open").arg(&url).status() {
        Ok(status) if status.success() => 0,
        // `open` already printed its own diagnostic (most often: no application
        // registered for the scheme, i.e. helm is not installed as a bundle).
        Ok(_) => 1,
        Err(err) => {
            eprintln!("helm: cannot run open: {err}");
            1
        }
    }
}

/// `helm://open?path=<percent-encoded>`; `None` for a non-UTF-8 path.
fn open_url(target: &Path) -> Option<String> {
    let path = target.to_str()?;
    Some(format!(
        "{URL_SCHEME}://{OPEN_HOST}?{PATH_PARAM}={}",
        percent_encode(path)
    ))
}

/// Inverse of [`open_url`]. An unknown scheme, host or missing `path` yields
/// `None`; unknown extra parameters are ignored so an older binary survives a
/// URL minted by a newer one. The path must be absolute — a relative one would
/// resolve against the app's working directory, not the caller's.
pub fn target_from_url(url: &str) -> Option<PathBuf> {
    let rest = strip_scheme(url)?;
    let (host, query) = rest.split_once('?')?;
    if !host.eq_ignore_ascii_case(OPEN_HOST) {
        return None;
    }
    let value = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == PATH_PARAM)
        .map(|(_, value)| value)?;
    let path = PathBuf::from(percent_decode(value)?);
    path.is_absolute().then_some(path)
}

fn strip_scheme(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    scheme.eq_ignore_ascii_case(URL_SCHEME).then_some(rest)
}

/// Everything outside the unreserved set is escaped; `/` is kept literal so the
/// URL stays readable in a log or a menu item.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `+` is **not** decoded as a space: that is form encoding, and a path may
/// legitimately contain a `+`.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3)?;
            // from_str_radix alone would accept a sign: `%+f` is not an escape.
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Advisory lock held for the lifetime of the windowed process. Prefs are
/// rewritten whole (`persistence::save_to`), so a second instance would silently
/// erase the first one's workspace.
pub struct InstanceLock {
    /// `None` when no lock file could be opened: a launch is never blocked on it.
    _file: Option<File>,
}

/// `None` ⇒ another instance already holds the lock.
pub fn acquire_instance_lock() -> Option<InstanceLock> {
    let Some(path) = lock_path() else {
        return Some(InstanceLock { _file: None });
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = File::create(&path) else {
        return Some(InstanceLock { _file: None });
    };
    // flock is released by the kernel when the process dies, crash included —
    // unlike a pid file, there is no stale lock to garbage-collect.
    let locked = unsafe {
        use std::os::unix::io::AsRawFd;
        libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0
    };
    locked.then_some(InstanceLock { _file: Some(file) })
}

fn lock_path() -> Option<PathBuf> {
    let prefs = crate::persistence::prefs_path()?;
    Some(prefs.parent()?.join("instance.lock"))
}

/// Raises the instance already running; `false` outside a bundle (`cargo run`),
/// where LaunchServices has nothing to activate and the caller must say so
/// itself rather than exit silently.
pub fn activate_running_instance() -> bool {
    let Some(bundle) = crate::update::bundle_path() else {
        return false;
    };
    let _ = Command::new("open").arg(bundle).status();
    true
}

/// State of the `helm` shell command, for the Preferences card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellCommand {
    /// Not running from a `.app`: there is no stable target to link to.
    Unbundled,
    Installed,
    Missing,
    /// A link (or file) is there but is not ours — an older install, or another
    /// program's `helm`.
    Foreign,
}

fn shell_command_path() -> PathBuf {
    Path::new(SHELL_COMMAND_DIR).join("helm")
}

/// The binary the symlink must point at: inside the bundle, so an in-place
/// update (update.md §5) keeps the command working.
fn shell_command_target() -> Option<PathBuf> {
    Some(crate::update::bundle_path()?.join("Contents/MacOS/helm"))
}

pub fn shell_command_state() -> ShellCommand {
    let Some(target) = shell_command_target() else {
        return ShellCommand::Unbundled;
    };
    let link = shell_command_path();
    match std::fs::read_link(&link) {
        Ok(current) if current == target => ShellCommand::Installed,
        Ok(_) => ShellCommand::Foreign,
        Err(_) if link.exists() => ShellCommand::Foreign,
        Err(_) => ShellCommand::Missing,
    }
}

/// Installs (or repairs) the symlink. Only ever replaces a **symlink** — a real
/// file at that path belongs to something else and is left untouched.
pub fn install_shell_command() -> Result<PathBuf, String> {
    let target = shell_command_target()
        .ok_or_else(|| "the shell command needs helm installed as an application".to_owned())?;
    let link = shell_command_path();
    if std::fs::symlink_metadata(&link).is_ok() {
        if std::fs::read_link(&link).is_err() {
            return Err(format!(
                "{} already exists and is not a symlink",
                link.display()
            ));
        }
        std::fs::remove_file(&link).map_err(|err| link_error(&link, &target, err))?;
    }
    std::os::unix::fs::symlink(&target, &link).map_err(|err| link_error(&link, &target, err))?;
    Ok(link)
}

/// A non-writable `/usr/local/bin` is the common case on a machine without
/// Homebrew: hand back the exact command to run instead of a bare errno.
fn link_error(link: &Path, target: &Path, err: std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        return format!(
            "{} is not writable — run: sudo ln -sf \"{}\" \"{}\"",
            link.parent().unwrap_or(link).display(),
            target.display(),
            link.display()
        );
    }
    format!("{err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Args {
        parse(list.iter().map(OsString::from))
    }

    #[test]
    fn no_argument_launches_the_gui() {
        assert_eq!(args(&[]), Args::Gui { open_url: None });
    }

    #[test]
    fn a_launch_services_process_serial_number_is_not_a_cli_call() {
        assert_eq!(args(&["-psn_0_774578"]), Args::Gui { open_url: None });
    }

    #[test]
    fn a_single_path_opens_it() {
        assert_eq!(args(&["/tmp/x"]), Args::Open(PathBuf::from("/tmp/x")));
    }

    #[test]
    fn open_url_flag_stays_in_gui_mode() {
        assert_eq!(
            args(&["--open-url", "helm://open?path=/tmp/x"]),
            Args::Gui {
                open_url: Some("helm://open?path=/tmp/x".to_owned())
            }
        );
    }

    #[test]
    fn help_and_version_are_recognized() {
        assert_eq!(args(&["--help"]), Args::Help);
        assert_eq!(args(&["-h"]), Args::Help);
        assert_eq!(args(&["--version"]), Args::Version);
        assert_eq!(args(&["-V"]), Args::Version);
    }

    #[test]
    fn extra_arguments_and_unknown_options_are_refused() {
        assert!(matches!(args(&["/tmp/a", "/tmp/b"]), Args::Usage(_)));
        assert!(matches!(args(&["--nope"]), Args::Usage(_)));
    }

    #[test]
    fn run_commands_default_to_the_current_directory() {
        assert_eq!(
            args(&["run", "status"]),
            Args::Run(RunArgs {
                op: RunOp::Status,
                path: Some(PathBuf::from(".")),
                lines: None,
                json: false,
            })
        );
        assert_eq!(
            args(&["run", "relaunch", "/tmp/x"]),
            Args::Run(RunArgs {
                op: RunOp::Relaunch,
                path: Some(PathBuf::from("/tmp/x")),
                lines: None,
                json: false,
            })
        );
    }

    #[test]
    fn run_list_takes_no_path_and_json_is_positional_free() {
        assert_eq!(
            args(&["run", "--json", "list"]),
            Args::Run(RunArgs {
                op: RunOp::List,
                path: None,
                lines: None,
                json: true,
            })
        );
        assert!(matches!(args(&["run", "list", "/tmp/x"]), Args::Usage(_)));
    }

    #[test]
    fn run_logs_takes_a_line_count_nobody_else_accepts() {
        assert_eq!(
            args(&["run", "logs", "-n", "200", "/tmp/x"]),
            Args::Run(RunArgs {
                op: RunOp::Logs,
                path: Some(PathBuf::from("/tmp/x")),
                lines: Some(200),
                json: false,
            })
        );
        assert_eq!(
            args(&["run", "logs"]),
            Args::Run(RunArgs {
                op: RunOp::Logs,
                path: Some(PathBuf::from(".")),
                lines: None,
                json: false,
            }),
            "without -n the default line count is applied at request time"
        );
        assert!(matches!(
            args(&["run", "status", "-n", "10"]),
            Args::Usage(_)
        ));
        assert!(matches!(args(&["run", "logs", "-n"]), Args::Usage(_)));
        assert!(matches!(
            args(&["run", "logs", "-n", "lots"]),
            Args::Usage(_)
        ));
    }

    #[test]
    fn an_unknown_run_command_or_option_is_refused() {
        assert!(matches!(args(&["run"]), Args::Usage(_)));
        assert!(matches!(args(&["run", "kill"]), Args::Usage(_)));
        assert!(matches!(
            args(&["run", "status", "--force"]),
            Args::Usage(_)
        ));
        assert!(matches!(
            args(&["run", "status", "/a", "/b"]),
            Args::Usage(_)
        ));
    }

    #[test]
    fn a_run_line_states_where_what_and_on_which_port() {
        let entry = crate::ipc::RunEntry {
            worktree: PathBuf::from("/dev/api.worktrees/feat-x"),
            project: "api".to_owned(),
            branch: Some("feat-x".to_owned()),
            state: crate::ipc::RunState::Running,
            port: Some(3001),
            command: "npm run dev -- --port $PORT".to_owned(),
            launch_command: "npm run dev -- --port 3001".to_owned(),
            error: None,
            exit_code: None,
        };
        assert_eq!(
            run_line(&entry),
            "running   api/feat-x                    :3001    npm run dev -- --port 3001"
        );

        let idle = crate::ipc::RunEntry {
            branch: None,
            state: crate::ipc::RunState::Stopped,
            port: None,
            command: String::new(),
            launch_command: String::new(),
            ..entry
        };
        assert_eq!(
            run_line(&idle),
            "stopped   api                                    (no run command)"
        );

        let crashed = crate::ipc::RunEntry {
            state: crate::ipc::RunState::Exited,
            exit_code: Some(1),
            ..idle
        };
        assert!(
            run_line(&crashed).starts_with("exited 1 "),
            "a process that died says with what: {}",
            run_line(&crashed)
        );
    }

    #[test]
    fn init_takes_one_known_agent() {
        assert_eq!(args(&["init", "claude"]), Args::Init(InitTarget::Claude));
        assert!(matches!(args(&["init"]), Args::Usage(_)));
        assert!(matches!(args(&["init", "codex"]), Args::Usage(_)));
        assert!(matches!(args(&["init", "claude", "x"]), Args::Usage(_)));
    }

    #[test]
    fn installing_writes_the_instructions_and_links_them_once() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("claude");

        let first = install_claude_instructions(&dir).unwrap();
        assert!(first.instructions_written && first.include_added);
        assert_eq!(
            std::fs::read_to_string(&first.instructions).unwrap(),
            AGENT_INSTRUCTIONS
        );
        assert_eq!(
            std::fs::read_to_string(&first.memory).unwrap(),
            "@HELM.md\n",
            "a missing memory file is created holding the include"
        );

        let again = install_claude_instructions(&dir).unwrap();
        assert!(
            !again.instructions_written && !again.include_added,
            "a second run has nothing to do: {again:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&again.memory).unwrap(),
            "@HELM.md\n",
            "the include is not appended twice"
        );
    }

    #[test]
    fn installing_only_appends_to_a_memory_file_the_user_owns() {
        let dir = tempfile::tempdir().unwrap();
        let memory = dir.path().join(CLAUDE_MEMORY_FILE);
        // No trailing newline: the include must not land on the user's last line.
        std::fs::write(&memory, "@RTK.md\n\n# Coding Behavior\n\nBe careful.").unwrap();

        install_claude_instructions(dir.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(&memory).unwrap(),
            "@RTK.md\n\n# Coding Behavior\n\nBe careful.\n@HELM.md\n"
        );
    }

    #[test]
    fn a_stale_instructions_file_is_refreshed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CLAUDE_INSTRUCTIONS_FILE), "# old helm\n").unwrap();
        std::fs::write(dir.path().join(CLAUDE_MEMORY_FILE), "@HELM.md\n").unwrap();

        let report = install_claude_instructions(dir.path()).unwrap();

        assert!(report.instructions_written, "an update rewrites the file");
        assert!(!report.include_added, "the link was already there");
        assert_eq!(
            std::fs::read_to_string(&report.instructions).unwrap(),
            AGENT_INSTRUCTIONS
        );
    }

    #[test]
    fn the_bundled_instructions_teach_the_run_commands() {
        for command in ["helm run status", "helm run list", "helm run logs"] {
            assert!(
                AGENT_INSTRUCTIONS.contains(command),
                "the shipped instructions must name `{command}`"
            );
        }
    }

    #[test]
    fn url_round_trips_a_path_with_spaces_and_accents() {
        let path = PathBuf::from("/Users/moi/dev/mon projet/été");
        let url = open_url(&path).expect("utf-8 path");
        assert_eq!(
            url,
            "helm://open?path=/Users/moi/dev/mon%20projet/%C3%A9t%C3%A9"
        );
        assert_eq!(target_from_url(&url), Some(path));
    }

    #[test]
    fn plus_is_a_literal_character_not_a_space() {
        let path = PathBuf::from("/tmp/c++");
        let url = open_url(&path).expect("utf-8 path");
        assert_eq!(target_from_url(&url), Some(path));
    }

    #[test]
    fn unknown_parameters_are_ignored_and_order_does_not_matter() {
        assert_eq!(
            target_from_url("helm://open?tab=3&path=/tmp/x"),
            Some(PathBuf::from("/tmp/x"))
        );
    }

    #[test]
    fn a_foreign_scheme_host_or_relative_path_is_rejected() {
        assert_eq!(target_from_url("zed://open?path=/tmp/x"), None);
        assert_eq!(target_from_url("helm://quit?path=/tmp/x"), None);
        assert_eq!(target_from_url("helm://open?path=relative"), None);
        assert_eq!(target_from_url("helm://open"), None);
        assert_eq!(target_from_url("not a url"), None);
    }

    #[test]
    fn the_scheme_and_host_are_case_insensitive() {
        assert_eq!(
            target_from_url("HELM://Open?path=/tmp/x"),
            Some(PathBuf::from("/tmp/x"))
        );
    }

    #[test]
    fn a_truncated_escape_is_rejected_rather_than_guessed() {
        assert_eq!(target_from_url("helm://open?path=/tmp/%2"), None);
        assert_eq!(target_from_url("helm://open?path=/tmp/%zz"), None);
        assert_eq!(
            target_from_url("helm://open?path=/tmp/%+f"),
            None,
            "a signed hex pair is not an escape"
        );
    }
}
