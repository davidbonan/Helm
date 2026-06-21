use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub enum CliError {
    /// `git` binary absent from PATH (or an invalid explicit path).
    NotFound,
    TimedOut(Duration),
    Io(std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    /// `None` if the process was killed by a signal.
    pub code: Option<i32>,
}

impl CliOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

pub fn run(workdir: &Path, args: &[&str]) -> Result<CliOutput, CliError> {
    run_program(Path::new("git"), workdir, args)
}

/// [`run`] with extra environment variables on the subprocess: the interactive
/// rebase injects its todo via `GIT_SEQUENCE_EDITOR` and pins `GIT_EDITOR`
/// (sync.rs) — the hardening below (prompt off, `LC_ALL=C`, timeout) applies
/// unchanged.
pub fn run_with_env(
    workdir: &Path,
    args: &[&str],
    envs: &[(&str, String)],
) -> Result<CliOutput, CliError> {
    run_program_with_timeout(Path::new("git"), workdir, args, DEFAULT_TIMEOUT, envs)
}

/// Proactive detection of the `git` binary (greyed-out toolbar, M12-9): same PATH
/// resolution as `Command::new("git")` (execvp), without spawning a process.
pub fn locate_git() -> Option<PathBuf> {
    locate_in(std::env::var_os("PATH"))
}

fn locate_in(path: Option<OsString>) -> Option<PathBuf> {
    std::env::split_paths(&path?).find_map(|dir| {
        let candidate = dir.join("git");
        is_executable(&candidate).then_some(candidate)
    })
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Seam: `run` pins the program to `git` (resolved via PATH); the parameter lets us
/// exercise the "binary not found" case without touching the process PATH.
pub fn run_program(program: &Path, workdir: &Path, args: &[&str]) -> Result<CliOutput, CliError> {
    run_program_with_timeout(program, workdir, args, DEFAULT_TIMEOUT, &[])
}

pub fn run_program_with_timeout(
    program: &Path,
    workdir: &Path,
    args: &[&str],
    timeout: Duration,
    envs: &[(&str, String)],
) -> Result<CliOutput, CliError> {
    let never = AtomicBool::new(false);
    let out = run_program_cancellable(program, workdir, args, timeout, envs, &never)?;
    Ok(out.expect("the flag is never raised"))
}

/// [`run_program_with_timeout`] with a caller-owned cancellation flag, checked
/// before the spawn and at every wait tick: once raised, the process group is
/// killed and the call returns `Ok(None)` — cancellation is the caller's
/// decision, not a process failure, so it stays out of [`CliError`].
pub fn run_program_cancellable(
    program: &Path,
    workdir: &Path,
    args: &[&str],
    timeout: Duration,
    envs: &[(&str, String)],
    cancel: &AtomicBool,
) -> Result<Option<CliOutput>, CliError> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut child = Command::new(program)
        .args(args)
        .current_dir(workdir)
        .env("GIT_TERMINAL_PROMPT", "0")
        // sync.rs classifies failures by matching English git messages; LC_ALL
        // overrides LANG and every LC_* category (POSIX), pinning the output.
        .env("LC_ALL", "C")
        .envs(envs.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => CliError::NotFound,
            _ => CliError::Io(err),
        })?;
    // Drained on dedicated threads **while** waiting: a pipe left unread blocks
    // the child as soon as it writes one buffer's worth (~64 KB) — `try_wait`
    // then never succeeds and every chatty command (large staged diff for the
    // AI prompt, verbose fetch) used to die on the timeout.
    let stdout = child.stdout.take().map(drain_pipe);
    let stderr = child.stderr.take().map(drain_pipe);
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(CliError::Io)? {
            return Ok(Some(CliOutput {
                stdout: join_pipe(stdout),
                stderr: join_pipe(stderr),
                code: status.code(),
            }));
        }
        if cancel.load(Ordering::Relaxed) {
            kill_process_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        if start.elapsed() >= timeout {
            kill_process_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::TimedOut(timeout));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn drain_pipe(pipe: impl std::io::Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut pipe = pipe;
        let mut buffer = Vec::new();
        let _ = std::io::Read::read_to_end(&mut pipe, &mut buffer);
        buffer
    })
}

