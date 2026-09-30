//! The phone page against a fake agent, for the iOS simulator (.claude/skills/mobile):
//! `cargo run --example phone_preview -- [--light] [--loopback] [-- <agent command>...]` prints
//! the pairing URL; a command after `--` runs in the first pane, e.g. `claude --resume <id>`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use helm::agent_watch::watcher::{AgentWatcher, WatchedPane};
use helm::remote::registry::{ExposedPane, Registry};
use helm::remote::server::PhoneServer;
use helm::terminal::pane::Pane;
use helm::theme;
use portable_pty::CommandBuilder;

/// A full-screen agent (alt screen, like Claude Code) with a colored transcript, echoing its input.
const FAKE_AGENT: &str = r#"#include <unistd.h>
#include <string.h>
static void say(const char *s) { write(1, s, strlen(s)); }
int main(void) {
  say("\033[?1049h\033[H");
  say("\033[38;5;173m*\033[0m \033[1mWelcome to Claude Code\033[0m\r\n\r\n");
  say("\033[2m> \033[0mfix the phone keyboard glitch\r\n\r\n");
  say("\033[38;5;173m*\033[0m Reading \033[1msrc/remote/assets/app.js\033[0m\r\n");
  say("  \033[32m+ 12\033[0m \033[31m- 3\033[0m lines\r\n\r\n");
  say("\033[36mDo you want to apply this edit?\033[0m\r\n");
  say("  \033[1m1. Yes\033[0m\r\n  2. No, and tell Claude what to do\r\n\r\n");
  char b[256]; ssize_t n;
  while ((n = read(0, b, sizeof b)) > 0) write(1, b, n);
  return 0;
}
"#;

fn compile_agent(dir: &Path) -> PathBuf {
    let source = dir.join("agent.c");
    std::fs::write(&source, FAKE_AGENT).unwrap();
    let bin = dir.join("claude");
    let status = std::process::Command::new("cc")
        .arg("-o")
        .arg(&bin)
        .arg(&source)
        .status()
        .expect("cc compiles the fake agent");
    assert!(status.success(), "fake agent compilation failed");
    bin
}

fn exposed(pane: &Pane, project: &str, tab: &str) -> ExposedPane {
    ExposedPane {
        uid: pane.uid(),
        project: project.to_owned(),
        branch: Some("main".to_owned()),
        tab: tab.to_owned(),
        handle: pane.handle(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (flags, command) = match args.iter().position(|arg| arg == "--") {
        Some(split) => (&args[..split], &args[split + 1..]),
        None => (&args[..], &[][..]),
    };
    let has = |flag: &str| flags.iter().any(|arg| arg == flag);
    let dir = tempfile::tempdir().unwrap();
    let bin = compile_agent(dir.path());
    let fake = || CommandBuilder::new(&bin);
    let first = match command {
        [program, rest @ ..] => {
            let mut real = CommandBuilder::new(program);
            real.args(rest);
            real.cwd(std::env::current_dir().unwrap());
            real
        }
        [] => fake(),
    };
    let panes: Vec<Pane> = [first, fake(), fake()]
        .into_iter()
        .map(|cmd| Pane::from_command(cmd, 40, 60, || {}).unwrap())
        .collect();
    let watcher = AgentWatcher::spawn(|| {});
    watcher.track(panes.iter().map(WatchedPane::of).collect(), HashSet::new());
    let registry = Registry::default();
    registry.publish(
        vec![
            exposed(&panes[0], "helm-studio", "Tab 1"),
            exposed(&panes[1], "helm-studio", "Tab 2"),
            exposed(&panes[2], "avoda", "Tab 1"),
        ],
        Some(watcher.link()),
        theme::preset("helm", !has("--light")),
    );
    let server = if has("--loopback") {
        PhoneServer::start_on_address([127, 0, 0, 1].into(), registry).expect("bind the loopback")
    } else {
        PhoneServer::start(registry).expect("a LAN address, like the real phone")
    };
    println!("{}", server.pairing_url());
    for pane in &panes {
        println!("#/pane/{}", pane.uid().get());
    }
    loop {
        std::thread::park();
    }
}
