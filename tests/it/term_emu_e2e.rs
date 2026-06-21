use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use helm::terminal::emu::{line_text, scrollback_len, Emulator, PtyWriter, SCROLLBACK_LINES};
use helm::terminal::pty::Pty;
use portable_pty::{CommandBuilder, PtySize};

fn writer_of(pty: &Pty) -> PtyWriter {
    Arc::new(Mutex::new(pty.take_writer().unwrap()))
}

fn size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn wait_for<F: Fn() -> bool>(predicate: F) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    predicate()
}

#[test]
fn pushed_bytes_appear_on_grid_line_zero_and_trigger_change() {
    let mut cmd = CommandBuilder::new("printf");
    cmd.arg("hi\\r\\n");
    let mut pty = Pty::spawn(cmd, size()).unwrap();

    let changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&changes);
    let mut emu = Emulator::spawn(pty.reader().unwrap(), 24, 80, writer_of(&pty), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let term = Arc::clone(emu.term());
    let saw_hi = wait_for(|| line_text(&term.lock(), 0).trim_end() == "hi");

    pty.child().wait().unwrap();
    emu.join();

    assert!(saw_hi, "grid line 0 should contain \"hi\"");
    assert!(
        changes.load(Ordering::SeqCst) >= 1,
        "on_change should fire on grid mutation"
    );
}

#[test]
fn pty_output_feeds_scrollback_bounded_by_ten_thousand_lines() {
    let emitted = SCROLLBACK_LINES + 5_000;
    let mut cmd = CommandBuilder::new("yes");
    cmd.arg("");
    let mut pty = Pty::spawn(cmd, size()).unwrap();
    let mut emu = Emulator::spawn(pty.reader().unwrap(), 24, 80, writer_of(&pty), |_| {});

    let term = Arc::clone(emu.term());
    let saturated = wait_for(|| scrollback_len(&term.lock()) >= SCROLLBACK_LINES);

    pty.child().kill().unwrap();
    pty.child().wait().unwrap();
    emu.join();

    assert!(
        saturated,
        "PTY output should feed the scrollback up to its 10000-line cap"
    );
    assert_eq!(
        scrollback_len(&term.lock()),
        SCROLLBACK_LINES,
        "scrollback must be bounded at 10000 lines (emitted {emitted})"
    );
}