/// The reader ends on pipe EOF, which the child's exit guarantees; a kill (or a
/// leaked descriptor in a grandchild) at worst delays EOF, never the exit path
/// above — `join` here is on an already-exited child.
fn join_pipe(handle: Option<thread::JoinHandle<Vec<u8>>>) -> String {
    let bytes = handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn kill_process_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::*;

    fn fake_git(dir: &Path, mode: u32) -> PathBuf {
        let path = dir.join("git");
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn locate_finds_an_executable_git_on_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let expected = fake_git(tmp.path(), 0o755);
        let path = std::env::join_paths([tmp.path()]).unwrap();
        assert_eq!(locate_in(Some(path)), Some(expected));
    }

    #[test]
    fn locate_skips_a_non_executable_git_file() {
        let tmp = tempfile::tempdir().unwrap();
        fake_git(tmp.path(), 0o644);
        let path = std::env::join_paths([tmp.path()]).unwrap();
        assert_eq!(locate_in(Some(path)), None);
    }

    #[test]
    fn locate_misses_when_no_dir_holds_git() {
        let tmp = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([tmp.path()]).unwrap();
        assert_eq!(locate_in(Some(path)), None);
        assert_eq!(locate_in(None), None);
    }

    #[test]
    fn locate_scans_dirs_in_path_order() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let expected = fake_git(second.path(), 0o755);
        let path = std::env::join_paths([first.path(), second.path()]).unwrap();
        assert_eq!(locate_in(Some(path)), Some(expected));
    }

    #[test]
    fn run_program_drains_large_output_without_deadlock() {
        let tmp = tempfile::tempdir().unwrap();
        // 256 KB on each pipe — far beyond the ~64 KB pipe buffer: without the
        // drain threads the child blocks writing and the call dies on timeout.
        let out = run_program_with_timeout(
            Path::new("/bin/sh"),
            tmp.path(),
            &[
                "-c",
                "dd if=/dev/zero bs=1024 count=256 2>/dev/null | tr '\\0' 'a'; \
                 dd if=/dev/zero bs=1024 count=256 2>/dev/null | tr '\\0' 'b' 1>&2",
            ],
            Duration::from_secs(10),
            &[],
        )
        .unwrap();

        assert!(out.success());
        assert_eq!(out.stdout.len(), 256 * 1024);
        assert_eq!(out.stderr.len(), 256 * 1024);
    }

    #[test]
    fn run_program_passes_extra_env_to_the_subprocess() {
        let tmp = tempfile::tempdir().unwrap();
        let out = run_program_with_timeout(
            Path::new("/bin/sh"),
            tmp.path(),
            &["-c", "printf %s \"$HELM_TEST_ENV\""],
            Duration::from_secs(5),
            &[("HELM_TEST_ENV", "injected".to_string())],
        )
        .unwrap();

        assert!(out.success());
        assert_eq!(out.stdout, "injected");
    }

    #[test]
    fn run_program_cancel_kills_the_process_before_its_end() {
        let tmp = tempfile::tempdir().unwrap();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&cancel);
        let killer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            flag.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let out = run_program_cancellable(
            Path::new("/bin/sh"),
            tmp.path(),
            &["-c", "sleep 5"],
            Duration::from_secs(10),
            &[],
            &cancel,
        )
        .unwrap();
        killer.join().unwrap();

        assert!(out.is_none(), "a cancelled run yields no output");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "the kill follows the flag, not the child's sleep"
        );
    }

    #[test]
    fn run_program_cancelled_up_front_spawns_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(true);
        let out = run_program_cancellable(
            Path::new("/bin/sh"),
            tmp.path(),
            &["-c", "touch marker"],
            Duration::from_secs(5),
            &[],
            &cancel,
        )
        .unwrap();

        assert!(out.is_none());
        assert!(
            !tmp.path().join("marker").exists(),
            "the child must not even spawn"
        );
    }

    #[test]
    fn run_program_times_out_and_kills_the_process() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_program_with_timeout(
            Path::new("/bin/sh"),
            tmp.path(),
            &["-c", "sleep 2"],
            Duration::from_millis(50),
            &[],
        )
        .unwrap_err();

        assert!(matches!(err, CliError::TimedOut(_)), "got {err:?}");
    }
}
