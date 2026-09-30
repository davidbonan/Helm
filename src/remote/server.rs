//! The phone server's lifecycle (specs/remote.md §3, §4): bound to one LAN address,
//! a thread per connection, stopped on drop, after the idle delay without a
//! client, or when the LAN address goes away. Nothing waits on the UI thread.

use std::io;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tungstenite::protocol::Role;
use tungstenite::WebSocket;

use crate::remote::access::{Access, IdleClock, Token};
use crate::remote::address::{interfaces, lan_address};
use crate::remote::awake::KeepAwake;
use crate::remote::http::{read_head, RequestHead, Response};
use crate::remote::launch::Launcher;
use crate::remote::protocol::PageTheme;
use crate::remote::registry::Registry;
use crate::remote::socket::{PhoneSocket, READ_TIMEOUT};
use crate::terminal::activity::now_ms;

const ACCEPT_POLL: Duration = Duration::from_millis(100);
const ADDRESS_CHECK: Duration = Duration::from_secs(5);
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);
/// A phone that stops reading (a suspended Safari tab) must not pin its thread.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_JS: &str = include_str!("assets/app.js");
const APP_CSS: &str = include_str!("assets/app.css");
const EXPIRED_HTML: &str = include_str!("assets/expired.html");

#[derive(Debug)]
pub enum StartError {
    NoLocalNetwork,
    Bind(io::Error),
}

pub struct PhoneServer {
    shared: Arc<Shared>,
    pairing_url: String,
    accept: Option<JoinHandle<()>>,
}

struct Shared {
    access: Access,
    registry: Registry,
    launcher: Launcher,
    idle: Mutex<IdleClock>,
    stopped: AtomicBool,
    awake: Mutex<Option<KeepAwake>>,
}

impl Shared {
    fn idle(&self) -> MutexGuard<'_, IdleClock> {
        self.idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.awake
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

impl PhoneServer {
    /// On the machine's LAN address; access stops if that address disappears.
    pub fn start(registry: Registry, launcher: Launcher) -> Result<Self, StartError> {
        let ip = lan_address(&interfaces()).ok_or(StartError::NoLocalNetwork)?;
        Self::start_on(ip, registry, launcher, true).map_err(StartError::Bind)
    }

    /// On `ip`, the address left unwatched (tests bind the loopback).
    pub fn start_on_address(
        ip: Ipv4Addr,
        registry: Registry,
        launcher: Launcher,
    ) -> io::Result<Self> {
        Self::start_on(ip, registry, launcher, false)
    }

    fn start_on(
        ip: Ipv4Addr,
        registry: Registry,
        launcher: Launcher,
        watch_address: bool,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((ip, 0))?;
        listener.set_nonblocking(true)?;
        let access = Access::new(Token::mint(), listener.local_addr()?);
        let pairing_url = access.pairing_url();
        let shared = Arc::new(Shared {
            access,
            registry,
            launcher,
            idle: Mutex::new(IdleClock::started(now_ms())),
            stopped: AtomicBool::new(false),
            awake: Mutex::new(Some(KeepAwake::begin())),
        });
        let accepting = Arc::clone(&shared);
        let accept = std::thread::Builder::new()
            .name("phone-accept".into())
            .spawn(move || accept_loop(&listener, &accepting, watch_address.then_some(ip)))?;
        Ok(Self {
            shared,
            pairing_url,
            accept: Some(accept),
        })
    }

    /// What the QR code encodes.
    pub fn pairing_url(&self) -> &str {
        &self.pairing_url
    }

    pub fn clients(&self) -> usize {
        self.shared.idle().clients()
    }

