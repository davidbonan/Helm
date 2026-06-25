use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::Result;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

const DEFAULT_SHELL: &str = "/bin/zsh";
const TERM: &str = "xterm-256color";
const COLORTERM: &str = "truecolor";

pub fn shell_program() -> OsString {
    shell_or_default(std::env::var_os("SHELL"))
}

fn shell_or_default(shell: Option<OsString>) -> OsString {
    shell.unwrap_or_else(|| OsString::from(DEFAULT_SHELL))
}

pub fn login_shell_command(program: impl Into<OsString>, cwd: &Path) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(program.into());
    cmd.arg("-l");
    cmd.cwd(cwd);
    cmd.env("TERM", TERM);
    cmd.env("COLORTERM", COLORTERM);
    cmd
}

/// Login shell running a single command then exiting (`$SHELL -lc <command>`), for
/// the per-project Run terminal (git.md §3): `-l` loads the user's profile so the
/// toolchain on `PATH` (node, cargo…) is found, like an interactive launch.
pub fn run_command(program: impl Into<OsString>, cwd: &Path, command: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(program.into());
    cmd.arg("-l");
    cmd.arg("-c");
    cmd.arg(command);
    cmd.cwd(cwd);
    cmd.env("TERM", TERM);
    cmd.env("COLORTERM", COLORTERM);
    cmd
}

/// Env var the review prompt is exported under (M-RC): [`agent_invocation`]
/// expands it as a single argv, so the prompt is never shell-escaped nor echoed
/// into the terminal.
pub const REVIEW_PROMPT_ENV: &str = "HELM_REVIEW_PROMPT";

/// Command fed into the agent pane's interactive login shell (M-RC): runs the
/// configured CLI with the review prompt (`$HELM_REVIEW_PROMPT`) as a single
/// argument. Running the agent as a job of the shell — rather than as the pane's
/// root process — keeps the terminal usable once the agent exits: Ctrl+C drops
/// back to the shell prompt instead of leaving a dead "[process exited]" pane.
pub fn agent_invocation(program: &str) -> String {
    format!("{program} \"${REVIEW_PROMPT_ENV}\"\n")
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    pub fn spawn(cmd: CommandBuilder, size: PtySize) -> Result<Self> {
        let pair = native_pty_system().openpty(size)?;
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        Ok(Self {
            master: pair.master,
            child,
        })
    }

    pub fn open_login_shell(cwd: &Path, size: PtySize) -> Result<Self> {
        Self::spawn(login_shell_command(shell_program(), cwd), size)
    }

    pub fn take_writer(&self) -> Result<Box<dyn Write + Send>> {
        self.master.take_writer()
    }

    pub fn reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader()
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    /// Pgid of the terminal's foreground group (`tcgetpgrp` on the master fd): the
    /// job the shell brought to the foreground, or the shell itself at the prompt.
    pub fn foreground_pgid(&self) -> Option<i32> {
        self.master.process_group_leader()
    }

    pub fn child(&mut self) -> &mut (dyn Child + Send + Sync) {
        &mut *self.child
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Kills the terminal's process tree: SIGHUP to the foreground group (terminal-
    /// close semantics), SIGKILL of the shell, then SIGKILL of the group —
    /// `child.kill()` only signals zsh, not its jobs, and an agent that ignores
    /// SIGHUP (e.g. Claude Code) would otherwise survive.
    ///
    /// Shell already exited (`exit` at the prompt, relaunch): nothing left to signal —
    /// `tcgetpgrp` still returns the **dead** shell's pgid (stale read, verified on
    /// macOS) and a recycled pid would make us signal a third-party process;
    /// `child.kill()` would likewise send a SIGHUP to that recyclable pid. A
    /// **distinct** job left in the foreground is, however, always signaled.
    pub fn shutdown(&mut self) {
        let reaped = matches!(self.child.try_wait(), Ok(Some(_)));
        let shell_pgid = self.child.process_id().map(|pid| pid as i32);
        let foreground = self
            .foreground_pgid()
            .filter(|pgid| !reaped || Some(*pgid) != shell_pgid);
        if let Some(pgid) = foreground {
            unsafe { libc::killpg(pgid, libc::SIGHUP) };
        }
        if !reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(pgid) = foreground {
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }
}

/// Single safety net: all close paths (tab, repo, group sync, relaunch, and CMD+Q —
/// eframe drops the app on `LoopExiting`) go through this drop.
impl Drop for Pty {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_program_uses_env_then_falls_back_to_zsh() {
        assert_eq!(shell_or_default(None), OsString::from(DEFAULT_SHELL));
        assert_eq!(
            shell_or_default(Some(OsString::from("/bin/bash"))),
            OsString::from("/bin/bash")
        );
    }

    #[test]
    fn login_command_sets_login_flag_cwd_and_color_env() {
        let cmd = login_shell_command("/bin/zsh", Path::new("/tmp"));
        let argv = cmd.get_argv();
        assert_eq!(argv[0], OsString::from("/bin/zsh"));
        assert_eq!(argv[1], OsString::from("-l"));
        assert_eq!(cmd.get_cwd(), Some(&OsString::from("/tmp")));
        assert_eq!(cmd.get_env("TERM"), Some(std::ffi::OsStr::new(TERM)));
        assert_eq!(
            cmd.get_env("COLORTERM"),
            Some(std::ffi::OsStr::new(COLORTERM))
        );
    }

    #[test]
    fn run_command_passes_command_to_login_shell() {
        let cmd = run_command("/bin/zsh", Path::new("/tmp"), "cargo run");
        let argv = cmd.get_argv();
        assert_eq!(argv[0], OsString::from("/bin/zsh"));
        assert_eq!(argv[1], OsString::from("-l"));
        assert_eq!(argv[2], OsString::from("-c"));
        assert_eq!(argv[3], OsString::from("cargo run"));
        assert_eq!(cmd.get_cwd(), Some(&OsString::from("/tmp")));
        assert_eq!(cmd.get_env("TERM"), Some(std::ffi::OsStr::new(TERM)));
    }

    #[test]
    fn agent_invocation_runs_the_cli_with_the_prompt_env() {
        assert_eq!(
            agent_invocation("claude"),
            "claude \"$HELM_REVIEW_PROMPT\"\n"
        );
        // Configured flags are preserved before the quoted prompt argument.
        assert_eq!(
            agent_invocation("claude --model opus"),
            "claude --model opus \"$HELM_REVIEW_PROMPT\"\n"
        );
    }
}
