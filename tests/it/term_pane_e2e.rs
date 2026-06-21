use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use helm::terminal::cwd::live_cwd;
use helm::terminal::emu::{line_text, ReplyListener};
use helm::terminal::pane::{CursorPos, Pane};
use portable_pty::CommandBuilder;

fn wait_for<F: Fn() -> bool>(predicate: F) -> bool {
    wait_until(predicate)
}

fn wait_until<F: FnMut() -> bool>(mut predicate: F) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    predicate()
}

fn teardown(mut pane: Pane) {
    pane.child().kill().unwrap();
    pane.child().wait().unwrap();
    pane.join();
}

#[test]
fn keystroke_is_written_to_pty_and_surfaces_on_grid() {
    let changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&changes);
    let pane = Pane::from_command(CommandBuilder::new("cat"), 24, 80, move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    // on_change only fires for on-screen panes; the render path sets this in the app.
    pane.set_visible(true);

    pane.input(b"hello\r\n").unwrap();

    let term = Arc::clone(pane.grid());
    let saw_hello = wait_for(|| line_text(&term.lock(), 0).trim_end() == "hello");

    teardown(pane);

    assert!(
        saw_hello,
        "keystroke should be echoed back onto grid line 0"
    );
    assert!(
        changes.load(Ordering::SeqCst) >= 1,
        "grid mutation should notify on_change"
    );
}

#[test]
fn paste_writes_text_to_pty_and_surfaces_on_grid() {
    let pane = Pane::from_command(CommandBuilder::new("cat"), 24, 80, || {}).unwrap();

    pane.paste("pasted-text").unwrap();
    pane.input(b"\r\n").unwrap();

    let term = Arc::clone(pane.grid());
    let saw_paste = wait_for(|| line_text(&term.lock(), 0).trim_end() == "pasted-text");

    teardown(pane);

    assert!(
        saw_paste,
        "pasted text should reach the PTY and echo onto grid line 0"
    );
}

#[test]
fn cursor_advances_with_input() {
    let pane = Pane::from_command(CommandBuilder::new("cat"), 24, 80, || {}).unwrap();

    pane.input(b"abc").unwrap();
    let advanced = wait_for(|| pane.cursor() == CursorPos { line: 0, col: 3 });

    teardown(pane);

    assert!(
        advanced,
        "cursor should advance to column 3 after echoing \"abc\""
    );
}

#[test]
fn resize_reflows_grid_and_pty() {
    let mut pane = Pane::from_command(CommandBuilder::new("cat"), 24, 80, || {}).unwrap();

    pane.resize(40, 120).unwrap();

    let term = Arc::clone(pane.grid());
    {
        let guard = term.lock();
        assert_eq!(guard.grid().columns(), 120);
        assert_eq!(guard.grid().screen_lines(), 40);
    }

    teardown(pane);
}

#[test]
fn drop_kills_foreground_process_group() {
    let tmp = tempfile::tempdir().unwrap();
    let pane = Pane::open(tmp.path(), 24, 80, || {}).unwrap();
    let shell_pgid = pane.foreground_pgid();

    pane.input(b"sleep 1000\r\n").unwrap();
    let mut job_pgid = None;
    wait_until(|| {
        job_pgid = pane
            .foreground_pgid()
            .filter(|pgid| Some(*pgid) != shell_pgid);
        job_pgid.is_some()
    });
    let job_pgid = job_pgid.expect("sleep should become the foreground job");

    drop(pane);

    // kill(pid, 0) probes existence; the zombie disappears once reaped by launchd.
    let job_gone = wait_until(|| unsafe { libc::kill(job_pgid, 0) } == -1);
    assert!(
        job_gone,
        "foreground job (pgid {job_pgid}) should be killed when the pane is dropped"
    );
}

#[test]
fn live_cwd_tracks_the_shell_after_cd() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let canonical_a = a.path().canonicalize().unwrap();
    let canonical_b = b.path().canonicalize().unwrap();

    let pane = Pane::open(a.path(), 24, 80, || {}).unwrap();
    let pid = pane.shell_pid().expect("login shell should expose a pid");

    assert_eq!(
        pane.spawn_cwd(),
        a.path(),
        "spawn_cwd reports the directory the shell was launched in"
    );

    let at_a = wait_until(|| live_cwd(pid).as_deref() == Some(canonical_a.as_path()));
    assert!(
        at_a,
        "live cwd should start at the spawn directory {canonical_a:?}"
    );

    pane.input(format!("cd {}\r\n", canonical_b.display()).as_bytes())
        .unwrap();
    let moved = wait_until(|| live_cwd(pid).as_deref() == Some(canonical_b.as_path()));

    teardown(pane);
    assert!(
        moved,
        "live cwd should follow the shell into {canonical_b:?}"
    );
}

#[test]
fn live_cwd_returns_none_for_an_unreadable_pid() {
    // pid 0 is the kernel scheduler: proc_pidinfo cannot read its vnode info, so
    // the caller falls back to the pane's spawn cwd.
    assert!(live_cwd(0).is_none());
}

fn grid_contains(term: &Arc<FairMutex<Term<ReplyListener>>>, needle: &str) -> bool {
    let guard = term.lock();
    let lines = guard.grid().screen_lines() as i32;
    (0..lines).any(|line| line_text(&guard, line).contains(needle))
}

/// End-to-end kitty negotiation: the program sends the `CSI ? u` query on its
/// tty and must receive the `CSI ? 0 u` reply on stdin — this is what makes
/// Claude Code / Codex enable the protocol (Shift+Enter).
#[test]
fn kitty_keyboard_query_receives_a_reply_on_the_pty() {
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg(r"stty raw -echo; printf '\033[?u'; dd bs=1 count=5 2>/dev/null | cat -v");
    let pane = Pane::from_command(cmd, 24, 80, || {}).unwrap();

    let term = Arc::clone(pane.grid());
    let saw_reply = wait_for(|| grid_contains(&term, "[?0u"));

    teardown(pane);

    assert!(
        saw_reply,
        "the emulator should answer the kitty keyboard query through the PTY writer"
    );
}

#[test]
fn exit_marks_process_ended_then_relaunch_restores_same_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical = tmp.path().canonicalize().unwrap();
    let canonical = canonical.to_string_lossy().into_owned();

    let mut pane = Pane::open(tmp.path(), 24, 80, || {}).unwrap();
    assert!(!pane.has_exited(), "freshly opened shell should be running");

    pane.input(b"exit\r\n").unwrap();
    let exited = wait_until(|| pane.has_exited());
    assert!(exited, "shell should terminate after `exit`");

    pane.relaunch().unwrap();
    assert!(!pane.has_exited(), "relaunched shell should be running");

    pane.input(b"pwd\r\n").unwrap();
    let term = Arc::clone(pane.grid());
    let same_cwd = wait_for(|| grid_contains(&term, &canonical));

    teardown(pane);

    assert!(
        same_cwd,
        "relaunched shell should report the original cwd ({canonical})"
    );
}
