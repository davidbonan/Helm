use std::io::{Read, Write};

use helm::terminal::pty::{login_shell_command, run_command, shell_program, Pty};
use portable_pty::{CommandBuilder, PtySize};

fn size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn read_all(pty: &Pty) -> String {
    let mut reader = pty.reader().unwrap();
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
    output
}

#[test]
fn spawn_runs_command_and_captures_output() {
    let mut cmd = CommandBuilder::new("printf");
    cmd.arg("hello-spawn");
    let mut pty = Pty::spawn(cmd, size()).unwrap();
    let output = read_all(&pty);
    pty.child().wait().unwrap();
    assert!(output.contains("hello-spawn"), "got: {output:?}");
}

#[test]
fn run_command_executes_in_the_worktree_then_exits() {
    // The Run strip's lifecycle (git.md §3): the configured command runs in a
    // login shell rooted at the worktree, then the shell exits — EOF on the
    // reader is what `read_all` waits for, mirroring a finished `cargo run`.
    let cwd = std::env::temp_dir();
    let cmd = run_command(shell_program(), &cwd, "echo run-marker");
    let mut pty = Pty::spawn(cmd, size()).unwrap();
    let output = read_all(&pty);
    pty.child().wait().unwrap();
    assert!(output.contains("run-marker"), "got: {output:?}");
}

#[test]
fn resize_propagates_new_winsize_to_the_child() {
    // `read x` parks the shell until we feed it a byte; the resize lands before
    // that byte, so `stty size` runs strictly after it and observes the new size.
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("read x; stty size");
    let mut pty = Pty::spawn(cmd, size()).unwrap();

    pty.resize(40, 120).unwrap();
    pty.take_writer().unwrap().write_all(b"\n").unwrap();

    let output = read_all(&pty);
    pty.child().wait().unwrap();
    assert!(
        output.contains("40 120"),
        "child should observe the resized winsize, got: {output:?}"
    );
}

#[test]
fn login_shell_inherits_term_and_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = login_shell_command("/bin/zsh", tmp.path());
    cmd.arg("-c");
    cmd.arg("printf '%s\\n%s\\n' \"$TERM\" \"$PWD\"");
    let mut pty = Pty::spawn(cmd, size()).unwrap();
    let output = read_all(&pty);
    pty.child().wait().unwrap();

    let canonical = tmp.path().canonicalize().unwrap();
    assert!(output.contains("xterm-256color"), "got: {output:?}");
    assert!(
        output.contains(&canonical.to_string_lossy().into_owned()),
        "cwd not honored, got: {output:?}"
    );
}
