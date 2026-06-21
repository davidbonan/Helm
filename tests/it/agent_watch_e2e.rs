//! Business E2E for agent detection (specs/agents.md): process probe on a real
//! PTY (layer A) and activity stamps of a real `Pane` (layer B). The agent is
//! simulated by a binary compiled on the fly under a watchlist name — copying
//! `/bin/cat` does not work (AMFI kills copies of arm64e platform binaries) and
//! a shebang script takes the comm of its interpreter.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use helm::agent_watch::probe;
use helm::terminal::pane::Pane;
use portable_pty::CommandBuilder;

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

fn fake_agent_named(dir: &Path, name: &str) -> PathBuf {
    let src = dir.join("agent.c");
    std::fs::write(
        &src,
        "#include <unistd.h>\nint main(void){pause();return 0;}\n",
    )
    .unwrap();
    let bin = dir.join(name);
    let status = std::process::Command::new("cc")
        .arg("-o")
        .arg(&bin)
        .arg(&src)
        .status()
        .expect("cc is present on any machine able to build the project");
    assert!(status.success(), "fake agent compilation failed");
    bin
}

fn fake_agent(dir: &Path) -> PathBuf {
    fake_agent_named(dir, "claude")
}

#[test]
fn a_fake_agent_is_detected_in_the_foreground_group() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = fake_agent(tmp.path());
    let pane = Pane::from_command(CommandBuilder::new(&bin), 24, 80, || {}).unwrap();

    let detected = wait_until(|| pane.foreground_pgid().is_some_and(probe::agent_in_group));

    teardown(pane);
    assert!(
        detected,
        "a foreground process named after a watchlist agent should be detected"
    );
}

#[test]
fn foreground_agent_returns_the_watchlist_name() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = fake_agent_named(tmp.path(), "codex");
    let pane = Pane::from_command(CommandBuilder::new(&bin), 24, 80, || {}).unwrap();

    let mut name = None;
    let found = wait_until(|| {
        name = pane.foreground_pgid().and_then(probe::foreground_agent);
        name.is_some()
    });

    teardown(pane);
    assert!(found, "the named agent should be classified");
    assert_eq!(
        name,
        Some("codex"),
        "the dashboard/notification needs the watchlist name, not just presence"
    );
}

#[test]
fn a_versioned_agent_behind_a_symlink_is_detected() {
    // Native Claude Code installer: `~/.local/bin/claude` is a symlink to
    // `versions/<x.y.z>` (file named after the version) ⇒ the kernel derives
    // p_comm from the resolved binary ("2.1.162", off the watchlist). The
    // invoked name only survives in argv[0].
    let tmp = tempfile::tempdir().unwrap();
    let versions = tmp.path().join("versions");
    std::fs::create_dir_all(&versions).unwrap();
    let real = fake_agent_named(&versions, "2.1.162");
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let link = bin_dir.join("claude");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let pane = Pane::from_command(CommandBuilder::new(&link), 24, 80, || {}).unwrap();

    let detected = wait_until(|| pane.foreground_pgid().is_some_and(probe::agent_in_group));

    teardown(pane);
    assert!(
        detected,
        "a versioned binary invoked through a `claude` symlink should be detected"
    );
}

#[test]
fn a_plain_process_is_not_an_agent() {
    let pane = Pane::from_command(CommandBuilder::new("cat"), 24, 80, || {}).unwrap();

    let has_foreground = wait_until(|| pane.foreground_pgid().is_some());
    assert!(has_foreground, "the PTY should have a foreground group");
    let agent = probe::agent_in_group(pane.foreground_pgid().unwrap());

    teardown(pane);
    assert!(!agent, "`cat` is not on the watchlist");
}

#[test]
fn probe_reads_comm_and_argv_of_the_foreground_group() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = fake_agent(tmp.path());
    let mut cmd = CommandBuilder::new(&bin);
    cmd.arg("--marker");
    let pane = Pane::from_command(cmd, 24, 80, || {}).unwrap();

    let mut member = None;
    let found = wait_until(|| {
        let pgid = match pane.foreground_pgid() {
            Some(p) => p,
            None => return false,
        };
        member = probe::group_comms(pgid)
            .into_iter()
            .find(|(_, comm)| comm == "claude");
        member.is_some()
    });
    assert!(found, "the fake agent should appear in its group's comms");

    let (pid, _) = member.unwrap();
    let argv = probe::argv(pid);
    teardown(pane);

    assert!(
        argv.first().is_some_and(|a| a.ends_with("claude")),
        "argv[0] should be the fake agent path (got {argv:?})"
    );
    assert_eq!(
        argv.get(1).map(String::as_str),
        Some("--marker"),
        "arguments should be readable for our own children"
    );
}

#[test]
fn feed_stamps_input_and_its_echo_is_not_spontaneous_output() {
    let outputs = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&outputs);
    let pane = Pane::from_command(CommandBuilder::new("cat"), 24, 80, move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    // on_change only fires for on-screen panes; the render path sets this in the app.
    pane.set_visible(true);
    assert_eq!(pane.activity().snapshot().last_input_ms, 0);

    pane.input(b"x").unwrap();
    let stamped = pane.activity().snapshot().last_input_ms;
    assert!(stamped > 0, "feed should stamp last_input");

    // The output callback fires right after `stamp_output` (pane::callback), so a
    // fired callback means `cat`'s echo has been classified. Inside the echo
    // window it must not count as spontaneous output.
    let processed = wait_until(|| outputs.load(Ordering::SeqCst) >= 1);
    let snapshot = pane.activity().snapshot();
    teardown(pane);
    assert!(
        processed,
        "cat should echo the keystroke back through the emulator"
    );
    assert_eq!(
        snapshot.last_spont_output_ms, 0,
        "the echo of a keystroke is not spontaneous output"
    );
}

#[test]
fn output_without_recent_input_is_stamped_as_spontaneous() {
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    // The initial sleep ensures no echo window is in play, and that the
    // "nothing before" assertion is observable.
    cmd.arg("sleep 0.5; echo agent-output; sleep 30");
    let pane = Pane::from_command(cmd, 24, 80, || {}).unwrap();
    assert_eq!(pane.activity().snapshot().last_spont_output_ms, 0);

    let stamped = wait_until(|| pane.activity().snapshot().last_spont_output_ms > 0);

    teardown(pane);
    assert!(stamped, "output with no input around is spontaneous");
}
