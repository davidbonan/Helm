//! Business E2E for phone access (specs/remote.md): a real server on the loopback,
//! real panes, a real WebSocket client — and the one assertion only the system
//! can answer (the sleep assertion).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use alacritty_terminal::grid::Dimensions;

use helm::agent_watch::watcher::{AgentWatcher, WatchedPane};
use helm::remote::awake::{KeepAwake, REASON};
use helm::remote::launch::{LaunchAgent, LaunchTarget, LaunchTargets, LaunchedPane, Launcher};
use helm::remote::registry::{ExposedPane, Registry};
use helm::remote::server::PhoneServer;
use helm::terminal::pane::Pane;
use helm::theme;
use portable_pty::CommandBuilder;
use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

use crate::agent_watch_e2e::{compile_agent, teardown, wait_until, wait_until_within};

/// A full-screen agent (alt screen, like Claude Code) that echoes its input: what
/// the phone types shows up on its screen.
const ECHO_AGENT: &str = "#include <unistd.h>\nint main(void){char b[256];ssize_t n;\
write(1,\"\\033[?1049h\",8);while((n=read(0,b,sizeof b))>0)write(1,b,n);return 0;}\n";

/// An agent that works for ~3 s then falls silent: a finished turn, not yet seen.
const TURN_AGENT: &str = "#include <unistd.h>\nint main(void){static const char s[]=\
\"working on the answer...\\r\\n\";for(int i=0;i<30;i++){write(1,s,sizeof s-1);\
usleep(100000);}pause();return 0;}\n";

/// Every server holds the same sleep assertion: the `pmset` check must run alone.
static AWAKE: Mutex<()> = Mutex::new(());