    /// `false` once access stopped on its own (idle delay, LAN address gone).
    pub fn is_running(&self) -> bool {
        !self.shared.is_stopped()
    }
}

impl Drop for PhoneServer {
    fn drop(&mut self) {
        self.shared.stop();
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

/// Connection threads are detached: they see `stopped` within [`READ_TIMEOUT`].
fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>, watched_ip: Option<Ipv4Addr>) {
    let mut address_checked = Instant::now();
    while !shared.is_stopped() {
        match listener.accept() {
            Ok((stream, _)) => {
                let serving = Arc::clone(shared);
                let _ = std::thread::Builder::new()
                    .name("phone-connection".into())
                    .spawn(move || serve(stream, &serving));
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(ACCEPT_POLL),
            Err(_) => std::thread::sleep(ACCEPT_POLL),
        }
        if shared.idle().is_expired(now_ms()) {
            shared.stop();
        }
        if let Some(ip) = watched_ip {
            if address_checked.elapsed() >= ADDRESS_CHECK {
                address_checked = Instant::now();
                if !interfaces().iter().any(|i| i.is_up && i.addr == ip) {
                    shared.stop();
                }
            }
        }
    }
}

fn serve(mut stream: TcpStream, shared: &Shared) {
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(HEAD_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
    {
        return;
    }
    let Ok(Some(head)) = read_head(&mut stream) else {
        return;
    };
    if head.method != "GET" {
        let _ = Response::new("405 Method Not Allowed", "text/plain", b"").write_to(&mut stream);
        return;
    }
    let paired = shared.access.is_paired(head.header("cookie"));
    let response = match head.path.as_str() {
        "/pair" => return pair(&mut stream, &head, &shared.access),
        "/ws" if head.is_websocket_upgrade() => return upgrade(stream, &head, shared),
        "/" | "/app.js" | "/app.css" if !paired => Response::new(
            "401 Unauthorized",
            "text/html; charset=utf-8",
            EXPIRED_HTML.as_bytes(),
        ),
        "/" => return index(&mut stream, &shared.registry),
        "/app.js" => Response::new("200 OK", "text/javascript", APP_JS.as_bytes()),
        "/app.css" => Response::new("200 OK", "text/css", APP_CSS.as_bytes()),
        _ => Response::new("404 Not Found", "text/plain", b""),
    };
    let _ = response.write_to(&mut stream);
}

fn index(stream: &mut TcpStream, registry: &Registry) {
    let page = index_page(&registry.page_theme());
    let _ = Response::new("200 OK", "text/html; charset=utf-8", page.as_bytes()).write_to(stream);
}

fn index_page(theme: &PageTheme) -> String {
    let scheme = if theme.dark { "dark" } else { "light" };
    INDEX_HTML.replacen(
        "<html lang=\"en\">",
        &format!(
            "<html lang=\"en\" data-theme=\"{scheme}\" style=\"{}\">",
            theme.css()
        ),
        1,
    )
}

/// A valid token becomes the session cookie; the token leaves the address bar.
fn pair(stream: &mut TcpStream, head: &RequestHead, access: &Access) {
    let response = if head.query_param("t").is_some_and(|t| access.pairs(t)) {
        Response::new("303 See Other", "text/plain", b"")
            .with("Location", "/".to_owned())
            .with("Set-Cookie", access.session_cookie())
    } else {
        Response::new(
            "401 Unauthorized",
            "text/html; charset=utf-8",
            EXPIRED_HTML.as_bytes(),
        )
    };
    let _ = response.write_to(stream);
}

fn upgrade(mut stream: TcpStream, head: &RequestHead, shared: &Shared) {
    let key = head.header("sec-websocket-key");
    let accepted = shared
        .access
        .accepts_upgrade(head.header("cookie"), head.header("origin"));
    let (Some(key), true) = (key, accepted) else {
        let _ = Response::new("401 Unauthorized", "text/plain", b"").write_to(&mut stream);
        return;
    };
    let switching = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        tungstenite::handshake::derive_accept_key(key.as_bytes())
    );
    if io::Write::write_all(&mut stream, switching.as_bytes()).is_err()
        || stream.set_read_timeout(Some(READ_TIMEOUT)).is_err()
    {
        return;
    }
    let ws = WebSocket::from_raw_socket(stream, Role::Server, None);
    shared.idle().connect();
    PhoneSocket::new(ws, shared.registry.clone(), shared.launcher.clone())
        .run(|| shared.is_stopped());
    shared.idle().disconnect(now_ms());
}
