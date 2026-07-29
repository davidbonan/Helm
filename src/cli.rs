//! Command-line entry point and the `helm://` URL contract (specs/cli.md).
//!
//! The GUI binary is also the CLI: `main` dispatches on argv. In CLI mode the
//! target is resolved and validated **here** — the terminal is where the error
//! belongs — then handed to LaunchServices as a `helm://open?path=…` URL, which
//! launches the app if needed and raises it. The app re-validates on arrival:
//! a URL can come from anywhere, not just from us.

use std::ffi::OsString;
use std::fs::File;
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
  helm --help          Show this message
  helm --version       Show the version";

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
    Help,
    Version,
    /// Misuse: the message to print on stderr.
    Usage(String),
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Args {
    let args: Vec<OsString> = args.into_iter().collect();
    let first = args.first().map(|a| a.to_string_lossy().into_owned());
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
    }
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