fn awake_lock() -> MutexGuard<'static, ()> {
    AWAKE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Fixture {
    _awake: MutexGuard<'static, ()>,
    agent: Pane,
    shell: Pane,
    /// Dropped before the watcher: it holds a link that keeps the watcher's thread alive.
    registry: Registry,
    _watcher: AgentWatcher,
    server: PhoneServer,
    launches: crossbeam_channel::Receiver<LaunchedPane>,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with_agent(ECHO_AGENT)
    }

    /// An agent compiled from `source` and a plain `cat`, both published, served on
    /// the loopback.
    fn with_agent(source: &str) -> Self {
        let awake = awake_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = compile_agent(dir.path(), "claude", source);
        let agent = Pane::from_command(CommandBuilder::new(&bin), 24, 80, || {}).unwrap();
        let shell = Pane::from_command(CommandBuilder::new("cat"), 24, 80, || {}).unwrap();
        let watcher = AgentWatcher::spawn(|| {});
        watcher.track(
            vec![WatchedPane::of(&agent), WatchedPane::of(&shell)],
            HashSet::new(),
        );
        let registry = Registry::default();
        registry.publish(
            vec![exposed(&agent, "Tab 1"), exposed(&shell, "Tab 2")],
            Some(watcher.link()),
            theme::preset("helm", true),
        );
        let (launcher, launches) = Launcher::channel(|| {});
        let server =
            PhoneServer::start_on_address([127, 0, 0, 1].into(), registry.clone(), launcher)
                .unwrap();
        Self {
            _awake: awake,
            agent,
            shell,
            registry,
            _watcher: watcher,
            server,
            launches,
            dir,
        }
    }

    fn origin(&self) -> String {
        let url = self.server.pairing_url();
        url[..url.find("/pair").unwrap()].to_owned()
    }

    fn get(&self, path_and_query: &str, cookie: Option<&str>) -> String {
        let host = self.origin().trim_start_matches("http://").to_owned();
        let mut stream = TcpStream::connect(&host).unwrap();
        let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {c}\r\n"));
        write!(
            stream,
            "GET {path_and_query} HTTP/1.1\r\nHost: {host}\r\n{cookie}\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    /// Pairs and returns the `name=value` session cookie.
    fn pair(&self) -> String {
        let url = self.server.pairing_url();
        let response = self.get(&url[url.find("/pair").unwrap()..], None);
        let set_cookie = response
            .lines()
            .find_map(|line| line.strip_prefix("Set-Cookie: "))
            .expect("a pairing sets the session cookie");
        set_cookie.split(';').next().unwrap().to_owned()
    }

    fn connect(&self, cookie: &str) -> WebSocket<TcpStream> {
        let host = self.origin().trim_start_matches("http://").to_owned();
        let stream = TcpStream::connect(&host).unwrap();
        let mut request = format!("ws://{host}/ws").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Cookie", cookie.parse().unwrap());
        request
            .headers_mut()
            .insert("Origin", self.origin().parse().unwrap());
        let ws = tungstenite::client::client(request, stream).unwrap().0;
        // Polling timeout set after the handshake: a loaded CI runner answers the upgrade late.
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        ws
    }

    fn close(self) {
        drop(self.server);
        teardown(self.agent);
        teardown(self.shell);
    }
}

fn exposed(pane: &Pane, tab: &str) -> ExposedPane {
    ExposedPane {
        uid: pane.uid(),
        project: "api".to_owned(),
        branch: Some("main".to_owned()),
        tab: tab.to_owned(),
        handle: pane.handle(),
    }
}

/// The next message of `kind` satisfying `accept`, within the helper's deadline.
fn wait_for(
    ws: &mut WebSocket<TcpStream>,
    kind: &str,
    accept: impl Fn(&Value) -> bool,
) -> Option<Value> {
    wait_for_within(ws, Duration::from_secs(5), kind, accept)
}

fn wait_for_within(
    ws: &mut WebSocket<TcpStream>,
    timeout: Duration,
    kind: &str,
    accept: impl Fn(&Value) -> bool,
) -> Option<Value> {
    let mut found = None;
    wait_until_within(timeout, || {
        if let Ok(Message::Text(text)) = ws.read() {
            let message: Value = serde_json::from_str(&text).unwrap();
            if message["type"] == kind && accept(&message) {
                found = Some(message);
            }
        }
        found.is_some()
    });
    found
}

#[test]
fn pairing_trades_the_token_for_a_session_cookie() {
    let fixture = Fixture::new();
    let url = fixture.server.pairing_url().to_owned();

    let response = fixture.get(&url[url.find("/pair").unwrap()..], None);

    fixture.close();
    assert!(response.starts_with("HTTP/1.1 303"), "{response}");
    assert!(
        response.contains("Location: /\r\n"),
        "the token leaves the address bar"
    );
    assert!(response.contains("Set-Cookie: helm_session="));
    assert!(response.contains("HttpOnly; SameSite=Strict"));
}

#[test]
fn an_unpaired_phone_gets_the_access_stopped_page() {
    let fixture = Fixture::new();

    let page = fixture.get("/", None);
    let wrong_token = fixture.get("/pair?t=00", None);

    fixture.close();
    assert!(page.starts_with("HTTP/1.1 401"), "{page}");
    assert!(page.contains("Access stopped"));
    assert!(wrong_token.starts_with("HTTP/1.1 401"));
}

#[test]
fn the_page_wears_helm_theme_from_its_first_paint() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();

    let page = fixture.get("/", Some(&cookie));
    let mut ws = fixture.connect(&cookie);
    let theme = wait_for(&mut ws, "theme", |_| true);

    fixture.close();
    assert!(page.contains("data-theme=\"dark\""), "{page}");
    assert!(
        page.contains("--canvas:#19222d;"),
        "the palette is inlined, not left to the phone's scheme"
    );
    let theme = theme.expect("the socket sends helm's theme on connect");
    assert_eq!(theme["tokens"]["canvas"], "#19222d");
}

