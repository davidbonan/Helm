//! Business E2E for phone access (specs/remote.md): a real server on the loopback,
//! real panes, a real WebSocket client — and the one assertion only the system
//! can answer (the sleep assertion).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use helm::agent_watch::watcher::{AgentWatcher, WatchedPane};
use helm::remote::awake::{KeepAwake, REASON};
use helm::remote::registry::{ExposedPane, Registry};
use helm::remote::server::PhoneServer;
use helm::terminal::palette::TermPalette;
use helm::terminal::pane::Pane;
use portable_pty::CommandBuilder;
use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

use crate::agent_watch_e2e::{compile_agent, teardown, wait_until};

/// An agent that echoes its input: what the phone types shows up on its screen.
const ECHO_AGENT: &str = "#include <unistd.h>\nint main(void){char b[256];ssize_t n;\
while((n=read(0,b,sizeof b))>0)write(1,b,n);return 0;}\n";

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
    _watcher: AgentWatcher,
    server: PhoneServer,
    _dir: tempfile::TempDir,
}

impl Fixture {
    /// An echo agent and a plain `cat`, both published, served on the loopback.
    fn new() -> Self {
        let awake = awake_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = compile_agent(dir.path(), "claude", ECHO_AGENT);
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
            Some(watcher.view()),
            TermPalette::dark(),
        );
        let server = PhoneServer::start_on_address([127, 0, 0, 1].into(), registry).unwrap();
        Self {
            _awake: awake,
            agent,
            shell,
            _watcher: watcher,
            server,
            _dir: dir,
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
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let mut request = format!("ws://{host}/ws").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Cookie", cookie.parse().unwrap());
        request
            .headers_mut()
            .insert("Origin", self.origin().parse().unwrap());
        tungstenite::client::client(request, stream).unwrap().0
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
    let mut found = None;
    wait_until(|| {
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
fn a_paired_phone_lists_the_agent_only_and_types_into_it() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);
    let agent_id = fixture.agent.uid().get();

    let agents = wait_for(&mut ws, "agents", |m| {
        m["agents"].as_array().is_some_and(|a| !a.is_empty())
    });
    ws.send(Message::text(
        json!({"type": "watch", "id": agent_id}).to_string(),
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

fn power_assertions() -> String {
    let out = std::process::Command::new("pmset")
        .args(["-g", "assertions"])
        .output()
        .expect("pmset ships with macOS");
    String::from_utf8_lossy(&out.stdout).into_owned()
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
