use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use helm::terminal::pane::Pane;
use helm::terminal::pty::run_command;
use helm::workflow::definition::{Step, StepKind};
use helm::workflow::hooks::{Hook, RUN_ENV};
use helm::workflow::run::RunState;
use helm::workflow::runner::{RunOrigin, RunnerConfig, WorkflowRunner};

/// Stands in for Claude: records its argv and run id, turns bracketed paste on like
/// the TUI does, then records what the PTY receives into `stdin.log`.
fn fake_claude(dir: &Path) -> PathBuf {
    let path = dir.join("claude");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{d}/argv.log'\nprintf '%s' \"${RUN_ENV}\" > '{d}/env.log'\nprintf '\\033[?2004h'\nexec cat > '{d}/stdin.log'\n",
        d = dir.display()
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    path
}

fn prompt(text: &str) -> Step {
    Step {
        title: text.to_owned(),
        kind: StepKind::Prompt {
            text: text.to_owned(),
        },
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn wait_for(mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn marked(turn: &str) -> Hook {
    Hook::Stop {
        turn: turn.to_owned(),
        last_message: Some("Done.\n\n[[STEP_DONE]]".to_owned()),
    }
}

#[test]
fn a_run_launches_claude_with_its_hooks_then_pastes_each_step_on_a_marked_stop() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let runner = WorkflowRunner::new(
        RunnerConfig {
            dir: dir.join("workflows"),
            helm_exe: PathBuf::from("/Apps/helm"),
            claude: fake_claude(dir).display().to_string(),
        },
        || {},
    );
    let origin = RunOrigin {
        name: "Ticket".to_owned(),
        repo: "helm".to_owned(),
        branch: "main".to_owned(),
    };
    let steps = vec![prompt("Implement it"), prompt("Review it\nthoroughly")];

    let (id, mut pane) = runner
        .start(steps, origin, |launch| {
            let mut cmd = run_command("/bin/sh", dir, launch.command);
            cmd.env(RUN_ENV, launch.run_id);
            let pane = Pane::from_command(cmd, 24, 80, || {})?;
            let handle = pane.handle();
            Ok((pane, handle))
        })
        .unwrap();

    let settings = dir.join("workflows").join(format!("{id}.json"));
    assert!(wait_for(
        || read(&dir.join("argv.log")).contains("Implement it")
    ));
    assert_eq!(
        read(&dir.join("argv.log")),
        format!("--settings\n{}\nImplement it\n", settings.display())
    );
    assert_eq!(read(&dir.join("env.log")), id);
    assert!(read(&settings).contains("'/Apps/helm' workflow-event stop"));

    runner.deliver(&id, marked("t1"));
    assert!(wait_for(
        || read(&dir.join("stdin.log")).contains("thoroughly")
    ));
    let pasted = read(&dir.join("stdin.log"));
    assert!(
        pasted.contains("\u{1b}[200~Review it\nthoroughly\u{1b}[201~"),
        "{pasted:?}"
    );
    assert_eq!(runner.snapshot(&id).unwrap().step_index(), 1);

    runner.deliver(&id, marked("t2"));
    assert_eq!(runner.snapshot(&id).unwrap().state(), RunState::Done);
    assert!(!settings.exists(), "settings file left behind");

    pane.child().kill().unwrap();
    pane.child().wait().unwrap();
    pane.join();
}

#[test]
fn an_unknown_run_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = WorkflowRunner::new(
        RunnerConfig {
            dir: tmp.path().to_path_buf(),
            helm_exe: PathBuf::from("/Apps/helm"),
            claude: "claude".to_owned(),
        },
        || panic!("no run changed"),
    );
    runner.deliver("nope", marked("t1"));
    assert!(runner.snapshot("nope").is_none());
}