#[test]
fn a_paired_phone_lists_the_agent_only_and_types_into_it() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);
    let agent_id = fixture.agent.uid().get();

    let agents = wait_for(&mut ws, "agents", |m| {
        m["agents"].as_array().is_some_and(|a| !a.is_empty())
    });
    ws.send(Message::text(
        json!({"type": "watch", "id": agent_id, "rows": 30, "cols": 50}).to_string(),
    ))
    .unwrap();
    ws.send(Message::text(
        json!({"type": "send", "id": agent_id, "text": "hello phone"}).to_string(),
    ))
    .unwrap();
    let echoed = wait_for(&mut ws, "screen", |m| m.to_string().contains("hello phone"));

    fixture.close();
    let listed: Vec<u64> = agents.expect("the agent is listed")["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_u64().unwrap())
        .collect();
    assert_eq!(listed, [agent_id], "the plain `cat` pane is never exposed");
    let screen = echoed.expect("the typed text reaches the agent's PTY and its screen");
    assert_eq!(screen["writable"], true);
}

/// The fixture's directory as the only entry, the echo agent (named `claude`, so
/// the watcher knows it) as the only agent.
fn offer_the_echo_agent(fixture: &Fixture) -> u64 {
    let entry = LaunchTarget::new(
        fixture.dir.path().to_path_buf(),
        "api".to_owned(),
        Some("main".to_owned()),
        false,
    );
    let entry_id = entry.id;
    let command = fixture.dir.path().join("claude").display().to_string();
    fixture.registry.publish_targets(LaunchTargets {
        entries: vec![entry],
        agents: vec![LaunchAgent::new("Claude Code", &command)],
    });
    entry_id
}

#[test]
fn a_phone_launch_runs_the_agent_with_no_ui_frame() {
    let fixture = Fixture::new();
    let entry_id = offer_the_echo_agent(&fixture);
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);

    let targets = wait_for(&mut ws, "targets", |m| m["entries"][0]["id"] == entry_id);
    ws.send(Message::text(
        json!({"type": "launch", "entry": entry_id, "agent": 0, "rows": 30, "cols": 50})
            .to_string(),
    ))
    .unwrap();
    let launched = wait_for(&mut ws, "launched", |_| true).expect("the launch answers its id");
    let id = launched["id"].as_u64().unwrap();
    ws.send(Message::text(
        json!({"type": "watch", "id": id, "rows": 30, "cols": 50}).to_string(),
    ))
    .unwrap();
    let listed =
        wait_for_within(&mut ws, Duration::from_secs(15), "agents", |m| {
            m["agents"].as_array().unwrap().iter().any(|row| {
                row["id"] == id && row["tab"] == "Claude Code" && row["project"] == "api"
            })
        });
    ws.send(Message::text(
        json!({"type": "send", "id": id, "text": "hello launch"}).to_string(),
    ))
    .unwrap();
    let echoed = wait_for(&mut ws, "screen", |m| {
        m["id"] == id && m.to_string().contains("hello launch")
    });
    let adopted = fixture.launches.try_recv();

    let entry = targets.expect("the phone receives what it may launch")["entries"][0].clone();
    assert!(entry.get("path").is_none(), "the phone never sees a path");
    assert!(
        listed.is_some(),
        "the launched agent is listed with its badge"
    );
    assert!(echoed.is_some(), "the phone types into the launched agent");
    let adopted = adopted.expect("the pane is handed to the UI");
    assert_eq!(adopted.entry, fixture.dir.path());
    assert_eq!(adopted.pane.uid().get(), id);
    teardown(adopted.pane);
    fixture.close();
}

#[test]
fn a_launch_of_an_unknown_entry_or_agent_fails() {
    let fixture = Fixture::new();
    let entry_id = offer_the_echo_agent(&fixture);
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);

    let mut failures = Vec::new();
    for (entry, agent) in [(entry_id.wrapping_add(1), 0), (entry_id, 1)] {
        ws.send(Message::text(
            json!({"type": "launch", "entry": entry, "agent": agent, "rows": 30, "cols": 50})
                .to_string(),
        ))
        .unwrap();
        failures.push(wait_for(&mut ws, "launch_failed", |_| true));
    }

    let nothing_spawned = fixture.launches.try_recv().is_err();

    fixture.close();
    assert!(failures.iter().all(Option::is_some), "{failures:?}");
    assert!(nothing_spawned);
}

