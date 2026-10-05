//! Business E2E for phone access (specs/remote.md): a real server on the loopback,
//! real panes, a real WebSocket client — and the one assertion only the system
//! can answer (the sleep assertion).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use alacritty_terminal::grid::Dimensions;

use helm::agent_watch::watcher::{AgentWatcher, WatchedPane};
use helm::agents::Agent;
use helm::remote::access::AccessAlert;
use helm::remote::address::Lan;
use helm::remote::awake::{KeepAwake, REASON};
use helm::remote::devices::PairedDevices;
use helm::remote::launch::{LaunchTarget, LaunchTargets, LaunchedPane, Launcher};
use helm::remote::registry::{ExposedPane, Registry};
use helm::remote::server::{PhoneServer, PhoneServices};
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
    registry: Registry,
    _watcher: AgentWatcher,
    devices: PairedDevices,
    alerts: Arc<Mutex<Vec<AccessAlert>>>,
    /// `None` only while restarting.
    server: Option<PhoneServer>,
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
        let worktree = dir.path().join("worktree");
        std::fs::create_dir(&worktree).unwrap();
        let registry = Registry::default();
        registry.publish(
            vec![
                exposed(&agent, "Tab 1", &worktree),
                exposed(&shell, "Tab 2", &worktree),
            ],
            Some(watcher.link()),
            theme::preset("helm", true),
        );
        let (launcher, launches) = Launcher::channel(|| {});
        let devices = PairedDevices::default();
        let alerts = Arc::new(Mutex::new(Vec::new()));
        let server = Some(start_server(&registry, launcher, &devices, &alerts));
        Self {
            _awake: awake,
            agent,
            shell,
            registry,
            _watcher: watcher,
            devices,
            alerts,
            server,
            launches,
            dir,
        }
    }

    fn origin(&self) -> String {
        self.server().origin().to_owned()
    }

    /// The path and query of a freshly offered pairing code.
    fn pairing_path(&self) -> String {
        let url = self.server().offer_pairing();
        url[url.find("/pair").unwrap()..].to_owned()
    }

    /// The directory both panes are published with.
    fn worktree(&self) -> PathBuf {
        self.dir.path().join("worktree")
    }

    fn server(&self) -> &PhoneServer {
        self.server.as_ref().expect("a running server")
    }

    /// A server restarted on the same devices' book, as after a Stop or a relaunch.
    fn restart_server(&mut self) {
        self.server = None;
        let (launcher, _) = Launcher::channel(|| {});
        self.server = Some(start_server(
            &self.registry,
            launcher,
            &self.devices,
            &self.alerts,
        ));
    }

    /// A server restarted on a network the test moves: what the returned value
    /// holds is what the server finds at its next look.
    fn restart_server_on_a_moving_lan(&mut self) -> Arc<Mutex<Lan>> {
        self.server = None;
        let (launcher, _) = Launcher::channel(|| {});
        let services = services(&self.registry, launcher, &self.devices, &self.alerts);
        let lan = Arc::new(Mutex::new(Lan::Address(LOOPBACK)));
        let found = Arc::clone(&lan);
        let locate = move |_| *found.lock().unwrap();
        self.server = Some(PhoneServer::start_following(LOOPBACK, locate, services).unwrap());
        lan
    }

    fn host(&self) -> String {
        self.origin().trim_start_matches("http://").to_owned()
    }

    fn get(&self, path_and_query: &str, cookie: Option<&str>) -> String {
        let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {c}\r\n"));
        self.get_with(path_and_query, &cookie)
    }

    /// `headers`: whole lines, each ended by `\r\n`.
    fn get_with(&self, path_and_query: &str, headers: &str) -> String {
        let host = self.host();
        let mut stream = TcpStream::connect(&host).unwrap();
        write!(
            stream,
            "GET {path_and_query} HTTP/1.1\r\nHost: {host}\r\n{headers}\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    /// Pairs and returns the `name=value` session cookie.
    fn pair(&self) -> String {
        let response = self.get(&self.pairing_path(), None);
        session_cookie_of(&response).expect("a pairing sets the session cookie")
    }

    fn connect(&self, cookie: &str) -> WebSocket<TcpStream> {
        let host = self.host();
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

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

fn start_server(
    registry: &Registry,
    launcher: Launcher,
    devices: &PairedDevices,
    alerts: &Arc<Mutex<Vec<AccessAlert>>>,
) -> PhoneServer {
    let services = services(registry, launcher, devices, alerts);
    PhoneServer::start_on_address(LOOPBACK, services).unwrap()
}

fn services(
    registry: &Registry,
    launcher: Launcher,
    devices: &PairedDevices,
    alerts: &Arc<Mutex<Vec<AccessAlert>>>,
) -> PhoneServices {
    let alerts = Arc::clone(alerts);
    PhoneServices {
        registry: registry.clone(),
        launcher,
        devices: devices.clone(),
        alert: Arc::new(move |alert| alerts.lock().unwrap().push(alert)),
    }
}

/// The server closed `ws`, or its connection broke.
fn is_closed(ws: &mut WebSocket<TcpStream>) -> bool {
    match ws.read() {
        Ok(message) => message.is_close(),
        Err(tungstenite::Error::Io(err)) => !matches!(
            err.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
        Err(_) => true,
    }
}

/// The `name=value` of a response's session `Set-Cookie`.
fn session_cookie_of(response: &str) -> Option<String> {
    let set_cookie = response
        .lines()
        .find_map(|line| line.strip_prefix("Set-Cookie: "))?;
    Some(set_cookie.split(';').next()?.to_owned())
}

fn exposed(pane: &Pane, tab: &str, worktree: &Path) -> ExposedPane {
    ExposedPane {
        uid: pane.uid(),
        project: "api".to_owned(),
        branch: Some("main".to_owned()),
        tab: tab.to_owned(),
        worktree: worktree.to_path_buf(),
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
fn pairing_spends_the_code_for_a_lasting_session_cookie() {
    let fixture = Fixture::new();
    let pairing = fixture.pairing_path();

    let response = fixture.get(&pairing, None);
    let replayed = fixture.get(&pairing, None);
    let alerts = fixture.alerts.lock().unwrap().clone();

    fixture.close();
    assert!(response.starts_with("HTTP/1.1 303"), "{response}");
    assert!(
        response.contains("Location: /\r\n"),
        "the code leaves the address bar"
    );
    assert!(response.contains("Set-Cookie: helm_session="));
    assert!(response.contains("HttpOnly; SameSite=Strict; Path=/; Max-Age="));
    assert!(
        replayed.starts_with("HTTP/1.1 401"),
        "the code is single use"
    );
    assert_eq!(
        alerts,
        vec![AccessAlert::Paired {
            device: "Browser".to_owned()
        }]
    );
}

#[test]
fn an_unpaired_phone_gets_the_not_paired_page() {
    let fixture = Fixture::new();
    fixture.server().offer_pairing();

    let page = fixture.get("/", None);
    let wrong_code = fixture.get("/pair?t=00", None);

    fixture.close();
    assert!(page.starts_with("HTTP/1.1 401"), "{page}");
    assert!(page.contains("This phone isn't paired"));
    assert!(wrong_code.starts_with("HTTP/1.1 401"));
}

#[test]
fn a_paired_phone_scanning_again_lands_on_its_agents_without_a_second_pairing() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();
    let pairing = fixture.pairing_path();

    let rescanned = fixture.get(&pairing, Some(&cookie));
    let other_phone = fixture.get(&pairing, None);
    let devices = fixture.devices.read(|book| book.devices.len());

    fixture.close();
    assert!(rescanned.starts_with("HTTP/1.1 303"), "{rescanned}");
    assert!(
        !rescanned.contains("Set-Cookie"),
        "the paired phone keeps its device"
    );
    assert!(
        other_phone.starts_with("HTTP/1.1 303"),
        "the code was not spent"
    );
    assert_eq!(devices, 2);
}

#[test]
fn a_page_load_rotates_the_session_cookie() {
    let fixture = Fixture::new();
    let paired = fixture.pair();

    let page = fixture.get("/", Some(&paired));
    let rotated = session_cookie_of(&page).expect("the page load hands a new cookie");
    let asset = fixture.get("/app.css", Some(&rotated));

    fixture.close();
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert_ne!(rotated, paired);
    assert!(asset.starts_with("HTTP/1.1 200"), "{asset}");
}

#[test]
fn a_pairing_outlives_the_server() {
    let mut fixture = Fixture::new();
    let cookie = fixture.pair();
    let port = fixture.origin();

    fixture.restart_server();
    let page = fixture.get("/app.css", Some(&cookie));
    let same_port = fixture.origin() == port;

    fixture.close();
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(same_port, "the phone's bookmark keeps pointing at helm");
}

#[test]
fn access_follows_the_address_across_a_lease() {
    let mut fixture = Fixture::new();
    let lan = fixture.restart_server_on_a_moving_lan();
    let cookie = fixture.pair();
    let host = fixture.host();
    let mut ws = fixture.connect(&cookie);
    wait_for(&mut ws, "agents", |_| true).expect("the paired phone is served");

    *lan.lock().unwrap() = Lan::Unsettled;
    let closed = wait_until_within(Duration::from_secs(5), || is_closed(&mut ws));
    let refused = TcpStream::connect(&host).is_err();
    *lan.lock().unwrap() = Lan::Address(LOOPBACK);
    let back = wait_until_within(Duration::from_secs(5), || TcpStream::connect(&host).is_ok());
    let page = fixture.get("/app.css", Some(&cookie));
    let running = fixture.server().is_running();

    fixture.close();
    assert!(closed, "a socket of the address left is closed");
    assert!(refused, "nothing listens between two leases");
    assert!(
        back,
        "the phone's bookmark is served again on the new lease"
    );
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(running, "access never stopped");
}

#[test]
fn another_network_stops_access() {
    let mut fixture = Fixture::new();
    let lan = fixture.restart_server_on_a_moving_lan();

    *lan.lock().unwrap() = Lan::Unrecorded;
    let stopped = wait_until_within(Duration::from_secs(5), || !fixture.server().is_running());

    fixture.close();
    assert!(stopped);
}

#[test]
fn a_revoked_phone_is_shut_out_and_its_socket_closes() {
    let fixture = Fixture::new();
    let cookie = fixture.pair();
    let mut ws = fixture.connect(&cookie);
    wait_for(&mut ws, "agents", |_| true).expect("the paired phone is served");
    let connected = fixture.server().connected_devices();

    fixture.devices.edit(|book| book.revoke_all());
    let closed = wait_until_within(Duration::from_secs(3), || is_closed(&mut ws));
    let page = fixture.get("/app.css", Some(&cookie));
    let left = wait_until_within(Duration::from_secs(3), || {
        fixture.server().connected_devices().is_empty()
    });

    fixture.close();
    assert_eq!(connected, vec!["Browser".to_owned()]);
    assert!(left, "a closed socket no longer counts as connected");
    assert!(closed, "the revoked phone's socket is closed");
    assert!(page.starts_with("HTTP/1.1 401"), "{page}");
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
        agents: vec![Agent::new("Claude Code", &command)],
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

/// A paired phone in front of an agent whose worktree holds files of every kind,
/// beside a secret that sits outside it.
struct FilesFixture {
    fixture: Fixture,
    cookie: String,
}

impl FilesFixture {
    fn new() -> Self {
        let fixture = Fixture::new();
        let worktree = fixture.worktree();
        std::fs::write(fixture.dir.path().join("secret.txt"), "outside").unwrap();
        std::fs::create_dir(worktree.join(".git")).unwrap();
        std::fs::write(worktree.join(".git/config"), "[core]").unwrap();
        std::fs::create_dir(worktree.join("out")).unwrap();
        std::fs::write(worktree.join("shot.png"), "0123456789").unwrap();
        std::fs::write(worktree.join("notes.log"), "plain words").unwrap();
        std::fs::write(worktree.join("report.html"), "<p>report</p>").unwrap();
        std::os::unix::fs::symlink("../secret.txt", worktree.join("leak.txt")).unwrap();
        age(&worktree.join("shot.png"), 30);
        age(&worktree.join("out"), 20);
        age(&worktree.join("notes.log"), 10);
        let listed = wait_until(|| !fixture.registry.agents().is_empty());
        assert!(listed, "the watcher sees the fixture's agent");
        let cookie = fixture.pair();
        Self { fixture, cookie }
    }

    fn get(&self, route: &str, pane: &Pane, path: &str, range: Option<&str>) -> String {
        let range = range.map_or(String::new(), |r| format!("Range: {r}\r\n"));
        let headers = format!("Cookie: {}\r\n{range}", self.cookie);
        let query = format!("/{route}?pane={}&path={path}", pane.uid().get());
        self.fixture.get_with(&query, &headers)
    }

    fn file(&self, path: &str) -> String {
        self.get("file", &self.fixture.agent, path, None)
    }
}

/// Sets the entry's modification time `seconds` back.
fn age(path: &Path, seconds: u64) {
    let then = SystemTime::now() - Duration::from_secs(seconds);
    std::fs::File::open(path)
        .unwrap()
        .set_modified(then)
        .unwrap();
}

fn body_of(response: &str) -> &str {
    response.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

#[test]
fn a_worktree_folder_lists_newest_first_without_git_nor_symlinks() {
    let files = FilesFixture::new();

    let response = files.get("files", &files.fixture.agent, "", None);

    files.fixture.close();
    let listing: Value = serde_json::from_str(body_of(&response)).expect(&response);
    let names: Vec<&str> = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["report.html", "notes.log", "out", "shot.png"]);
    assert_eq!(listing["entries"][2]["dir"], true);
    assert_eq!(listing["entries"][3]["size"], 10);
}

#[test]
fn a_file_goes_out_with_its_type_whole_or_the_range_asked_for() {
    let files = FilesFixture::new();

    let whole = files.file("shot.png");
    let part = files.get("file", &files.fixture.agent, "shot.png", Some("bytes=2-4"));
    let past = files.get("file", &files.fixture.agent, "shot.png", Some("bytes=50-"));
    let unknown_extension = files.file("notes.log");

    files.fixture.close();
    assert!(whole.starts_with("HTTP/1.1 200"), "{whole}");
    assert!(whole.contains("Content-Type: image/png\r\n"));
    assert_eq!(body_of(&whole), "0123456789");
    assert!(part.starts_with("HTTP/1.1 206"), "{part}");
    assert!(part.contains("Content-Range: bytes 2-4/10\r\n"));
    assert_eq!(body_of(&part), "234");
    assert!(past.starts_with("HTTP/1.1 416"), "{past}");
    assert!(unknown_extension.contains("Content-Type: text/plain; charset=utf-8\r\n"));
}

#[test]
fn an_html_file_goes_out_sandboxed() {
    let files = FilesFixture::new();

    let report = files.file("report.html");

    files.fixture.close();
    assert!(report.starts_with("HTTP/1.1 200"), "{report}");
    assert!(report.contains("Content-Security-Policy: sandbox allow-scripts\r\n"));
}

#[test]
fn nothing_outside_the_worktree_nor_its_git_directory_is_served() {
    let files = FilesFixture::new();

    let parent = files.file("..%2Fsecret.txt");
    let symlink = files.file("leak.txt");
    let git = files.file(".git%2Fconfig");
    let git_listing = files.get("files", &files.fixture.agent, ".git", None);

    files.fixture.close();
    for refused in [parent, symlink, git, git_listing] {
        assert!(refused.starts_with("HTTP/1.1 404"), "{refused}");
    }
}

#[test]
fn files_are_served_to_a_paired_phone_for_an_agents_pane_only() {
    let files = FilesFixture::new();

    let plain_shell = files.get("file", &files.fixture.shell, "shot.png", None);
    let unpaired = files.fixture.get(
        &format!(
            "/file?pane={}&path=shot.png",
            files.fixture.agent.uid().get()
        ),
        None,
    );

    files.fixture.close();
    assert!(plain_shell.starts_with("HTTP/1.1 404"), "{plain_shell}");
    assert!(unpaired.starts_with("HTTP/1.1 401"), "{unpaired}");
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