fn watch(ws: &mut WebSocket<TcpStream>, agent_id: u64) {
    wait_for(ws, "agents", |m| {
        m["agents"].as_array().is_some_and(|a| !a.is_empty())
    });
    ws.send(Message::text(
        json!({"type": "watch", "id": agent_id, "rows": 30, "cols": 50}).to_string(),
    ))
    .unwrap();
}

#[test]
fn a_swipe_reaches_a_full_screen_agent_as_the_wheel() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);
    let agent_id = fixture.agent.uid().get();

    watch(&mut ws, agent_id);
    let full_screen = wait_for(&mut ws, "screen", |m| m["app_scrolls"] == true);
    ws.send(Message::text(
        json!({"type": "scroll", "id": agent_id, "lines": 2, "line": 3, "col": 4}).to_string(),
    ))
    .unwrap();
    // The PTY echoes the control bytes it receives as `^[`.
    let scrolled = wait_for(&mut ws, "screen", |m| m.to_string().contains("^[[A^[[A"));

    fixture.close();
    assert!(
        full_screen.is_some(),
        "the frame tells the phone the app scrolls"
    );
    assert!(
        scrolled.is_some(),
        "two lines up reach the agent as two ↑ arrows"
    );
}

#[test]
fn watching_a_done_agent_on_the_phone_acknowledges_it() {
    let fixture = Fixture::with_agent(TURN_AGENT);
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);
    let agent_id = fixture.agent.uid().get();
    let done = wait_for_within(&mut ws, Duration::from_secs(20), "agents", |m| {
        m["agents"][0]["badge"] == "done"
    });

    watch(&mut ws, agent_id);
    let seen = wait_for(&mut ws, "agents", |m| m["agents"][0]["badge"] == "idle");

    fixture.close();
    assert!(done.is_some(), "the finished turn turns green");
    assert!(
        seen.is_some(),
        "the phone showing the agent acknowledges its green"
    );
}

fn columns(pane: &Pane) -> usize {
    pane.grid().lock().grid().columns()
}

#[test]
fn a_watching_phone_sizes_the_agent_until_it_leaves() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);
    let agent_id = fixture.agent.uid().get();
    wait_for(&mut ws, "agents", |m| {
        m["agents"].as_array().is_some_and(|a| !a.is_empty())
    });

    ws.send(Message::text(
        json!({"type": "watch", "id": agent_id, "rows": 30, "cols": 50}).to_string(),
    ))
    .unwrap();
    let phone_frame = wait_for(&mut ws, "screen", |m| m["cols"] == 50);
    let phone_columns = columns(&fixture.agent);
    ws.close(None).unwrap();
    let _ = ws.flush();
    let given_back = wait_until(|| columns(&fixture.agent) == 80);

    fixture.close();
    assert!(
        phone_frame.is_some(),
        "the phone gets frames at its own width"
    );
    assert_eq!(phone_columns, 50, "the agent's PTY takes the phone's size");
    assert!(
        given_back,
        "the Mac gets its size back once the phone leaves"
    );
}

/// This process's power assertions: a helm running beside the tests holds its own.
fn power_assertions() -> String {
    let out = std::process::Command::new("pmset")
        .args(["-g", "assertions"])
        .output()
        .expect("pmset ships with macOS");
    let ours = format!("pid {}(", std::process::id());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.contains(&ours))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn keep_awake_holds_a_sleep_assertion_until_dropped() {
    let _alone = awake_lock();
    let awake = KeepAwake::begin();
    let held = power_assertions().contains(REASON);

    drop(awake);
    let released = !power_assertions().contains(REASON);

    assert!(
        held,
        "the activity must prevent idle system sleep (pmset lists it)"
    );
    assert!(released, "ending the activity must let the Mac sleep again");
}
